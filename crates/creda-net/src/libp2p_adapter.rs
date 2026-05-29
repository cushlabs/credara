//! libp2p adapter — the assembled networking layer (feature `libp2p`). **Isolation point.**
//!
//! This is the one module that touches `rust-libp2p`. It implements [`NetworkTransport`] over a
//! libp2p `Swarm` composing the primitives the spec assembles rather than builds (§6.2.1):
//! gossipsub (bucketed topics, §6.2.4), Kademlia DHT (subgraph routing, §6.1.5), Noise transport
//! (§6.2.3), request/response (targeted event fetch, §6.1.5/§6.1.8), and identify.
//!
//! ## Why this module is quarantined
//!
//! rust-libp2p's API changes between minor versions (the `SwarmBuilder`, `NetworkBehaviour`
//! derive, gossipsub/kad event shapes, and codec traits have all moved historically). By
//! keeping every libp2p reference here, behind an off-by-default feature, those changes can
//! never break the rest of the workspace — only this file needs reconciliation when the pinned
//! version (currently `libp2p = "0.54"`, see Cargo.toml) is bumped.
//!
//! ## Status: documented scaffold
//!
//! This captures the intended architecture — a background Swarm task driven by a command channel,
//! with [`NetworkTransport`] methods translating to gossipsub/kad/request-response operations.
//! The spots that are most version-specific are marked `TODO(libp2p-verify)` and must be
//! reconciled against the pinned libp2p version on first compile of this feature. It is not
//! built or tested by the default workspace build by design (see the crate docs).
//!
//! `TODO(open-question-13.3)`: DHT query-privacy (§8.5) is unresolved — Kademlia lookups reveal
//! the queried key to the peers on the lookup path. This adapter wires the DHT but does not
//! solve query-privacy; do not represent it as solved.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use libp2p::futures::StreamExt;
use libp2p::kad::QueryId;
use libp2p::request_response::{Message, OutboundRequestId, ResponseChannel};
use libp2p::swarm::SwarmEvent;
use libp2p::{gossipsub, identify, kad, request_response, Multiaddr, PeerId, Swarm};
use tokio::sync::{mpsc, oneshot};

use creda_events::{EventId, IdentityEventNode};

use crate::bucketing::{topic_for_bucket, DhtKey};
use crate::error::{Error, Result};
use crate::gossip::GossipBatch;
use crate::transport::{EventSource, NetworkTransport};

/// The composed libp2p behaviour for a Creda peer (§6.2.1). Each field is a primitive Creda
/// assembles rather than builds.
///
/// It lives in its own module so the `#[derive(NetworkBehaviour)]` macro's generated code (which
/// uses a bare `Result<..>`) resolves to the std-prelude `Result`, not this crate's `Result`
/// alias (which takes one type parameter and would otherwise break the generated `Debug` and
/// connection-handler impls). TODO(libp2p-verify).
mod behaviour {
    use libp2p::swarm::NetworkBehaviour;
    use libp2p::{gossipsub, identify, kad, request_response};

    use super::{EventRequest, EventResponse};

    #[derive(NetworkBehaviour)]
    pub struct CredaBehaviour {
        /// Event propagation over bucketed topics (§6.2.4).
        pub gossipsub: gossipsub::Behaviour,
        /// Subgraph routing: which peers hold a patient's events (§6.1.5).
        pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
        /// Targeted event fetch after a DHT lookup and during anti-entropy (§6.1.5, §6.1.8).
        /// The protocol payload is a CBOR-encoded request/response of event ids ↔ events.
        pub request_response: request_response::cbor::Behaviour<EventRequest, EventResponse>,
        /// Peer metadata exchange.
        pub identify: identify::Behaviour,
    }
}
pub use behaviour::{CredaBehaviour, CredaBehaviourEvent};

/// A request for specific events by id (§6.1.5/§6.1.8 transfer step).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EventRequest {
    pub ids: Vec<EventId>,
}

/// The events returned for an [`EventRequest`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EventResponse {
    pub events: Vec<IdentityEventNode>,
}

