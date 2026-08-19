//! Behavior of the routes themselves: the write loop, the read shapes, the
//! error codes. Security is pinned in `security.rs`; this file pins what
//! the frontend actually consumes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "test-token";

fn test_app() -> (axum::Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let dit = dit_core::Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();
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
    // The repo root arrives in git's shape (forward slashes, sometimes an
    // 8.3 user name) while the tempdir canonicalizes differently per platform
    // (symlinked /tmp on macOS, a verbatim \\?\ prefix on Windows) — so both
    // sides are canonicalized before comparing, which erases those shapes.
    let repo = std::path::PathBuf::from(info["repo"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    let root = tmp.path().canonicalize().unwrap();
    assert!(
        repo.starts_with(&root),
        "{} vs {}",
        repo.display(),
        root.display()
    );
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

#[tokio::test]
async fn settings_expose_the_layout_and_the_panel_can_change_it() {
    let (app, tmp) = test_app();

    let (status, settings, _) = req(&app, "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["layout"], "root");
    assert_eq!(settings["numbering"], "local");
    let templates = settings["templates"].as_array().unwrap();
    assert!(templates.iter().any(|t| t == "bug"), "{templates:?}");

    // A policy flip takes effect on the very next create: no number.
    let (status, _, _) = req(
        &app,
        "PUT",
        "/api/settings",
        Some(json!({ "numbering": "on-merge" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, created, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Bot will number me" })),
    )
    .await;
    assert_eq!(created["number"], Value::Null, "{created}");

    // The layout change is the guided migration, over the wire.
    let (status, settings, _) = req(
        &app,
        "PUT",
        "/api/settings",
        Some(json!({ "layout": "dotdir" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(settings["layout"], "dotdir");
    assert!(
        tmp.path().join(".dit/issues").is_dir(),
        "content moved under .dit/"
    );
    assert!(!tmp.path().join("issues").exists());
    // And the workspace still answers after the rebuild.
    let (_, list, _) = req(&app, "GET", "/api/issues", None).await;
    assert_eq!(list["total"], 1, "{list}");

    // A refusal is a 409 that carries its own way out, not a 500.
    let (status, _, text) = req(
        &app,
        "PUT",
        "/api/settings",
        Some(json!({ "layout": "dotdir" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{text}");
    assert!(text.contains("already on the"), "{text}");

    // A bogus enum is a 400 naming the value.
    let (status, _, text) = req(
        &app,
        "PUT",
        "/api/settings",
        Some(json!({ "numbering": "whenever" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    assert!(text.contains("`whenever` is not a numbering"), "{text}");
}

#[tokio::test]
async fn the_docs_editor_round_trips_a_page() {
    let (app, _tmp) = test_app();

    // An untouched workspace lists pages, not an error.
    let (status, list, _) = req(&app, "GET", "/api/docs", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list.as_array().map(Vec::len), Some(0), "{list}");

    // Save a new page: the response carries the formatted body that landed.
    let (status, saved, text) = req(
        &app,
        "PUT",
        "/api/docs/docs/editor-notes.md",
        Some(json!({ "body": "# Notes\n\nFirst page.\n" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(saved["path"], "docs/editor-notes.md", "{saved}");
    assert!(saved["body"].as_str().unwrap().contains("First page."));

    // It reads back unchanged and appears in the listing.
    let (status, page, text) = req(&app, "GET", "/api/docs/docs/editor-notes.md", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(page["body"], saved["body"], "{page}");
    let (_, entries, _) = req(&app, "GET", "/api/docs", None).await;
    let paths: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert_eq!(paths, ["docs/editor-notes.md"], "{entries}");

    // Delete: 204 with no body, then the page is gone.
    let (status, _, text) = req(&app, "DELETE", "/api/docs/docs/editor-notes.md", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
    assert!(text.is_empty(), "{text}");
    let (status, body, text) = req(&app, "GET", "/api/docs/docs/editor-notes.md", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
    assert!(
        body["error"].as_str().unwrap().contains("editor-notes"),
        "{body}"
    );
}

#[tokio::test]
async fn doc_paths_that_cannot_exist_are_400s_and_missing_pages_are_404s() {
    let (app, _tmp) = test_app();

    // Traversal, non-markdown and wrong-root paths are malformed requests
    // the editor can show inline — the `DocPath` sandbox surfaced as HTTP.
    for uri in [
        "/api/docs/docs/%2E%2E/outside.md",
        "/api/docs/docs/notes.txt",
        "/api/docs/issues/2026/x.md",
        "/api/docs/docs/UPPER.md",
    ] {
        let (status, body, text) = req(&app, "PUT", uri, Some(json!({ "body": "x" }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body} {text}");
    }

    // Reading or deleting a page that was never there is a 404.
    let (status, _, text) = req(&app, "GET", "/api/docs/docs/never-there.md", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
    let (status, _, text) = req(&app, "DELETE", "/api/docs/docs/never-there.md", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
}
