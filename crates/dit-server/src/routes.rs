//! Every route. Handlers are thin: authenticate (middleware), park the
//! workspace work on the blocking pool, map the error, shape the JSON.
//! The blocking pool matters because the facade is sync — a git status or
//! an index rebuild must never stall the async runtime other requests
//! share.

use std::sync::Arc;

use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use dit_core::{Dit, DitError};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::dto::{
    self, BoardColumnDto, BoardDto, BoardIssueDto, CommentDto, DocBodyDto, DocEntryDto,
    FieldEventDto, IssueDto, IssueListDto, RenderInputDto, RenderOutputDto, SchemaDto, SettingsDto,
    StatusInfo,
};
use crate::state::AppState;

/// Build the whole application. Routes are written flat with their full
/// `/api/...` paths rather than nested under a prefix — a layer inside a
/// `nest` sees the path *after* the prefix is stripped, which would leave
/// middleware and route tables disagreeing about what "this path" is.
///
/// The layer order is: security headers (outermost, so even a rejection
/// carries the CSP), then the Host check, then token auth, then handlers.
pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/status", get(get_status))
        .route("/api/schema", get(get_schema))
        .route("/api/issues", get(list_issues).post(create_issue))
        .route("/api/issues/{id}", get(get_issue).patch(patch_issue))
        .route("/api/issues/{id}/body", put(put_body))
        .route(
            "/api/issues/{id}/comments",
            get(list_comments).post(post_comment),
        )
        .route("/api/issues/{id}/history", get(get_history))
        .route("/api/board", get(get_board))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/docs", get(list_docs))
        .route(
            "/api/docs/{*path}",
            get(get_doc).put(put_doc).delete(delete_doc),
        )
        .route("/api/markdown/render", post(render_markdown))
        .route("/api/events", get(events))
        .fallback(serve_uri)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::security::require_token,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::security::require_local_host,
        ))
        .layer(axum::middleware::from_fn(crate::security::security_headers))
        .with_state(state)
}

// -- errors ------------------------------------------------------------------

/// What a handler can fail with, before it becomes an HTTP response.
enum ServerError {
    /// Passed through from the workspace; carries its own message.
    Dit(DitError),
    /// The request named something that isn't a type/priority we know.
    BadRequest(String),
    /// Resolution found nothing under that id or short ref.
    NotFound(String),
    /// A state the workspace cannot be in after a successful write — the
    /// kind of thing that means a bug, reported rather than panicked on.
    Internal(String),
}

impl From<DitError> for ServerError {
    fn from(e: DitError) -> Self {
        ServerError::Dit(e)
    }
}

/// The `{ "error": ... }` body every failure returns — one shape, so the
/// client has one thing to parse.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> ApiError {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn not_found(message: String) -> ApiError {
        ApiError {
            status: StatusCode::NOT_FOUND,
            message,
        }
    }

    fn internal(message: &str) -> ApiError {
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.to_owned(),
        }
    }
}

impl From<ServerError> for ApiError {
    fn from(e: ServerError) -> Self {
        match e {
            // Busy is the one conflict-shaped failure: another writer holds
            // the lock, and the client's honest move is to retry, not to
            // report breakage.
            ServerError::Dit(err @ DitError::Busy { .. }) => ApiError {
                status: StatusCode::CONFLICT,
                message: err.to_string(),
            },
            // A DQL string the compiler rejects is the request's fault.
            ServerError::Dit(DitError::Query(q)) => ApiError {
                status: StatusCode::BAD_REQUEST,
                message: q.to_string(),
            },
            // A missing issue is the request naming something that isn't
            // there — the same 404 the resolver produces directly.
            ServerError::Dit(DitError::NotFound(m)) => ApiError::not_found(m),
            // A page path that is not a legal location (wrong root,
            // traversal shape, not `.md`) is a malformed request the
            // editor can show inline — same class as a DQL parse error.
            ServerError::Dit(err @ DitError::DocPath(_)) => ApiError {
                status: StatusCode::BAD_REQUEST,
                message: err.to_string(),
            },
            // The facade refusing a takeover (migrating a dirty tree, a
            // layout the workspace already has) is a state the client can
            // surface and retry from — not a server fault.
            ServerError::Dit(err @ DitError::Refuse(_)) => ApiError {
                status: StatusCode::CONFLICT,
                message: err.to_string(),
            },
            ServerError::Dit(e) => ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: e.to_string(),
            },
            ServerError::BadRequest(m) => ApiError::bad_request(m),
            ServerError::NotFound(m) => ApiError::not_found(m),
            ServerError::Internal(m) => ApiError::internal(&m),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, Json(body)).into_response()
    }
}

