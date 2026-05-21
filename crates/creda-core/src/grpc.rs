//! gRPC server adapter (feature `grpc`) — the network face of the engine (spec §10.1.3).
//!
//! **Opt-in scaffold.** This is a thin transport over [`crate::engine::CredaCore`]: the engine
//! holds all the logic; this module decodes requests, calls the engine, and encodes replies.
//! Events transit as canonical-CBOR bytes (see `proto/creda.proto`) to avoid mirroring the event
//! schema in protobuf. It compiles only with `--features grpc` (and `protoc` present at build).
//!
//! Spots that are tonic/version-sensitive — and the Unix-socket incoming wiring in [`serve`] —
//! are marked `TODO(grpc-verify)` and must be reconciled against the pinned tonic version on
//! first build of this feature. The engine is synchronous; a production server would dispatch
//! each call via `tokio::task::spawn_blocking` to keep the async runtime unblocked (§10.1.5).

use std::sync::Arc;

use tonic::{Request, Response, Status};

use creda_events::{canonical, EventId, EventPayload};
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
        let node = self
            .core
            .create_event(payload, parents)
            .map_err(|e| Status::internal(e.to_string()))?;
        let event_cbor = canonical::to_vec(&node).map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(EventReply { event_cbor }))
    }

    async fn get_event(
        &self,
        request: Request<GetEventRequest>,
    ) -> std::result::Result<Response<GetEventReply>, Status> {
        let id = EventId::from_slice(&request.into_inner().id)
            .map_err(|e| Status::invalid_argument(format!("bad id: {e}")))?;
        match self.core.get_event(&id).map_err(|e| Status::internal(e.to_string()))? {
            Some(node) => {
                let event_cbor = canonical::to_vec(&node).map_err(|e| Status::internal(e.to_string()))?;
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
        let ei = self
            .core
            .effective_identity(&entries)
            .map_err(|e| Status::internal(e.to_string()))?;
        // TODO(grpc-verify): return a structured reply once the projection types are
        // wire-serializable; for now a debug rendering keeps the RPC functional.
        Ok(Response::new(EffectiveIdentityReply {
            effective_identity_debug: format!("{ei:#?}"),
        }))
    }

    async fn match_by_tokens(
        &self,
        request: Request<MatchRequest>,
    ) -> std::result::Result<Response<MatchReply>, Status> {
        let tokens = request.into_inner().tokens;
        let ids = self
            .core
            .match_by_tokens(&tokens)
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(MatchReply {
            ids: ids.iter().map(|id| id.as_bytes().to_vec()).collect(),
        }))
    }

    async fn evaluate_authorization(
        &self,
        _request: Request<AuthRequest>,
    ) -> std::result::Result<Response<AuthReply>, Status> {
        // TODO(grpc-verify): the wire shape for AuthorizationQuery is not yet defined (the graph
        // query type is not serde-serializable). Wiring it is a follow-up; the engine path
        // (CredaCore::evaluate_authorization) is implemented and tested directly.
        Err(Status::unimplemented(
            "EvaluateAuthorization gRPC wiring is a follow-up; use the engine API directly",
        ))
    }

    async fn get_metrics(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<Metrics>, Status> {
        let event_count = self.core.event_count().map_err(|e| Status::internal(e.to_string()))? as u64;
        Ok(Response::new(Metrics { event_count }))
    }
}

/// Run the gRPC daemon: open the store, build the engine, and serve on the configured Unix
/// socket. Builds its own tokio runtime so the default (sync) `main` need not depend on tokio.
pub fn serve(config: CredaConfig) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Io(e.to_string()))?;

    runtime.block_on(async move {
        let store = RocksdbStore::open(&config.data_dir)?;
        let signer = InMemorySigner::generate()?; // TODO: source key from k8s Secret/HSM (§10.1.4)
        let core = Arc::new(CredaCore::new(Box::new(store), Box::new(signer), config.clone()));
        let service = CredaService { core };

        // TODO(grpc-verify): serve over the Unix domain socket at `config.grpc_socket` (§10.1.1)
        // via tokio's UnixListener wrapped as an incoming stream. The exact tonic incoming-stream
        // API is version-sensitive; this scaffold logs intent. A TCP fallback can be wired for
        // local development.
        eprintln!(
            "creda serve: engine ready (events={}); gRPC socket wiring at {} is TODO(grpc-verify)",
            core_event_count(&service),
            config.grpc_socket
        );
        let _ = CredaServer::new(service); // ensures the service type is constructed/compiled
        Ok::<(), Error>(())
    })
}

fn core_event_count(service: &CredaService) -> usize {
    service.core.event_count().unwrap_or(0)
}
