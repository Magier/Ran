mod kubelet_exec;
mod session;

use std::io::Write as _;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "ranplant",
    version,
    about = "Ran target-side execution implant"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve Ran's structured command protocol over stdin and stdout.
    Session(SessionArgs),
    /// Connect directly to a Ran listener and serve the structured protocol.
    Connect(ConnectArgs),
    /// Execute a command through the Kubernetes kubelet WebSocket API.
    KubeletExec(KubeletExecArgs),
}

#[derive(Debug, Args)]
struct SessionArgs {
    /// Use stdin and stdout as the transport.
    #[arg(long, default_value_t = true)]
    stdio: bool,
}

#[derive(Debug, Args)]
struct ConnectArgs {
    /// Listener host name or IP address.
    #[arg(long)]
    host: String,
    /// Listener TCP port.
    #[arg(long)]
    port: u16,
    /// Return after a child connection completes the Ranplant handshake.
    #[arg(long)]
    detach: bool,
    /// Internal child mode used by --detach.
    #[arg(long, hide = true)]
    detached_child: bool,
}

#[derive(Debug, Args)]
struct KubeletExecArgs {
    /// Full Kubernetes WebSocket exec URL.
    #[arg(long)]
    url: String,
    /// Read the bearer token from this file. If omitted, RANPLANT_TOKEN and
    /// then TOKEN are consulted.
    #[arg(long)]
    token_file: Option<PathBuf>,
    /// Trust a PEM certificate, such as the mounted Kubernetes service-account CA.
    #[arg(long)]
    ca_file: Option<PathBuf>,
    /// Disable certificate and hostname verification.
    #[arg(long)]
    insecure_skip_tls_verify: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let detached_child = matches!(
        &cli.command,
        Command::Connect(args) if args.detached_child
    );
    let result = match cli.command {
        Command::Session(args) => {
            if !args.stdio {
                Err("only stdio session transport is currently supported".to_string())
            } else {
                session::run_stdio().await
            }
        }
        Command::Connect(args) if args.detach && !args.detached_child => {
            session::spawn_detached(&args.host, args.port).await
        }
        Command::Connect(args) => {
            session::connect(&args.host, args.port, args.detached_child).await
        }
        Command::KubeletExec(args) => kubelet_exec::run(
            &args.url,
            args.token_file.as_deref(),
            args.ca_file.as_deref(),
            args.insecure_skip_tls_verify,
        ),
    };

    if let Err(error) = result {
        if detached_child {
            let _ = writeln!(std::io::stdout(), "ERROR {error}");
        } else {
            eprintln!("ranplant: {error}");
        }
        std::process::exit(1);
    }
}
