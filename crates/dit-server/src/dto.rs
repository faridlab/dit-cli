//! The wire types. These are a projection of the domain types, not a
//! mirror of them: the browser gets display-ready shapes (rendered bodies,
//! lowercase enum names) so the frontend never re-implements model rules.
//! Field names follow the glossary — `short_ref`, `assignees`, `seq` — so
//! a wire change and a docs change never happen in separate universes.

use dit_core::{
    render_markdown, ActivitySummary, ClearableField, Comment, DataLayout, DocEntry, FieldPatch,
    IndexedIssue, IndexedRelease, Issue, IssueKind, Numbering, Priority, ReleasePatch,
    ReleaseStatus, StoredFieldEvent, Workflow, WorkflowStatus, WorkspaceComment,
};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

/// Three states for a clearable field: the key absent from the JSON means
/// "untouched" (`None`), an explicit `null` means "clear" (`Some(None)`), a
/// value means "set". Serde's default `Option` treats absent and `null` the
/// same, so the outer layer is reconstructed here.
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

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
    /// The lane registry (ADR 0015); empty when the workspace declares none.
    pub lanes: Vec<LaneDto>,
    /// The coordination knobs (ADR 0015) the UI needs to render claim age.
    pub coordination: CoordinationDto,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct LaneDto {
    pub id: String,
    pub label: String,
    pub owners: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CoordinationDto {
    pub claim_ttl_minutes: u32,
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
    /// When the work is planned to begin, `YYYY-MM-DD`. Absent for most
    /// issues — the plan views infer a bar from `due` and the estimate
    /// rather than writing one back.
    pub start: Option<String>,
    /// Ids of the issues this one waits on, in the file's order. Empty when
    /// nothing blocks it — always present so the client never has to guess.
    pub blocked_by: Vec<String>,
    /// The lane this issue belongs to (ADR 0015); absent = Unlaned.
    pub lane: Option<String>,
    /// Who claims exclusive intent (ADR 0015); absent = unclaimed. Liveness
    /// is derived client-side from `claimed_at` + the TTL in the schema.
    pub claimed_by: Option<String>,
    /// RFC3339, written by `claim` alongside `claimed_by`.
    pub claimed_at: Option<String>,
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
    /// The parent comment this replies to (§4.4); absent = top-level.
    pub reply_to: Option<String>,
    pub body: String,
    pub body_html: String,
}

/// One row of the workspace comment feed: a comment plus enough of its
/// issue to render it without a second request per row.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkspaceCommentDto {
    pub id: String,
    pub issue_id: String,
    /// The issue's permanent handle, for opening it from the feed.
    pub short_ref: String,
    /// `Some(12)` displays as `#12`; absent until the issue is numbered.
    pub number: Option<u32>,
    /// Empty when the issue is no longer indexed.
    pub title: String,
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

/// One row of the workspace activity feed: a field change, plus enough of
/// the issue to render it without a second request per row.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ActivityEventDto {
    pub seq: i64,
    pub issue_id: String,
    /// The issue's permanent handle, for opening it from the feed.
    pub short_ref: String,
    /// `Some(12)` displays as `#12`; absent until the issue is numbered.
    pub number: Option<u32>,
    /// Empty when the issue no longer exists — history outlives its subject.
    pub title: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub author: String,
    pub ts: String,
    pub commit_sha: String,
}

/// A page of the feed. `next_before_seq` is the cursor for the next page,
/// or absent at the end of history.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ActivityPageDto {
    pub events: Vec<ActivityEventDto>,
    pub next_before_seq: Option<i64>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CategoryCountsDto {
    pub todo: usize,
    pub doing: usize,
    pub done: usize,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DayCountDto {
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    pub count: usize,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ChangeSummaryDto {
    pub touched: usize,
    pub created: usize,
    pub finished: usize,
    pub reprioritized: usize,
}

/// The workspace then, the workspace now, and what happened in between —
/// all recomputed from `field_events` on read (invariant 5).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ActivitySummaryDto {
    /// The cutoff this summary was taken at, as a position in the commit
    /// graph. Dates map to one only through an author's clock; a `seq` is
    /// exact.
    pub seq: i64,
    /// The end of recorded history: the `seq` that means "now".
    pub max_seq: i64,
    pub days: Vec<DayCountDto>,
    pub at_cutoff: CategoryCountsDto,
    pub now: CategoryCountsDto,
    pub since: ChangeSummaryDto,
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
    /// The lane the issue is born into (ADR 0015); absent = Unlaned.
    #[serde(default)]
    #[ts(optional)]
    pub lane: Option<String>,
    #[serde(default)]
    pub body: String,
}

/// The patch request: `{ "set": { ...fields } }`. Absent fields are
/// untouched. The optional fields (`priority`, `epic`, `estimate`, `sprint`,
/// `due`, `start`) can also be cleared: send `null`, or `""` for the string
/// ones, and the key is removed from the file.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct SetIssueDto {
    pub set: FieldPatchDto,
    /// The `--force` escape (ADR 0015): write the status even when it is not
    /// one of the workflow's statuses. Absent = validated.
    #[serde(default)]
    pub force: bool,
}

