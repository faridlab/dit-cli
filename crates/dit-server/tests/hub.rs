//! One server, many workspaces (ADR 0028): `/w/<name>/…` reaches the right
//! workspace, `/api/workspaces` manages the list under the same token, and
//! the browser cannot point the server at a folder that is not a workspace.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "hub-token";

struct Fixture {
    app: axum::Router,
    tmp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    fixture_with("127.0.0.1", None)
}

fn fixture_with(bind_host: &str, chooser: Option<dit_server::FolderChooser>) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let root = tmp.path().join("Documents/DIT");
    let exe = std::env::current_exe().unwrap();
    let mut registry = dit_core::Registry::load(&config).unwrap();
    registry.create("acme", &root, &exe).unwrap();
    registry.create("home", &root, &exe).unwrap();
    let hub = dit_server::Hub::new(dit_server::HubOptions {
        token: TOKEN.into(),
        bind_host: bind_host.into(),
        me: Some("tester".into()),
        config_dir: config,
        workspace_root: root,
        driver: exe,
        live_updates: false,
        folder_chooser: chooser,
    });
    Fixture {
        app: dit_server::hub_app(hub),
        tmp,
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    token: bool,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "localhost:7700");
    if token {
        b = b.header("authorization", format!("Bearer {TOKEN}"));
    }
    let req = match body {
        Some(json) => b
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let raw = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&raw).unwrap_or(Value::Null))
}

