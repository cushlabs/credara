//! Testbed driver — inject + observe events against a Creda peer's gRPC TCP endpoint.
//!
//! Subcommands:
//!   inject        — construct a synthetic test-data Assert payload and call CreateEvent on the
//!             target peer. Prints the resulting event id (hex UUID) on stdout. The local peer is
//!             the author; its institutional signing key must be in the network's participant
//!             registry for other peers to admit the event during gossip ingest.
//!   inject-grant  — create an AuthorizationGrant covering a subject subgraph (§4.3.1), parented to
//!             the subject's entry-point Assert. Prints the grant event id. Used by the
//!             revocation-latency scenario.
//!   inject-revoke — create an AuthorizationRevocation superseding a prior Grant (§4.3.2), parented
//!             to that Grant so a peer holding the Grant validates it on arrival (§4.6 step 2).
//!             Prints the revocation event id.
//!   inject-link   — create a Link fusing two subgraph heads with a given method + confidence
//!             (§5.1.1). Used by the rogue-link scenario to gossip both a weak (rogue) Link and a
//!             strong (trusted) control Link. Prints the link event id.
//!   check-authz   — call EvaluateAuthorization on the target peer for a requester/purpose/use-mode
//!             against a subgraph, and assert the decision matches an expected authorized|denied.
//!             The rogue-link scenario's verdict assertion (§4.6 step 5.5).
//!   time-revocation — inject a Revocation at `--peer` AND poll `--observe-peer` for it in one
//!             process, so t0 is the injecting RPC and t1 is when the second peer first sees it.
//!             Prints the true inject→observed propagation latency in ms, with no inter-Job gap.
//!   observe — poll GetEvent on the target peer until the given event id is present or the
//!             timeout expires. Prints the latency in milliseconds on stdout.
//!   check-absent — one-shot GetEvent that succeeds (prints "absent") if the event is NOT present
//!             and errors if it is. The isolation assertion for the partition-rejoin scenario.
//!
//! Wire format mirrors the Bridge / CLI: payload bytes are canonical CBOR (creda-events).

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use creda_events::{
    canonical, AdministrativeGender, AttestPurpose, AuthorizationScope, Demographics,
    EventPayload, GrantAudience, GrantPurpose, InstitutionalIdentifier, LinkMethod,
    StructuredAddress, TokenizedDate, TokenizedString, UseMode, VerificationMethod,
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
    /// Inject an AuthorizationGrant covering a subject subgraph (§4.3.1). Prints the grant id.
    InjectGrant {
        /// Subject subgraph entry-point event id (hex UUID), e.g. the output of `inject`. The grant
        /// is parented to this id, so it lands in that fragment's subgraph.
        #[arg(long)]
        subject: String,
        /// Audience institution class the grant covers (§4.3.1 / §4.6 step 3). A requester whose
        /// `check-authz --requester-class` matches satisfies the audience.
        #[arg(long = "audience-class", default_value = "revocation-latency")]
        audience_class: String,
    },
    /// Inject a Link fusing two subgraph heads with a method + confidence (§5.1.1). Prints the id.
    InjectLink {
        /// First subgraph head (hex UUID) — e.g. the injecting peer's own Assert.
        #[arg(long)]
        a: String,
        /// Second subgraph head (hex UUID) — e.g. the responder's real patient Assert.
        #[arg(long)]
        b: String,
        /// Link method: manual | algorithmic | referral | insurance-crosswalk | other. The method
        /// sets the confidence ceiling the responder's link-chain check caps to (§4.6 step 5.5).
        #[arg(long, default_value = "manual")]
        method: String,
        /// Match confidence in basis points (0–10000). Capped to the method ceiling on evaluation.
        #[arg(long)]
        confidence: u16,
    },
    /// Evaluate authorization on the target peer and assert the decision matches `--expect`.
    CheckAuthz {
        /// Subgraph entry-point event id(s) to evaluate against (hex UUID). Repeatable.
        #[arg(long = "entry", required = true)]
        entries: Vec<String>,
        /// Audience class the requester satisfies (§4.6 step 3). Repeatable; empty is allowed.
        #[arg(long = "requester-class")]
        requester_classes: Vec<String>,
        /// Grant purpose: treatment | payment | operations | public-health | research |
        /// ai-training | ai-inference | federal-program.
        #[arg(long, default_value = "treatment")]
        purpose: String,
        /// Use mode: read-only | read-and-rely | read-and-export.
        #[arg(long = "use-mode", default_value = "read-only")]
        use_mode: String,
        /// Expected outcome: authorized | denied. The command errors if the peer disagrees.
        #[arg(long)]
        expect: String,
    },
    /// Inject an AuthorizationRevocation superseding a prior Grant (§4.3.2). Prints the
    /// revocation id. Parented to the Grant, so a peer holding the Grant validates it on
    /// arrival (§4.6 step 2) — which is what makes the observed propagation a revocation that
    /// has *taken effect*, not merely an event that arrived.
    InjectRevoke {
        /// The Grant event id to revoke (hex UUID).
        #[arg(long)]
        grant: String,
    },
    /// Inject a Revocation at `--peer` and time its propagation to `--observe-peer`, all in this
    /// one process — so the measured window is the true inject→observed cross-peer latency, with
    /// no inter-Job scheduling gap to swallow it. Prints the latency in milliseconds.
    TimeRevocation {
        /// The Grant event id to revoke (hex UUID). Revoked at `--peer`, observed at `--observe-peer`.
        #[arg(long)]
        grant: String,
        /// Second peer's gRPC endpoint to observe propagation on (e.g. cross-namespace DNS).
        #[arg(long)]
        observe_peer: String,
        /// Max wait in milliseconds before giving up.
        #[arg(long, default_value_t = 5000)]
        timeout_ms: u64,
        /// Poll interval in milliseconds (tighter than `observe` for finer latency resolution).
        #[arg(long, default_value_t = 25)]
        poll_ms: u64,
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
    /// One-shot GetEvent: succeed (print "absent") if the event is NOT present, error if it is.
    /// The isolation assertion for partition-rejoin — proves a partitioned peer did not receive
    /// the other side's event.
    CheckAbsent {
        /// Event id that must NOT be present (hex UUID).
        #[arg(long)]
        event_id: String,
    },
    /// Derive the Ed25519 public key from a 32-byte secret file and print it in
    /// participant-registry format (`ed25519 <hex>`). Used by the scenario script to populate
    /// the shared participants ConfigMap.
    DerivePubkey {
        /// Path to a file containing exactly 32 bytes of secret material.
        #[arg(long)]
        secret_file: String,
    },
    /// Seed the demo dataset the persona clients render (Maria Gonzalez: two linked Asserts +
    /// Attest + a Mercy General grant; James Whitfield: two Asserts with conflicting DOBs +
    /// a low-confidence Link). Tokens are stable (`tok:demo:*`) so clients resolve patients via
    /// `Patient?_creda-token=` rather than hardcoded ids; event ids are fresh per seeding.
    /// Prints `name=<uuid>` lines for every created event. Used by `make -C testbed reset/seed`.
    SeedDemo,
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
        Command::InjectGrant { subject, audience_class } => {
            inject_grant(&mut client, &subject, &audience_class).await
        }
        Command::InjectLink { a, b, method, confidence } => {
            inject_link(&mut client, &a, &b, &method, confidence).await
        }
        Command::CheckAuthz { entries, requester_classes, purpose, use_mode, expect } => {
            check_authz(&mut client, &entries, &requester_classes, &purpose, &use_mode, &expect).await
        }
        Command::InjectRevoke { grant } => inject_revoke(&mut client, &grant).await,
        Command::TimeRevocation { grant, observe_peer, timeout_ms, poll_ms } => {
            time_revocation(&mut client, &observe_peer, &grant, timeout_ms, poll_ms).await
        }
        Command::Observe { event_id, timeout_ms, poll_ms } => {
            observe(&mut client, &event_id, timeout_ms, poll_ms).await
        }
        Command::CheckAbsent { event_id } => check_absent(&mut client, &event_id).await,
        Command::SeedDemo => seed_demo(&mut client).await,
        Command::DerivePubkey { .. } => unreachable!("handled above"),
    }
}

