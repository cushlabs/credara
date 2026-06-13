//! gRPC server adapter (feature `grpc`) — the network face of the engine (spec §10.1.3).
//!
//! A thin transport over [`crate::engine::CredaCore`]: the engine holds all the logic; this
//! module decodes requests, calls the engine, and encodes replies. Events transit as canonical-
//! CBOR bytes (see `proto/creda.proto`) to avoid mirroring the event schema in protobuf; the
//! authorization query, by contrast, is a structured proto message (it is not serde-serializable,
//! and a structured contract is friendlier for the Java Bridge).
//!
//! The daemon serves on a **Unix domain socket** (§10.1.1, §10.5.1) — the socket the FHIR Bridge
//! connects to over the shared pod volume at `/var/run/creda`. A `tcp://host:port` (or bare
//! `host:port`) value for `grpc_socket` switches to TCP for local development. The engine is
//! synchronous, so each call is dispatched on `spawn_blocking` to keep the async runtime
//! unblocked (§10.1.5). It compiles only with `--features grpc` (and `protoc` present at build).

// `tonic::Status` is large (~176 bytes), and the generated gRPC service contract mandates
// `Result<_, Status>` on every method — so the helper functions that feed those methods return it
// too. Boxing it would fight the framework at every call site; allow the lint for this module.
#![allow(clippy::result_large_err)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tonic::transport::Server;
use tonic::{Request, Response, Status};

use creda_events::{
    canonical, CertificateFingerprint, EventId, EventPayload, GrantPurpose, IdentityEventNode,
    IdentityEventType, UseMode,
};
use creda_graph::{AuthorizationQuery, FieldKey, RequesterContext};
use creda_store::RocksdbStore;

use crate::config::CredaConfig;
use crate::engine::CredaCore;
use crate::error::{Error, Result};
use crate::signer::InMemorySigner;

/// Generated protobuf types and service traits.
pub mod pb {
    tonic::include_proto!("creda");
}

use pb::creda_server::{Creda, CredaServer};
use pb::{
    AuthReply, AuthRequest, CreateEventRequest, EffectiveIdentityReply, Empty, EntryPoints,
    EventReply, GetEventReply, GetEventRequest, InstitutionsReply, MatchReply, MatchRequest,
    Metrics, SubgraphEventsReply, SubgraphEventsRequest,
};

/// Fire-and-forget hook the gRPC service invokes after a locally created event has been signed
/// and stored — implementations enqueue the event onto the outbound replication path and **must
/// not block**. The libp2p daemon wires this to the [`crate::replication::Replicator`] so that
/// events created locally actually gossip outward; without a publisher the service is read/write
/// to the engine only and replicates nothing outbound.
pub trait EventPublisher: Send + Sync {
    fn publish(&self, node: &IdentityEventNode);
}

/// The gRPC service, backed by a shared engine plus an optional outbound publisher.
pub struct CredaService {
    core: Arc<CredaCore>,
    publisher: Option<Arc<dyn EventPublisher>>,
}

fn ids_from_bytes(raw: &[Vec<u8>]) -> std::result::Result<Vec<EventId>, Status> {
    raw.iter()
        .map(|b| EventId::from_slice(b).map_err(|e| Status::invalid_argument(format!("bad id: {e}"))))
        .collect()
}

/// Run a synchronous engine call on the blocking pool and normalize errors to `Status` (§10.1.5).
async fn blocking<T, F>(f: F) -> std::result::Result<T, Status>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(Status::internal(e.to_string())),
        Err(e) => Err(Status::internal(format!("worker task failed: {e}"))),
    }
}

fn map_purpose(v: i32) -> std::result::Result<GrantPurpose, Status> {
    use pb::GrantPurpose as P;
    let p = P::try_from(v).map_err(|_| Status::invalid_argument(format!("unknown purpose {v}")))?;
    Ok(match p {
        P::Treatment => GrantPurpose::Treatment,
        P::Payment => GrantPurpose::Payment,
        P::Operations => GrantPurpose::Operations,
        P::PublicHealth => GrantPurpose::PublicHealth,
        P::Research => GrantPurpose::Research,
        P::AiTraining => GrantPurpose::AiTraining,
        P::AiInference => GrantPurpose::AiInference,
        P::FederalProgram => GrantPurpose::FederalProgram,
        // The `*_UNSPECIFIED` (value 0) variant; matched by wildcard because prost strips the
        // enum-name prefix and renames it to `Unspecified`. Fail loudly (§10.1.6).
        _ => return Err(Status::invalid_argument("purpose is required (UNSPECIFIED)")),
    })
}