#[tokio::test]
async fn each_workspace_answers_at_its_own_prefix() {
    let f = fixture();
    let (status, acme) = call(&f.app, "GET", "/w/acme/api/status", None, true).await;
    assert_eq!(status, StatusCode::OK, "{acme}");
    assert!(acme["repo"].as_str().unwrap().ends_with("acme"), "{acme}");
    let (_, home) = call(&f.app, "GET", "/w/home/api/status", None, true).await;
    assert!(home["repo"].as_str().unwrap().ends_with("home"), "{home}");

    // A write lands in the workspace it was sent to.
    let (status, created) = call(
        &f.app,
        "POST",
        "/w/home/api/issues",
        Some(json!({ "title": "Only at home" })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (_, home_list) = call(&f.app, "GET", "/w/home/api/issues", None, true).await;
    let (_, acme_list) = call(&f.app, "GET", "/w/acme/api/issues", None, true).await;
    assert_eq!(home_list["total"], 1);
    assert_eq!(acme_list["total"], 0);

    let (status, _) = call(&f.app, "GET", "/w/ghost/api/status", None, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_hub_wants_the_token_like_every_api() {
    let f = fixture();
    for uri in ["/api/workspaces", "/w/acme/api/status"] {
        let (status, _) = call(&f.app, "GET", uri, None, false).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    let req = Request::builder()
        .uri("/api/workspaces")
        .header("host", "evil.test")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        f.app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn workspaces_are_listed_created_by_name_and_removed_without_touching_files() {
    let f = fixture();
    let (status, list) = call(&f.app, "GET", "/api/workspaces", None, true).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = list["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["acme", "home"]);
    assert_eq!(list["default"], "acme");

    let (status, made) = call(
        &f.app,
        "POST",
        "/api/workspaces",
        Some(json!({ "name": "side" })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert!(f
        .tmp
        .path()
        .join("Documents/DIT/side/.dit/config.yaml")
        .exists());
    let (status, _) = call(&f.app, "GET", "/w/side/api/status", None, true).await;
    assert_eq!(status, StatusCode::OK, "a new workspace is served at once");
    let (status, _) = call(
        &f.app,
        "POST",
        "/api/workspaces",
        Some(json!({ "name": "../escape" })),
        true,
    )
    .await;
    assert!(status.is_client_error());

    let (status, _) = call(&f.app, "POST", "/api/workspaces/home/default", None, true).await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(&f.app, "GET", "/api/workspaces", None, true).await;
    assert_eq!(list["default"], "home");

    let (status, _) = call(&f.app, "DELETE", "/api/workspaces/side", None, true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        f.tmp
            .path()
            .join("Documents/DIT/side/.dit/config.yaml")
            .exists(),
        "files stay"
    );
    let (status, _) = call(&f.app, "GET", "/w/side/api/status", None, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_browser_adds_only_a_folder_that_already_is_a_workspace() {
    let f = fixture();
    // A plain repository would open as a read-only code map — through the
    // browser, a way to read any repository on the disk.
    let plain = f.tmp.path().join("someone-elses-code");
    std::fs::create_dir_all(plain.join(".git")).unwrap();
    let (status, body) = call(
        &f.app,
        "POST",
        "/api/workspaces/add",
        Some(json!({ "path": plain })),
        true,
    )
    .await;
    assert!(status.is_client_error(), "{body}");
    // An existing workspace folder is fine (here: one removed earlier).
    call(&f.app, "DELETE", "/api/workspaces/home", None, true).await;
    let home = f.tmp.path().join("Documents/DIT/home");
    let (status, added) = call(
        &f.app,
        "POST",
        "/api/workspaces/add",
        Some(json!({ "path": home })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    assert_eq!(added["name"], "home");
}

#[tokio::test]
async fn an_api_path_outside_every_workspace_is_a_json_404_not_the_page() {
    let f = fixture();
    // The page at `/` (no workspace yet) asks for status; it must learn there
    // is no workspace here, not receive the HTML shell with a 200.
    let (status, body) = call(&f.app, "GET", "/api/status", None, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body["error"].as_str().unwrap().contains("workspace"),
        "{body}"
    );
}

/// A forwarded request must not carry the hub's own path parameters
/// (`name`, `rest`) into the workspace's router: there they were counted
/// with the route's own, and every handler taking a path parameter — an
/// issue, its comments, a page — answered 500.
#[tokio::test]
async fn routes_with_path_parameters_work_behind_the_prefix() {
    let f = fixture();
    let (status, created) = call(
        &f.app,
        "POST",
        "/w/home/api/issues",
        Some(json!({ "title": "Open me" })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_owned();
    for path in [
        format!("/w/home/api/issues/{id}"),
        format!("/w/home/api/issues/{id}/comments"),
    ] {
        let (status, body) = call(&f.app, "GET", &path, None, true).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
    }
    let (status, body) = call(&f.app, "GET", "/w/home/api/issues/NOSUCHID", None, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// A chooser standing in for the system's folder dialog (ADR 0030): it
/// answers what a person would have picked.
fn chooser(answer: Option<&'static str>) -> dit_server::FolderChooser {
    std::sync::Arc::new(move |_prompt: &str| Ok(answer.map(std::path::PathBuf::from)))
}

#[tokio::test]
async fn the_folder_dialog_answers_the_one_path_a_person_chose_or_null() {
    let f = fixture_with("127.0.0.1", Some(chooser(Some("/Users/someone/Projects"))));
    let (_, list) = call(&f.app, "GET", "/api/workspaces", None, true).await;
    assert_eq!(list["can_choose_folder"], true);
    let (status, chosen) = call(
        &f.app,
        "POST",
        "/api/workspaces/choose-folder",
        Some(json!({ "purpose": "new" })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{chosen}");
    assert_eq!(chosen["path"], "/Users/someone/Projects");

    let f = fixture_with("127.0.0.1", Some(chooser(None)));
    let (_, cancelled) = call(
        &f.app,
        "POST",
        "/api/workspaces/choose-folder",
        Some(json!({ "purpose": "add" })),
        true,
    )
    .await;
    assert_eq!(cancelled["path"], serde_json::Value::Null);

    // The token guards it like every other route.
    let (status, _) = call(&f.app, "POST", "/api/workspaces/choose-folder", None, false).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn no_dialog_opens_for_a_server_reachable_from_the_network_or_without_a_chooser() {
    for f in [
        fixture_with("0.0.0.0", Some(chooser(Some("/tmp")))),
        fixture_with("127.0.0.1", None),
    ] {
        let (_, list) = call(&f.app, "GET", "/api/workspaces", None, true).await;
        assert_eq!(list["can_choose_folder"], false);
        let (status, _) = call(
            &f.app,
            "POST",
            "/api/workspaces/choose-folder",
            Some(json!({ "purpose": "new" })),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn a_new_workspace_can_go_to_a_chosen_folder_that_exists() {
    let f = fixture();
    let place = f.tmp.path().join("Clients");
    std::fs::create_dir_all(&place).unwrap();
    let (status, made) = call(
        &f.app,
        "POST",
        "/api/workspaces",
        Some(json!({ "name": "globex", "at": place })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert!(place.join("globex/.dit/config.yaml").exists());

    let (status, body) = call(
        &f.app,
        "POST",
        "/api/workspaces",
        Some(json!({ "name": "initech", "at": f.tmp.path().join("nowhere") })),
        true,
    )
    .await;
    assert!(status.is_client_error(), "{body}");
    assert!(
        !f.tmp.path().join("nowhere").exists(),
        "no folder is made up"
    );
}
