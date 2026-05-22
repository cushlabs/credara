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
    canonical, CertificateFingerprint, EventId, EventPayload, GrantPurpose, IdentityEventType,
    UseMode,
};
use creda_graph::{AuthorizationQuery, RequesterContext};
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
    EventReply, GetEventReply, GetEventRequest, MatchReply, MatchRequest, Metrics,
};

/// The gRPC service, backed by a shared engine.
pub struct CredaService {
    core: Arc<CredaCore>,
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

    async fn get_effective_identity(
        &self,
        request: Request<EntryPoints>,
    ) -> std::result::Result<Response<EffectiveIdentityReply>, Status> {
        let entries = ids_from_bytes(&request.into_inner().ids)?;
        let core = self.core.clone();
        let ei = blocking(move || core.effective_identity(&entries)).await?;
        // TODO(grpc-structured-identity): return a structured reply once the projection types are
        // wire-serializable; for now a debug rendering keeps the RPC functional. Tracked separately
        // from the (now-wired) authorization path.
        Ok(Response::new(EffectiveIdentityReply {
            effective_identity_debug: format!("{ei:#?}"),
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
pub async fn serve_with_core<S>(core: Arc<CredaCore>, socket: &str, shutdown: S) -> Result<()>
where
    S: std::future::Future<Output = ()> + Send + 'static,
{
    let service = CredaService { core };
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
        let signer = InMemorySigner::generate()?; // TODO: source key from k8s Secret/HSM (§10.1.4)
        let core = Arc::new(CredaCore::new(Box::new(store), Box::new(signer), config.clone()));
        eprintln!(
            "creda serve: engine ready (events={}); listening on {}",
            core.event_count().unwrap_or(0),
            config.grpc_socket
        );

        // Start P2P replication when libp2p is built in: bring up the swarm, subscribe to the
        // configured buckets, and pump received gossip batches into the engine's ingest gate.
        #[cfg(feature = "libp2p")]
        {
            let (transport, mut inbound) =
                creda_net::Libp2pTransport::generate_and_spawn(&config.libp2p_listen, Vec::new())
                    .await?;
            // TODO(libp2p-verify): this resolver must be backed by the UDAP / participant registry
            // (open question, App C). Until then it resolves no keys, so received events are
            // refused — the replication data-plane is wired end to end but *inert* until key
            // resolution lands. Do not represent inbound replication as functional before then.
            let resolver: Arc<dyn crate::engine::VerifyingKeyResolver> = Arc::new(RegistryResolverTodo);
            let replicator = Arc::new(crate::replication::Replicator::new(
                core.clone(),
                transport,
                resolver,
                100_000,
            ));
            replicator.subscribe_buckets(&config.subscribed_buckets).await?;
            let repl = replicator.clone();
            tokio::spawn(async move {
                while let Some(bytes) = inbound.recv().await {
                    if let Err(e) = repl.ingest_batch(&bytes) {
                        eprintln!("creda serve: gossip ingest error: {e}");
                    }
                }
            });
            eprintln!(
                "creda serve: libp2p replication active (listen={}, buckets={})",
                config.libp2p_listen,
                config.subscribed_buckets.len()
            );
            // NOTE: outbound publish-on-create (notifying this replicator of locally created
            // events) and the anti-entropy peer-exchange loop are the remaining hooks — both
            // land with the multi-peer test bed (DQ-3), which can drive real peers.
        }

        serve_with_core(core, &config.grpc_socket, shutdown_signal()).await
    })
}

/// Placeholder verifying-key resolver used until the UDAP/participant-registry integration lands
/// (open question, App C). It resolves no keys, so every received event is refused at ingest.
#[cfg(feature = "libp2p")]
struct RegistryResolverTodo;

#[cfg(feature = "libp2p")]
impl crate::engine::VerifyingKeyResolver for RegistryResolverTodo {
    fn resolve(
        &self,
        _fingerprint: &creda_events::CertificateFingerprint,
    ) -> Option<creda_events::VerifyingKey> {
        None
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
            serve_with_core(core, &sock_str, async move {
                let _ = rx.await;
            })
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