fn map_use_mode(v: i32) -> std::result::Result<UseMode, Status> {
    use pb::UseMode as U;
    let u = U::try_from(v).map_err(|_| Status::invalid_argument(format!("unknown use_mode {v}")))?;
    Ok(match u {
        U::ReadOnly => UseMode::ReadOnly,
        U::ReadAndRely => UseMode::ReadAndRely,
        U::ReadAndExport => UseMode::ReadAndExport,
        // `*_UNSPECIFIED` (value 0); see note in `map_purpose`.
        _ => return Err(Status::invalid_argument("use_mode is required (UNSPECIFIED)")),
    })
}

/// Render a `FieldKey` as the kebab token the bridge/clients key on.
fn field_key_name(k: &FieldKey) -> String {
    match k {
        FieldKey::NameFamily => "name-family".to_string(),
        FieldKey::NameGiven => "name-given".to_string(),
        FieldKey::NameMiddle => "name-middle".to_string(),
        FieldKey::DateOfBirth => "date-of-birth".to_string(),
        FieldKey::Sex => "sex".to_string(),
        FieldKey::Address => "address".to_string(),
        FieldKey::SsnLastFour => "ssn-last-four".to_string(),
        FieldKey::Mrn => "mrn".to_string(),
        FieldKey::InsuranceMemberId => "insurance-member-id".to_string(),
        FieldKey::Extension(s) => format!("ext:{s}"),
    }
}

fn parse_event_type(s: &str) -> std::result::Result<IdentityEventType, Status> {
    use IdentityEventType::*;
    Ok(match s {
        "Assert" => Assert,
        "Link" => Link,
        "Contest" => Contest,
        "Attest" => Attest,
        "Amend" => Amend,
        "Tombstone" => Tombstone,
        "DeceasedDeclaration" => DeceasedDeclaration,
        "AuthorizationGrant" => AuthorizationGrant,
        "AuthorizationRevocation" => AuthorizationRevocation,
        "ExportReceipt" => ExportReceipt,
        other => return Err(Status::invalid_argument(format!("unknown event type {other:?}"))),
    })
}

