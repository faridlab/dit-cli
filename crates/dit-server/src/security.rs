//! The three guards in front of every request: Host validation, token
//! auth, and the security headers. See `tests/security.rs` for the
//! behavior each one is pinned to.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::state::{host_hostname, AppState};

/// Refuse any request whose `Host` header is not a local name.
///
/// A page at `evil.com` can make the browser request
/// `http://127.0.0.1:7700/api/...` — same-origin policy does not stop the
/// *request*, only the reading of the response, and DNS rebinding makes
/// even that boundary negotiable. The one thing the attacker cannot forge
/// is the `Host` header the browser itself sets, so it is the gate.
pub async fn require_local_host(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let Some(host) = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
    else {
        return forbidden("no Host header");
    };
    let hostname = host_hostname(host);
    if state.allowed_hosts.contains(&hostname) || is_private_ip(&hostname) {
        next.run(req).await
    } else {
        forbidden("this server only answers to local hostnames")
    }
}

/// A `Host` that is a literal private-network address. When the server is
/// opened to the LAN there is no way to enumerate the addresses it will be
/// reached by, so those are accepted wholesale — safe because a DNS
/// rebinding attack sends a *domain* in the `Host` header (the browser
/// echoes the URL's hostname, never its resolved address), so an IP literal
/// means the URL itself named that IP.
fn is_private_ip(hostname: &str) -> bool {
    let unbracketed = hostname.trim_start_matches('[').trim_end_matches(']');
    let fields: Vec<&str> = unbracketed.split('.').collect();
    if fields.len() == 4
        && fields
            .iter()
            .all(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
    {
        let octets: Vec<u16> = fields.iter().filter_map(|f| f.parse().ok()).collect();
        if octets.len() == 4 {
            let [a, b, _, _] = [octets[0], octets[1], octets[2], octets[3]];
            return matches!(a, 10 | 127)
                || (a == 192 && b == 168)
                || (a == 172 && (16..=31).contains(&b));
        }
    }
    // IPv6: unique-local fc00::/7 and link-local fe80::/10.
    let hex = unbracketed.split(':').find(|s| !s.is_empty()).unwrap_or("");
    if let Ok(first) = u16::from_str_radix(hex, 16) {
        return (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80;
    }
    // Addresses written without a leading hextet ("::1", "::").
    unbracketed == "::" || unbracketed == "::1"
}

/// Bearer-token auth for `/api`. `/api/events` authenticates from the query
/// string instead of a header, because a browser cannot set headers on a
/// WebSocket handshake — and the check must happen here rather than in the
/// handler, so that a request without upgrade headers still gets a real 401
/// instead of the extractor's protocol error.
pub async fn require_token(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    // Paths outside /api are the UI shell: static files with no workspace
    // data in them, and the very page the token prompt lives on — they
    // cannot be asked to prove a token first.
    if !req.uri().path().starts_with("/api/") {
        return next.run(req).await;
    }
    if req.uri().path() == "/api/events" {
        return match query_token(req.uri().query()) {
            Some(token) if token == state.token => next.run(req).await,
            _ => unauthorized(),
        };
    }
    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    match provided {
        Some(token) if token == state.token => next.run(req).await,
        // Deliberately one message for "missing" and "wrong": which one it
        // is makes no difference to a legitimate client, and telling an
        // attacker their guess was close buys them an oracle.
        _ => unauthorized(),
    }
}

/// The token from a query string, if there is one. The token is hex, so a
/// plain split on `=` and `&` is exact — no percent-decoding needed.
fn query_token(query: Option<&str>) -> Option<&str> {
    let query = query?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .filter(|t| !t.is_empty())
}

pub fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
        axum::Json(json!({ "error": "a valid bearer token is required" })),
    )
        .into_response()
}

fn forbidden(reason: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(json!({ "error": reason })),
    )
        .into_response()
}

/// The CSP and hardening headers stamped on every response, static assets
/// included. `default-src 'none'` and then only what the app actually
/// needs; scripts are `'self'` (bundled, hashed files) and nothing inline.
/// `'wasm-unsafe-eval'` is the narrowest token that lets the editor's Rust
/// bridge compile its `.wasm` — it permits WebAssembly compilation only,
/// never JavaScript `eval`, and browsers without wasm support ignore it.
pub async fn security_headers(req: Request<Body>, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; \
             style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; font-src 'self'; connect-src 'self'; \
             base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}