type CResult<T> = Result<T, ServerError>;

// -- blocking bridge ----------------------------------------------------------

/// Run read-only workspace work on the blocking pool. The mutex is taken
/// inside the blocking thread, never held across an await.
async fn read_dit<T, F>(state: &Arc<AppState>, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&mut Dit) -> CResult<T> + Send + 'static,
{
    let dit = state.dit.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut guard = match dit.lock() {
            Ok(guard) => guard,
            // A panic in another request leaves the lock poisoned. The
            // workspace files are fine — git is the truth — but this
            // process should not pretend to keep serving them.
            Err(_) => {
                return Err(ApiError::internal(
                    "the workspace crashed during an earlier request — restart dit-server",
                ))
            }
        };
        f(&mut guard).map_err(ApiError::from)
    })
    .await
    .map_err(|_| ApiError::internal("workspace task failed"))?;
    outcome
}

/// Like `read_dit`, and announces the index change to every connected
/// client when the work succeeds.
async fn write_dit<T, F>(state: &Arc<AppState>, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&mut Dit) -> CResult<T> + Send + 'static,
{
    let out = read_dit(state, f).await?;
    state.announce();
    Ok(out)
}

// -- handlers -----------------------------------------------------------------

async fn get_status(State(state): State<Arc<AppState>>) -> Result<Json<StatusInfo>, ApiError> {
    let me = state.me.clone();
    let info = read_dit(&state, move |dit| {
        let repo = dit.status();
        Ok(StatusInfo {
            ok: true,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            repo: dit.root().display().to_string(),
            branch: repo.branch,
            head: (!repo.head.is_empty()).then_some(repo.head),
            dirty: repo.dirty,
            me: (!me.is_empty()).then_some(me),
        })
    })
    .await?;
    Ok(Json(info))
}

async fn get_schema(State(state): State<Arc<AppState>>) -> Result<Json<SchemaDto>, ApiError> {
    let schema = read_dit(&state, move |dit| Ok(dto::schema_dto(dit.workflow()))).await?;
    Ok(Json(schema))
}