/// Create one event via CreateEvent and return its id (the peer signs with its own key).
async fn create(
    client: &mut CredaClient<tonic::transport::Channel>,
    payload: &EventPayload,
    parents: &[creda_events::EventId],
) -> Result<creda_events::EventId> {
    let req = pb::CreateEventRequest {
        event_payload_cbor: canonical::to_vec(payload).context("serialize EventPayload")?,
        parent_ids: parents.iter().map(|p| p.as_bytes().to_vec()).collect(),
    };
    let node: creda_events::IdentityEventNode = canonical::from_slice(
        &client.create_event(req).await.context("CreateEvent RPC")?.into_inner().event_cbor,
    )
    .context("decode reply event")?;
    Ok(node.id)
}

/// An Assert with stable demo tokens, so clients can find the patient with
/// `Patient?_creda-token=tok:demo:<family>` after every reseed. Carries the issuing institution's
/// MRN and a city/state address so the clinician's MRNs/address surfaces project from real data
/// (the issuing institution lives in the MRN payload, independent of the event signer).
#[allow(clippy::too_many_arguments)]
fn demo_assert(
    family: &str,
    given: &str,
    dob: &str,
    vm: VerificationMethod,
    mrn_institution: &str,
    mrn_value: &str,
    city: &str,
    state: &str,
) -> EventPayload {
    EventPayload::Assert {
        demographics: Demographics {
            name_family: Some(vec![TokenizedString(format!("tok:demo:{family}"))]),
            name_given: Some(vec![TokenizedString(format!("tok:demo:{given}"))]),
            date_of_birth: Some(TokenizedDate(format!("tok:demo:{dob}"))),
            sex: Some(AdministrativeGender::Other),
            address: Some(StructuredAddress {
                city: Some(TokenizedString(format!("tok:demo:{city}"))),
                state: Some(TokenizedString(format!("tok:demo:{state}"))),
                ..Default::default()
            }),
            mrns: vec![InstitutionalIdentifier {
                institution_id: TokenizedString(format!("tok:demo:{mrn_institution}")),
                value: TokenizedString(format!("tok:demo:{mrn_value}")),
            }],
            ..Default::default()
        },
        verification_method: vm,
    }
}

