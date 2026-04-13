use std::path::PathBuf;

use anyhow::Result;
use armory::Armory;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[derive(Debug, Parser)]
#[command(name = "ran", about = "Ran CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Emulate(EmulateArgs),
    Armory(ArmoryArgs),
}

#[derive(Debug, Clone, Parser)]
#[command(about = "Show the contents of the armory")]
struct ArmoryArgs {
    /// Path to the armory TTPs directory (default: ./armory/TTPs).
    #[arg(long = "armory")]
    armory: Option<PathBuf>,
}

#[derive(Debug, Clone, Parser)]
struct EmulateArgs {
    #[arg(long = "kubeconfig")]
    kubeconfig: Option<PathBuf>,

    #[arg(long = "armory")]
    armory: Option<PathBuf>,

    /// Path to ran.yaml config file (default: ./ran.yaml).
    #[arg(long = "config")]
    config: Option<PathBuf>,

    #[arg(long = "namespace")]
    namespace: Option<String>,

    #[arg(short = 'p', long = "port", default_value_t = 8080)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Commands::Emulate(args) => run_emulate(args).await,
        Commands::Armory(args) => run_show_armory(args),
    }
}

async fn run_emulate(args: EmulateArgs) -> Result<()> {
    let cfg = app::config::load(args.config)?;

    if let Some(ns) = &args.namespace {
        if !ns.is_empty() {
            info!(namespace = %ns, "namespace flag passed (informational only; use ran.yaml for filtering)");
        }
    }

    app::start(app::ServerConfig {
        kubeconfig: args.kubeconfig,
        armory_dir: args.armory,
        port: args.port,
        namespace_filter: cfg.namespaces,
    })
    .await
}

fn run_show_armory(args: ArmoryArgs) -> Result<()> {
    let armory_dir = resolve_armory_dir(args.armory)?;
    let armory = Armory::load_from_dir(&armory_dir)?;
    let ttps = armory.ttps();

    // Column widths
    let id_w = ttps.iter().map(|t| t.id.len()).max().unwrap_or(6).max(6);
    let name_w = ttps.iter().map(|t| t.name.len()).max().unwrap_or(4).max(4);
    let tactic_w = ttps.iter().map(|t| t.tactic.len()).max().unwrap_or(6).max(6);
    let status_w = ttps.iter().map(|t| t.status.len()).max().unwrap_or(6).max(6);
    let desc_w = 60usize;

    let sep = format!(
        "+-{}-+-{}-+-{}-+-{}-+-{}-+",
        "-".repeat(id_w),
        "-".repeat(name_w),
        "-".repeat(tactic_w),
        "-".repeat(status_w),
        "-".repeat(desc_w),
    );

    println!("{sep}");
    println!(
        "| {:id_w$} | {:name_w$} | {:tactic_w$} | {:status_w$} | {:desc_w$} |",
        "TTP ID", "Name", "Tactic", "Status", "Description",
        id_w = id_w, name_w = name_w, tactic_w = tactic_w, status_w = status_w, desc_w = desc_w,
    );
    println!("{sep}");

    for ttp in ttps {
        let desc = if ttp.description.len() > desc_w {
            format!("{}…", &ttp.description[..desc_w - 1])
        } else {
            ttp.description.clone()
        };
        println!(
            "| {:id_w$} | {:name_w$} | {:tactic_w$} | {:status_w$} | {:desc_w$} |",
            ttp.id, ttp.name, ttp.tactic, ttp.status, desc,
            id_w = id_w, name_w = name_w, tactic_w = tactic_w, status_w = status_w, desc_w = desc_w,
        );
    }

    println!("{sep}");
    println!("{} TTPs loaded from {}", ttps.len(), armory_dir.display());

    Ok(())
}

fn init_tracing() {
    let mut filter = EnvFilter::try_from_env("RAN_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Keep Ran logs configurable via RAN_LOG while muting very chatty HTTP internals.
    // This prevents flooding from lines like "connecting to 127.0.0.1:5173".
    for directive in [
        "hyper=info",
        "hyper_util=info",
        "h2=info",
        "reqwest=info",
        "tower=info",
        "tower_http=info",
    ] {
        if let Ok(parsed) = directive.parse() {
            filter = filter.add_directive(parsed);
        }
    }

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}

fn resolve_armory_dir(arg: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = arg {
        return Ok(path);
    }
    let cwd = std::env::current_dir()?;
    Ok(cwd.join("armory").join("TTPs"))
}
