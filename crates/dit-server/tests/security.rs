//! Security tests for the local server: token auth, Host validation, and
//! the security headers. These are the paths where a bug is not "the UI
//! shows the wrong thing" but "any webpage in the browser can read or write
//! your workspace" — so they are written first and never skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

const TOKEN: &str = "test-token";

/// A router over a real throwaway workspace, plus the tempdir keeping it
/// alive for as long as the test runs.
fn test_app() -> (Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let dit = dit_core::Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();
    let state = dit_server::AppState::new(dit, "tester", TOKEN);
    (dit_server::app(state), tmp)
}

async fn get(app: &Router, path: &str, headers: &[(&str, &str)]) -> axum::response::Response {
    let mut builder = Request::builder().uri(path).method("GET");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[tokio::test]
async fn api_without_a_token_is_rejected() {
    let (app, _tmp) = test_app();
    let res = get(&app, "/api/status", &[("host", "localhost:7700")]).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_with_the_right_bearer_token_is_served() {
    let (app, _tmp) = test_app();
    let res = get(
        &app,
        "/api/status",
        &[
            ("host", "localhost:7700"),
            ("authorization", &format!("Bearer {TOKEN}")),
        ],
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK, "{}", body_text(res).await);
}

#[tokio::test]
async fn a_wrong_token_is_still_rejected() {
    let (app, _tmp) = test_app();
    let res = get(
        &app,
        "/api/status",
        &[
            ("host", "localhost:7700"),
            ("authorization", "Bearer not-the-token"),
        ],
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_bearer_scheme_is_required_a_bare_token_is_not_enough() {
    let (app, _tmp) = test_app();
    let res = get(
        &app,
        "/api/status",
        &[("host", "localhost:7700"), ("authorization", TOKEN)],
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_websocket_endpoint_takes_its_token_from_the_query() {
    let (app, _tmp) = test_app();
    // No browser can set headers on a WebSocket handshake, so this endpoint
    // authenticates from ?token=. Without any token it must refuse before
    // attempting the upgrade...
    let res = get(&app, "/api/events", &[("host", "localhost:7700")]).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    // ...and with the right one it gets past auth. The handshake itself
    // cannot complete over oneshot (no socket), so "anything but 401" is
    // the observable proof here.
    let res = get(
        &app,
        &format!("/api/events?token={TOKEN}"),
        &[("host", "localhost:7700")],
    )
    .await;
    assert_ne!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_foreign_host_header_is_refused() {
    // DNS rebinding: a page at evil.com makes the browser send Host:
    // evil.com to 127.0.0.1. Only local names may ever answer.
    let (app, _tmp) = test_app();
    for host in ["evil.com", "evil.example:7700", "", "127.0.0.1.evil.com"] {
        let res = get(&app, "/api/status", &[("host", host)]).await;
        assert_eq!(
            res.status(),
            StatusCode::FORBIDDEN,
            "host `{host}` must not be served"
        );
    }
}

#[tokio::test]
async fn local_host_headers_are_accepted() {
    let (app, _tmp) = test_app();
    for host in [
        "localhost:7700",
        "127.0.0.1:7700",
        "[::1]:7700",
        "localhost",
    ] {
        let res = get(
            &app,
            "/api/status",
            &[
                ("host", host),
                ("authorization", &format!("Bearer {TOKEN}")),
            ],
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK, "host `{host}`");
    }
}

#[tokio::test]
async fn host_names_are_matched_case_insensitively_and_without_a_trailing_dot() {
    // Host is case-insensitive by specification, and a fully-qualified
    // `localhost.` is the same name as `localhost`. Both spellings come
    // from the user's own browser or terminal, so refusing them breaks
    // local access — they must never widen what is accepted.
    let (app, _tmp) = test_app();
    for host in [
        "LocalHost:7700",
        "LOCALHOST",
        "localhost.",
        "localhost.:7700",
    ] {
        let res = get(
            &app,
            "/api/status",
            &[
                ("host", host),
                ("authorization", &format!("Bearer {TOKEN}")),
            ],
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK, "host `{host}`");
    }
    // The same spellings of a foreign name are still refused.
    for host in ["evil.com.", "Evil.Com:7700"] {
        let res = get(&app, "/api/status", &[("host", host)]).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "host `{host}`");
    }
}

/// A token file that already exists with loose permissions must be
/// tightened when it is read, not only when it is first created — the file
/// may have been restored from a backup or copied over by hand.
#[test]
#[cfg(unix)]
fn reading_an_existing_token_file_restricts_its_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("server-token");
    std::fs::write(&file, "stored-token\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let token = dit_server::config::load_or_create_token(tmp.path()).unwrap();
    assert_eq!(token, "stored-token");
    let mode = std::fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "reading must tighten the permissions");
}

#[tokio::test]
async fn every_api_response_carries_the_security_headers() {
    let (app, _tmp) = test_app();
    let res = get(
        &app,
        "/api/status",
        &[
            ("host", "localhost:7700"),
            ("authorization", &format!("Bearer {TOKEN}")),
        ],
    )
    .await;
    let csp = res
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(!csp.is_empty(), "a CSP must be set");
    assert!(
        csp.contains("default-src 'none'") || csp.contains("default-src 'none';"),
        "the CSP must start from nothing: {csp}"
    );
    // Token-level, not substring: the CSP must carry 'wasm-unsafe-eval' for
    // the editor's Rust bridge, and that string *contains* "unsafe-eval" —
    // a substring check here would false-positive on exactly that token.
    let script_src = csp
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("script-src"))
        .unwrap_or_default()
        .to_owned();
    assert!(!script_src.is_empty(), "script-src must be set: {csp}");
    let tokens: Vec<&str> = script_src.split_whitespace().skip(1).collect();
    assert!(
        tokens.contains(&"'wasm-unsafe-eval'"),
        "script-src must allow wasm compilation for the editor bridge: {csp}"
    );
    assert!(
        !tokens.contains(&"'unsafe-eval'"),
        "plain unsafe-eval stays banned: {csp}"
    );
    assert!(res.headers().contains_key("x-content-type-options"));
    assert!(res.headers().contains_key("referrer-policy"));
    assert!(res.headers().contains_key("x-frame-options"));
    assert!(res.headers().contains_key("cache-control"));
}

#[tokio::test]
async fn unauthenticated_responses_do_not_leak_the_reason_beyond_the_fact() {
    let (app, _tmp) = test_app();
    let res = get(&app, "/api/status", &[("host", "localhost:7700")]).await;
    let text = body_text(res).await;
    assert!(
        text.contains("token"),
        "the message says what is missing: {text}"
    );
    assert!(
        !text.contains(TOKEN),
        "the response must never echo the valid token: {text}"
    );
}