async fn seed_demo(client: &mut CredaClient<tonic::transport::Channel>) -> Result<()> {
    // ---- Maria Gonzalez: the well-linked patient with an active Mercy General grant ----------
    let m_mercy = create(
        client,
        &demo_assert(
            "gonzalez", "maria", "1984-03-12", VerificationMethod::GovernmentPhotoId,
            "Mercy General Hospital", "5582019", "Oakland", "CA",
        ),
        &[],
    )
    .await?;
    let m_north = create(
        client,
        &demo_assert(
            "gonzalez", "maria", "1984-03-12", VerificationMethod::InsuranceCard,
            "Northside Clinic", "A-7741", "Oakland", "CA",
        ),
        &[],
    )
    .await?;
    let m_link = create(
        client,
        &EventPayload::Link {
            target_subgraph_heads: (m_mercy, m_north),
            confidence_score: 9400,
            method: LinkMethod::Algorithmic,
        },
        &[m_mercy, m_north],
    )
    .await?;
    let m_attest = create(
        client,
        &EventPayload::Attest { target_event_ids: vec![m_link], purpose: AttestPurpose::Treatment },
        &[m_link],
    )
    .await?;
    let m_grant = create(
        client,
        &EventPayload::AuthorizationGrant {
            scope: AuthorizationScope::default(),
            audience: GrantAudience::InstitutionClass("Mercy General Hospital".into()),
            purpose: GrantPurpose::Treatment,
            expiration: None,
            volume_constraints: None,
            use_mode: UseMode::ReadAndRely,
        },
        &[m_mercy],
    )
    .await?;

    // ---- James Whitfield: two Asserts that DISAGREE on DOB, joined by a tentative Link --------
    // The clinician app's "resolve DOB" challenge derives from this real conflict; an Amend
    // against one of these Asserts is how a resolution persists.
    let j_mercy = create(
        client,
        &demo_assert(
            "whitfield", "james", "1971-08-04", VerificationMethod::GovernmentPhotoId,
            "Mercy General Hospital", "6610042", "Fresno", "CA",
        ),
        &[],
    )
    .await?;
    let j_lakeside = create(
        client,
        &demo_assert(
            "whitfield", "james", "1971-08-14", VerificationMethod::SelfReport,
            "Lakeside Hospital", "LH-3098", "Fresno", "CA",
        ),
        &[],
    )
    .await?;
    let j_link = create(
        client,
        &EventPayload::Link {
            target_subgraph_heads: (j_mercy, j_lakeside),
            confidence_score: 7100,
            method: LinkMethod::Algorithmic,
        },
        &[j_mercy, j_lakeside],
    )
    .await?;

    println!("maria-assert-mercy={m_mercy}");
    println!("maria-assert-northside={m_north}");
    println!("maria-link={m_link}");
    println!("maria-attest={m_attest}");
    println!("maria-grant-mercy={m_grant}");
    println!("james-assert-mercy={j_mercy}");
    println!("james-assert-lakeside={j_lakeside}");
    println!("james-link={j_link}");
    Ok(())
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

/// Inject an `AuthorizationGrant` covering `subject`'s subgraph, parented to that entry-point
/// (§4.3.1). A minimal treatment-purpose grant — enough for the revocation-latency scenario to
/// have something a later Revocation can supersede. Prints the grant event id.
async fn inject_grant(
    client: &mut CredaClient<tonic::transport::Channel>,
    subject_str: &str,
    audience_class: &str,
) -> Result<()> {
    let subject = creda_events::EventId::parse_str(subject_str)
        .map_err(|e| anyhow!("invalid subject UUID {subject_str:?}: {e}"))?;
    let payload = EventPayload::AuthorizationGrant {
        scope: AuthorizationScope::default(),
        audience: GrantAudience::InstitutionClass(audience_class.to_string()),
        purpose: GrantPurpose::Treatment,
        expiration: None,
        volume_constraints: None,
        use_mode: UseMode::ReadAndRely,
    };
    let id = create(client, &payload, &[subject]).await?;
    println!("{id}");
    Ok(())
}

/// Inject a `Link` fusing two subgraph heads (§5.1.1), parented to both so it materializes into
/// each fragment's subgraph. `method` sets the confidence ceiling the responder's link-chain check
/// caps to (§4.6 step 5.5): a `manual` link at 10000 is capped below the trust floor and cannot
/// carry authorization across institutions, while an `insurance-crosswalk` link can. Prints the id.
async fn inject_link(
    client: &mut CredaClient<tonic::transport::Channel>,
    a_str: &str,
    b_str: &str,
    method_str: &str,
    confidence: u16,
) -> Result<()> {
    let a = creda_events::EventId::parse_str(a_str)
        .map_err(|e| anyhow!("invalid head UUID {a_str:?}: {e}"))?;
    let b = creda_events::EventId::parse_str(b_str)
        .map_err(|e| anyhow!("invalid head UUID {b_str:?}: {e}"))?;
    let payload = EventPayload::Link {
        target_subgraph_heads: (a, b),
        confidence_score: confidence,
        method: parse_link_method(method_str)?,
    };
    let id = create(client, &payload, &[a, b]).await?;
    println!("{id}");
    Ok(())
}

/// Call `EvaluateAuthorization` on the target peer and assert the decision matches `expect`
/// (§4.6). Prints `<authorized|denied> (reason: ...)` on success; bails on mismatch so a scenario
/// script fails loudly. The rogue-link scenario runs this against the responder peer after gossip
/// converges: a rogue Link-reached Grant must yield `denied`, a trusted Link-reached one `authorized`.
async fn check_authz(
    client: &mut CredaClient<tonic::transport::Channel>,
    entries: &[String],
    requester_classes: &[String],
    purpose_str: &str,
    use_mode_str: &str,
    expect_str: &str,
) -> Result<()> {
    let entry_points = entries
        .iter()
        .map(|s| uuid_to_bytes(s))
        .collect::<Result<Vec<_>>>()?;
    let expect_authorized = match expect_str.to_ascii_lowercase().as_str() {
        "authorized" | "allow" | "allowed" => true,
        "denied" | "deny" => false,
        other => bail!("invalid --expect {other:?} (expected authorized|denied)"),
    };
    let req = pb::AuthRequest {
        entry_points,
        requester: Some(pb::RequesterContext {
            fingerprint: Vec::new(),
            classes: requester_classes.to_vec(),
            wildcards: Vec::new(),
        }),
        purpose: parse_purpose(purpose_str)? as i32,
        use_mode: parse_use_mode(use_mode_str)? as i32,
        requested_event_types: Vec::new(),
        requested_segments: Vec::new(),
        requested_data_categories: Vec::new(),
    };
    let reply = client
        .evaluate_authorization(req)
        .await
        .context("EvaluateAuthorization RPC")?
        .into_inner();
    let verdict = if reply.authorized { "authorized" } else { "denied" };
    if reply.authorized != expect_authorized {
        bail!(
            "authorization mismatch: expected {expect_str}, got {verdict} (reason: {})",
            reply.reason
        );
    }
    println!("{verdict} (reason: {})", reply.reason);
    Ok(())
}

fn parse_link_method(s: &str) -> Result<LinkMethod> {
    Ok(match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "manual" => LinkMethod::Manual,
        "algorithmic" => LinkMethod::Algorithmic,
        "referral" => LinkMethod::Referral,
        "insurance-crosswalk" => LinkMethod::InsuranceCrosswalk,
        "other" => LinkMethod::Other,
        other => bail!(
            "unknown link method {other:?} (expected manual|algorithmic|referral|\
             insurance-crosswalk|other)"
        ),
    })
}

