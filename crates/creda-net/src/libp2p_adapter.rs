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
//! ## Status: compiles + clippy-clean against the pinned libp2p (0.56)
//!
//! A background Swarm task driven by a command channel, with [`NetworkTransport`] methods
//! translating to gossipsub/kad/request-response operations. It builds and lints cleanly against
//! libp2p 0.56, and CI's `libp2p-adapter` job (ci-rust.yml) compiles + clippies it on every push,
//! so a version bump that changes the constructor or event shapes fails there rather than silently.
//! The version-sensitive spots carry a `libp2p 0.56` note (the places to re-check on a bump). It is
//! off the default workspace build by design (see the crate docs); the live multi-peer convergence
//! tests run in the testbed.
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
/// connection-handler impls). Builds clean against libp2p 0.56.
mod behaviour {
    use libp2p::swarm::NetworkBehaviour;
    use libp2p::{gossipsub, identify, kad, request_response};

    use super::{PeerRequest, PeerResponse};

    #[derive(NetworkBehaviour)]
    pub struct CredaBehaviour {
        /// Event propagation over bucketed topics (§6.2.4).
        pub gossipsub: gossipsub::Behaviour,
        /// Subgraph routing: which peers hold a patient's events (§6.1.5).
        pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
        /// Targeted event fetch (§6.1.5) and anti-entropy manifest exchange (§6.1.8) — both
        /// carried as a CBOR-encoded `PeerRequest` ↔ `PeerResponse` pair on one codec.
        pub request_response: request_response::cbor::Behaviour<PeerRequest, PeerResponse>,
        /// Peer metadata exchange.
        pub identify: identify::Behaviour,
    }
}
pub use behaviour::{CredaBehaviour, CredaBehaviourEvent};

/// Peer-to-peer request carried over `/creda/events/1`. A single CBOR codec handles both the
/// targeted event fetch (§6.1.5 + §6.1.8 transfer step) and the manifest exchange (§6.1.8
/// reconciliation step) so we don't need a second request_response behaviour.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PeerRequest {
    /// Send back the events with these ids (any not held locally are simply omitted).
    Events { ids: Vec<EventId> },
    /// Send back every event id held locally — the responder's UUID-set manifest, used by the
    /// requester to compute the anti-entropy reconciliation delta.
    Manifest,
}

/// Response to a [`PeerRequest`].
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PeerResponse {
    Events { events: Vec<IdentityEventNode> },
    Manifest { ids: Vec<EventId> },
}

// Aliases keep the run_swarm / handle_command signatures readable and clippy quiet on
// type_complexity. The outbound map carries fetched peer responses (the callers unwrap the
// variant); the queries map carries DHT get_providers results.
type PeerReply = oneshot::Sender<Result<PeerResponse>>;
type ProviderReply = oneshot::Sender<Result<Vec<Vec<u8>>>>;
type PendingOutbound = HashMap<OutboundRequestId, PeerReply>;
type PendingQueries = HashMap<QueryId, (ProviderReply, HashSet<PeerId>)>;