// Aliases keep the run_swarm / handle_command signatures readable and clippy quiet on
// type_complexity. The outbound map carries fetched events; the queries map carries DHT
// get_providers results.
type EventReply = oneshot::Sender<Result<Vec<IdentityEventNode>>>;
type ProviderReply = oneshot::Sender<Result<Vec<Vec<u8>>>>;
type PendingOutbound = HashMap<OutboundRequestId, EventReply>;
type PendingQueries = HashMap<QueryId, (ProviderReply, HashSet<PeerId>)>;

/// Commands sent from [`Libp2pTransport`] handles to the background Swarm task. Keeping the
/// Swarm in one task (it is `!Sync`) and talking to it over a channel is the idiomatic way to
/// expose an ergonomic async API.
enum Command {
    PublishBatch { bucket: u64, bytes: Vec<u8>, reply: oneshot::Sender<Result<()>> },
    Subscribe { bucket: u64, reply: oneshot::Sender<Result<()>> },
    Unsubscribe { bucket: u64, reply: oneshot::Sender<Result<()>> },
    DhtProvide { key: DhtKey, reply: oneshot::Sender<Result<()>> },
    DhtFindProviders { key: DhtKey, reply: oneshot::Sender<Result<Vec<Vec<u8>>>> },
    RequestEvents { peer: Vec<u8>, ids: Vec<EventId>, reply: oneshot::Sender<Result<Vec<IdentityEventNode>>> },
    /// Self-command emitted by the inbound-request task to send a response back through the
    /// swarm (where `&mut Swarm` is available).
    RespondEvents { channel: ResponseChannel<EventResponse>, events: Vec<IdentityEventNode> },
}

/// Handle to a running Creda libp2p peer. Cloneable; all clones drive the same Swarm task.
#[derive(Clone)]
pub struct Libp2pTransport {
    cmd_tx: mpsc::Sender<Command>,
    local_peer_id: Vec<u8>,
}

impl Libp2pTransport {
    /// Build the Swarm, spawn its background task, and return a handle.
    ///
    /// `listen_on` is the multiaddr to listen on (e.g. `/ip4/0.0.0.0/tcp/0`); `bootstrap` are
    /// addresses of bootstrap peers (§6.1.3). Noise + SPIFFE keying (§6.2.3) is wired by the
    /// caller via the provided keypair.
    ///
    /// Returns the handle plus an **inbound channel** of received gossip-batch bytes: the daemon
    /// drains it and feeds each batch to the engine's ingest path (`Replicator::ingest_batch`),
    /// which is where mandatory signature verification happens (§3.6). This is the transport→engine
    /// half of replication.
    ///
    /// TODO(libp2p-verify): the `SwarmBuilder` chain, the gossipsub/kad/request_response
    /// constructors, and the executor wiring are the version-sensitive lines — reconcile against
    /// the pinned libp2p version on first build.
    pub async fn spawn(
        keypair: libp2p::identity::Keypair,
        listen_on: Multiaddr,
        bootstrap: Vec<(PeerId, Multiaddr)>,
        source: Arc<dyn EventSource>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let local_peer_id = PeerId::from(keypair.public());

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new, // Noise transport (§6.2.3)
                libp2p::yamux::Config::default,
            )
            .map_err(|e| Error::Transport(format!("tcp/noise/yamux setup: {e}")))?
            .with_behaviour(build_behaviour)
            .map_err(|e| Error::Transport(format!("behaviour setup: {e}")))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        swarm
            .listen_on(listen_on)
            .map_err(|e| Error::Transport(format!("listen: {e}")))?;
        for (peer, addr) in bootstrap {
            swarm.behaviour_mut().kademlia.add_address(&peer, addr);
        }

        // Two command channels: `cmd_rx` carries external API calls (and closes when every
        // Libp2pTransport handle is dropped, signalling shutdown); `self_rx` carries swarm-task
        // self-commands (Command::RespondEvents from inbound-request fetch tasks). A single
        // shared channel would never close on shutdown because the task holds a clone.
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let (self_tx, self_rx) = mpsc::channel(64);
        let (inbound_tx, inbound_rx) = mpsc::channel(1024);
        tokio::spawn(run_swarm(swarm, cmd_rx, self_tx, self_rx, inbound_tx, source));

