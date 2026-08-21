//! The wire types. These are a projection of the domain types, not a
//! mirror of them: the browser gets display-ready shapes (rendered bodies,
//! lowercase enum names) so the frontend never re-implements model rules.
//! Field names follow the glossary — `short_ref`, `assignees`, `seq` — so
//! a wire change and a docs change never happen in separate universes.

use dit_core::{
    render_markdown, Comment, DataLayout, DocEntry, FieldPatch, IndexedIssue, Issue, IssueKind,
    Numbering, Priority, StoredFieldEvent, Workflow, WorkflowStatus,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

// Every DTO derives TS and exports a .ts file into the web app (the target
// directory is pinned in the repo's .cargo/config.toml). The generated
// files are committed, and CI regenerates + diffs them, so a wire change
// can never quietly leave the client behind — the drift is a red build,
// not a runtime surprise.

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct StatusInfo {
    pub ok: bool,
    pub version: String,
    pub repo: String,
    pub branch: String,
    pub head: Option<String>,
    pub dirty: bool,
    /// The alias writes are attributed to, if the server knows one.
    pub me: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SchemaDto {
    pub workflow: WorkflowDto,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkflowDto {
    pub statuses: Vec<StatusDto>,
    pub transitions: Vec<TransitionDto>,
    pub derived: Vec<DerivedDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct StatusDto {
    pub id: String,
    pub label: String,
    pub category: String,
    pub terminal: bool,
    pub wip_limit: Option<u32>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct TransitionDto {
    pub from: Vec<String>,
    pub to: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DerivedDto {
    pub on: String,
    pub implies: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct IssueListDto {
    pub total: usize,
    pub items: Vec<IssueDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct IssueDto {
    pub id: String,
    pub short_ref: String,
    /// The human-friendly handle (ADR 0007): `Some(12)` displays as `#12`.
    /// Absent until assigned — never invented client-side.
    pub number: Option<u32>,
    pub title: String,
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub kind: String,
    pub status: String,
    pub priority: Option<String>,
    pub reporter: Option<String>,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub epic: Option<String>,
    pub estimate: Option<u32>,
    pub sprint: Option<String>,
    pub due: Option<String>,
    pub created: String,
    pub updated: String,
    pub body: String,
    pub body_html: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CommentDto {
    pub id: String,
    pub issue_id: String,
    pub author: String,
    pub created: String,
    pub body: String,
    pub body_html: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FieldEventDto {
    pub seq: i64,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub author: String,
    pub ts: String,
    pub commit_sha: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BoardDto {
    pub columns: Vec<BoardColumnDto>,
}

/// Flat on purpose: the stray "not in workflow" column has no workflow
/// status behind it, so there is no `StatusDto` to nest — the client that
/// wants categories already fetched `/api/schema`.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BoardColumnDto {
    pub id: String,
    pub label: String,
    pub wip_limit: Option<u32>,
    pub issues: Vec<BoardIssueDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BoardIssueDto {
    pub id: String,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub priority: Option<String>,
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub kind: String,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub estimate: Option<u32>,
    pub updated: String,
}

/// The create request. Everything but the title is optional; the server
/// stamps reporter and timestamps.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct NewIssueDto {
    pub title: String,
    #[serde(rename = "type", default)]
    #[ts(rename = "type", optional)]
    pub kind: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub status: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub priority: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub assignees: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub estimate: Option<u32>,
    #[serde(default)]
    pub body: String,
}

/// The patch request: `{ "set": { ...fields } }`. Absent fields are
/// untouched; there is no way to clear a field in v0.1 — that is a
/// deliberate limit of the write surface, not an oversight.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetIssueDto {
    pub set: FieldPatchDto,
}

#[derive(Debug, Deserialize, Default, TS)]
#[ts(export)]
pub struct FieldPatchDto {
    #[serde(default)]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(rename = "type", default)]
    #[ts(rename = "type", optional)]
    pub kind: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub status: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub priority: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub assignees: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub reporter: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub estimate: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub sprint: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub due: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct BodyDto {
    pub body: String,
}

/// A page move: `from` and `to` are workspace-relative doc paths.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct MoveDocDto {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct CommentInputDto {
    pub body: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct RenderInputDto {
    pub text: String,
}

/// One row of the Docs listing (ADR 0010). `updated_ms` is the file's
/// mtime, display metadata only — the page's real history is git.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DocEntryDto {
    pub path: String,
    pub updated_ms: i64,
    pub bytes: u64,
}

/// A page's contents, addressed by its `docs/…` path. Saves return the
/// formatted body that landed, so the editor can show the canonical form.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DocBodyDto {
    pub path: String,
    pub body: String,
}

/// The workspace's user-facing configuration (ADRs 0005 + 0007) — the thing
/// `dit ui` shows so the layout is never a surprise and never a CLI-only
/// knob. Both fields are closed enums on the wire; there is no free-form
/// path to mistype into.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct SettingsDto {
    /// `root` or `dotdir` — where issue content lives.
    pub layout: String,
    /// `local` or `on-merge` — when an issue gets its `number:`.
    pub numbering: String,
    /// Template names creation can seed a body from.
    pub templates: Vec<String>,
}

/// The change request. Absent fields are untouched — the same contract as
/// the issue patch.
#[derive(Debug, Deserialize, Default, TS)]
#[ts(export)]
pub struct SetSettingsDto {
    #[serde(default)]
    #[ts(optional)]
    pub layout: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub numbering: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct RenderOutputDto {
    pub html: String,
}

// -- mapping -----------------------------------------------------------------

pub fn kind_str(kind: IssueKind) -> String {
    kind.as_str().to_owned()
}

pub fn priority_str(p: Priority) -> String {
    p.as_str().to_owned()
}

/// `"p1"` → `Priority::P1`, used by the patch and create endpoints.
pub fn parse_priority(text: &str) -> Option<Priority> {
    match text {
        "p0" => Some(Priority::P0),
        "p1" => Some(Priority::P1),
        "p2" => Some(Priority::P2),
        "p3" => Some(Priority::P3),
        "p4" => Some(Priority::P4),
        _ => None,
    }
}

pub fn parse_kind(text: &str) -> Option<IssueKind> {
    match text {
        "task" => Some(IssueKind::Task),
        "bug" => Some(IssueKind::Bug),
        "story" => Some(IssueKind::Story),
        "spike" => Some(IssueKind::Spike),
        "chore" => Some(IssueKind::Chore),
        _ => None,
    }
}

pub fn parse_layout(text: &str) -> Option<DataLayout> {
    DataLayout::parse(text)
}

pub fn parse_numbering(text: &str) -> Option<Numbering> {
    Numbering::parse(text)
}

/// The settings projection: read straight off the facade, no interpretation.
pub fn settings_dto(dit: &dit_core::Dit) -> SettingsDto {
    SettingsDto {
        layout: dit.layout().as_str().to_owned(),
        numbering: dit.config().numbering.as_str().to_owned(),
        templates: dit.templates(),
    }
}

pub fn issue_dto(issue: &Issue) -> IssueDto {
    IssueDto {
        id: issue.id.as_str().to_owned(),
        short_ref: issue.id.short_ref().as_str().to_owned(),
        number: issue.number,
        title: issue.title.clone(),
        kind: kind_str(issue.kind),
        status: issue.status.clone(),
        priority: issue.priority.map(priority_str),
        reporter: issue.reporter.clone(),
        assignees: issue.assignees.clone(),
        labels: issue.labels.clone(),
        epic: issue.epic.as_ref().map(|e| e.as_str().to_owned()),
        estimate: issue.estimate,
        sprint: issue.sprint.clone(),
        due: issue.due.clone(),
        created: issue.created.clone(),
        updated: issue.updated.clone(),
        body: issue.body.clone(),
        body_html: render_markdown(&issue.body),
    }
}

pub fn indexed_dto(hit: &IndexedIssue) -> IssueDto {
    issue_dto(&hit.issue)
}

pub fn doc_entry_dto(entry: &DocEntry) -> DocEntryDto {
    DocEntryDto {
        path: entry.path.as_str().to_owned(),
        updated_ms: entry.updated_ms,
        bytes: entry.bytes,
    }
}

pub fn comment_dto(issue_id: &dit_core::IssueId, comment: &Comment) -> CommentDto {
    CommentDto {
        id: comment.id.as_str().to_owned(),
        issue_id: issue_id.as_str().to_owned(),
        author: comment.author.clone(),
        created: comment.created.clone(),
        body: comment.body.clone(),
        body_html: render_markdown(&comment.body),
    }
}

pub fn field_event_dto(e: &StoredFieldEvent) -> FieldEventDto {
    FieldEventDto {
        seq: e.seq,
        field: e.field.clone(),
        old_value: e.old_value.clone(),
        new_value: e.new_value.clone(),
        author: e.author.clone(),
        ts: e.ts.clone(),
        commit_sha: e.commit_sha.clone(),
    }
}

pub fn status_dto(status: &WorkflowStatus) -> StatusDto {
    StatusDto {
        id: status.id.clone(),
        label: status.label.clone(),
        category: status.category.as_str().to_owned(),
        terminal: status.terminal,
        wip_limit: status.wip_limit,
    }
}

pub fn schema_dto(workflow: &Workflow) -> SchemaDto {
    SchemaDto {
        workflow: WorkflowDto {
            statuses: workflow.statuses.iter().map(status_dto).collect(),
            transitions: workflow
                .transitions
                .iter()
                .map(|t| TransitionDto {
                    from: t.from.clone(),
                    to: t.to.clone(),
                })
                .collect(),
            derived: workflow
                .derived
                .iter()
                .map(|d| DerivedDto {
                    on: match d.signal {
                        dit_core::DerivedSignal::CommitTrailer => "commit_trailer",
                        dit_core::DerivedSignal::PrMerged => "pr_merged",
                    }
                    .to_owned(),
                    implies: d.implies.clone(),
                })
                .collect(),
        },
    }
}

/// Wire patch → domain patch. Enum names are validated here so the error a
/// user sees names the field, not a parser stack.
pub fn to_field_patch(dto: FieldPatchDto) -> Result<FieldPatch, String> {
    let kind = match &dto.kind {
        Some(text) => Some(parse_kind(text).ok_or_else(|| format!("`{text}` is not a type"))?),
        None => None,
    };
    let priority = match &dto.priority {
        Some(text) => {
            Some(parse_priority(text).ok_or_else(|| format!("`{text}` is not a priority"))?)
        }
        None => None,
    };
    Ok(FieldPatch {
        title: dto.title,
        kind,
        status: dto.status,
        priority,
        // Number stays facade-owned (ADR 0007): the API offers no renumber
        // hatch — repairs go through the CLI's field edit, deliberately.
        number: None,
        assignees: dto.assignees,
        labels: dto.labels,
        reporter: dto.reporter,
        epic: None,
        estimate: dto.estimate,
        sprint: dto.sprint,
        due: dto.due,
        blocked_by: None,
    })
}