/// One issue patch. Three states per optional field: key absent — untouched;
/// `null` (or `""` for strings) — cleared, the key leaves the file; a value —
/// set. Required fields (`title`, `type`, `status`) only take values, and the
/// list fields clear by being set to `[]`.
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
    /// `p0`..`p4`; `null` or `""` clears.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub priority: Option<Option<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub assignees: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub reporter: Option<String>,
    /// The full 26-character id of the parent epic; `null` or `""` clears.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub epic: Option<Option<String>>,
    /// `null` clears.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub estimate: Option<Option<u32>>,
    /// `null` or `""` clears.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub sprint: Option<Option<String>>,
    /// `YYYY-MM-DD`; `null` or `""` clears.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub due: Option<Option<String>>,
    /// `YYYY-MM-DD`; `null` or `""` clears.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub start: Option<Option<String>>,
    /// Replaces the whole list, like `assignees` and `labels`. Each entry is
    /// a full 26-character issue id.
    #[serde(default)]
    #[ts(optional)]
    pub blocked_by: Option<Vec<String>>,
    /// The lane id (ADR 0015); `null` or `""` clears back to Unlaned.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub lane: Option<Option<String>>,
}

/// The claim request (ADR 0015): `{"action":"claim"}` plus the escape
/// hatches. `action` defaults to `claim`.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct ClaimRequestDto {
    /// `claim` | `renew` | `takeover` | `release`.
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub force: bool,
}

/// What `claim` did — `wrote: false` means no commit was needed.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ClaimReportDto {
    pub wrote: bool,
    pub note: String,
}