        Ok((
            Self {
                cmd_tx,
                local_peer_id: local_peer_id.to_bytes(),
            },
            inbound_rx,
        ))
    }

    /// Convenience constructor that hides libp2p types from callers (Creda Core): generate a
    /// fresh Ed25519 identity, parse the listen multiaddr, and spawn. Returns the handle and the
    /// inbound gossip-batch channel.
    ///
    /// TODO(libp2p-verify): derive the identity from the institution signing key / SPIFFE SVID
    /// rather than generating a throwaway one (§6.2.3), and parse `bootstrap` entries of the form
    /// `/ip4/.../tcp/.../p2p/<peer-id>` into `(PeerId, Multiaddr)` pairs (left empty for now).
    pub async fn generate_and_spawn(
        listen: &str,
        _bootstrap: Vec<String>,
        source: Arc<dyn EventSource>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let listen_on: Multiaddr = listen
            .parse()
            .map_err(|e| Error::Transport(format!("bad listen multiaddr {listen:?}: {e}")))?;
        Self::spawn(keypair, listen_on, Vec::new(), source).await
    }

    async fn send<T>(&self, make: impl FnOnce(oneshot::Sender<Result<T>>) -> Command) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(make(tx))
            .await
            .map_err(|_| Error::Transport("swarm task is gone".into()))?;
        rx.await
            .map_err(|_| Error::Transport("swarm dropped the reply".into()))?
    }
}

impl NetworkTransport for Libp2pTransport {
    async fn publish_batch(&self, bucket: u64, batch: &GossipBatch) -> Result<()> {
        let bytes = batch.to_bytes()?;
        self.send(|reply| Command::PublishBatch { bucket, bytes, reply }).await
    }

    async fn subscribe_bucket(&self, bucket: u64) -> Result<()> {
        self.send(|reply| Command::Subscribe { bucket, reply }).await
    }

    async fn unsubscribe_bucket(&self, bucket: u64) -> Result<()> {
        self.send(|reply| Command::Unsubscribe { bucket, reply }).await
    }

    async fn dht_provide(&self, key: DhtKey) -> Result<()> {
        self.send(|reply| Command::DhtProvide { key, reply }).await
    }

    async fn dht_find_providers(&self, key: DhtKey) -> Result<Vec<Vec<u8>>> {
        self.send(|reply| Command::DhtFindProviders { key, reply }).await
    }

    async fn request_events(&self, peer: &[u8], ids: &[EventId]) -> Result<Vec<IdentityEventNode>> {
        let peer = peer.to_vec();
        let ids = ids.to_vec();
        self.send(|reply| Command::RequestEvents { peer, ids, reply }).await
    }

    fn local_peer_id(&self) -> Vec<u8> {
        self.local_peer_id.clone()
    }
}