#[derive(Deserialize)]
struct ListParams {
    /// A DQL expression; empty means every issue.
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_issues(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<IssueListDto>, ApiError> {
    let q = params.q.unwrap_or_default();
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    let offset = params.offset.unwrap_or(0);
    let me = state.me.clone();
    let list = read_dit(&state, move |dit| {
        dit.query(&q, (!me.is_empty()).then_some(me.as_str()))
            .map_err(ServerError::Dit)
    })
    .await?;
    let total = list.len();
    let items = list
        .iter()
        .skip(offset)
        .take(limit)
        .map(dto::indexed_dto)
        .collect();
    Ok(Json(IssueListDto { total, items }))
}

/// Resolve a path parameter against the workspace: full id or short ref.
fn resolve(dit: &Dit, needle: &str) -> CResult<dit_core::IndexedIssue> {
    match dit.get(needle).map_err(ServerError::Dit)? {
        Some(hit) => Ok(hit),
        None => Err(ServerError::NotFound(format!(
            "no issue matches `{needle}`"
        ))),
    }
}

async fn get_issue(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<IssueDto>, ApiError> {
    let issue = read_dit(&state, move |dit| {
        Ok(dto::issue_dto(&resolve(dit, &id)?.issue))
    })
    .await?;
    Ok(Json(issue))
}

async fn create_issue(
    State(state): State<Arc<AppState>>,
    Json(input): Json<dto::NewIssueDto>,
) -> Result<(StatusCode, Json<IssueDto>), ApiError> {
    let kind = match &input.kind {
        Some(text) => dto::parse_kind(text)
            .ok_or_else(|| format!("`{text}` is not a type"))
            .map_err(ServerError::BadRequest)?,
        None => dit_core::IssueKind::Task,
    };
    let priority = match &input.priority {
        Some(text) => Some(
            dto::parse_priority(text)
                .ok_or_else(|| format!("`{text}` is not a priority"))
                .map_err(ServerError::BadRequest)?,
        ),
        None => None,
    };
    let me = state.me.clone();
    let title = input.title.clone();
    let issue = write_dit(&state, move |dit| {
        let draft = dit_core::IssueDraft {
            title: input.title,
            kind,
            status: input.status,
            priority,
            // The number is facade-owned (ADR 0007): `numbering: local`
            // assigns it in the transaction, and it is never client input.
            number: None,
            // The reporter is who clicked, not who the request claims —
            // identity is server-side state, not client input.
            reporter: (!me.is_empty()).then(|| me.clone()),
            // Absent on the wire means empty, the same default serde applies.
            assignees: input.assignees.unwrap_or_default(),
            labels: input.labels.unwrap_or_default(),
            epic: None,
            estimate: input.estimate,
            sprint: None,
            due: None,
            blocked_by: Vec::new(),
            body: input.body,
        };
        let mut tx = dit.transaction(&me).map_err(ServerError::Dit)?;
        let id = tx.create_issue(draft).map_err(ServerError::Dit)?;
        let short = id.short_ref().as_str().to_owned();
        tx.commit(&format!("create {short}: {title}"))
            .map_err(ServerError::Dit)?;
        let stored = resolve(dit, id.as_str())?;
        Ok(dto::issue_dto(&stored.issue))
    })
    .await?;
    Ok((StatusCode::CREATED, Json(issue)))
}

async fn patch_issue(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<dto::SetIssueDto>,
) -> Result<Json<IssueDto>, ApiError> {
    let patch = dto::to_field_patch(input.set).map_err(ServerError::BadRequest)?;
    let me = state.me.clone();
    let needle = id;
    let issue = write_dit(&state, move |dit| {
        let target = resolve(dit, &needle)?;
        let id = target.issue.id;
        let short = target.issue.id.short_ref().as_str().to_owned();
        let mut tx = dit.transaction(&me).map_err(ServerError::Dit)?;
        tx.set_fields(&id, patch).map_err(ServerError::Dit)?;
        tx.commit(&format!("update {short}"))
            .map_err(ServerError::Dit)?;
        let stored = resolve(dit, id.as_str())?;
        Ok(dto::issue_dto(&stored.issue))
    })
    .await?;
    Ok(Json(issue))
}

async fn put_body(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<dto::BodyDto>,
) -> Result<Json<IssueDto>, ApiError> {
    let me = state.me.clone();
    let needle = id;
    let issue = write_dit(&state, move |dit| {
        let target = resolve(dit, &needle)?;
        let id = target.issue.id;
        let short = target.issue.id.short_ref().as_str().to_owned();
        let mut tx = dit.transaction(&me).map_err(ServerError::Dit)?;
        tx.set_body(&id, &input.body).map_err(ServerError::Dit)?;
        tx.commit(&format!("update {short}: body"))
            .map_err(ServerError::Dit)?;
        let stored = resolve(dit, id.as_str())?;
        Ok(dto::issue_dto(&stored.issue))
    })
    .await?;
    Ok(Json(issue))
}

// -- docs (ADR 0010) -----------------------------------------------------------

async fn list_docs(State(state): State<Arc<AppState>>) -> Result<Json<Vec<DocEntryDto>>, ApiError> {
    let entries = read_dit(&state, move |dit| {
        Ok(dit.list_docs().iter().map(dto::doc_entry_dto).collect())
    })
    .await?;
    Ok(Json(entries))
}

async fn get_doc(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Json<DocBodyDto>, ApiError> {
    let page = read_dit(&state, move |dit| {
        dit.read_doc(&path)
            .map(|body| DocBodyDto { path, body })
            .map_err(ServerError::Dit)
    })
    .await?;
    Ok(Json(page))
}

/// Create or overwrite a page. One save is one commit, formatted by
/// `dit fmt` — the response carries the formatted body back so the editor
/// can show exactly what landed.
async fn put_doc(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    Json(input): Json<dto::BodyDto>,
) -> Result<Json<DocBodyDto>, ApiError> {
    let me = state.me.clone();
    let saved = write_dit(&state, move |dit| {
        let mut tx = dit.transaction(&me).map_err(ServerError::Dit)?;
        tx.write_doc(&path, &input.body).map_err(ServerError::Dit)?;
        tx.commit(&format!("dit docs save: {path}"))
            .map_err(ServerError::Dit)?;
        dit.read_doc(&path)
            .map(|body| DocBodyDto { path, body })
            .map_err(ServerError::Dit)
    })
    .await?;
    Ok(Json(saved))
}

async fn delete_doc(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<StatusCode, ApiError> {
    let me = state.me.clone();
    write_dit(&state, move |dit| {
        let mut tx = dit.transaction(&me).map_err(ServerError::Dit)?;
        tx.delete_doc(&path).map_err(ServerError::Dit)?;
        tx.commit(&format!("dit docs delete: {path}"))
            .map_err(ServerError::Dit)?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_comments(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<CommentDto>>, ApiError> {
    let needle = id;
    let comments = read_dit(&state, move |dit| {
        let target = resolve(dit, &needle)?;
        let list = dit.comments(&target.issue.id).map_err(ServerError::Dit)?;
        Ok(list
            .iter()
            .map(|c| dto::comment_dto(&target.issue.id, c))
            .collect())
    })
    .await?;
    Ok(Json(comments))
}

async fn post_comment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<dto::CommentInputDto>,
) -> Result<(StatusCode, Json<CommentDto>), ApiError> {
    let me = state.me.clone();
    let needle = id;
    let comment = write_dit(&state, move |dit| {
        let target = resolve(dit, &needle)?;
        let issue_id = target.issue.id;
        let short = target.issue.id.short_ref().as_str().to_owned();
        let mut tx = dit.transaction(&me).map_err(ServerError::Dit)?;
        tx.comment(&issue_id, &me, &input.body)
            .map_err(ServerError::Dit)?;
        tx.commit(&format!("comment on {short}"))
            .map_err(ServerError::Dit)?;
        // The stored comment carries the id and timestamp; the transaction
        // mints both, so read back rather than guess.
        let stored = dit
            .comments(&issue_id)
            .map_err(ServerError::Dit)?
            .pop()
            .ok_or_else(|| ServerError::Internal("comment vanished after commit".to_owned()))?;
        Ok(dto::comment_dto(&issue_id, &stored))
    })
    .await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

#[derive(Deserialize)]
struct HistoryParams {
    field: Option<String>,
}

async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<FieldEventDto>>, ApiError> {
    let needle = id;
    let field = params.field;
    let events = read_dit(&state, move |dit| {
        let target = resolve(dit, &needle)?;
        let list = dit
            .history(&target.issue.id, field.as_deref())
            .map_err(ServerError::Dit)?;
        Ok(list.iter().map(dto::field_event_dto).collect())
    })
    .await?;
    Ok(Json(events))
}

async fn get_board(State(state): State<Arc<AppState>>) -> Result<Json<BoardDto>, ApiError> {
    let board = read_dit(&state, move |dit| {
        let board = dit.board().map_err(ServerError::Dit)?;
        Ok(BoardDto {
            columns: board
                .columns
                .iter()
                .map(|col| BoardColumnDto {
                    id: col.status.clone(),
                    label: col.label.clone(),
                    wip_limit: col.wip_limit,
                    issues: col
                        .issues
                        .iter()
                        .map(|hit| BoardIssueDto {
                            id: hit.issue.id.as_str().to_owned(),
                            short_ref: hit.issue.id.short_ref().as_str().to_owned(),
                            number: hit.issue.number,
                            title: hit.issue.title.clone(),
                            priority: hit.issue.priority.map(dto::priority_str),
                            kind: dto::kind_str(hit.issue.kind),
                            assignees: hit.issue.assignees.clone(),
                            labels: hit.issue.labels.clone(),
                            estimate: hit.issue.estimate,
                            updated: hit.issue.updated.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
    })
    .await?;
    Ok(Json(board))
}

// -- settings ------------------------------------------------------------------

async fn get_settings(State(state): State<Arc<AppState>>) -> Result<Json<SettingsDto>, ApiError> {
    let settings = read_dit(&state, |dit| Ok(dto::settings_dto(dit))).await?;
    Ok(Json(settings))
}

/// Change layout and/or numbering. Both values are validated before anything
/// is written; numbering applies first (a config-only commit), then the
/// layout migration — which moves files and rebuilds the index, and may
/// refuse (dirty tree) after the numbering change already landed. The panel
/// sends one field at a time, and a refusal carries its own way out.
async fn put_settings(
    State(state): State<Arc<AppState>>,
    Json(input): Json<dto::SetSettingsDto>,
) -> Result<Json<SettingsDto>, ApiError> {
    let layout = match &input.layout {
        Some(text) => Some(
            dto::parse_layout(text)
                .ok_or_else(|| format!("`{text}` is not a layout (root or dotdir)"))
                .map_err(ServerError::BadRequest)?,
        ),
        None => None,
    };
    let numbering = match &input.numbering {
        Some(text) => Some(
            dto::parse_numbering(text)
                .ok_or_else(|| format!("`{text}` is not a numbering policy (local or on-merge)"))
                .map_err(ServerError::BadRequest)?,
        ),
        None => None,
    };
    let settings = write_dit(&state, move |dit| {
        if let Some(n) = numbering {
            dit.set_numbering(n)?;
        }
        if let Some(l) = layout {
            dit.migrate_layout(l)?;
        }
        Ok(dto::settings_dto(dit))
    })
    .await?;
    Ok(Json(settings))
}

async fn render_markdown(
    Json(input): Json<RenderInputDto>,
) -> Result<Json<RenderOutputDto>, ApiError> {
    // Rendering touches no workspace state — the same pure function the
    // stored views use, run on whatever the editor is typing.
    Ok(Json(RenderOutputDto {
        html: dit_core::render_markdown(&input.text),
    }))
}

// -- live updates --------------------------------------------------------------

async fn events(State(state): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| async move {
        let mut rx = state.subscribe();
        let (mut sender, mut receiver) = socket.split();
        loop {
            tokio::select! {
                frame = rx.recv() => match frame {
                    Ok(frame) => {
                        if sender.send(axum::extract::ws::Message::Text(frame.into())).await.is_err() {
                            break;
                        }
                    }
                    // The client fell behind between writes; the next frame
                    // will trigger its refetch anyway, so lag is survivable.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                // Clients only send close frames and pings (answered by the
                // runtime); anything else is noise to ignore, and a closed
                // socket is the loop's exit.
                msg = receiver.next() => match msg {
                    Some(Ok(_)) => continue,
                    _ => break,
                },
            }
        }
    })
}

// -- the UI --------------------------------------------------------------------

#[cfg(feature = "embed-ui")]
async fn serve_uri(uri: axum::http::Uri) -> Response {
    // One name covers both: the derive macro that builds the struct and
    // the trait whose `get` reads from it.
    use rust_embed::RustEmbed;

    #[derive(RustEmbed)]
    #[folder = "$CARGO_MANIFEST_DIR/../../apps/web/dist"]
    struct Assets;

    // Client-side routing owns every non-/api path: an unknown path serves
    // the shell, which then renders the view for it. Known paths serve the
    // hashed asset directly so a cache can keep it.
    let path = uri.path().trim_start_matches('/');
    let file = if path.is_empty() {
        None
    } else {
        Assets::get(path)
    };
    let (name, data) = match file {
        Some(file) => (path, file.data),
        None => match Assets::get("index.html") {
            Some(file) => ("index.html", file.data),
            None => return axum::http::StatusCode::NOT_FOUND.into_response(),
        },
    };
    {
        (
            [(axum::http::header::CONTENT_TYPE, asset_mime(name))],
            axum::body::Body::from(data.into_owned()),
        )
            .into_response()
    }
}

/// The Content-Type for an embedded asset, by file extension. Compiled for
/// tests too so its table is checkable without an `embed-ui` build — a
/// wrong MIME here is invisible in dev and breaks only the embedded
/// production server.
#[cfg(any(test, feature = "embed-ui"))]
fn asset_mime(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" | "map" => "application/json",
        "woff2" => "font/woff2",
        // WebAssembly.instantiateStreaming refuses anything else — with
        // octet-stream the browser makes the editor bridge fall back to
        // ArrayBuffer instantiation, or rejects it outright.
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(not(feature = "embed-ui"))]
async fn serve_uri(_uri: axum::http::Uri) -> Response {
    // Dev builds have no UI inside them on purpose: the frontend dev server
    // proxies /api here, and embedding would couple every Rust rebuild to a
    // Node build. This page is what you see when that contract is forgotten.
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!doctype html><meta charset=\"utf-8\"><title>DIT</title>\
         <p>This build does not embed the UI.</p>\
         <p>Either run the frontend dev server (<code>npm run dev</code> in \
         <code>apps/web</code>) or build with <code>--features embed-ui</code>.</p>",
    )
        .into_response()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::asset_mime;

    #[test]
    fn wasm_assets_get_the_wasm_mime_type() {
        // The editor's Rust bridge: streaming instantiation requires the
        // exact MIME, and octet-stream (the fallback) would break it only
        // in the embedded production build.
        assert_eq!(
            asset_mime("assets/dit_wasm_bg-HASH.wasm"),
            "application/wasm"
        );
    }

    #[test]
    fn the_existing_table_is_unchanged() {
        assert_eq!(asset_mime("index.html"), "text/html; charset=utf-8");
        assert_eq!(asset_mime("app-HASH.js"), "text/javascript");
        assert_eq!(asset_mime("app-HASH.css"), "text/css");
        assert_eq!(asset_mime("dit_wasm-HASH.js"), "text/javascript");
        assert_eq!(asset_mime("noext"), "application/octet-stream");
    }
}