/// The coordination board (ADR 0015): one row per lane, Unlaned last, every
/// card carrying its derived readiness, blocker states and claim liveness.
/// Read-only — writes go through the issue and claim endpoints.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkflowBoardDto {
    pub lanes: Vec<WorkflowLaneDto>,
    pub claim_ttl_minutes: u32,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkflowLaneDto {
    /// Absent for the Unlaned row.
    pub id: Option<String>,
    pub label: String,
    pub owners: Vec<String>,
    pub cards: Vec<WorkflowCardDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct WorkflowCardDto {
    pub id: String,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    pub status_label: String,
    pub category: Option<String>,
    pub priority: Option<String>,
    /// `ready` | `not_pickable` | `blocked`.
    pub readiness: String,
    /// The blockers keeping a card back, each with where it stands.
    pub blockers: Vec<BlockerDto>,
    pub claim: Option<ClaimDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct BlockerDto {
    pub id: String,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    /// `satisfied` | `unsatisfied` | `broken` (cancelled or gone).
    pub state: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ClaimDto {
    pub claimed_by: String,
    pub claimed_at: String,
    pub stale: bool,
}

pub fn workflow_board_dto(board: &dit_core::WorkflowBoard) -> WorkflowBoardDto {
    WorkflowBoardDto {
        claim_ttl_minutes: board.claim_ttl_minutes,
        lanes: board
            .lanes
            .iter()
            .map(|lane| WorkflowLaneDto {
                id: lane.id.clone(),
                label: lane.label.clone(),
                owners: lane.owners.clone(),
                cards: lane
                    .cards
                    .iter()
                    .map(|card| WorkflowCardDto {
                        id: card.id.as_str().to_owned(),
                        short_ref: card.short_ref.clone(),
                        number: card.number,
                        title: card.title.clone(),
                        status: card.status.clone(),
                        status_label: card.status_label.clone(),
                        category: card.category.map(|c| c.as_str().to_owned()),
                        priority: card.priority.map(priority_str),
                        readiness: match card.readiness {
                            dit_core::Readiness::Ready => "ready".into(),
                            dit_core::Readiness::NotPickable => "not_pickable".into(),
                            dit_core::Readiness::Blocked { .. } => "blocked".into(),
                        },
                        blockers: card
                            .blockers
                            .iter()
                            .map(|b| BlockerDto {
                                id: b.id.as_str().to_owned(),
                                short_ref: b.short_ref.clone(),
                                number: b.number,
                                title: b.title.clone(),
                                status: b.status.clone(),
                                state: match b.state {
                                    dit_core::BlockerDisposition::Satisfied => "satisfied".into(),
                                    dit_core::BlockerDisposition::Unsatisfied => {
                                        "unsatisfied".into()
                                    }
                                    dit_core::BlockerDisposition::Broken => "broken".into(),
                                },
                            })
                            .collect(),
                        claim: card.claim.as_ref().map(|c| ClaimDto {
                            claimed_by: c.claimed_by.clone(),
                            claimed_at: c.claimed_at.clone(),
                            stale: c.stale,
                        }),
                    })
                    .collect(),
            })
            .collect(),
    }
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
    /// The parent comment this replies to (§4.4): its id or 7-char short
    /// form, resolved among this issue's comments. Absent = top-level.
    #[serde(default)]
    #[ts(optional)]
    pub reply_to: Option<String>,
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
    /// The alias writes are attributed to — the same value `/api/status`
    /// shows. Absent when the server knows nobody.
    pub me: Option<String>,
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
    /// The alias later writes are attributed to. Saved in the clone's git
    /// config (never committed); lowercase letters, digits and dashes.
    #[serde(default)]
    #[ts(optional)]
    pub me: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct RenderOutputDto {
    pub html: String,
}

/// A release plan (DESIGN.md §15.2, ADR 0014) as the roadmap reads it. Every
/// field is what the file says — `includes` is the release's *claim*, not a
/// verified fact; nothing here has asked git.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ReleaseDto {
    pub version: String,
    /// `planned | in_dev | in_uat | released | rolled_back`.
    pub status: String,
    pub target_ref: Option<String>,
    pub repo: Option<String>,
    /// The planned date, `YYYY-MM-DD` — where the roadmap draws the milestone.
    pub target: Option<String>,
    /// Full ids of the issues the plan claims, in the file's order.
    pub includes: Vec<String>,
    /// Repo-relative path of the plan file, for opening it.
    pub path: String,
}

/// The release patch: `{ status?, target? }`. Absent fields are untouched.
/// Only these two are editable here — the scope (`includes`) is filled by
/// `dit release plan` (v0.9), and the version is the file's identity.
#[derive(Debug, Deserialize, Default, TS)]
#[ts(export)]
pub struct ReleasePatchDto {
    #[serde(default)]
    #[ts(optional)]
    pub status: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub target: Option<String>,
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
/// `me` is the server's current alias, passed in because it is process state
/// (the facade only knows what the clone saved).
pub fn settings_dto(dit: &dit_core::Dit, me: &str) -> SettingsDto {
    SettingsDto {
        layout: dit.layout().as_str().to_owned(),
        numbering: dit.config().numbering.as_str().to_owned(),
        templates: dit.templates(),
        me: (!me.is_empty()).then(|| me.to_owned()),
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
        start: issue.start.clone(),
        blocked_by: issue
            .blocked_by
            .iter()
            .map(|b| b.as_str().to_owned())
            .collect(),
        lane: issue.lane.clone(),
        claimed_by: issue.claimed_by.clone(),
        claimed_at: issue.claimed_at.clone(),
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
        reply_to: comment.reply_to.as_ref().map(|r| r.as_str().to_owned()),
        body: comment.body.clone(),
        body_html: render_markdown(&comment.body),
    }
}

pub fn release_dto(hit: &IndexedRelease) -> ReleaseDto {
    ReleaseDto {
        version: hit.release.version.clone(),
        status: hit.release.status.as_str().to_owned(),
        target_ref: hit.release.target_ref.clone(),
        repo: hit.release.repo.clone(),
        target: hit.release.target.clone(),
        includes: hit
            .release
            .includes
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect(),
        path: hit.path.clone(),
    }
}

/// Wire patch → domain patch. Both values are validated here so the error a
/// user sees names the field, and nothing reaches the transaction that the
/// parser would refuse.
pub fn to_release_patch(dto: ReleasePatchDto) -> Result<ReleasePatch, String> {
    let status = match &dto.status {
        Some(text) => Some(ReleaseStatus::parse(text).ok_or_else(|| {
            format!(
                "`{text}` is not a release status (planned, in_dev, in_uat, released, rolled_back)"
            )
        })?),
        None => None,
    };
    if let Some(date) = &dto.target {
        dit_core::validate_date(date).map_err(|e| e.to_string())?;
    }
    Ok(ReleasePatch {
        status,
        target: dto.target,
    })
}

pub fn workspace_comment_dto(row: &WorkspaceComment) -> WorkspaceCommentDto {
    WorkspaceCommentDto {
        id: row.comment.id.as_str().to_owned(),
        issue_id: row.issue_id.as_str().to_owned(),
        short_ref: row.issue_id.short_ref().as_str().to_owned(),
        number: row.number,
        title: row.title.clone(),
        author: row.comment.author.clone(),
        created: row.comment.created.clone(),
        body: row.comment.body.clone(),
        body_html: render_markdown(&row.comment.body),
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

pub fn activity_event_dto(e: &StoredFieldEvent, issue: Option<&Issue>) -> ActivityEventDto {
    ActivityEventDto {
        seq: e.seq,
        issue_id: e.issue_id.clone(),
        short_ref: issue
            .map(|i| i.id.short_ref().as_str().to_owned())
            .unwrap_or_else(|| e.issue_id.clone()),
        number: issue.and_then(|i| i.number),
        title: issue.map(|i| i.title.clone()).unwrap_or_default(),
        field: e.field.clone(),
        old_value: e.old_value.clone(),
        new_value: e.new_value.clone(),
        author: e.author.clone(),
        ts: e.ts.clone(),
        commit_sha: e.commit_sha.clone(),
    }
}

pub fn activity_summary_dto(summary: &ActivitySummary) -> ActivitySummaryDto {
    let counts = |c: &dit_core::CategoryCounts| CategoryCountsDto {
        todo: c.todo,
        doing: c.doing,
        done: c.done,
    };
    ActivitySummaryDto {
        seq: summary.seq,
        max_seq: summary.max_seq,
        days: summary
            .days
            .iter()
            .map(|d| DayCountDto {
                day: d.day.clone(),
                count: d.count,
            })
            .collect(),
        at_cutoff: counts(&summary.at_cutoff),
        now: counts(&summary.now),
        since: ChangeSummaryDto {
            touched: summary.since.touched,
            created: summary.since.created,
            finished: summary.since.finished,
            reprioritized: summary.since.reprioritized,
        },
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
            lanes: workflow
                .lanes
                .iter()
                .map(|l| LaneDto {
                    id: l.id.clone(),
                    label: l.label.clone(),
                    owners: l.owners.clone(),
                })
                .collect(),
            coordination: CoordinationDto {
                claim_ttl_minutes: workflow.coordination.claim_ttl_minutes,
            },
        },
    }
}

/// Fold the wire's three states into the domain's two: a value to set, or a
/// note in `clear`. `""` counts as clear for string fields so a form that
/// empties a text box does the obvious thing.
fn tri_state<'a, T>(
    field: ClearableField,
    value: &'a Option<Option<T>>,
    is_blank: impl Fn(&T) -> bool,
    clear: &mut Vec<ClearableField>,
) -> Option<&'a T> {
    match value {
        None => None,
        Some(None) => {
            clear.push(field);
            None
        }
        Some(Some(v)) if is_blank(v) => {
            clear.push(field);
            None
        }
        Some(Some(v)) => Some(v),
    }
}

/// Wire patch → domain patch. Enum names are validated here so the error a
/// user sees names the field, not a parser stack.
pub fn to_field_patch(dto: FieldPatchDto) -> Result<FieldPatch, String> {
    let kind = match &dto.kind {
        Some(text) => Some(parse_kind(text).ok_or_else(|| format!("`{text}` is not a type"))?),
        None => None,
    };
    let mut clear = Vec::new();
    let blank = |s: &String| s.trim().is_empty();
    let never = |_: &u32| false;
    let priority = match tri_state(ClearableField::Priority, &dto.priority, blank, &mut clear) {
        Some(text) => {
            Some(parse_priority(text).ok_or_else(|| format!("`{text}` is not a priority"))?)
        }
        None => None,
    };
    let epic = match tri_state(ClearableField::Epic, &dto.epic, blank, &mut clear) {
        Some(text) => Some(
            dit_core::IssueId::parse(text)
                .map_err(|e| format!("`{text}` is not an issue id: {e}"))?,
        ),
        None => None,
    };
    let estimate = tri_state(ClearableField::Estimate, &dto.estimate, never, &mut clear).copied();
    let sprint = tri_state(ClearableField::Sprint, &dto.sprint, blank, &mut clear).cloned();
    let due = tri_state(ClearableField::Due, &dto.due, blank, &mut clear).cloned();
    let start = tri_state(ClearableField::Start, &dto.start, blank, &mut clear).cloned();
    let lane = tri_state(ClearableField::Lane, &dto.lane, blank, &mut clear).cloned();
    let blocked_by = match &dto.blocked_by {
        Some(ids) => Some(
            ids.iter()
                .map(|text| {
                    dit_core::IssueId::parse(text)
                        .map_err(|e| format!("`{text}` is not an issue id: {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
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
        epic,
        estimate,
        sprint,
        due,
        start,
        blocked_by,
        lane,
        // Claims are `POST /api/issues/{id}/claim`'s to write (ADR 0015) —
        // never a generic field edit.
        claimed_by: None,
        claimed_at: None,
        clear,
    })
}
