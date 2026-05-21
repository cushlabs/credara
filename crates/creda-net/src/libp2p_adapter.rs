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

use std::time::Duration;

use libp2p::futures::StreamExt;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{gossipsub, identify, kad, request_response, Multiaddr, PeerId, Swarm};
use tokio::sync::{mpsc, oneshot};

use creda_events::{EventId, IdentityEventNode};

use crate::bucketing::{topic_for_bucket, DhtKey};
use crate::error::{Error, Result};
use crate::gossip::GossipBatch;
use crate::transport::NetworkTransport;

/// The composed libp2p behaviour for a Creda peer (§6.2.1). Each field is a primitive Creda
/// assembles rather than builds.
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
    /// TODO(libp2p-verify): the `SwarmBuilder` chain, the gossipsub/kad/request_response
    /// constructors, and the executor wiring are the version-sensitive lines — reconcile against
    /// the pinned libp2p version on first build.
    pub async fn spawn(
        keypair: libp2p::identity::Keypair,
        listen_on: Multiaddr,
        bootstrap: Vec<(PeerId, Multiaddr)>,
    ) -> Result<Self> {
        let local_peer_id = PeerId::from(keypair.public());

        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new, // Noise transport (§6.2.3)
                libp2p::yamux::Config::default,
            )
            .map_err(|e| Error::Transport(format!("tcp/noise/yamux setup: {e}")))?
            .with_behaviour(|key| build_behaviour(key))
            .map_err(|e| Error::Transport(format!("behaviour setup: {e}")))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        swarm
            .listen_on(listen_on)
            .map_err(|e| Error::Transport(format!("listen: {e}")))?;
        for (peer, addr) in bootstrap {
            swarm.behaviour_mut().kademlia.add_address(&peer, addr);
        }

        let (cmd_tx, cmd_rx) = mpsc::channel(256);
        tokio::spawn(run_swarm(swarm, cmd_rx));

        Ok(Self {
            cmd_tx,
            local_peer_id: local_peer_id.to_bytes(),
        })
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
/// TODO(libp2p-verify): the `SwarmEvent`/`CredaBehaviourEvent` match arms (gossipsub messages,
/// kad query results, request_response messages) are where most version-specific reconciliation
/// happens. Inbound gossip batches and event requests are surfaced to Core via channels that
/// Core (M5) supplies; this scaffold focuses on the outbound command path.
async fn run_swarm(mut swarm: Swarm<CredaBehaviour>, mut cmd_rx: mpsc::Receiver<Command>) {
    loop {
        tokio::select! {
            command = cmd_rx.recv() => {
                let Some(command) = command else { break }; // all handles dropped
                handle_command(&mut swarm, command);
            }
            event = swarm.select_next_some() => {
                // TODO(libp2p-verify): handle SwarmEvent::Behaviour(...) — deliver received
                // gossip batches to Core's ingest path, answer EventRequests from the local
                // store, and complete pending DHT/request_response queries.
                let _ = event;
            }
        }
    }
}

fn handle_command(swarm: &mut Swarm<CredaBehaviour>, command: Command) {
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
            // TODO(libp2p-verify): get_providers is asynchronous — the providers arrive via a
            // later kad query-result event. A production impl registers `reply` keyed by the
            // QueryId and completes it when the result event fires. This scaffold starts the
            // query and returns an empty set immediately.
            swarm
                .behaviour_mut()
                .kademlia
                .get_providers(kad::RecordKey::new(&key));
            let _ = reply.send(Ok(Vec::new()));
        }
        Command::RequestEvents { peer, ids, reply } => {
            // TODO(libp2p-verify): like the DHT query, request_response is async — the response
            // arrives as a later behaviour event. A production impl correlates by OutboundRequestId.
            match peer_id_from_bytes(&peer) {
                Ok(peer_id) => {
                    swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer_id, EventRequest { ids });
                    let _ = reply.send(Ok(Vec::new()));
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
        }
    }
}

fn peer_id_from_bytes(bytes: &[u8]) -> Result<PeerId> {
    PeerId::from_bytes(bytes).map_err(|e| Error::Transport(format!("bad peer id: {e}")))
}
