//! The `creda` binary — daemon + CLI modes (spec §10.1.1).
//!
//! The same binary serves both: `serve` runs the long-lived peer (gRPC + networking; requires
//! the `grpc`/`libp2p` features), while `init` / `snapshot` / `inspect` are one-shot
//! administrative operations usable in the default build.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use creda_core::{CredaConfig, CredaCore, InMemorySigner, Result};
use creda_store::RocksdbStore;

#[derive(Parser)]
#[command(name = "creda", version, about = "Creda peer daemon and CLI (spec §10.1)")]
struct Cli {
    /// Path to a TOML configuration file (overlays the baked-in defaults; env vars overlay it).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a signing key and write a default configuration (§10.1.1).
    Init,
    /// Force a snapshot of the local event store (§6.2.5 / §10.1.1).
    Snapshot {
        /// Output path; defaults to `<data_dir>/snapshot.cbor`.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Inspect a single event by UUID (debug, §10.1.1).
    Inspect {
        /// The event UUID.
        uuid: String,
    },
    /// Run the long-lived peer daemon (gRPC API + networking). Requires the `grpc` feature.
    Serve,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(cli.config.as_deref())?;

    match cli.command {
        Command::Init => cmd_init(&config),
        Command::Snapshot { out } => cmd_snapshot(&config, out),
        Command::Inspect { uuid } => cmd_inspect(&config, &uuid),
        Command::Serve => cmd_serve(config),
    }
}

/// Resolve config: defaults → TOML file (if given) → env, then validate (fail-loud, §10.1.6).
fn load_config(path: Option<&std::path::Path>) -> Result<CredaConfig> {
    let toml = match path {
        Some(p) => Some(std::fs::read_to_string(p)?),
        None => None,
    };
    let config = CredaConfig::load(toml.as_deref())?;
    config.validate()?;
    Ok(config)
}

fn cmd_init(config: &CredaConfig) -> Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;
    let config_path = std::path::Path::new(&config.data_dir).join("config.toml");
    if !config_path.exists() {
        let toml = toml::to_string_pretty(config)
            .map_err(|e| creda_core::Error::Config(format!("serialize config: {e}")))?;
        std::fs::write(&config_path, toml)?;
    }
    let signer = InMemorySigner::generate()?;
    // NOTE: private-key persistence (k8s Secret / HSM, §10.1.4) is a deployment step and is not
    // written to disk here.
    println!("Initialized Creda peer.");
    println!("  data_dir:        {}", config.data_dir);
    println!("  config:          {}", config_path.display());
    println!("  institution_id:  {}", hex(signer.institution_id().as_bytes()));
    Ok(())
}

fn cmd_snapshot(config: &CredaConfig, out: Option<PathBuf>) -> Result<()> {
    let core = open_core(config)?;
    let bytes = core.snapshot_bytes()?;
    let path = out.unwrap_or_else(|| std::path::Path::new(&config.data_dir).join("snapshot.cbor"));
    std::fs::write(&path, &bytes)?;
    println!("Wrote snapshot ({} bytes) to {}", bytes.len(), path.display());
    Ok(())
}

fn cmd_inspect(config: &CredaConfig, uuid: &str) -> Result<()> {
    let id = creda_events::EventId::parse_str(uuid)
        .map_err(|e| creda_core::Error::Config(format!("invalid UUID {uuid:?}: {e}")))?;
    let core = open_core(config)?;
    match core.get_event(&id)? {
        Some(node) => println!("{node:#?}"),
        None => println!("event {uuid} not found in local store"),
    }
    Ok(())
}

fn cmd_serve(config: CredaConfig) -> Result<()> {
    #[cfg(feature = "grpc")]
    {
        creda_core::grpc::serve(config)
    }
    #[cfg(not(feature = "grpc"))]
    {
        let _ = config;
        eprintln!(
            "`creda serve` requires the gRPC feature.\n\
             Rebuild with: cargo build -p creda-core --features grpc[,libp2p]"
        );
        std::process::exit(2);
    }
}

/// Open the engine over a RocksDB store at the configured data dir. CLI read/write ops use an
/// ephemeral in-memory signer (creating events from the CLI is not a supported path; events come
/// from the institution's systems via the gRPC API).
fn open_core(config: &CredaConfig) -> Result<CredaCore> {
    let store = RocksdbStore::open(&config.data_dir)?;
    let signer = InMemorySigner::generate()?;
    Ok(CredaCore::new(Box::new(store), Box::new(signer), config.clone()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