#[tonic::async_trait]
impl Creda for CredaService {
    async fn create_event(
        &self,
        request: Request<CreateEventRequest>,
    ) -> std::result::Result<Response<EventReply>, Status> {
        let req = request.into_inner();
        let payload: EventPayload = canonical::from_slice(&req.event_payload_cbor)
            .map_err(|e| Status::invalid_argument(format!("bad payload: {e}")))?;
        let parents = ids_from_bytes(&req.parent_ids)?;
        let core = self.core.clone();
        let node = blocking(move || core.create_event(payload, parents)).await?;
        // Real-time activity line so `kubectl logs -f` shows events as they happen (not just
        // startup). `test=…` flags synthetic events (§11.4).
        eprintln!(
            "creda serve: created {:?} {}{}",
            node.event_type,
            node.id,
            if node.is_test_data() { " test=true" } else { "" },
        );
        // Fire-and-forget outbound publish. The publisher must not block; if absent, this peer
        // does not gossip its locally-created events (anti-entropy still backstops on testbed).
        if let Some(publisher) = &self.publisher {
            publisher.publish(&node);
        }
        let event_cbor = canonical::to_vec(&node).map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(EventReply { event_cbor }))
    }

    async fn get_event(
        &self,
        request: Request<GetEventRequest>,
    ) -> std::result::Result<Response<GetEventReply>, Status> {
        let id = EventId::from_slice(&request.into_inner().id)
            .map_err(|e| Status::invalid_argument(format!("bad id: {e}")))?;
        let core = self.core.clone();
        match blocking(move || core.get_event(&id)).await? {
            Some(node) => {
                let event_cbor =
                    canonical::to_vec(&node).map_err(|e| Status::internal(e.to_string()))?;
                Ok(Response::new(GetEventReply { found: true, event_cbor }))
            }
            None => Ok(Response::new(GetEventReply { found: false, event_cbor: Vec::new() })),
        }
    }

    /// List a subgraph's events, optionally filtered by type (§10.1.3) — the read surface behind
    /// the Bridge's `Consent?patient=` search. Sorted by logical clock for a causally-coherent
    /// order (the same convention as `$creda-provenance`, §8.2.5).
    async fn get_subgraph_events(
        &self,
        request: Request<SubgraphEventsRequest>,
    ) -> std::result::Result<Response<SubgraphEventsReply>, Status> {
        let req = request.into_inner();
        let entries = ids_from_bytes(&req.entry_points)?;
        let types = req
            .event_types
            .iter()
            .map(|s| parse_event_type(s.as_str()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let core = self.core.clone();
        let nodes = blocking(move || {
            let subgraph = core.get_subgraph(&entries)?;
            let mut nodes: Vec<IdentityEventNode> = subgraph
                .nodes()
                .filter(|n| types.is_empty() || types.contains(&n.event_type))
                .cloned()
                .collect();
            nodes.sort_by_key(|n| n.logical_clock);
            Ok(nodes)
        })
        .await?;
        let event_cbor = nodes
            .iter()
            .map(|n| canonical::to_vec(n).map_err(|e| Status::internal(e.to_string())))
            .collect::<std::result::Result<Vec<_>, Status>>()?;
        Ok(Response::new(SubgraphEventsReply { event_cbor }))
    }

    async fn get_effective_identity(
        &self,
        request: Request<EntryPoints>,
    ) -> std::result::Result<Response<EffectiveIdentityReply>, Status> {
        let entries = ids_from_bytes(&request.into_inner().ids)?;
        let core = self.core.clone();
        let ei = blocking(move || core.effective_identity(&entries)).await?;
        // Structured per-field projection (§5.2.4 / §5.3): value + confidence + supporting ids +
        // disputed flag, exactly as the engine computed it (attestation amplification included).
        let fields = ei
            .fields
            .iter()
            .map(|(key, entry)| pb::EffectiveIdentityField {
                key: field_key_name(key),
                disputed: entry.disputed,
                values: entry
                    .values
                    .iter()
                    .map(|v| pb::EffectiveIdentityValue {
                        value: v.value.clone(),
                        confidence: u32::from(v.confidence),
                        supporting: v.supporting.iter().map(|id| id.as_bytes().to_vec()).collect(),
                    })
                    .collect(),
            })
            .collect();
        Ok(Response::new(EffectiveIdentityReply {
            effective_identity_debug: format!("{ei:#?}"),
            fields,
        }))
    }

    async fn match_by_tokens(
        &self,
        request: Request<MatchRequest>,
    ) -> std::result::Result<Response<MatchReply>, Status> {
        let tokens = request.into_inner().tokens;
        let core = self.core.clone();
        let ids = blocking(move || core.match_by_tokens(&tokens)).await?;
        Ok(Response::new(MatchReply {
            ids: ids.iter().map(|id| id.as_bytes().to_vec()).collect(),
        }))
    }

    async fn evaluate_authorization(
        &self,
        request: Request<AuthRequest>,
    ) -> std::result::Result<Response<AuthReply>, Status> {
        let req = request.into_inner();
        let entries = ids_from_bytes(&req.entry_points)?;
        let rc = req
            .requester
            .ok_or_else(|| Status::invalid_argument("requester is required"))?;
        let requester = RequesterContext {
            fingerprint: CertificateFingerprint::new(rc.fingerprint),
            classes: rc.classes,
            wildcards: rc.wildcards,
        };
        let purpose = map_purpose(req.purpose)?;
        let use_mode = map_use_mode(req.use_mode)?;
        let requested_event_types = req
            .requested_event_types
            .iter()
            .map(|s| parse_event_type(s.as_str()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let requested_segments = ids_from_bytes(&req.requested_segments)?;
        let query = AuthorizationQuery {
            requester,
            purpose,
            use_mode,
            requested_event_types,
            requested_segments,
            requested_data_categories: req.requested_data_categories,
        };

        let core = self.core.clone();
        let decision = blocking(move || core.evaluate_authorization(&entries, &query)).await?;
        Ok(Response::new(AuthReply {
            authorized: decision.authorized,
            covering_grants: decision
                .covering_grants
                .iter()
                .map(|id| id.as_bytes().to_vec())
                .collect(),
            reason: decision.reason,
        }))
    }

    async fn get_metrics(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<Metrics>, Status> {
        let core = self.core.clone();
        let event_count = blocking(move || core.event_count()).await? as u64;
        Ok(Response::new(Metrics { event_count }))
    }

    async fn list_institutions(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<InstitutionsReply>, Status> {
        let core = self.core.clone();
        let names = blocking(move || core.list_institutions()).await?;
        Ok(Response::new(InstitutionsReply { names }))
    }
}

/// Where to listen: a Unix domain socket path (the default, §10.5.1) or a TCP address (dev).
#[derive(Debug, PartialEq, Eq)]
enum Endpoint {
    Tcp(SocketAddr),
    Uds(PathBuf),
}

/// Interpret a `grpc_socket` value. Explicit `tcp://` / `unix://` schemes win; otherwise a
/// value that parses as a socket address is TCP, and anything else is treated as a UDS path.
fn parse_endpoint(s: &str) -> Endpoint {
    if let Some(rest) = s.strip_prefix("tcp://") {
        if let Ok(addr) = rest.parse::<SocketAddr>() {
            return Endpoint::Tcp(addr);
        }
    }
    if let Some(rest) = s.strip_prefix("unix://") {
        return Endpoint::Uds(PathBuf::from(rest));
    }
    if let Ok(addr) = s.parse::<SocketAddr>() {
        return Endpoint::Tcp(addr);
    }
    Endpoint::Uds(PathBuf::from(s))
}

/// Bind a Unix domain socket: create the parent directory, remove any stale socket left by a
/// previous run, bind, and restrict permissions to owner+group (the Bridge shares the pod's
/// `fsGroup`, §10.5.1).
fn bind_uds(path: &Path) -> std::io::Result<tokio::net::UnixListener> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    }
    Ok(listener)
}

/// Resolve until a shutdown signal (SIGINT or, on unix, SIGTERM) is received.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}

/// Serve the gRPC API for a prebuilt engine on `socket`, shutting down when `shutdown` resolves.
/// Factored out of [`serve`] so tests can drive it with an in-memory engine and an explicit
/// shutdown trigger.
pub async fn serve_with_core<S>(
    core: Arc<CredaCore>,
    socket: &str,
    shutdown: S,
    publisher: Option<Arc<dyn EventPublisher>>,
) -> Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    let service = CredaService { core, publisher };
    let router = Server::builder().add_service(CredaServer::new(service));

    match parse_endpoint(socket) {
        Endpoint::Tcp(addr) => {
            router
                .serve_with_shutdown(addr, shutdown)
                .await
                .map_err(|e| Error::Io(format!("gRPC TCP serve failed: {e}")))?;
        }
        Endpoint::Uds(path) => {
            let listener = bind_uds(&path).map_err(|e| {
                Error::Io(format!("binding gRPC socket {}: {e}", path.display()))
            })?;
            let incoming = tokio_stream::wrappers::UnixListenerStream::new(listener);
            let result = router.serve_with_incoming_shutdown(incoming, shutdown).await;
            // Best-effort cleanup so the next start does not trip over a stale socket.
            let _ = std::fs::remove_file(&path);
            result.map_err(|e| Error::Io(format!("gRPC UDS serve failed: {e}")))?;
        }
    }
    Ok(())
}

/// Run the gRPC daemon: open the store, build the engine, and serve on the configured socket
/// until SIGINT/SIGTERM. Builds its own tokio runtime so the default (sync) `main` need not
/// depend on tokio.
pub fn serve(config: CredaConfig) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Io(e.to_string()))?;

    runtime.block_on(async move {
        let store = RocksdbStore::open(&config.data_dir)?;
        // Load the institutional signing key (§10.1.4): from the configured Secret/file path if
        // set, otherwise generate an ephemeral one. The ephemeral fallback is useful for one-off
        // dev runs but fatal in production (institution_id would change per restart, breaking
        // other peers' trust); we log loudly when it's hit.
        let signer = match config.signing_key_path.as_deref() {
            Some(path) => {
                eprintln!("creda serve: loading Ed25519 signing key from {path}");
                InMemorySigner::from_ed25519_secret_file(path).map_err(|e| {
                    Error::Config(format!("loading signing key from {path}: {e}"))
                })?
            }
            None => {
                eprintln!(
                    "creda serve: WARNING — no signing_key_path configured; generating an \
                     ephemeral key. The institution_id will change on every restart. Set \
                     CREDA_SIGNING_KEY_PATH (or [signing_key_path] in config) to a Secret-mounted \
                     file in production."
                );
                InMemorySigner::generate()?
            }
        };
        let core = Arc::new(CredaCore::new(Box::new(store), Box::new(signer), config.clone()));
        eprintln!(
            "creda serve: engine ready (events={}); listening on {}",
            core.event_count().unwrap_or(0),
            config.grpc_socket
        );

        // Health endpoint (§10.5.3). Spawned before the libp2p block so /livez goes green
        // immediately and Kubernetes won't restart the pod during a slow libp2p init. /readyz
        // stays 503 until the `ready` flag is flipped at the bottom of startup.
        let ready = crate::health::ReadyFlag::new();
        {
            let ready_for_health = ready.clone();
            let core_for_health = core.clone();
            let health_listen = config.health_listen.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::health::serve_health(&health_listen, ready_for_health, core_for_health)
                        .await
                {
                    eprintln!("creda serve: health endpoint exited: {e}");
                }
            });
        }

        // Outbound publisher (set under libp2p; absent otherwise — without it, locally created
        // events are still signed and stored, just not gossiped).
        let publisher: Option<Arc<dyn EventPublisher>>;

        // Start P2P replication when libp2p is built in: bring up the swarm, subscribe to the
        // configured buckets, pump received gossip batches into the engine's ingest gate, and
        // wire a publish-on-create channel that drains into Replicator::publish_event.
        #[cfg(feature = "libp2p")]
        {
            // Bring the NetworkTransport trait into scope so the AE scheduler can call
            // `connected_peers()` on the concrete Libp2pTransport behind the Replicator.
            use creda_net::NetworkTransport;

            // The transport asks this back when a peer sends a PeerRequest (events fetch or
            // manifest), so the swarm can answer from the local store (anti-entropy +
            // targeted-fetch transfer step, §6.1.5, §6.1.8). Held as Arc<dyn EventSource>;
            // called on spawn_blocking inside the adapter.
            let event_source: Arc<dyn creda_net::EventSource> =
                Arc::new(CoreEventSource { core: core.clone() });
            let (transport, mut inbound) =
                creda_net::Libp2pTransport::generate_and_spawn(
                    &config.libp2p_listen,
                    config.bootstrap_peers.clone(),
                    event_source,
                )
                .await?;
            // Resolve event-author keys from the configured participant registry (§3.6). The
            // registry's *source* (UDAP/TEFCA sync, cert-chain validation, rotation) is an open
            // question (App C); an empty registry means no participants are admitted yet, so
            // received events are refused at the signature gate.
            let registry = match &config.participant_registry {
                Some(dir) => crate::registry::KeyRegistry::load_dir(dir)?,
                None => crate::registry::KeyRegistry::new(),
            };
            if registry.is_empty() {
                eprintln!(
                    "creda serve: WARNING — participant registry is empty (set participant_registry); \
                     received events are refused at the signature gate until participants are admitted."
                );
            } else {
                eprintln!("creda serve: participant registry loaded ({} admitted)", registry.len());
            }
            let resolver: Arc<dyn crate::engine::VerifyingKeyResolver> = Arc::new(registry);
            let replicator = Arc::new(crate::replication::Replicator::new(
                core.clone(),
                transport,
                resolver,
                100_000,
            ));
            // If `subscribe_all_buckets` is set (testbed convenience), expand to the full bucket
            // space so synthetic events landing in any bucket reach this peer. Otherwise honor
            // the explicit list in config.
            let bucket_list: Vec<u64> = if config.subscribe_all_buckets {
                (0..creda_net::BUCKET_COUNT).collect()
            } else {
                config.subscribed_buckets.clone()
            };
            replicator.subscribe_buckets(&bucket_list).await?;
            let repl = replicator.clone();
            tokio::spawn(async move {
                while let Some(bytes) = inbound.recv().await {
                    match repl.ingest_batch(&bytes) {
                        // Real-time activity line: surface inbound events as they arrive over
                        // gossip (quiet when a batch is all duplicates).
                        Ok(summary) => {
                            if summary.accepted > 0 || summary.rejected > 0 {
                                eprintln!(
                                    "creda serve: gossip ingest accepted={} duplicates={} rejected={}",
                                    summary.accepted, summary.duplicates, summary.rejected,
                                );
                            }
                        }
                        Err(e) => eprintln!("creda serve: gossip ingest error: {e}"),
                    }
                }
            });
            // Outbound publish-on-create: a bounded channel + drain task. The publisher hands
            // freshly-created events to this channel (fire-and-forget); the drain calls the
            // Replicator on them. If the channel is full or closed, we drop and log — gossip is
            // best-effort and anti-entropy backstops the loss.
            let (pub_tx, mut pub_rx) =
                tokio::sync::mpsc::channel::<IdentityEventNode>(1024);
            let repl_out = replicator.clone();
            tokio::spawn(async move {
                // "No peers subscribed" is normal for a lone peer (nothing to gossip to yet) —
                // the event is stored locally and anti-entropy backfills once a peer joins. Log it
                // ONCE rather than per event so a single-peer testbed log isn't drowned in it.
                let mut warned_no_peers = false;
                while let Some(node) = pub_rx.recv().await {
                    match repl_out.publish_event(&node).await {
                        Ok(Some(_bucket)) => {}
                        Ok(None) => eprintln!(
                            "creda serve: publish skipped — event has no routable bucket yet"
                        ),
                        Err(e) if e.to_string().contains("NoPeersSubscribedToTopic") => {
                            if !warned_no_peers {
                                warned_no_peers = true;
                                eprintln!(
                                    "creda serve: no peers subscribed to gossip yet — events are \
                                     stored locally and will sync via anti-entropy when a peer \
                                     joins (normal for a single-peer deployment; silencing repeats)"
                                );
                            }
                        }
                        Err(e) => eprintln!("creda serve: publish error: {e}"),
                    }
                }
            });
            publisher = Some(Arc::new(ChannelPublisher { tx: pub_tx }) as Arc<dyn EventPublisher>);

            // Anti-entropy peer-exchange scheduler (§6.1.8 backstop): every AE_INTERVAL_SECS,
            // ask the transport for connected peers, pick a small sample, and drive a full
            // round per peer (manifest -> reconcile -> request_events -> ingest). Gossip is
            // best-effort; this is what guarantees eventual consistency under loss/partition.
            const AE_INTERVAL_SECS: u64 = 30;
            const AE_FANOUT: usize = 3;
            let repl_ae = replicator.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(AE_INTERVAL_SECS));
                // Skip the immediate first tick; let connections settle.
                tick.tick().await;
                loop {
                    tick.tick().await;
                    let peers = match repl_ae.transport().connected_peers().await {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("creda serve: AE could not list connected peers: {e}");
                            continue;
                        }
                    };
                    for peer in peers.into_iter().take(AE_FANOUT) {
                        match repl_ae.run_anti_entropy_round(&peer).await {
                            Ok(s) => {
                                if s.accepted > 0 || s.rejected > 0 {
                                    eprintln!(
                                        "creda serve: AE round accepted={} duplicates={} rejected={}",
                                        s.accepted, s.duplicates, s.rejected
                                    );
                                }
                            }
                            Err(e) => eprintln!("creda serve: AE round error: {e}"),
                        }
                    }
                }
            });

            eprintln!(
                "creda serve: libp2p replication active (listen={}, buckets={}, AE every {AE_INTERVAL_SECS}s)",
                config.libp2p_listen,
                bucket_list.len()
            );
        }
        #[cfg(not(feature = "libp2p"))]
        {
            publisher = None;
        }

        // Startup sequence finished — flip /readyz to 200. From this point Kubernetes will
        // route traffic to this pod (rolling upgrade waits on this; §10.6.7).
        ready.set_ready();
        eprintln!("creda serve: /readyz now reports ready");

        serve_with_core(core, &config.grpc_socket, shutdown_signal(), publisher).await
    })
}