/// Commands sent from [`Libp2pTransport`] handles to the background Swarm task. Keeping the
/// Swarm in one task (it is `!Sync`) and talking to it over a channel is the idiomatic way to
/// expose an ergonomic async API.
enum Command {
    PublishBatch {
        bucket: u64,
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<()>>,
    },
    Subscribe {
        bucket: u64,
        reply: oneshot::Sender<Result<()>>,
    },
    Unsubscribe {
        bucket: u64,
        reply: oneshot::Sender<Result<()>>,
    },
    DhtProvide {
        key: DhtKey,
        reply: oneshot::Sender<Result<()>>,
    },
    DhtFindProviders {
        key: DhtKey,
        reply: oneshot::Sender<Result<Vec<Vec<u8>>>>,
    },
    RequestEvents {
        peer: Vec<u8>,
        ids: Vec<EventId>,
        reply: oneshot::Sender<Result<PeerResponse>>,
    },
    /// Anti-entropy manifest fetch (§6.1.8): "give me your UUID set so I can reconcile."
    RequestManifest {
        peer: Vec<u8>,
        reply: oneshot::Sender<Result<PeerResponse>>,
    },
    /// Self-command emitted by the inbound-request task to send a response back through the
    /// swarm (where `&mut Swarm` is available).
    RespondPeer {
        channel: ResponseChannel<PeerResponse>,
        response: PeerResponse,
    },
    /// Snapshot of currently connected peer ids — for the daemon's anti-entropy round.
    ConnectedPeers {
        reply: oneshot::Sender<Result<Vec<Vec<u8>>>>,
    },
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
    /// libp2p 0.56 (version-sensitive): the `SwarmBuilder` chain, the gossipsub/kad/request_response
    /// constructors, and the executor wiring are the lines most likely to change on a libp2p bump —
    /// re-check them then (CI's `libp2p-adapter` job will flag it). Verified against 0.56.
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
                // SECURITY: pass `Config::default` as-is — do NOT call any setter
                // (e.g. `set_max_num_streams`, `set_receive_window_size`) on the
                // returned `yamux::Config`. `libp2p-yamux 0.47` carries TWO yamux
                // implementations side-by-side: yamux 0.13 (default, fixed) and
                // yamux 0.12 (vulnerable to the malformed-Data-frame panic CVE).
                // ANY mutation of the config silently flips it from the v13 path
                // to the v12 path — see libp2p-yamux `fn set` and the
                // `config_set_switches_to_v012` test. Both versions negotiate on
                // the wire as `/yamux/1.0.0`, so the choice is ours alone; a peer
                // cannot force the v12 code path. Keep this default; re-audit if
                // you ever need to tune yamux.
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

        // Print the local peer id so operators (and the testbed peer-multiaddr.sh helper) can
        // grab it without a separate API call. The format is stable: `local libp2p peer id:
        // <peer-id>` followed by the peer id; the testbed greps for `12D3KooW...`.
        eprintln!("creda-net: local libp2p peer id: {local_peer_id}");

        for (peer, addr) in bootstrap {
            eprintln!("creda-net: adding bootstrap peer {peer} at {addr}");
            swarm.behaviour_mut().kademlia.add_address(&peer, addr);
        }

        // Two command channels: `cmd_rx` carries external API calls (and closes when every
        // Libp2pTransport handle is dropped, signalling shutdown); `self_rx` carries swarm-task
        // self-commands (Command::RespondPeer from inbound-request fetch tasks). A single
        // shared channel would never close on shutdown because the task holds a clone.
        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        let (self_tx, self_rx) = mpsc::channel(64);
        let (inbound_tx, inbound_rx) = mpsc::channel(1024);
        tokio::spawn(run_swarm(
            swarm, cmd_rx, self_tx, self_rx, inbound_tx, source,
        ));