/// Construct the composed behaviour.
///
/// TODO(libp2p-verify): gossipsub/kad/request_response constructor signatures are
/// version-sensitive. The request/response protocol uses the CBOR codec over a single protocol
/// id; gossipsub uses the default config with message-id derived from content.
fn build_behaviour(key: &libp2p::identity::Keypair) -> CredaBehaviour {
    let peer_id = PeerId::from(key.public());

    let gossipsub = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(key.clone()),
        gossipsub::Config::default(),
    )
    .expect("valid gossipsub config");

    let kademlia = kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));

    let request_response = request_response::cbor::Behaviour::new(
        [(
            libp2p::StreamProtocol::new("/creda/events/1"),
            request_response::ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );

    let identify = identify::Behaviour::new(identify::Config::new(
        "/creda/1".into(),
        key.public(),
    ));

    CredaBehaviour {
        gossipsub,
        kademlia,
        request_response,
        identify,
    }
}

/// The background event loop: service commands from handles and react to Swarm events.
///
/// Wiring:
/// - Gossipsub messages are forwarded on `inbound_tx` for the engine's ingest gate (§3.6); a full
///   or closed channel drops the batch (gossip is best-effort, anti-entropy heals — §6.1.8).
/// - Outbound `request_events` calls are correlated by `OutboundRequestId` via `pending_outbound`,
///   so the awaiting caller is woken when the matching response arrives (or fails).
/// - Inbound `EventRequest`s are answered from the local store via [`EventSource`]: the lookup
///   runs on `spawn_blocking` so the swarm loop never blocks, and the response is sent back
///   through `self_tx` as `Command::RespondEvents` so `send_response` happens on the swarm task
///   with `&mut Swarm` in hand. A separate `self_rx` is drained alongside `cmd_rx` (split into
///   two channels so `cmd_rx` closes cleanly on external-handle shutdown).
///
/// - Outbound Kademlia `get_providers` calls are correlated by `QueryId` via `pending_queries`,
///   with a `HashSet<PeerId>` accumulator across multi-step progress events; the awaiting Sender
///   is completed when the query terminates (`step.last`).
///
/// TODO(libp2p-verify): the exact field names of `SwarmEvent`/`request_response::Event`/
/// `kad::Event::OutboundQueryProgressed`/`kad::QueryResult::GetProviders` are libp2p 0.54
/// version-sensitive — adjust the match arms if the field/variant names drifted.
async fn run_swarm(
    mut swarm: Swarm<CredaBehaviour>,
    mut cmd_rx: mpsc::Receiver<Command>,
    self_tx: mpsc::Sender<Command>,
    mut self_rx: mpsc::Receiver<Command>,
    inbound_tx: mpsc::Sender<Vec<u8>>,
    source: Arc<dyn EventSource>,
) {
    // Outbound request_response correlation: completed when the matching response (or failure)
    // arrives. Dropped on shutdown — pending callers see "swarm dropped the reply".
    let mut pending_outbound: PendingOutbound = HashMap::new();

    // Outbound Kademlia `get_providers` correlation. A single query may emit multiple
    // `OutboundQueryProgressed` events (one per route segment that yields providers), so we keep
    // an accumulating `HashSet<PeerId>` per QueryId and complete the awaiting Sender when the
    // query terminates (`step.last`).
    let mut pending_queries: PendingQueries = HashMap::new();

    loop {
        tokio::select! {
            command = cmd_rx.recv() => {
                let Some(command) = command else { break }; // all external handles dropped
                handle_command(&mut swarm, &mut pending_outbound, &mut pending_queries, command);
            }
            command = self_rx.recv() => {
                if let Some(command) = command {
                    handle_command(&mut swarm, &mut pending_outbound, &mut pending_queries, command);
                }
            }
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(CredaBehaviourEvent::Gossipsub(
                        gossipsub::Event::Message { message, .. },
                    )) => {
                        // Deliver the received gossip batch to the engine's ingest path.
                        let _ = inbound_tx.try_send(message.data);
                    }

                    // Inbound EventRequest from a peer: gather events from the local store and
                    // send the response (via self_tx so the actual send_response runs on the
                    // swarm task). spawn_blocking keeps the swarm event loop responsive.
                    SwarmEvent::Behaviour(CredaBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            message: Message::Request { request, channel, .. },
                            ..
                        },
                    )) => {
                        let source = source.clone();
                        let self_tx = self_tx.clone();
                        let ids = request.ids;
                        tokio::spawn(async move {
                            let events = tokio::task::spawn_blocking(move || source.get_events(&ids))
                                .await
                                .unwrap_or_default();
                            let _ = self_tx
                                .send(Command::RespondEvents { channel, events })
                                .await;
                        });
                    }

                    // Outbound EventRequest completed: deliver to the awaiting caller.
                    SwarmEvent::Behaviour(CredaBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            message: Message::Response { request_id, response },
                            ..
                        },
                    )) => {
                        if let Some(tx) = pending_outbound.remove(&request_id) {
                            let _ = tx.send(Ok(response.events));
                        }
                    }

                    // Outbound request failed (timeout, disconnect, …): error the awaiting caller.
                    SwarmEvent::Behaviour(CredaBehaviourEvent::RequestResponse(
                        request_response::Event::OutboundFailure { request_id, error, .. },
                    )) => {
                        if let Some(tx) = pending_outbound.remove(&request_id) {
                            let _ = tx.send(Err(Error::Transport(format!(
                                "request_response outbound failure: {error}"
                            ))));
                        }
                    }

                    // Kademlia query progress. We care specifically about `get_providers`
                    // results — accumulate any providers reported in this step, and on the
                    // terminating step (`step.last`) pop the pending entry and deliver the
                    // accumulated peer-id bytes to the awaiting caller. Other kad events
                    // (routing-table updates, other query kinds) we currently ignore.
                    //
                    // TODO(libp2p-verify): the exact `kad::Event::OutboundQueryProgressed` field
                    // names and the `QueryResult::GetProviders` variant shape are libp2p 0.54
                    // version-sensitive (`Result<GetProvidersOk, GetProvidersError>`). If 0.54
                    // dropped the error half, adjust the match accordingly.
                    SwarmEvent::Behaviour(CredaBehaviourEvent::Kademlia(
                        kad::Event::OutboundQueryProgressed {
                            id,
                            result: kad::QueryResult::GetProviders(query_result),
                            step,
                            ..
                        },
                    )) => {
                        if let Some((_, acc)) = pending_queries.get_mut(&id) {
                            if let Ok(kad::GetProvidersOk::FoundProviders {
                                providers, ..
                            }) = query_result
                            {
                                for peer in providers {
                                    acc.insert(peer);
                                }
                            }
                        }
                        if step.last {
                            if let Some((reply, acc)) = pending_queries.remove(&id) {
                                let peers: Vec<Vec<u8>> =
                                    acc.into_iter().map(|p| p.to_bytes()).collect();
                                let _ = reply.send(Ok(peers));
                            }
                        }
                    }
                    SwarmEvent::Behaviour(CredaBehaviourEvent::Kademlia(_)) => {}
                    _ => {}
                }
            }
        }
    }
}

