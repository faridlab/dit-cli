//! One server, many workspaces (ADR 0028).
//!
//! The hub sits in front of the per-workspace router `app()` has always
//! built: `/w/<name>/…` is handed — prefix taken off — to that workspace's
//! router, opened on first use with its own index and watcher, and
//! `/api/workspaces` manages the per-machine list. Static files are shared,
//! and so are the guards: one token and one set of local hostnames for the
//! whole process.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use dit_core::{Dit, DitError, Registry};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use ts_rs::TS;

use crate::security::Guard;
use crate::state::AppState;

/// How the hub is set up.
pub struct HubOptions {
    pub token: String,
    /// The address bound to; its name joins the local hostnames.
    pub bind_host: String,
    /// `--me`, which beats each workspace's own alias.
    pub me: Option<String>,
    /// Where `workspaces.yaml` lives.
    pub config_dir: PathBuf,
    /// Where "New workspace" puts a workspace.
    pub workspace_root: PathBuf,
    /// This binary, registered as each new workspace's merge driver.
    pub driver: PathBuf,
    /// Start each opened workspace's watcher (off in tests).
    pub live_updates: bool,
}

// The token never appears in a debug dump, not even masked (as AppState).
impl std::fmt::Debug for HubOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubOptions")
            .field("bind_host", &self.bind_host)
            .field("config_dir", &self.config_dir)
            .field("workspace_root", &self.workspace_root)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

pub struct Hub {
    guard: Arc<Guard>,
    options: HubOptions,
    /// Routers of the workspaces opened so far, by name.
    open: Mutex<BTreeMap<String, Router>>,
}

impl Hub {
    pub fn new(options: HubOptions) -> Arc<Hub> {
        let mut allowed = crate::state::local_host_names();
        let name = crate::state::host_hostname(&options.bind_host);
        if !allowed.contains(&name) {
            allowed.push(name);
        }
        Arc::new(Hub {
            guard: Arc::new(Guard {
                token: options.token.clone(),
                allowed_hosts: allowed,
            }),
            options,
            open: Mutex::new(BTreeMap::new()),
        })
    }

    fn registry(&self) -> Result<Registry, HubError> {
        Registry::load(&self.options.config_dir).map_err(HubError::Dit)
    }