        Ok((
            Self {
                cmd_tx,
                local_peer_id: local_peer_id.to_bytes(),
            },
            inbound_rx,
        ))
    }

    /// Convenience constructor that hides libp2p types from callers (Creda Core): generate a
    /// fresh Ed25519 identity, parse the listen multiaddr, parse the bootstrap multiaddrs, and
    /// spawn. Returns the handle and the inbound gossip-batch channel.
    ///
    /// `bootstrap` entries are multiaddrs of the form `/ip4/.../tcp/.../p2p/<peer-id>` (the
    /// trailing `/p2p/<peer-id>` segment is required so the address can be installed into the
    /// Kademlia routing table). Entries that fail to parse are logged and skipped rather than
    /// aborting startup, so a stale bootstrap list can't keep the peer from coming up.
    ///
    /// TODO(peer-identity, §6.2.3): derive the libp2p identity from the institution signing key /
    /// SPIFFE SVID rather than generating a throwaway one. (Not a version concern — a real follow-up.)
    pub async fn generate_and_spawn(
        listen: &str,
        bootstrap: Vec<String>,
        source: Arc<dyn EventSource>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        let listen_on: Multiaddr = listen
            .parse()
            .map_err(|e| Error::Transport(format!("bad listen multiaddr {listen:?}: {e}")))?;
        let bootstrap = parse_bootstrap(&bootstrap);
        Self::spawn(keypair, listen_on, bootstrap, source).await
    }

    /// Like [`generate_and_spawn`](Self::generate_and_spawn), but the libp2p identity is loaded from
    /// a **stable, persistent transport key** (`ed25519_secret`, a 32-byte Ed25519 seed, typically a
    /// mounted Secret) rather than freshly generated. This makes the `PeerId` stable across restarts,
    /// which is required so the DHT routing tables and bootstrap don't churn every time the peer
    /// process cycles. The key is a dedicated **transport** credential, NOT the institution's signing
    /// key; *which institution* operates a peer is established separately, at the application layer
    /// (UDAP, §9.2), built with the cross-institution transport.
    pub async fn from_persistent_identity_and_spawn(
        ed25519_secret: [u8; 32],
        listen: &str,
        bootstrap: Vec<String>,
        source: Arc<dyn EventSource>,
    ) -> Result<(Self, mpsc::Receiver<Vec<u8>>)> {
        let keypair = libp2p::identity::Keypair::ed25519_from_bytes(ed25519_secret)
            .map_err(|e| Error::Transport(format!("libp2p identity from persistent key: {e}")))?;
        let listen_on: Multiaddr = listen
            .parse()
            .map_err(|e| Error::Transport(format!("bad listen multiaddr {listen:?}: {e}")))?;
        let bootstrap = parse_bootstrap(&bootstrap);
        Self::spawn(keypair, listen_on, bootstrap, source).await
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
        self.send(|reply| Command::PublishBatch {
            bucket,
            bytes,
            reply,
        })
        .await
    }

    async fn subscribe_bucket(&self, bucket: u64) -> Result<()> {
        self.send(|reply| Command::Subscribe { bucket, reply })
            .await
    }

    async fn unsubscribe_bucket(&self, bucket: u64) -> Result<()> {
        self.send(|reply| Command::Unsubscribe { bucket, reply })
            .await
    }

    async fn dht_provide(&self, key: DhtKey) -> Result<()> {
        self.send(|reply| Command::DhtProvide { key, reply }).await
    }

    async fn dht_find_providers(&self, key: DhtKey) -> Result<Vec<Vec<u8>>> {
        self.send(|reply| Command::DhtFindProviders { key, reply })
            .await
    }

    async fn request_events(&self, peer: &[u8], ids: &[EventId]) -> Result<Vec<IdentityEventNode>> {
        let peer = peer.to_vec();
        let ids = ids.to_vec();
        match self
            .send(|reply| Command::RequestEvents { peer, ids, reply })
            .await?
        {
            PeerResponse::Events { events } => Ok(events),
            PeerResponse::Manifest { .. } => Err(Error::Transport(
                "peer returned a manifest in response to an events request".into(),
            )),
        }
    }

    async fn request_manifest(&self, peer: &[u8]) -> Result<Vec<EventId>> {
        let peer = peer.to_vec();
        match self
            .send(|reply| Command::RequestManifest { peer, reply })
            .await?
        {
            PeerResponse::Manifest { ids } => Ok(ids),
            PeerResponse::Events { .. } => Err(Error::Transport(
                "peer returned events in response to a manifest request".into(),
            )),
        }
    }

    async fn connected_peers(&self) -> Result<Vec<Vec<u8>>> {
        self.send(|reply| Command::ConnectedPeers { reply }).await
    }

    fn local_peer_id(&self) -> Vec<u8> {
        self.local_peer_id.clone()
    }
}

