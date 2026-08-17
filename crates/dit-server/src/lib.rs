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
pub mod routes;
pub mod security;
pub mod state;

pub use routes::app;
pub use state::AppState;

/// Bind `host:port` and serve until the process is stopped. `on_bound` runs
/// once the socket is live, so callers announce (or open a browser) only
/// after the port is actually theirs — never into a page that cannot load.
/// The dance lives here so `dit ui` can serve in-process without depending
/// on axum itself.
pub async fn serve(
    app: axum::Router,
    host: &str,
    port: u16,
    on_bound: impl FnOnce(),
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    on_bound();
    axum::serve(listener, app).await
}
