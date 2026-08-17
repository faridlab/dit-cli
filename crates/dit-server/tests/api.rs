//! Behavior of the routes themselves: the write loop, the read shapes, the
//! error codes. Security is pinned in `security.rs`; this file pins what
//! the frontend actually consumes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "test-token";

fn test_app() -> (axum::Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let dit = dit_core::Dit::init(tmp.path(), Path::new("/bin/true")).unwrap();
    let state = dit_server::AppState::new(dit, "tester", TOKEN);
    (dit_server::app(state), tmp)
}

async fn req(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value, String) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "localhost:7700")
        .header("authorization", format!("Bearer {TOKEN}"));
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(request).await.unwrap();
    let status = res.status();
    let raw = res.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&raw).to_string();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json, text)
}

#[tokio::test]
async fn the_write_and_read_loop_round_trips() {
    let (app, _tmp) = test_app();

    // Create.
    let (status, created, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({
            "title": "Login fails on Safari",
            "type": "bug",
            "priority": "p1",
            "labels": ["auth"],
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["reporter"], "tester");
    assert_eq!(created["type"], "bug");
    assert_eq!(created["priority"], "p1");
    assert_eq!(created["status"], "todo");
    let short_ref = created["short_ref"].as_str().unwrap().to_owned();
    let id = created["id"].as_str().unwrap().to_owned();

    // The stored body renders on the way out, and raw HTML never survives.
    let (status, updated, _) = req(
        &app,
        "PUT",
        &format!("/api/issues/{short_ref}/body"),
        Some(json!({ "body": "Steps:\n\n1. open the page\n\n<script>alert(1)</script>\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert!(updated["body_html"].as_str().unwrap().contains("<ol>"));
    assert!(updated["body_html"]
        .as_str()
        .unwrap()
        .contains("raw HTML omitted"));

    // Patch by short ref.
    let (status, patched, _) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": { "status": "in_progress", "assignees": ["budi"] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["status"], "in_progress");
    assert_eq!(patched["assignees"], json!(["budi"]));

    // Comment, then read comments back.
    let (status, comment, _) = req(
        &app,
        "POST",
        &format!("/api/issues/{short_ref}/comments"),
        Some(json!({ "body": "Reproed on macOS too." })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{comment}");
    assert_eq!(comment["author"], "tester");
    let (status, comments, _) = req(
        &app,
        "GET",
        &format!("/api/issues/{short_ref}/comments"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(comments.as_array().unwrap().len(), 1);
    assert_eq!(comments[0]["body"], "Reproed on macOS too.");

    // History records the field changes in order.
    let (_, history, _) = req(
        &app,
        "GET",
        &format!("/api/issues/{short_ref}/history"),
        None,
    )
    .await;
    let events = history.as_array().unwrap();
    assert!(events.iter().any(|e| e["field"] == "status"
        && e["old_value"] == "todo"
        && e["new_value"] == "in_progress"));

    // The board has the issue in the right column.
    let (_, board, _) = req(&app, "GET", "/api/board", None).await;
    let column = board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "in_progress")
        .unwrap();
    assert!(column["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["id"] == id.as_str()));

    // List finds it by query (DQL is infix: `type = bug`).
    let (_, list, _) = req(&app, "GET", "/api/issues?q=type%20%3D%20bug", None).await;
    assert_eq!(list["total"], 1);
    assert_eq!(list["items"][0]["short_ref"], short_ref.as_str());
}

#[tokio::test]
async fn an_unknown_issue_is_a_404_and_a_bad_request_is_a_400() {
    let (app, _tmp) = test_app();

    let (status, body, _) = req(&app, "GET", "/api/issues/nosuch", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body["error"].as_str().unwrap().contains("nosuch"));

    let (status, body, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "x", "type": "not-a-type" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("not-a-type"));

    let (status, _, _) = req(&app, "GET", "/api/issues?q=)", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn markdown_rendering_is_the_same_function_the_stored_views_use() {
    let (app, _tmp) = test_app();
    let (status, out, _) = req(
        &app,
        "POST",
        "/api/markdown/render",
        Some(json!({ "text": "**bold** and <img onerror=alert(1)>" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html = out["html"].as_str().unwrap();
    assert!(html.contains("<strong>bold</strong>"));
    assert!(!html.contains("<img"));
}

#[tokio::test]
async fn status_reports_the_workspace_it_serves() {
    let (app, tmp) = test_app();
    let (status, info, _) = req(&app, "GET", "/api/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["ok"], true);
    assert_eq!(info["me"], "tester");
    // The repo root comes back canonicalized; the tempdir is not, on macOS.
    let root = tmp.path().canonicalize().unwrap();
    assert!(info["repo"]
        .as_str()
        .unwrap()
        .starts_with(root.display().to_string().as_str()));
    assert!(info["branch"].is_string());
}

#[tokio::test]
async fn schema_describes_the_workflow_the_server_validates_against() {
    let (app, _tmp) = test_app();
    let (status, schema, _) = req(&app, "GET", "/api/schema", None).await;
    assert_eq!(status, StatusCode::OK);
    let statuses = schema["workflow"]["statuses"].as_array().unwrap();
    assert!(statuses.iter().any(|s| s["id"] == "backlog"));
    let transitions = schema["workflow"]["transitions"].as_array().unwrap();
    assert!(!transitions.is_empty());
}