/// Construct the composed behaviour.
///
/// libp2p 0.56 (version-sensitive): gossipsub/kad/request_response constructor signatures are the
/// kind of thing that shifts on a libp2p bump — re-check here then. The request/response protocol
/// uses the CBOR codec over a single protocol id; gossipsub uses the default config with message-id
/// derived from content. Verified against 0.56.
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

    let identify = identify::Behaviour::new(identify::Config::new("/creda/1".into(), key.public()));

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
/// - Inbound `PeerRequest`s (events fetch or manifest) are answered from the local store via
///   [`EventSource`]: the lookup runs on `spawn_blocking` so the swarm loop never blocks, and
///   the response is sent back through `self_tx` as `Command::RespondPeer` so `send_response`
///   happens on the swarm task with `&mut Swarm` in hand. A separate `self_rx` is drained
///   alongside `cmd_rx` (split into two channels so `cmd_rx` closes cleanly on external-handle
///   shutdown).
///
/// - Outbound Kademlia `get_providers` calls are correlated by `QueryId` via `pending_queries`,
///   with a `HashSet<PeerId>` accumulator across multi-step progress events; the awaiting Sender
///   is completed when the query terminates (`step.last`).
///
/// libp2p 0.56 (version-sensitive): the exact field names of `SwarmEvent`/`request_response::Event`/
/// `kad::Event::OutboundQueryProgressed`/`kad::QueryResult::GetProviders` can shift on a libp2p
/// bump — adjust the match arms then. The arms below are verified against 0.56.
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

                    // Inbound PeerRequest from a peer: dispatch on the variant. Events and
                    // Manifest both go through spawn_blocking + a self-command so the actual
                    // send_response runs on the swarm task with &mut Swarm in hand.
                    SwarmEvent::Behaviour(CredaBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            message: Message::Request { request, channel, .. },
                            ..
                        },
                    )) => {
                        let source = source.clone();
                        let self_tx = self_tx.clone();
                        tokio::spawn(async move {
                            let response = match request {
                                PeerRequest::Events { ids } => {
                                    let events = tokio::task::spawn_blocking(move || {
                                        source.get_events(&ids)
                                    })
                                    .await
                                    .unwrap_or_default();
                                    PeerResponse::Events { events }
                                }
                                PeerRequest::Manifest => {
                                    let ids = tokio::task::spawn_blocking(move || {
                                        source.all_event_ids()
                                    })
                                    .await
                                    .unwrap_or_default();
                                    PeerResponse::Manifest { ids }
                                }
                            };
                            let _ = self_tx
                                .send(Command::RespondPeer { channel, response })
                                .await;
                        });
                    }

                    // Outbound request completed: deliver the PeerResponse to the awaiting
                    // caller; the caller (request_events / request_manifest) unwraps the variant.
                    SwarmEvent::Behaviour(CredaBehaviourEvent::RequestResponse(
                        request_response::Event::Message {
                            message: Message::Response { request_id, response },
                            ..
                        },
                    )) => {
                        if let Some(tx) = pending_outbound.remove(&request_id) {
                            let _ = tx.send(Ok(response));
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
                    // libp2p 0.56 (version-sensitive): the exact `kad::Event::OutboundQueryProgressed`
                    // field names and the `QueryResult::GetProviders` variant shape
                    // (`Result<GetProvidersOk, GetProvidersError>`) can shift on a libp2p bump —
                    // adjust the match arms then. Verified against 0.56.
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
            // libp2p-gossipsub 0.49 changed `unsubscribe` to return `bool`
            // (true = was subscribed, false = no-op). It can no longer fail,
            // so we always reply Ok(()) — caller never inspected the bool.
            let _ = swarm.behaviour_mut().gossipsub.unsubscribe(&topic);
            let _ = reply.send(Ok(()));
        }
        Command::PublishBatch {
            bucket,
            bytes,
            reply,
        } => {
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
        Command::RequestEvents { peer, ids, reply } => match peer_id_from_bytes(&peer) {
            Ok(peer_id) => {
                let request_id = swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer_id, PeerRequest::Events { ids });
                pending_outbound.insert(request_id, reply);
            }
            Err(e) => {
                let _ = reply.send(Err(e));
            }
        },
        Command::RequestManifest { peer, reply } => match peer_id_from_bytes(&peer) {
            Ok(peer_id) => {
                let request_id = swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer_id, PeerRequest::Manifest);
                pending_outbound.insert(request_id, reply);
            }
            Err(e) => {
                let _ = reply.send(Err(e));
            }
        },
        Command::RespondPeer { channel, response } => {
            // send_response returns the response back if the channel is closed (peer gone); drop.
            let _ = swarm
                .behaviour_mut()
                .request_response
                .send_response(channel, response);
        }
        Command::ConnectedPeers { reply } => {
            let peers: Vec<Vec<u8>> = swarm.connected_peers().map(|p| p.to_bytes()).collect();
            let _ = reply.send(Ok(peers));
        }
    }
}

