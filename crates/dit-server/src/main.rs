//! Start the local server for one workspace. This binary is the only
//! printer in the crate: it announces the URL (with the token in the
//! fragment, where it never travels to a server) and hands everything else
//! to the library.

#![allow(clippy::print_stdout, clippy::print_stderr)] // the CLI is the printer

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "dit-server",
    version,
    about = "Serve one DIT workspace to the browser"
)]
struct Args {
    /// The workspace to serve (default: the current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Interface to bind. 127.0.0.1 keeps it on this machine; anything else
    /// opens it to the network the interface sits on.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 7700)]
    port: u16,
    /// The alias writes are attributed to. Default: $DIT_ME, then $USER.
    #[arg(long)]
    me: Option<String>,
    /// Use this token instead of the one stored in the workspace cache.
    #[arg(long)]
    token: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let workspace = match &args.workspace {
        Some(path) => path.clone(),
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!("cannot determine the current directory: {e}");
                return ExitCode::FAILURE;
            }
        },
    };
    match run(args, workspace) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dit-server: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args, workspace: PathBuf) -> Result<(), String> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let dit = dit_core::Dit::open(&workspace)
        .map_err(|e| format!("`{}` is not a DIT workspace: {e}", workspace.display()))?;

    let me = args
        .me
        .or_else(|| std::env::var("DIT_ME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default();

    // The token lives next to the disposable index: inside the workspace
    // tree but gitignored, so it is never committed, and mode 600 so no
    // other account on this machine reads it.
    let token = match args.token {
        Some(token) => token,
        None => dit_server::config::load_or_create_token(&workspace.join(".dit-cache"))
            .map_err(|e| format!("cannot create the session token: {e}"))?,
    };

    let state = dit_server::AppState::with_bind_host(dit, &me, &token, &args.host);
    let app = dit_server::app(state);

    let display_host = if args.host == "0.0.0.0" {
        // 0.0.0.0 is "everywhere", not a place — print a URL that opens.
        "127.0.0.1".to_owned()
    } else {
        args.host.clone()
    };
    println!("DIT listening on http://{display_host}:{}/", args.port);
    println!("open: http://{display_host}:{}/#token={token}", args.port);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start the async runtime: {e}"))?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind((args.host.as_str(), args.port))
                .await
                .map_err(|e| {
                    format!(
                        "cannot bind {host}:{port}: {e}",
                        host = args.host,
                        port = args.port
                    )
                })?;
            axum::serve(listener, app)
                .await
                .map_err(|e| format!("server stopped: {e}"))
        })
}