    /// The router of workspace `name`, opening it on first use.
    fn router_for(&self, name: &str) -> Result<Router, HubError> {
        if let Some(router) = self.lock().get(name) {
            return Ok(router.clone());
        }
        let registry = self.registry()?;
        let entry = registry.get(name).ok_or_else(|| {
            HubError::NotFound(format!("no workspace called `{name}` on this machine"))
        })?;
        let dit = Dit::open_for_ui(&entry.path).map_err(HubError::Dit)?;
        let me = self
            .options
            .me
            .clone()
            .or_else(|| dit.me())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "unknown".into());
        let code_only = dit.code_only();
        let state = AppState::with_guard(dit, &me, &self.guard);
        if self.options.live_updates && !code_only {
            state.start_live_updates();
        }
        let router = crate::routes::app(state);
        self.lock().insert(name.to_owned(), router.clone());
        Ok(router)
    }

    /// Forget an opened workspace — after it is removed from the list.
    fn close(&self, name: &str) {
        self.lock().remove(name);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Router>> {
        match self.open.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// The hub's routes, behind the same guards as every workspace's.
pub fn hub_app(hub: Arc<Hub>) -> Router {
    let guard = hub.guard.clone();
    Router::new()
        .route(
            "/api/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route("/api/workspaces/add", post(add_workspace))
        .route("/api/workspaces/{name}", delete(remove_workspace))
        .route("/api/workspaces/{name}/default", post(set_default))
        .route("/api/{*rest}", any(no_workspace_here))
        .route("/", get(root))
        // `/w/<name>/…` is matched by hand in the fallback, not as a route:
        // a route's path parameters ride along in the request's extensions,
        // and the workspace's router would count `name` and `rest` with its
        // own — every handler taking a path parameter answered 500.
        .fallback(dispatch)
        .layer(axum::middleware::from_fn_with_state(
            guard.clone(),
            crate::security::require_token,
        ))
        .layer(axum::middleware::from_fn_with_state(
            guard,
            crate::security::require_local_host,
        ))
        .layer(axum::middleware::from_fn(crate::security::security_headers))
        .with_state(hub)
}

// ---- the list ---------------------------------------------------------------

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspaceDto {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspacesDto {
    pub workspaces: Vec<WorkspaceDto>,
    pub default: Option<String>,
    /// Where "New workspace" puts one.
    pub root: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct NewWorkspaceDto {
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct AddWorkspaceDto {
    pub path: String,
    #[serde(default)]
    #[ts(optional)]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspaceAddedDto {
    pub name: String,
}

fn list_dto(hub: &Hub, registry: &Registry) -> WorkspacesDto {
    WorkspacesDto {
        workspaces: registry
            .workspaces()
            .iter()
            .map(|w| WorkspaceDto {
                name: w.name.clone(),
                path: w.path.display().to_string(),
            })
            .collect(),
        default: registry.default_entry().map(|w| w.name.clone()),
        root: hub.options.workspace_root.display().to_string(),
    }
}

async fn list_workspaces(State(hub): State<Arc<Hub>>) -> Result<Json<WorkspacesDto>, HubError> {
    let registry = hub.registry()?;
    Ok(Json(list_dto(&hub, &registry)))
}

async fn create_workspace(
    State(hub): State<Arc<Hub>>,
    Json(input): Json<NewWorkspaceDto>,
) -> Result<(StatusCode, Json<WorkspacesDto>), HubError> {
    let hub2 = hub.clone();
    let list = tokio::task::spawn_blocking(move || -> Result<WorkspacesDto, HubError> {
        let mut registry = hub2.registry()?;
        registry
            .create(
                input.name.trim(),
                &hub2.options.workspace_root,
                &hub2.options.driver,
            )
            .map_err(HubError::Dit)?;
        Ok(list_dto(&hub2, &registry))
    })
    .await
    .map_err(|_| HubError::Internal("the workspace task failed".into()))??;
    Ok((StatusCode::CREATED, Json(list)))
}

async fn add_workspace(
    State(hub): State<Arc<Hub>>,
    Json(input): Json<AddWorkspaceDto>,
) -> Result<Json<WorkspaceAddedDto>, HubError> {
    let mut registry = hub.registry()?;
    // The browser's rule (ADR 0028): only a folder that already is a
    // workspace — never a repository that would open as a code map.
    let name = registry
        .add(
            input.name.as_deref(),
            &PathBuf::from(input.path.trim()),
            true,
        )
        .map_err(HubError::Dit)?;
    Ok(Json(WorkspaceAddedDto { name }))
}

async fn remove_workspace(
    State(hub): State<Arc<Hub>>,
    Path(name): Path<String>,
) -> Result<Json<WorkspacesDto>, HubError> {
    let mut registry = hub.registry()?;
    registry.remove(&name).map_err(HubError::Dit)?;
    hub.close(&name);
    Ok(Json(list_dto(&hub, &registry)))
}

async fn set_default(
    State(hub): State<Arc<Hub>>,
    Path(name): Path<String>,
) -> Result<Json<WorkspacesDto>, HubError> {
    let mut registry = hub.registry()?;
    registry.set_default(&name).map_err(HubError::Dit)?;
    Ok(Json(list_dto(&hub, &registry)))
}

// ---- dispatch ---------------------------------------------------------------

/// `/` goes to the default workspace; with none, the shell's first-run page.
async fn root(State(hub): State<Arc<Hub>>, uri: Uri) -> Response {
    match hub
        .registry()
        .ok()
        .and_then(|r| r.default_entry().map(|w| w.name.clone()))
    {
        Some(name) => Redirect::temporary(&format!("/w/{name}/")).into_response(),
        None => crate::routes::serve_uri(uri).await,
    }
}

/// Every other API path lives under a workspace. Answering the page's
/// shell here would hand a script HTML where it expects JSON.
async fn no_workspace_here() -> HubError {
    HubError::NotFound(
        "no workspace is open at this address — the workspace API lives under /w/<name>/api".into(),
    )
}

/// `/w/<name>/<rest>` goes to workspace `name` as `/<rest>`; `/w/<name>`
/// gains its slash; anything else is the shell's static files.
async fn dispatch(State(hub): State<Arc<Hub>>, req: Request<Body>) -> Response {
    let path = req.uri().path().to_owned();
    let Some(after) = path.strip_prefix("/w/") else {
        return crate::routes::serve_uri(req.uri().clone()).await;
    };
    match after.split_once('/') {
        Some((name, rest)) if !name.is_empty() => forward(&hub, name, rest, req).await,
        None if !after.is_empty() => Redirect::permanent(&format!("/w/{after}/")).into_response(),
        _ => crate::routes::serve_uri(req.uri().clone()).await,
    }
}

/// Hand a request to workspace `name` as if it had arrived at `/<rest>`.
/// Extensions ride along, so a WebSocket upgrade still upgrades.
async fn forward(hub: &Arc<Hub>, name: &str, rest: &str, req: Request<Body>) -> Response {
    let hub2 = hub.clone();
    let owned = name.to_owned();
    let router = match tokio::task::spawn_blocking(move || hub2.router_for(&owned)).await {
        Ok(Ok(router)) => router,
        Ok(Err(e)) => return e.into_response(),
        Err(_) => return HubError::Internal("the workspace task failed".into()).into_response(),
    };
    let (mut parts, body) = req.into_parts();
    let path_and_query = match parts.uri.query() {
        Some(q) => format!("/{rest}?{q}"),
        None => format!("/{rest}"),
    };
    parts.uri = match path_and_query.parse() {
        Ok(uri) => uri,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match router.oneshot(Request::from_parts(parts, body)).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

// ---- errors -----------------------------------------------------------------

#[derive(Debug)]
pub enum HubError {
    Dit(DitError),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for HubError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            HubError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            HubError::Dit(e @ (DitError::NotFound(_) | DitError::Missing(_))) => {
                (StatusCode::NOT_FOUND, e.to_string())
            }
            HubError::Dit(e @ DitError::Refuse(_)) => (StatusCode::BAD_REQUEST, e.to_string()),
            HubError::Dit(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            HubError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
