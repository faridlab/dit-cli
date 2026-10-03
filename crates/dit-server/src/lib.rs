//! The local server: the browser's window onto one workspace.
//!
//! Security is the load-bearing part of this crate, not a feature. The
//! server binds to 127.0.0.1, authenticates every `/api` request with a
//! bearer token (a browser page that was never given the token must be able
//! to learn nothing), refuses any `Host` header that is not a local name
//! (DNS rebinding), and stamps every response with a strict CSP and the
//! standard hardening headers. `tests/security.rs` pins all of it.
//!
//! The blocking facade (`Dit`) sits behind a mutex and every handler runs
//! its workspace work on the blocking thread pool — the async runtime only
//! ever waits.

pub mod config;
pub mod dto;
pub mod folder_dialog;
pub mod hub;
pub mod routes;
pub mod security;
pub mod state;

/// The router `serve` takes, so a caller need not depend on axum itself.
pub use axum::Router;
pub use folder_dialog::{system_folder_chooser, FolderChooser};
pub use hub::{hub_app, Hub, HubOptions};
pub use routes::app;
pub use state::AppState;

/// Bind `host:port` and serve until the process is stopped. `on_bound` runs
/// once the socket is live, so callers announce (or open a browser) only
/// after the port is actually theirs — never into a page that cannot load.
/// The dance lives here so `dit ui` can serve in-process without depending
/// on axum itself.
///
/// It stops on SIGINT or SIGTERM (Ctrl-C, or the menu bar app's Stop) the
/// way [`serve_until`] stops: requests in flight — a write is a commit —
/// finish first.
pub async fn serve(
    app: axum::Router,
    host: &str,
    port: u16,
    on_bound: impl FnOnce(),
) -> std::io::Result<()> {
    serve_until(
        app,
        host.to_owned(),
        port,
        on_bound,
        stop_signal(),
        STOP_GRACE,
    )
    .await
}

/// How long a stop waits for connections that do not end on their own —
/// the live-update WebSocket never does.
pub const STOP_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// Serve until `stop` resolves, then stop gracefully (ADR 0029): refuse new
/// connections, let requests in flight finish — a process killed mid-commit
/// leaves `.git/index.lock` and every later write failing — and give
/// connections that never finish `grace` before returning. Work already
/// handed to the blocking pool runs to its end either way: the runtime
/// waits for it when it is dropped.
pub async fn serve_until(
    app: axum::Router,
    host: String,
    port: u16,
    on_bound: impl FnOnce(),
    stop: impl std::future::Future<Output = ()> + Send + 'static,
    grace: std::time::Duration,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind((host.as_str(), port)).await?;
    on_bound();
    let stopping = std::sync::Arc::new(tokio::sync::Notify::new());
    let notify = stopping.clone();
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        stop.await;
        tracing::info!("stopping: finishing requests in flight");
        notify.notify_one();
    });
    tokio::select! {
        served = std::future::IntoFuture::into_future(server) => served,
        () = async {
            stopping.notified().await;
            tokio::time::sleep(grace).await;
        } => {
            tracing::info!("stopped after the grace period; open connections were closed");
            Ok(())
        }
    }
}

/// Resolves on Ctrl-C, or on SIGTERM where there is one.
pub async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