fn peer_id_from_bytes(bytes: &[u8]) -> Result<PeerId> {
    PeerId::from_bytes(bytes).map_err(|e| Error::Transport(format!("bad peer id: {e}")))
}

/// Parse bootstrap multiaddrs of the form `/ip4/.../tcp/.../p2p/<peer-id>` into the
/// `(PeerId, Multiaddr)` pairs Kademlia's routing table needs. The `/p2p/<peer-id>` segment is
/// required — without it we can't tell Kademlia which peer the address belongs to. Entries that
/// fail to parse are logged to stderr and skipped (a stale bootstrap list shouldn't break peer
/// startup; the peer can still come up and discover peers via gossip mesh push).
fn parse_bootstrap(addrs: &[String]) -> Vec<(PeerId, Multiaddr)> {
    let mut out = Vec::with_capacity(addrs.len());
    for s in addrs {
        match parse_one_bootstrap(s) {
            Ok(pair) => out.push(pair),
            Err(e) => eprintln!("creda-net: skipping bootstrap peer {s:?}: {e}"),
        }
    }
    out
}

fn parse_one_bootstrap(s: &str) -> std::result::Result<(PeerId, Multiaddr), String> {
    let addr: Multiaddr = s.parse().map_err(|e| format!("bad multiaddr: {e}"))?;
    // Walk the components; the last /p2p/<peer-id> is what we need. Strip it from the dial address
    // so Kademlia stores the network-level address and the peer id separately.
    let mut peer_id: Option<PeerId> = None;
    let mut dial = Multiaddr::empty();
    for proto in addr.iter() {
        if let libp2p::multiaddr::Protocol::P2p(pid) = &proto {
            peer_id = Some(*pid);
        } else {
            dial.push(proto);
        }
    }
    let peer_id = peer_id.ok_or_else(|| {
        "missing /p2p/<peer-id> suffix; expected e.g. /ip4/1.2.3.4/tcp/4001/p2p/12D3KooW..."
            .to_string()
    })?;
    Ok((peer_id, dial))
}

#[cfg(test)]
mod bootstrap_tests {
    use super::*;

    #[test]
    fn parse_strips_p2p_segment_and_extracts_peer_id() {
        // A real peer-id-shaped string from libp2p's docs.
        let s = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWGjJpdU4F3VC8AxQ7gZP4mTpRf6FY2gAaXSjB3HfGm3kp";
        let (pid, dial) = parse_one_bootstrap(s).unwrap();
        assert_eq!(
            pid.to_string(),
            "12D3KooWGjJpdU4F3VC8AxQ7gZP4mTpRf6FY2gAaXSjB3HfGm3kp"
        );
        assert_eq!(dial.to_string(), "/ip4/127.0.0.1/tcp/4001");
    }

    #[test]
    fn parse_rejects_missing_p2p_suffix() {
        let s = "/ip4/127.0.0.1/tcp/4001";
        let err = parse_one_bootstrap(s).unwrap_err();
        assert!(err.contains("missing /p2p"));
    }

    #[test]
    fn parse_rejects_bad_multiaddr() {
        assert!(parse_one_bootstrap("not-a-multiaddr").is_err());
    }

    #[test]
    fn parse_bootstrap_skips_bad_entries() {
        let inputs = vec![
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWGjJpdU4F3VC8AxQ7gZP4mTpRf6FY2gAaXSjB3HfGm3kp"
                .into(),
            "not-a-multiaddr".into(),
            "/ip4/10.0.0.5/tcp/4001".into(), // missing /p2p
        ];
        let parsed = parse_bootstrap(&inputs);
        assert_eq!(parsed.len(), 1, "only the well-formed entry survives");
    }
}
