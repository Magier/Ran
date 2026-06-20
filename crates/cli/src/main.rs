use std::collections::HashMap;
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
    Trigger(TriggerArgs),
}

#[derive(Debug, Clone, Parser)]
#[command(
    about = "Execute a single TTP atomically against a target pod",
    long_about = "Executes one TTP from the armory against a specific target pod and prints \
                  the raw output together with any entities and relations discovered by the \
                  output parsers and analyzers. Use `ran armory` to browse available TTPs."
)]
struct TriggerArgs {
    /// TTP ID to execute (see `ran armory` for available IDs).
    action_id: String,

    /// Target pod entity ID in the form ns/<namespace>/pod/<name>.
    #[arg(short = 't', long = "target")]
    target_id: String,

    #[arg(long = "kubeconfig")]
    kubeconfig: Option<PathBuf>,

    #[arg(long = "armory")]
    armory: Option<PathBuf>,

    /// Path to ran.yaml config file (default: ./ran.yaml).
    #[arg(long = "config")]
    config: Option<PathBuf>,

    /// Override the execution system ID.
    #[arg(long = "exec-system")]
    exec_system_id: Option<String>,

    /// Override the procedure ID.
    #[arg(long = "procedure")]
    procedure_id: Option<String>,

    /// TTP parameter as key=value. May be repeated.
    #[arg(long = "arg", value_name = "KEY=VALUE")]
    args: Vec<String>,
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
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    init_tracing();

    let cli = Cli::parse();
    match cli.command {
        Commands::Emulate(args) => run_emulate(args).await,
        Commands::Armory(args) => run_show_armory(args),
        Commands::Trigger(args) => run_trigger(args).await,
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
        scoring: cfg.scoring,
    })
    .await
}

async fn run_trigger(args: TriggerArgs) -> Result<()> {
    let cfg = app::config::load(args.config)?;

    let mut params: HashMap<String, String> = HashMap::new();
    for kv in &args.args {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--arg '{}' is not in key=value format", kv))?;
        params.insert(k.to_string(), v.to_string());
    }

    app::trigger(app::TriggerConfig {
        kubeconfig: args.kubeconfig,
        armory_dir: args.armory,
        namespace_filter: cfg.namespaces,
        action_id: args.action_id,
        target_id: args.target_id,
        exec_system_id: args.exec_system_id,
        procedure_id: args.procedure_id,
        args: params,
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
    let tactic_w = ttps
        .iter()
        .map(|t| t.tactic.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let status_w = ttps
        .iter()
        .map(|t| t.status.len())
        .max()
        .unwrap_or(6)
        .max(6);
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
        "TTP ID",
        "Name",
        "Tactic",
        "Status",
        "Description",
        id_w = id_w,
        name_w = name_w,
        tactic_w = tactic_w,
        status_w = status_w,
        desc_w = desc_w,
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
            ttp.id,
            ttp.name,
            ttp.tactic,
            ttp.status,
            desc,
            id_w = id_w,
            name_w = name_w,
            tactic_w = tactic_w,
            status_w = status_w,
            desc_w = desc_w,
        );
    }

    println!("{sep}");
    println!("{} TTPs loaded from {}", ttps.len(), armory_dir.display());

    Ok(())
}

fn init_tracing() {
    let mut filter = EnvFilter::try_from_env("RAN_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

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

    struct HmsTimer;
    impl tracing_subscriber::fmt::time::FormatTime for HmsTimer {
        fn format_time(
            &self,
            w: &mut tracing_subscriber::fmt::format::Writer<'_>,
        ) -> std::fmt::Result {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            write!(
                w,
                "{:02}:{:02}:{:02}",
                (secs / 3600) % 24,
                (secs / 60) % 60,
                secs % 60
            )
        }
    }

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .with_timer(HmsTimer)
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