/// Backs the libp2p adapter's inbound PeerRequest answering with the engine's local store: when
/// a peer asks for events by id OR for our manifest (all event ids), the swarm asks here (on
/// `spawn_blocking`) and sends back what we have. Missing events are omitted (no "not found"),
/// matching anti-entropy semantics.
#[cfg(feature = "libp2p")]
struct CoreEventSource {
    core: Arc<CredaCore>,
}

#[cfg(feature = "libp2p")]
impl creda_net::EventSource for CoreEventSource {
    fn get_events(
        &self,
        ids: &[creda_events::EventId],
    ) -> Vec<creda_events::IdentityEventNode> {
        self.core.get_events(ids).unwrap_or_default()
    }
    fn all_event_ids(&self) -> Vec<creda_events::EventId> {
        self.core.all_event_ids().unwrap_or_default()
    }
}

/// Bounded outbound publisher: hands locally created events to the libp2p drain task without
/// blocking. A full channel drops with a warning; the anti-entropy backstop heals gossip loss.
#[cfg(feature = "libp2p")]
struct ChannelPublisher {
    tx: tokio::sync::mpsc::Sender<IdentityEventNode>,
}

#[cfg(feature = "libp2p")]
impl EventPublisher for ChannelPublisher {
    fn publish(&self, node: &IdentityEventNode) {
        if self.tx.try_send(node.clone()).is_err() {
            eprintln!(
                "creda serve: outbound publish queue full or closed; event dropped \
                 (anti-entropy will heal)"
            );
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use creda_store::MemoryStore;
    use std::time::Duration;

    #[test]
    fn parse_endpoint_distinguishes_uds_and_tcp() {
        assert_eq!(
            parse_endpoint("/run/creda/creda.sock"),
            Endpoint::Uds(PathBuf::from("/run/creda/creda.sock"))
        );
        assert_eq!(
            parse_endpoint("127.0.0.1:50051"),
            Endpoint::Tcp("127.0.0.1:50051".parse().unwrap())
        );
        assert_eq!(
            parse_endpoint("tcp://0.0.0.0:9000"),
            Endpoint::Tcp("0.0.0.0:9000".parse().unwrap())
        );
        assert_eq!(
            parse_endpoint("unix:///tmp/x.sock"),
            Endpoint::Uds(PathBuf::from("/tmp/x.sock"))
        );
    }

    #[test]
    fn enum_mapping_is_faithful() {
        assert_eq!(
            map_purpose(pb::GrantPurpose::Treatment as i32).unwrap(),
            GrantPurpose::Treatment
        );
        assert_eq!(
            map_purpose(pb::GrantPurpose::AiTraining as i32).unwrap(),
            GrantPurpose::AiTraining
        );
        assert!(map_purpose(0).is_err()); // 0 == *_UNSPECIFIED
        assert_eq!(
            map_use_mode(pb::UseMode::ReadAndRely as i32).unwrap(),
            UseMode::ReadAndRely
        );
        assert!(map_use_mode(0).is_err()); // 0 == *_UNSPECIFIED
        assert!(map_purpose(999).is_err()); // out-of-range enum value
        assert_eq!(parse_event_type("Assert").unwrap(), IdentityEventType::Assert);
        assert!(parse_event_type("Nope").is_err());
    }

    #[derive(Default)]
    struct CountingPublisher {
        calls: std::sync::atomic::AtomicUsize,
    }
    impl EventPublisher for CountingPublisher {
        fn publish(&self, _node: &IdentityEventNode) {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn publisher_is_called_on_create_event() {
        let core = Arc::new(CredaCore::new(
            Box::new(MemoryStore::new()),
            Box::new(InMemorySigner::generate().unwrap()),
            CredaConfig::default(),
        ));
        let counter: Arc<CountingPublisher> = Arc::new(CountingPublisher::default());
        let publisher: Arc<dyn EventPublisher> = counter.clone();
        let service = CredaService { core, publisher: Some(publisher) };

        let payload = creda_events::EventPayload::Assert {
            demographics: creda_events::Demographics::default(),
            verification_method: creda_events::VerificationMethod::SelfReport,
        };
        let bytes = canonical::to_vec(&payload).unwrap();
        let req = Request::new(pb::CreateEventRequest {
            event_payload_cbor: bytes,
            parent_ids: Vec::new(),
        });
        service.create_event(req).await.unwrap();

        assert_eq!(counter.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// The read path behind `Consent?patient=`: a Grant whose parent entry-point node is NOT in
    /// the store (the first-encounter / demo-patient case) must still be returned by
    /// GetSubgraphEvents — this exercises both the new RPC and the materialize fix that follows
    /// the parent→child index past absent nodes.
    #[tokio::test]
    async fn subgraph_events_finds_grant_under_absent_entry_point() {
        let core = Arc::new(CredaCore::new(
            Box::new(MemoryStore::new()),
            Box::new(InMemorySigner::generate().unwrap()),
            CredaConfig::default(),
        ));
        let service = CredaService { core, publisher: None };

        // An entry-point id with no stored node — exactly the demo-patient shape.
        let entry = creda_events::EventId::from_u128(0x00010203_0405_0607_0809_0a0b0c0d0e0f);

        let grant = creda_events::EventPayload::AuthorizationGrant {
            scope: creda_events::AuthorizationScope::default(),
            audience: creda_events::GrantAudience::InstitutionClass("any-tefca-qhin".into()),
            purpose: GrantPurpose::Treatment,
            expiration: None,
            volume_constraints: None,
            use_mode: UseMode::ReadAndRely,
        };
        let create = Request::new(pb::CreateEventRequest {
            event_payload_cbor: canonical::to_vec(&grant).unwrap(),
            parent_ids: vec![entry.as_bytes().to_vec()],
        });
        service.create_event(create).await.unwrap();

        let reply = service
            .get_subgraph_events(Request::new(pb::SubgraphEventsRequest {
                entry_points: vec![entry.as_bytes().to_vec()],
                event_types: vec!["AuthorizationGrant".into()],
            }))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(reply.event_cbor.len(), 1, "grant under an absent entry point must be found");
        let node: IdentityEventNode = canonical::from_slice(&reply.event_cbor[0]).unwrap();
        assert_eq!(node.event_type, IdentityEventType::AuthorizationGrant);
        assert_eq!(node.parent_ids, vec![entry]);
    }

    #[tokio::test]
    async fn serves_and_cleans_up_a_unix_socket() {
        let dir = std::env::temp_dir().join(format!("creda-grpc-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("creda.sock");

        let core = Arc::new(CredaCore::new(
            Box::new(MemoryStore::new()),
            Box::new(InMemorySigner::generate().unwrap()),
            CredaConfig::default(),
        ));
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let sock_str = sock.to_string_lossy().to_string();
        let handle = tokio::spawn(async move {
            serve_with_core(
                core,
                &sock_str,
                async move {
                    let _ = rx.await;
                },
                None,
            )
            .await
        });

        // Wait for the socket to appear, then confirm a raw client can connect.
        let mut bound = false;
        for _ in 0..100 {
            if sock.exists() {
                bound = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(bound, "gRPC socket file should be created");
        assert!(
            tokio::net::UnixStream::connect(&sock).await.is_ok(),
            "a client should be able to connect to the gRPC UDS"
        );

        // Trigger graceful shutdown and confirm the socket is cleaned up.
        let _ = tx.send(());
        let _ = handle.await.expect("serve task joins");
        assert!(!sock.exists(), "socket should be removed on shutdown");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