fn parse_purpose(s: &str) -> Result<pb::GrantPurpose> {
    Ok(match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "treatment" => pb::GrantPurpose::Treatment,
        "payment" => pb::GrantPurpose::Payment,
        "operations" => pb::GrantPurpose::Operations,
        "public-health" => pb::GrantPurpose::PublicHealth,
        "research" => pb::GrantPurpose::Research,
        "ai-training" => pb::GrantPurpose::AiTraining,
        "ai-inference" => pb::GrantPurpose::AiInference,
        "federal-program" => pb::GrantPurpose::FederalProgram,
        other => bail!("unknown purpose {other:?}"),
    })
}

fn parse_use_mode(s: &str) -> Result<pb::UseMode> {
    Ok(match s.to_ascii_lowercase().replace('_', "-").as_str() {
        "read-only" => pb::UseMode::ReadOnly,
        "read-and-rely" => pb::UseMode::ReadAndRely,
        "read-and-export" => pb::UseMode::ReadAndExport,
        other => bail!("unknown use mode {other:?}"),
    })
}

/// Inject an `AuthorizationRevocation` that supersedes `grant`, parented to it (§4.3.2). Prints
/// the revocation event id. Because the revocation references the Grant as its parent, a peer that
/// already holds the Grant validates it on arrival (§4.6 step 2) — so the scenario's measured
/// propagation is the revocation *taking effect*, not just an opaque event landing.
async fn inject_revoke(
    client: &mut CredaClient<tonic::transport::Channel>,
    grant_str: &str,
) -> Result<()> {
    let grant = creda_events::EventId::parse_str(grant_str)
        .map_err(|e| anyhow!("invalid grant UUID {grant_str:?}: {e}"))?;
    let payload = EventPayload::AuthorizationRevocation { target_grant_id: grant };
    let id = create(client, &payload, &[grant]).await?;
    println!("{id}");
    Ok(())
}

