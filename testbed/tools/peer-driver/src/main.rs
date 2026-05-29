//! Testbed driver — inject + observe events against a Creda peer's gRPC TCP endpoint.
//!
//! Subcommands:
//!   inject  — construct a synthetic test-data Assert payload and call CreateEvent on the target
//!             peer. Prints the resulting event id (hex UUID) on stdout. The local peer is the
//!             author; its institutional signing key must be in the network's participant
//!             registry for other peers to admit the event during gossip ingest.
//!   observe — poll GetEvent on the target peer until the given event id is present or the
//!             timeout expires. Prints the latency in milliseconds on stdout.
//!
//! Wire format mirrors the Bridge / CLI: payload bytes are canonical CBOR (creda-events).

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use creda_events::{
    canonical, AdministrativeGender, Demographics, EventPayload, StructuredAddress,
    TokenizedDate, TokenizedString, VerificationMethod,
};

mod pb {
    tonic::include_proto!("creda");
}
use pb::creda_client::CredaClient;

#[derive(Parser)]
#[command(name = "peer-driver", about = "Creda testbed driver")]
struct Cli {
    /// Peer gRPC endpoint, e.g. `http://localhost:50051`. Plaintext only — the testbed runs in a
    /// trusted local network. Not required for `derive-pubkey`.
    #[arg(long, env = "PEER_DRIVER_PEER", global = true)]
    peer: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inject a synthetic Assert event into the target peer. Prints the resulting event id.
    Inject {
        /// Token tag for the synthetic patient (so different scenario runs don't collide).
        #[arg(long, default_value = "smoke")]
        tag: String,
    },
    /// Poll GetEvent on the target peer until the given event id is present.
    Observe {
        /// Event id to look for (hex UUID, e.g. `0190a3c4...`).
        #[arg(long)]
        event_id: String,
        /// Max wait in milliseconds before giving up.
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        /// Poll interval in milliseconds.
        #[arg(long, default_value_t = 100)]
        poll_ms: u64,
    },
    /// Derive the Ed25519 public key from a 32-byte secret file and print it in
    /// participant-registry format (`ed25519 <hex>`). Used by the scenario script to populate
    /// the shared participants ConfigMap.
    DerivePubkey {
        /// Path to a file containing exactly 32 bytes of secret material.
        #[arg(long)]
        secret_file: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // derive-pubkey doesn't need a gRPC connection.
    if let Command::DerivePubkey { secret_file } = &cli.command {
        return derive_pubkey(secret_file);
    }

    let peer = cli
        .peer
        .as_ref()
        .ok_or_else(|| anyhow!("--peer is required for this subcommand"))?;
    let mut client = CredaClient::connect(peer.clone())
        .await
        .with_context(|| format!("connecting to {peer}"))?;

    match cli.command {
        Command::Inject { tag } => inject(&mut client, &tag).await,
        Command::Observe { event_id, timeout_ms, poll_ms } => {
            observe(&mut client, &event_id, timeout_ms, poll_ms).await
        }
        Command::DerivePubkey { .. } => unreachable!("handled above"),
    }
}

fn derive_pubkey(secret_file: &str) -> Result<()> {
    let secret = std::fs::read(secret_file)
        .with_context(|| format!("reading {secret_file}"))?;
    let key = creda_events::SigningKey::ed25519_from_secret_bytes(&secret)
        .context("loading Ed25519 secret")?;
    let pubkey = key.verifying_key().public_key_bytes();
    let hex: String = pubkey.iter().map(|b| format!("{b:02x}")).collect();
    println!("ed25519 {hex}");
    Ok(())
}

async fn inject(
    client: &mut CredaClient<tonic::transport::Channel>,
    tag: &str,
) -> Result<()> {
    let payload = synthetic_assert(tag);
    let payload_cbor = canonical::to_vec(&payload).context("serialize EventPayload")?;
    let req = pb::CreateEventRequest {
        event_payload_cbor: payload_cbor,
        parent_ids: Vec::new(),
    };
    let reply = client
        .create_event(req)
        .await
        .context("CreateEvent RPC")?
        .into_inner();

    // Decode the IdentityEventNode CBOR enough to pull the event id out. We only need the first
    // map entry, so use the full decode for simplicity.
    let node: creda_events::IdentityEventNode =
        canonical::from_slice(&reply.event_cbor).context("decode reply event")?;

    // Print the event id as the standard UUID hyphenated form so scripts can pass it back into
    // observe.
    println!("{}", node.id);
    Ok(())
}

async fn observe(
    client: &mut CredaClient<tonic::transport::Channel>,
    event_id_str: &str,
    timeout_ms: u64,
    poll_ms: u64,
) -> Result<()> {
    let event_id = uuid_to_bytes(event_id_str)?;
    let start = Instant::now();
    let deadline = start + Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(poll_ms);

    loop {
        let reply = client
            .get_event(pb::GetEventRequest { id: event_id.clone() })
            .await
            .context("GetEvent RPC")?
            .into_inner();
        if reply.found {
            let latency_ms = start.elapsed().as_millis();
            println!("{latency_ms}");
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out after {timeout_ms} ms — event {event_id_str} did not propagate to peer"
            );
        }
        tokio::time::sleep(poll).await;
    }
}

/// Build a minimal, valid test-data Assert payload. The exact fields don't matter for the smoke
/// test — what matters is that the payload encodes, signs, and matches the validation rules.
fn synthetic_assert(tag: &str) -> EventPayload {
    let tagged = |s: &str| TokenizedString(format!("tok:{tag}:{s}"));
    EventPayload::Assert {
        demographics: Demographics {
            name_family: Some(vec![tagged("smoke-family")]),
            name_given: Some(vec![tagged("smoke-given")]),
            date_of_birth: Some(TokenizedDate(format!("tok:{tag}:1970-01-01"))),
            sex: Some(AdministrativeGender::Other),
            address: Some(StructuredAddress {
                city: Some(tagged("smoke-city")),
                state: Some(tagged("smoke-state")),
                ..Default::default()
            }),
            ..Default::default()
        },
        verification_method: VerificationMethod::SelfReport,
    }
}

fn uuid_to_bytes(s: &str) -> Result<Vec<u8>> {
    let parsed = creda_events::EventId::parse_str(s)
        .map_err(|e| anyhow!("invalid UUID {s:?}: {e}"))?;
    Ok(parsed.as_bytes().to_vec())
}
