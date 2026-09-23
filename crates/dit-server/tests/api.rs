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

#[tokio::test]
async fn moving_a_page_serves_it_from_the_new_path() {
    let (app, _tmp) = test_app();

    let (_, _, text) = req(
        &app,
        "PUT",
        "/api/docs/docs/flows/auth.md",
        Some(json!({ "body": "# Auth\n\nbody text\n" })),
    )
    .await;
    assert!(text.contains("auth"), "{text}");

    // Move: 204, then the bytes live at the new path and only there.
    let (status, _, text) = req(
        &app,
        "POST",
        "/api/docs/move",
        Some(json!({ "from": "docs/flows/auth.md", "to": "notes/auth.md" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{text}");

    let (status, page, _) = req(&app, "GET", "/api/docs/notes/auth.md", None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page["body"].as_str().unwrap().contains("body text"));

    let (status, _, _) = req(&app, "GET", "/api/docs/docs/flows/auth.md", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Moving onto an occupied path is refused — 409, nothing overwritten.
    let (_, _, _) = req(
        &app,
        "PUT",
        "/api/docs/notes/taken.md",
        Some(json!({ "body": "# Taken\n" })),
    )
    .await;
    let (status, body, text) = req(
        &app,
        "POST",
        "/api/docs/move",
        Some(json!({ "from": "notes/auth.md", "to": "notes/taken.md" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body} {text}");
    assert!(text.contains("nothing was moved"), "{text}");
    let (_, untouched, _) = req(&app, "GET", "/api/docs/notes/taken.md", None).await;
    assert!(untouched["body"].as_str().unwrap().contains("Taken"));

    // A missing source is a 404, and a malformed path a 400 — same shapes
    // as the rest of the docs surface.
    let (status, _, _) = req(
        &app,
        "POST",
        "/api/docs/move",
        Some(json!({ "from": "docs/ghost.md", "to": "notes/ghost.md" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = req(
        &app,
        "POST",
        "/api/docs/move",
        Some(json!({ "from": "docs/ok.md", "to": "docs/UPPER.md" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_activity_feed_spans_issues_and_pages_by_cursor() {
    let (app, _tmp) = test_app();

    for title in ["Login timeout", "Merge driver drops changes"] {
        let (status, _, text) = req(
            &app,
            "POST",
            "/api/issues",
            Some(json!({ "title": title, "type": "bug" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
    }

    let (status, page, text) = req(&app, "GET", "/api/activity?limit=100", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let events = page["events"].as_array().unwrap();
    assert!(events.len() >= 2, "{text}");

    // Newest first, and every row carries enough issue to render it.
    let first = &events[0];
    assert!(first["seq"].as_i64().unwrap() > events[1]["seq"].as_i64().unwrap());
    assert!(!first["short_ref"].as_str().unwrap().is_empty());
    assert!(!first["title"].as_str().unwrap().is_empty());
    assert!(!first["field"].as_str().unwrap().is_empty());
    // A full page is needed before a cursor is offered.
    assert!(page["next_before_seq"].is_null(), "{text}");

    // One row per page: the cursor walks history without repeating a row.
    let (_, first_page, _) = req(&app, "GET", "/api/activity?limit=1", None).await;
    let cursor = first_page["next_before_seq"].as_i64().expect("a cursor");
    let (_, second_page, _) = req(
        &app,
        "GET",
        &format!("/api/activity?limit=1&before_seq={cursor}"),
        None,
    )
    .await;
    let next_seq = second_page["events"][0]["seq"].as_i64().unwrap();
    assert!(next_seq < cursor);
}

#[tokio::test]
async fn the_activity_summary_answers_what_the_board_looked_like_then() {
    let (app, _tmp) = test_app();

    let (_, created, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Login timeout", "type": "bug" })),
    )
    .await;
    let id = created["short_ref"].as_str().unwrap().to_owned();

    let (_, before, _) = req(&app, "GET", "/api/activity/summary", None).await;
    let cutoff = before["max_seq"].as_i64().unwrap();
    assert_eq!(before["now"]["todo"], 1);
    assert_eq!(before["now"]["done"], 0);
    assert_eq!(before["seq"], cutoff, "no seq means now");

    // Finish it, then look back at the moment before.
    let (status, _, text) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{id}"),
        Some(json!({ "set": { "status": "done" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");

    let (status, after, text) = req(
        &app,
        "GET",
        &format!("/api/activity/summary?seq={cutoff}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(after["at_cutoff"]["todo"], 1, "then: still to do — {text}");
    assert_eq!(after["at_cutoff"]["done"], 0);
    assert_eq!(after["now"]["done"], 1, "now: finished — {text}");
    assert_eq!(after["since"]["finished"], 1);
    assert_eq!(after["since"]["created"], 0, "nothing was born since");
    assert!(after["days"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["count"].as_i64().unwrap() > 0));
}

#[tokio::test]
async fn blocked_by_rides_the_wire_and_bad_ids_are_400s() {
    let (app, _tmp) = test_app();

    let (_, blocker, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Land the schema first" })),
    )
    .await;
    let blocker_id = blocker["id"].as_str().unwrap().to_owned();
    let (status, blocked, text) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Then wire the UI" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    // A fresh issue has nothing blocking it, and says so explicitly.
    assert_eq!(blocked["blocked_by"], json!([]), "{blocked}");
    let short_ref = blocked["short_ref"].as_str().unwrap().to_owned();

    // The patch sets the list; the response and a fresh read both carry it.
    let (status, patched, text) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": { "blocked_by": [blocker_id] } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(patched["blocked_by"], json!([blocker_id]), "{patched}");
    let (_, read_back, _) = req(&app, "GET", &format!("/api/issues/{short_ref}"), None).await;
    assert_eq!(read_back["blocked_by"], json!([blocker_id]));

    // Something that is not an issue id is the request's fault.
    let (status, body, text) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": { "blocked_by": ["not-an-id"] } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    assert!(
        body["error"].as_str().unwrap().contains("not-an-id"),
        "{body}"
    );
}

#[tokio::test]
async fn the_comment_feed_spans_the_workspace_newest_first() {
    let (app, _tmp) = test_app();

    // An untouched workspace has an empty feed, not an error.
    let (status, feed, text) = req(&app, "GET", "/api/comments", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(feed.as_array().map(Vec::len), Some(0), "{text}");

    let mut short_refs = Vec::new();
    for title in ["Login timeout", "Merge driver drops changes"] {
        let (_, created, _) = req(
            &app,
            "POST",
            "/api/issues",
            Some(json!({ "title": title, "type": "bug" })),
        )
        .await;
        short_refs.push(created["short_ref"].as_str().unwrap().to_owned());
    }
    for (short_ref, body) in short_refs
        .iter()
        .zip(["first", "second <script>x</script>"])
    {
        let (status, _, text) = req(
            &app,
            "POST",
            &format!("/api/issues/{short_ref}/comments"),
            Some(json!({ "body": body })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
    }

    let (status, feed, text) = req(&app, "GET", "/api/comments?limit=10", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let rows = feed.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{text}");
    // Newest first, and every row carries enough issue to render a line.
    assert_eq!(rows[0]["short_ref"], short_refs[1].as_str());
    assert_eq!(rows[0]["title"], "Merge driver drops changes");
    assert_eq!(rows[0]["number"], 2);
    assert_eq!(rows[0]["author"], "tester");
    assert!(rows[0]["body"].as_str().unwrap().contains("second"));
    // Rendered through the same sanitizer as everything else.
    assert!(!rows[0]["body_html"].as_str().unwrap().contains("<script"));
    assert!(rows[0]["created"].is_string());
    assert!(rows[0]["issue_id"].as_str().unwrap().len() == 26);
    assert!(rows[0]["id"].as_str().unwrap().len() == 26);
    assert_eq!(rows[1]["short_ref"], short_refs[0].as_str());

    // The limit is honoured.
    let (_, one, _) = req(&app, "GET", "/api/comments?limit=1", None).await;
    assert_eq!(one.as_array().map(Vec::len), Some(1));
}

/// Commit a release plan by hand, the way `dit release plan` (v0.9) will —
/// the read model ships before its writer, so the fixture is git itself.
fn commit_release(root: &std::path::Path, version: &str, text: &str) {
    let dir = root.join(".dit/releases").join(version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("release.md"), text).unwrap();
    for args in [
        vec!["add", ".dit/releases"],
        vec!["commit", "-q", "-m", "plan a release"],
    ] {
        let out = std::process::Command::new("git")
            .args(&args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[tokio::test]
async fn releases_are_listed_and_patched_over_the_wire() {
    // A workspace with no `.dit/releases/` at all answers with an empty
    // list — the roadmap must render on day one.
    let (app, _tmp) = test_app();
    let (status, list, text) = req(&app, "GET", "/api/releases", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(list.as_array().map(Vec::len), Some(0), "{text}");
    let (status, _, _) = req(
        &app,
        "PATCH",
        "/api/releases/v0.2.0",
        Some(json!({ "status": "released" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Now with plans in git, indexed before the server opens the workspace.
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = dit_core::Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();
    commit_release(
        tmp.path(),
        "v0.2.0",
        "---\nversion: v0.2.0\nstatus: in_uat\ntarget_ref: release/0.2.0\nrepo: api\n\
         target: 2026-10-01\nincludes: [01K3M9ZXQ2R7VN8P4TDBCEFGHJ]\n---\n",
    );
    commit_release(
        tmp.path(),
        "v1.0.0",
        "---\nversion: v1.0.0\nstatus: planned\n---\n",
    );
    dit.reindex(dit_core::ReindexMode::All).unwrap();
    let app = dit_server::app(dit_server::AppState::new(dit, "tester", TOKEN));

    let (status, list, text) = req(&app, "GET", "/api/releases", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let rows = list.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{text}");
    assert_eq!(rows[0]["version"], "v0.2.0", "dated first");
    assert_eq!(rows[0]["status"], "in_uat");
    assert_eq!(rows[0]["target_ref"], "release/0.2.0");
    assert_eq!(rows[0]["repo"], "api");
    assert_eq!(rows[0]["target"], "2026-10-01");
    assert_eq!(rows[0]["includes"], json!(["01K3M9ZXQ2R7VN8P4TDBCEFGHJ"]));
    assert_eq!(rows[0]["path"], ".dit/releases/v0.2.0/release.md");
    assert_eq!(rows[1]["version"], "v1.0.0");
    assert_eq!(rows[1]["target"], Value::Null);
    assert_eq!(rows[1]["includes"], json!([]));

    // Patch: one commit, and the response is the updated plan.
    let (status, patched, text) = req(
        &app,
        "PATCH",
        "/api/releases/v0.2.0",
        Some(json!({ "status": "released", "target": "2026-10-03" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(patched["status"], "released");
    assert_eq!(patched["target"], "2026-10-03");
    assert_eq!(
        patched["target_ref"], "release/0.2.0",
        "untouched fields survive"
    );
    let (_, list, _) = req(&app, "GET", "/api/releases", None).await;
    assert_eq!(list[0]["status"], "released");

    // Bad enum, bad date, and an impossible version are all 400s.
    let (status, body, _) = req(
        &app,
        "PATCH",
        "/api/releases/v0.2.0",
        Some(json!({ "status": "shipped" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("shipped"));
    let (status, body, _) = req(
        &app,
        "PATCH",
        "/api/releases/v0.2.0",
        Some(json!({ "target": "next week" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _, _) = req(
        &app,
        "PATCH",
        "/api/releases/..%2Fetc",
        Some(json!({ "status": "planned" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn setting_the_alias_changes_who_later_writes_are_attributed_to() {
    let (app, tmp) = test_app();
    let (_, settings, _) = req(&app, "GET", "/api/settings", None).await;
    assert_eq!(settings["me"], "tester", "the server's startup alias");

    let (status, settings, text) =
        req(&app, "PUT", "/api/settings", Some(json!({ "me": "farid" }))).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(settings["me"], "farid");
    let (_, info, _) = req(&app, "GET", "/api/status", None).await;
    assert_eq!(info["me"], "farid", "the status bar follows");

    // The next write is attributed to the new alias: reporter, comment
    // author, and the commit trailer that history reads.
    let (status, created, text) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Attributed to farid" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{text}");
    assert_eq!(created["reporter"], "farid");
    let short_ref = created["short_ref"].as_str().unwrap().to_owned();
    let (_, comment, _) = req(
        &app,
        "POST",
        &format!("/api/issues/{short_ref}/comments"),
        Some(json!({ "body": "signed" })),
    )
    .await;
    assert_eq!(comment["author"], "farid");
    let log = std::process::Command::new("git")
        .args(["log", "-2", "--format=%B"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);
    assert!(log.contains("Dit-Author: farid"), "{log}");
    assert!(!log.contains("Dit-Author: tester"), "{log}");
    // And the history walker sees the same name.
    let (_, history, _) = req(
        &app,
        "GET",
        &format!("/api/issues/{short_ref}/history"),
        None,
    )
    .await;
    assert!(
        history
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["author"] == "farid"),
        "{history}"
    );

    // The alias is the clone's, so a fresh open of the workspace knows it.
    assert_eq!(
        dit_core::Dit::open(tmp.path()).unwrap().me().as_deref(),
        Some("farid")
    );

    // Empty, blank, and unusable aliases are 400s that change nothing.
    for bad in ["", "   ", "Farid", "far id"] {
        let (status, body, _) = req(&app, "PUT", "/api/settings", Some(json!({ "me": bad }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}: {body}");
    }
    let (_, settings, _) = req(&app, "GET", "/api/settings", None).await;
    assert_eq!(settings["me"], "farid");
}

#[tokio::test]
async fn optional_fields_can_be_cleared_and_the_epic_can_be_set() {
    let (app, tmp) = test_app();
    let (_, epic, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "The epic", "type": "story" })),
    )
    .await;
    let epic_id = epic["id"].as_str().unwrap().to_owned();
    let (_, created, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Child", "priority": "p1", "estimate": 3 })),
    )
    .await;
    let short_ref = created["short_ref"].as_str().unwrap().to_owned();
    let path = tmp.path().join(
        dit_core::Dit::open(tmp.path())
            .unwrap()
            .get(&short_ref)
            .unwrap()
            .unwrap()
            .path,
    );

    // Set: epic, due, start, sprint.
    let (status, patched, text) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": {
            "epic": epic_id, "due": "2026-09-30", "start": "2026-09-01", "sprint": "2026-W40"
        } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(patched["epic"], epic_id.as_str());
    assert_eq!(patched["due"], "2026-09-30");
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("due: 2026-09-30"));

    // Clear: null for any clearable field, "" for the string ones.
    let (status, cleared, text) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": {
            "due": null, "start": "", "priority": "", "estimate": null, "sprint": null, "epic": null
        } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    for field in ["due", "start", "priority", "estimate", "sprint", "epic"] {
        assert_eq!(cleared[field], Value::Null, "{field}: {cleared}");
    }
    assert_eq!(cleared["title"], "Child", "untouched");
    let file = std::fs::read_to_string(&path).unwrap();
    for key in [
        "due:",
        "start:",
        "priority:",
        "estimate:",
        "sprint:",
        "epic:",
    ] {
        assert!(!file.contains(key), "{key} should be gone:\n{file}");
    }
    // Absent still means untouched: an empty set changes nothing.
    let (status, same, _) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": {} })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(same["updated"], cleared["updated"]);

    // A bad epic id is a 400 naming the value.
    let (status, body, _) = req(
        &app,
        "PATCH",
        &format!("/api/issues/{short_ref}"),
        Some(json!({ "set": { "epic": "nope" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("nope"));
}

#[tokio::test]
async fn deleting_an_issue_is_a_204_and_then_a_404() {
    let (app, tmp) = test_app();
    let (_, created, _) = req(
        &app,
        "POST",
        "/api/issues",
        Some(json!({ "title": "Doomed" })),
    )
    .await;
    let short_ref = created["short_ref"].as_str().unwrap().to_owned();
    let id = created["id"].as_str().unwrap().to_owned();
    req(
        &app,
        "POST",
        &format!("/api/issues/{short_ref}/comments"),
        Some(json!({ "body": "last words" })),
    )
    .await;
    let (_, before, _) = req(&app, "GET", "/api/activity", None).await;
    let events_before = before["events"].as_array().unwrap().len();

    let (status, _, text) = req(&app, "DELETE", &format!("/api/issues/{short_ref}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
    assert!(text.is_empty());

    let (status, _, _) = req(&app, "GET", &format!("/api/issues/{short_ref}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = req(&app, "DELETE", &format!("/api/issues/{short_ref}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list, _) = req(&app, "GET", "/api/issues", None).await;
    assert_eq!(list["total"], 0);
    let (_, comments, _) = req(&app, "GET", "/api/comments", None).await;
    assert_eq!(comments.as_array().map(Vec::len), Some(0));

    // History outlives its subject: the feed still names the issue's id,
    // with an empty title, and the deletion added events.
    let (_, after, _) = req(&app, "GET", "/api/activity", None).await;
    let events = after["events"].as_array().unwrap();
    assert!(events.len() > events_before, "{after}");
    assert!(
        events
            .iter()
            .any(|e| e["issue_id"] == id.as_str() && e["title"] == ""),
        "{after}"
    );

    // Nothing left on disk, and the tree is committed clean.
    assert!(
        !tmp.path().join("issues").join("2026").exists()
            || std::fs::read_dir(tmp.path().join("issues"))
                .unwrap()
                .all(|e| { e.unwrap().file_name() == "README.md" })
    );
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

// ---- The Morse workbench (ADR 0023) ----------------------------------------

#[tokio::test]
async fn environments_cross_the_wire_as_names_and_never_as_values() {
    let (app, tmp) = test_app();
    std::fs::write(
        tmp.path().join(dit_core::MORSE_LOCAL_PATH),
        "envs:\n  local:\n    server: \"http://localhost:3000\"\n    vars:\n      token: \"s3cr3t-value\"\nallow_hosts:\n  - localhost\n",
    )
    .unwrap();
    let (status, envs, text) = req(&app, "GET", "/api/morse/envs", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(envs["envs"][0]["name"], "local", "{envs}");
    assert_eq!(envs["envs"][0]["vars"], json!(["token"]), "{envs}");
    assert_eq!(envs["allow_hosts"], json!(["localhost"]), "{envs}");
    assert!(
        !text.contains("s3cr3t-value"),
        "a value must never reach the page: {text}"
    );
}

#[tokio::test]
async fn a_send_the_server_cannot_make_says_why_and_a_malformed_one_is_a_400() {
    let (app, _tmp) = test_app();
    let step = |operation: &str| {
        json!({ "env": null, "step": {
            "id": "one", "operation": operation, "request": null,
            "params": [], "query": [], "headers": [], "body": null,
            "status": 200, "checks": [], "capture": []
        }})
    };
    let (status, body, text) =
        req(&app, "POST", "/api/morse/send", Some(step("auth/getUser"))).await;
    assert!(status.is_client_error(), "{status} {text}");
    assert!(
        body["error"].as_str().unwrap_or_default().contains("auth"),
        "an unregistered spec is named: {body}"
    );

    let (status, body, _) = req(&app, "POST", "/api/morse/send", Some(step("unqualified"))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let mut bad_body = step("auth/getUser");
    bad_body["step"]["body"] = json!("{ not json");
    let (status, body, _) = req(&app, "POST", "/api/morse/send", Some(bad_body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_scenario_that_does_not_exist_is_a_404_and_history_starts_empty() {
    let (app, _tmp) = test_app();
    let (status, _, text) = req(&app, "GET", "/api/morse/scenarios/nowhere", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{text}");
    let (status, runs, text) = req(&app, "GET", "/api/morse/runs", None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(runs, json!([]));
}