/// Inject a Revocation at `inject_client`'s peer and poll `observe_peer` for it in one process,
/// measuring the true inject→observed propagation latency (§4.7 Bound 1). `start` is taken at the
/// injecting RPC, so the window is the real cross-peer gossip time with no inter-Job gap.
async fn time_revocation(
    inject_client: &mut CredaClient<tonic::transport::Channel>,
    observe_peer: &str,
    grant_str: &str,
    timeout_ms: u64,
    poll_ms: u64,
) -> Result<()> {
    let grant = creda_events::EventId::parse_str(grant_str)
        .map_err(|e| anyhow!("invalid grant UUID {grant_str:?}: {e}"))?;
    let mut observe_client = CredaClient::connect(observe_peer.to_string())
        .await
        .with_context(|| format!("connecting to observe peer {observe_peer}"))?;

    let payload = EventPayload::AuthorizationRevocation { target_grant_id: grant };
    let start = Instant::now();
    let revocation = create(inject_client, &payload, &[grant]).await?;

    let deadline = start + Duration::from_millis(timeout_ms);
    let poll = Duration::from_millis(poll_ms);
    let id_bytes = revocation.as_bytes().to_vec();
    loop {
        let reply = observe_client
            .get_event(pb::GetEventRequest { id: id_bytes.clone() })
            .await
            .context("GetEvent RPC (observe peer)")?
            .into_inner();
        if reply.found {
            println!("{}", start.elapsed().as_millis());
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out after {timeout_ms} ms — revocation {revocation} did not propagate to \
                 the observe peer {observe_peer}"
            );
        }
        tokio::time::sleep(poll).await;
    }
}

/// One-shot GetEvent: succeed (print "absent") when the event is NOT present; error if it is. The
/// isolation assertion for partition-rejoin — a leaked event is a real failure, so it bails.
async fn check_absent(
    client: &mut CredaClient<tonic::transport::Channel>,
    event_id_str: &str,
) -> Result<()> {
    let event_id = uuid_to_bytes(event_id_str)?;
    let reply = client
        .get_event(pb::GetEventRequest { id: event_id })
        .await
        .context("GetEvent RPC")?
        .into_inner();
    if reply.found {
        bail!("event {event_id_str} is present but was expected ABSENT (partition leaked)");
    }
    println!("absent");
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
