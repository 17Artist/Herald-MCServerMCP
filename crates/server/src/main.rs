use std::path::PathBuf;

use anyhow::Context;
use herald_mcserver_auth::AuthStore;
use herald_mcserver_core::{paths, Config};

mod app;
mod error;
mod mcp;
mod middleware;
mod routes;
mod state;
mod static_assets;
mod util;
mod ws;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = parse_args();

    let config = match cli.config_path {
        Some(p) => {
            tracing::info!("loading config from {}", p.display());
            Config::load_from(&p).with_context(|| format!("load config {}", p.display()))?
        }
        None => {
            let default = std::path::PathBuf::from("./config.toml");
            if default.exists() {
                tracing::info!("loading config from {}", default.display());
                Config::load_from(&default)?
            } else {
                tracing::warn!(
                    "no --config given and ./config.toml absent — running with defaults"
                );
                Config::default()
            }
        }
    };

    let data_dir = paths::resolve_data_dir(Some(&config.server.data_dir));
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("create data dir {}", data_dir.display()))?;
    tracing::info!("data dir: {}", data_dir.display());

    let auth = AuthStore::open(&paths::auth_db(&data_dir))
        .with_context(|| "open auth.db")?;

    let listen = config.server.listen.clone();
    let state = state::AppStateInner::new(config, auth, data_dir);

    if state.is_initialized() {
        tracing::info!("auth: owner already configured");
    } else {
        tracing::warn!(
            "auth: no owner yet — open the web UI to complete first-time setup"
        );
        // 残留 setup.lock 是上一次失败留下的，清掉避免锁死。
        if state.setup_lock.exists() {
            tracing::warn!(
                "removing stale setup.lock at {}",
                state.setup_lock.display()
            );
            let _ = std::fs::remove_file(&state.setup_lock);
        }
    }

    let app = app::build(state);

    let addr: std::net::SocketAddr = listen
        .parse()
        .with_context(|| format!("parse listen address {listen}"))?;
    tracing::info!("listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .init();
}

struct Cli {
    config_path: Option<PathBuf>,
}

/// 极简 CLI 解析。够用就行，不引入 clap。
fn parse_args() -> Cli {
    let mut args = std::env::args().skip(1);
    let mut config_path = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" | "-c" => {
                config_path = args.next().map(PathBuf::from);
            }
            "--help" | "-h" => {
                eprintln!(
                    "Herald-MCServerMCP\n\nUSAGE:\n  herald-mcserver [--config PATH]\n"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    Cli { config_path }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut s) = signal(SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