fn handle_command(
    swarm: &mut Swarm<CredaBehaviour>,
    pending_outbound: &mut PendingOutbound,
    pending_queries: &mut PendingQueries,
    command: Command,
) {
    match command {
        Command::Subscribe { bucket, reply } => {
            let topic = gossipsub::IdentTopic::new(topic_for_bucket(bucket));
            let res = swarm
                .behaviour_mut()
                .gossipsub
                .subscribe(&topic)
                .map(|_| ())
                .map_err(|e| Error::Transport(format!("subscribe: {e}")));
            let _ = reply.send(res);
        }
        Command::Unsubscribe { bucket, reply } => {
            let topic = gossipsub::IdentTopic::new(topic_for_bucket(bucket));
            let res = swarm
                .behaviour_mut()
                .gossipsub
                .unsubscribe(&topic)
                .map(|_| ())
                .map_err(|e| Error::Transport(format!("unsubscribe: {e}")));
            let _ = reply.send(res);
        }
        Command::PublishBatch { bucket, bytes, reply } => {
            let topic = gossipsub::IdentTopic::new(topic_for_bucket(bucket));
            let res = swarm
                .behaviour_mut()
                .gossipsub
                .publish(topic, bytes)
                .map(|_| ())
                .map_err(|e| Error::Transport(format!("publish: {e}")));
            let _ = reply.send(res);
        }
        Command::DhtProvide { key, reply } => {
            let res = swarm
                .behaviour_mut()
                .kademlia
                .start_providing(kad::RecordKey::new(&key))
                .map(|_| ())
                .map_err(|e| Error::Transport(format!("dht provide: {e}")));
            let _ = reply.send(res);
        }
        Command::DhtFindProviders { key, reply } => {
            // `get_providers` returns synchronously with a `QueryId` and starts a Kademlia query;
            // the actual providers arrive as one or more later `OutboundQueryProgressed` events.
            // Park the reply Sender + an accumulator under the QueryId and complete it when the
            // query terminates (see the kad arm in run_swarm).
            let query_id = swarm
                .behaviour_mut()
                .kademlia
                .get_providers(kad::RecordKey::new(&key));
            pending_queries.insert(query_id, (reply, HashSet::new()));
        }
        Command::RequestEvents { peer, ids, reply } => {
            // send_request returns immediately with an OutboundRequestId; the reply Sender is
            // parked in pending_outbound and completed when the matching response event arrives
            // (or OutboundFailure errors it). This is the await-able half of anti-entropy.
            match peer_id_from_bytes(&peer) {
                Ok(peer_id) => {
                    let request_id = swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer_id, EventRequest { ids });
                    pending_outbound.insert(request_id, reply);
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
        }
        Command::RespondEvents { channel, events } => {
            // send_response returns the response back if the channel is closed (peer gone); drop.
            // TODO(libp2p-verify): consider surfacing this as a transport-level warn metric.
            let _ = swarm
                .behaviour_mut()
                .request_response
                .send_response(channel, EventResponse { events });
        }
    }
}

fn peer_id_from_bytes(bytes: &[u8]) -> Result<PeerId> {
    PeerId::from_bytes(bytes).map_err(|e| Error::Transport(format!("bad peer id: {e}")))
}
