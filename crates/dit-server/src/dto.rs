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
    /// `workspace`, or `code` when `dit ui` serves a repository that is not a
    /// workspace: only its code map, read-only.
    pub mode: String,
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
    /// Non-gating relations (ADR 0020): what feeds this issue.
    pub fed_by: Vec<String>,
    /// The lane this issue belongs to (ADR 0015); absent = Unlaned.
    pub lane: Option<String>,
    /// The orchestrations this issue belongs to (ADR 0019), by name.
    pub flows: Vec<String>,
    /// Scenarios that must be proven for `env` before this is ready (ADR 0024).
    pub needs_scenarios: Vec<String>,
    /// Scenarios this issue delivers (ADR 0024).
    pub proves: Vec<String>,
    /// The environment this issue works against, by name (ADR 0024).
    pub env: Option<String>,
    /// Who claims exclusive intent (ADR 0015); absent = unclaimed. Liveness
    /// is derived client-side from `claimed_at` + the TTL in the schema.
    pub claimed_by: Option<String>,
    /// RFC3339, written by `claim` alongside `claimed_by`.
    pub claimed_at: Option<String>,
    pub created: String,
    pub updated: String,
    pub body: String,
    pub body_html: String,
    /// The issue's folder, relative to the content root
    /// (`issues/2026/10/<folder>`): what a relative link in its body or
    /// comments — an attachment (ADR 0026) — resolves against.
    pub dir: String,
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
    /// The free-form lane, for the board's lane filter (ADR 0019).
    pub lane: Option<String>,
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
    /// The flows the issue is born into (ADR 0019).
    #[serde(default)]
    #[ts(optional)]
    pub flows: Option<Vec<String>>,
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
    /// The non-gating relation (ADR 0020). Replaces the whole list.
    #[serde(default)]
    #[ts(optional)]
    pub fed_by: Option<Vec<String>>,
    /// The lane id (ADR 0015); `null` or `""` clears back to Unlaned.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub lane: Option<Option<String>>,
    /// Replaces the whole membership set, like `labels` (ADR 0019).
    #[serde(default)]
    #[ts(optional)]
    pub flows: Option<Vec<String>>,
    /// Replaces the set of scenarios this issue needs proven (ADR 0024).
    #[serde(default)]
    #[ts(optional)]
    pub needs_scenarios: Option<Vec<String>>,
    /// Replaces the set of scenarios this issue proves (ADR 0024).
    #[serde(default)]
    #[ts(optional)]
    pub proves: Option<Vec<String>>,
    /// The environment name; `null` or `""` clears it.
    #[serde(default, deserialize_with = "double_option")]
    #[ts(optional)]
    pub env: Option<Option<String>>,
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

/// Morse (§20, ADR 0022). A read-only view: the catalogue derived from each
/// registered OpenAPI document, and every scenario judged against it. No
/// endpoint here sends a request — Morse 1 has no egress at all (I11).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseOperationDto {
    pub operation_id: String,
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
    /// The operation's first OpenAPI tag — how the explorer groups it.
    pub tag: Option<String>,
    pub params: Vec<MorseParamDto>,
    /// Top-level JSON body fields, to pre-fill a draft from.
    pub body: Vec<MorseFieldDto>,
    pub responses: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseParamDto {
    pub name: String,
    /// `path` | `query` | `header` | `cookie`.
    pub location: String,
    pub required: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseFieldDto {
    pub name: String,
    pub kind: String,
    pub required: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseServerDto {
    pub url: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseSpecDto {
    pub id: String,
    /// The linked repo holding it (Mode A), absent for this workspace.
    pub repo: Option<String>,
    pub path: String,
    pub title: Option<String>,
    pub version: Option<String>,
    pub head: Option<String>,
    /// The document's `servers:`. A relative `/` names no host, and the
    /// screen says so before anyone presses Send.
    pub servers: Vec<MorseServerDto>,
    pub operations: Vec<MorseOperationDto>,
    /// Why the document could not be read. A spec with a problem is still
    /// listed: a service whose document went missing is worth saying.
    pub problem: Option<String>,
}

/// One step of a run, as a screen shows it. No response body and no captured
/// value: a response is the likeliest place in the product for a real token
/// to appear, so what crosses this boundary is that a capture happened, not
/// what it was (§20.7).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseRunStepDto {
    pub id: String,
    pub method: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    /// The response body's size — present only for a run just made.
    pub bytes: Option<u64>,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseRunDto {
    pub scenario: String,
    /// Seconds since the epoch.
    pub ran_at: i64,
    pub passed: bool,
    /// Set when this machine does not allow the host, carrying the command
    /// that would change that. The browser never changes it itself.
    pub refused: Option<String>,
    pub steps: Vec<MorseRunStepDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseScenarioDto {
    pub scenario: String,
    pub path: String,
    pub line: usize,
    pub spec_id: String,
    pub pin: String,
    pub env: Option<String>,
    pub steps: Vec<String>,
    /// Variable *names* the environment must provide — never values.
    pub requires: Vec<String>,
    /// `fresh` | `stale` | `broken` | `unreadable`.
    pub health: String,
    /// Commits the spec has moved since the pin, when stale.
    pub stale_by: Option<usize>,
    /// Why it cannot be run as written, or why the fence did not parse.
    pub reasons: Vec<String>,
    /// The most recent run in this workspace, gone at the next reindex.
    pub last_run: Option<MorseRunDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseReportDto {
    pub specs: Vec<MorseSpecDto>,
    pub scenarios: Vec<MorseScenarioDto>,
    /// Nothing broken or unreadable. Stale does not make it dirty.
    pub clean: bool,
}

pub fn morse_report_dto(report: &dit_core::MorseReport) -> MorseReportDto {
    MorseReportDto {
        clean: report.is_clean(),
        specs: report
            .specs
            .iter()
            .map(|s| MorseSpecDto {
                id: s.id.clone(),
                repo: s.repo.clone(),
                path: s.path.clone(),
                title: s.title.clone(),
                version: s.version.clone(),
                head: s.head.clone(),
                servers: s
                    .servers
                    .iter()
                    .map(|v| MorseServerDto {
                        url: v.url.clone(),
                        description: v.description.clone(),
                    })
                    .collect(),
                operations: s
                    .operations
                    .iter()
                    .map(|o| MorseOperationDto {
                        operation_id: o.operation_id.clone(),
                        method: o.method.clone(),
                        path: o.path.clone(),
                        summary: o.summary.clone(),
                        tag: o.tag.clone(),
                        params: o
                            .params
                            .iter()
                            .map(|p| MorseParamDto {
                                name: p.name.clone(),
                                location: p.location.clone(),
                                required: p.required,
                            })
                            .collect(),
                        body: o
                            .body
                            .iter()
                            .map(|f| MorseFieldDto {
                                name: f.name.clone(),
                                kind: f.kind.clone(),
                                required: f.required,
                            })
                            .collect(),
                        responses: o.responses.clone(),
                    })
                    .collect(),
                problem: s.problem.clone(),
            })
            .collect(),
        scenarios: report
            .scenarios
            .iter()
            .map(|s| {
                let (stale_by, reasons) = match &s.health {
                    dit_core::ScenarioHealth::Stale { commits } => (Some(*commits), Vec::new()),
                    dit_core::ScenarioHealth::Broken { reasons } => (None, reasons.clone()),
                    dit_core::ScenarioHealth::Unreadable { detail } => (None, vec![detail.clone()]),
                    dit_core::ScenarioHealth::Fresh => (None, Vec::new()),
                };
                MorseScenarioDto {
                    scenario: s.scenario.clone(),
                    path: s.path.clone(),
                    line: s.line,
                    spec_id: s.spec_id.clone(),
                    pin: s.pin.clone(),
                    env: s.env.clone(),
                    steps: s.steps.clone(),
                    requires: s.requires.clone(),
                    health: s.health.label().to_owned(),
                    stale_by,
                    reasons,
                    last_run: s.last_run.as_ref().map(|run| MorseRunDto {
                        scenario: s.scenario.clone(),
                        ran_at: run.ran_at,
                        passed: run.passed,
                        refused: run.refused.clone(),
                        steps: run.steps.iter().map(run_step_dto).collect(),
                    }),
                }
            })
            .collect(),
    }
}

fn run_step_dto(step: &dit_core::RunStepLine) -> MorseRunStepDto {
    MorseRunStepDto {
        id: step.id.clone(),
        method: step.method.clone(),
        status: step.status,
        duration_ms: step.duration_ms,
        bytes: None,
        passed: step.passed,
        detail: step.detail.clone(),
    }
}

/// What a run the browser asked for did.
pub fn morse_run_dto(outcome: &dit_core::RunOutcome) -> MorseRunDto {
    MorseRunDto {
        scenario: outcome.scenario.clone(),
        // A run just made happened now; the screen shows it as such.
        ran_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        passed: outcome.passed(),
        refused: outcome.refused.clone(),
        steps: outcome
            .steps
            .iter()
            .map(|s| MorseRunStepDto {
                id: s.id.clone(),
                method: s.method.clone(),
                status: s.status,
                duration_ms: s.duration_ms,
                bytes: s.bytes,
                passed: s.passed(),
                detail: if s.failures.is_empty() && s.error.is_none() {
                    s.captured
                        .iter()
                        .map(|(n, _)| format!("captured {n}"))
                        .collect::<Vec<_>>()
                        .join("; ")
                } else {
                    s.error
                        .clone()
                        .into_iter()
                        .chain(s.failures.iter().cloned())
                        .collect::<Vec<_>>()
                        .join("; ")
                },
            })
            .collect(),
    }
}

// ---- The Morse workbench (ADR 0023) ----------------------------------------
//
// A step on the wire, in the fence's own vocabulary. Values are text; a body
// is JSON text, as the Body tab holds it. Nothing here can name a host: the
// method and path come from the spec, the base URL from the spec or this
// machine, never from what the page sends.

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MorsePairDto {
    pub key: String,
    pub value: String,
}

/// A step's body in one of its four shapes (ADR 0027). `json` carries the
/// JSON as text, the way the Body tab edits it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[ts(export)]
pub enum MorseBodyDto {
    Json {
        text: String,
    },
    Form {
        fields: Vec<MorsePairDto>,
    },
    Raw {
        media_type: String,
        text: Option<String>,
        file: Option<String>,
    },
    Multipart {
        parts: Vec<MorsePartDto>,
    },
}

/// One change to a scenario from the screen (ADR 0027).
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum MorseScenarioEditDto {
    Rename { to: String },
    SetEnv { env: Option<String> },
    SetRequires { names: Vec<String> },
    DeleteStep { step: String },
    MoveStep { step: String, to: usize },
    DuplicateStep { step: String, as_id: String },
    RenameStep { step: String, to: String },
}

impl From<MorseScenarioEditDto> for dit_core::ScenarioEdit {
    fn from(dto: MorseScenarioEditDto) -> Self {
        use dit_core::ScenarioEdit as E;
        match dto {
            MorseScenarioEditDto::Rename { to } => E::Rename {
                to: to.trim().to_owned(),
            },
            MorseScenarioEditDto::SetEnv { env } => E::SetEnv(env),
            MorseScenarioEditDto::SetRequires { names } => E::SetRequires(names),
            MorseScenarioEditDto::DeleteStep { step } => E::DeleteStep { step },
            MorseScenarioEditDto::MoveStep { step, to } => E::MoveStep { step, to },
            MorseScenarioEditDto::DuplicateStep { step, as_id } => E::DuplicateStep {
                step,
                as_id: as_id.trim().to_owned(),
            },
            MorseScenarioEditDto::RenameStep { step, to } => E::RenameStep {
                step,
                to: to.trim().to_owned(),
            },
        }
    }
}

/// Register an OpenAPI document as a spec (ADR 0027).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseRegisterSpecDto {
    pub id: String,
    pub path: String,
    /// A `repos:` entry holding the file (Mode A); absent for this repo.
    #[serde(default)]
    #[ts(optional)]
    pub repo: Option<String>,
}

/// What to import (ADR 0027): a `curl` command or a Postman collection.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseImportDto {
    /// `curl` or `postman`.
    pub kind: String,
    pub text: String,
    /// The scenario a curl import is called.
    #[serde(default)]
    #[ts(optional)]
    pub scenario: Option<String>,
    /// The spec a `{{baseUrl}}` host, or a host nothing names, stands for.
    #[serde(default)]
    #[ts(optional)]
    pub spec: Option<String>,
    /// Where the fences go; required to import, ignored by a preview.
    #[serde(default)]
    #[ts(optional)]
    pub doc: Option<String>,
}

impl MorseImportDto {
    pub fn source(&self) -> Result<dit_core::ImportSource, String> {
        match self.kind.as_str() {
            "curl" => Ok(dit_core::ImportSource::Curl {
                command: self.text.clone(),
                scenario: self
                    .scenario
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "imported".into()),
            }),
            "postman" => Ok(dit_core::ImportSource::Postman {
                json: self.text.clone(),
            }),
            other => Err(format!(
                "`{other}` is not something Morse imports — `curl` or `postman`"
            )),
        }
    }
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseImportStepDto {
    pub id: String,
    pub method: String,
    /// `<spec>/<operationId>`, or `request <path>` for one no spec describes.
    pub target: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseImportScenarioDto {
    pub name: String,
    pub spec: String,
    pub requires: Vec<String>,
    pub steps: Vec<MorseImportStepDto>,
}

/// What an import would write — or, after an import, what it wrote.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseImportPreviewDto {
    pub scenarios: Vec<MorseImportScenarioDto>,
    pub notes: Vec<String>,
}

pub fn import_preview_dto(
    report: &dit_core::ImportReport,
    catalogue: &dit_core::MorseReport,
) -> MorseImportPreviewDto {
    MorseImportPreviewDto {
        scenarios: report
            .scenarios
            .iter()
            .map(|s| MorseImportScenarioDto {
                name: s.name.clone(),
                spec: s.spec.clone(),
                requires: s.requires.clone(),
                steps: s
                    .steps
                    .iter()
                    .map(|st| {
                        let (method, target) = match &st.operation {
                            dit_core::StepTarget::Operation(op) => (
                                catalogue
                                    .specs
                                    .iter()
                                    .find(|sp| sp.id == op.spec)
                                    .and_then(|sp| {
                                        sp.operations
                                            .iter()
                                            .find(|o| o.operation_id == op.operation)
                                    })
                                    .map(|o| o.method.clone())
                                    .unwrap_or_default(),
                                op.qualified(),
                            ),
                            dit_core::StepTarget::Inline(id) => {
                                let r = s.requests.iter().find(|r| &r.id == id);
                                (
                                    r.map(|r| r.method.clone()).unwrap_or_default(),
                                    format!(
                                        "request {}",
                                        r.map(|r| r.path.as_str()).unwrap_or("?")
                                    ),
                                )
                            }
                        };
                        MorseImportStepDto {
                            id: st.id.clone(),
                            method,
                            target,
                        }
                    })
                    .collect(),
            })
            .collect(),
        notes: report.notes.clone(),
    }
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseImportedDto {
    pub scenarios: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseImportEnvDto {
    pub text: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseEnvImportedDto {
    pub name: String,
    pub notes: Vec<String>,
    pub envs: MorseEnvsDto,
}

/// One `multipart/form-data` part: a value or a repository file.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MorsePartDto {
    pub name: String,
    pub value: Option<String>,
    pub file: Option<String>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MorseCheckDto {
    /// A JSONPath into the response body.
    pub path: String,
    /// `exists` | `equals`.
    pub rule: String,
    /// What `equals` compares with — a literal or a `{{name}}`.
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MorseCaptureDto {
    pub name: String,
    /// `$.path`, `header:Name` or `status`.
    pub from: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MorseStepDto {
    pub id: String,
    /// `<spec>/<operationId>`, or absent when the step calls `request`.
    pub operation: Option<String>,
    /// An inline request declared in the fence's `requests:`.
    pub request: Option<String>,
    pub params: Vec<MorsePairDto>,
    pub query: Vec<MorsePairDto>,
    pub headers: Vec<MorsePairDto>,
    /// Absent for no body.
    pub body: Option<MorseBodyDto>,
    pub status: Option<u16>,
    pub checks: Vec<MorseCheckDto>,
    pub capture: Vec<MorseCaptureDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MorseInlineRequestDto {
    pub id: String,
    pub method: String,
    pub path: String,
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseScenarioDetailDto {
    pub scenario: String,
    pub path: String,
    pub line: usize,
    pub spec_id: String,
    pub pin: String,
    pub env: Option<String>,
    pub requires: Vec<String>,
    pub requests: Vec<MorseInlineRequestDto>,
    pub steps: Vec<MorseStepDto>,
    /// The fence as the document holds it — what the Fence panel shows.
    pub fence: String,
    /// False when the fence has a `#` comment, which a form would drop.
    pub editable: bool,
}

/// What the Send control posts.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct MorseSendDto {
    pub env: Option<String>,
    pub step: MorseStepDto,
    /// What an inline step calls (ADR 0027): the spec whose server it goes
    /// to, and the method and path the page typed. Never a host.
    #[serde(default)]
    #[ts(optional)]
    pub request: Option<MorseSendRequestDto>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseSendRequestDto {
    pub spec: String,
    pub method: String,
    pub path: String,
}

/// A step to save, and — when it calls a request the page typed — that
/// request, so both land in one commit (ADR 0027).
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseSaveStepDto {
    #[serde(flatten)]
    pub step: MorseStepDto,
    #[serde(default)]
    #[ts(optional)]
    pub define_request: Option<MorseInlineRequestDto>,
}

pub fn inline_request_from(dto: &MorseInlineRequestDto) -> dit_core::InlineRequest {
    dit_core::InlineRequest {
        id: dto.id.trim().to_owned(),
        method: dto.method.trim().to_owned(),
        path: dto.path.trim().to_owned(),
        summary: dto.summary.clone().filter(|s| !s.trim().is_empty()),
    }
}

/// What "Save to scenario" posts for a scenario that does not exist yet.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct MorseCreateDto {
    /// The document the fence is appended to, created if absent.
    pub doc: String,
    pub name: String,
    pub spec_id: String,
    pub env: Option<String>,
    pub step: MorseStepDto,
    /// Requests the first step calls that no spec describes (ADR 0027).
    #[serde(default)]
    #[ts(optional)]
    pub requests: Option<Vec<MorseInlineRequestDto>>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseEnvDto {
    pub name: String,
    pub server: Option<String>,
    /// Variable *names*. A value never crosses this boundary (§20.6).
    pub vars: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct MorseEnvsDto {
    pub envs: Vec<MorseEnvDto>,
    pub allow_hosts: Vec<String>,
}

/// Set an environment's server and values from the page (ADR 0027). Values
/// travel one way: the answer to this is `MorseEnvsDto`, which holds names.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseEnvSetDto {
    /// Absent keeps the server; `null` clears it.
    #[serde(default, deserialize_with = "present")]
    #[ts(optional)]
    pub server: Option<Option<String>>,
    #[serde(default)]
    pub vars: Vec<MorseEnvVarSetDto>,
}

/// One variable: a value to set, or `null` to remove it.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct MorseEnvVarSetDto {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum MorseEnvEditDto {
    Rename { to: String },
}

/// A field that was sent at all, even as `null` — so "clear it" and "leave
/// it" are different requests.
fn present<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

pub fn morse_envs_dto(view: &dit_core::MorseEnvsView) -> MorseEnvsDto {
    MorseEnvsDto {
        envs: view
            .envs
            .iter()
            .map(|e| MorseEnvDto {
                name: e.name.clone(),
                server: e.server.clone(),
                vars: e.vars.clone(),
            })
            .collect(),
        allow_hosts: view.allow_hosts.clone(),
    }
}

pub fn morse_runs_dto(records: &[dit_core::MorseRunRecord]) -> Vec<MorseRunDto> {
    records
        .iter()
        .map(|r| MorseRunDto {
            scenario: r.key.clone(),
            ran_at: r.run.ran_at,
            passed: r.run.passed,
            refused: r.run.refused.clone(),
            steps: r.run.steps.iter().map(run_step_dto).collect(),
        })
        .collect()
}

fn pair_value(value: &dit_core::MorseValue) -> String {
    match value {
        dit_core::MorseValue::Str(text) => text.clone(),
        other => dit_core::morse_value_to_json(other),
    }
}

fn pairs_dto(pairs: &[(String, dit_core::MorseValue)]) -> Vec<MorsePairDto> {
    pairs
        .iter()
        .map(|(k, v)| MorsePairDto {
            key: k.clone(),
            value: pair_value(v),
        })
        .collect()
}

/// A pair's value back into a fence value. Text stays text; a value that
/// is a JSON array or object — how a structured one was shown — is read back
/// into its structure.
fn pair_from(value: &str) -> dit_core::MorseValue {
    let trimmed = value.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(parsed) = dit_core::morse_value_from_json(value) {
            return parsed;
        }
    }
    dit_core::MorseValue::Str(value.to_owned())
}

fn pairs_from(pairs: &[MorsePairDto]) -> Vec<(String, dit_core::MorseValue)> {
    pairs
        .iter()
        .filter(|p| !p.key.trim().is_empty())
        .map(|p| (p.key.trim().to_owned(), pair_from(&p.value)))
        .collect()
}

fn body_dto(body: &dit_core::RequestBody) -> MorseBodyDto {
    use dit_core::{PartContent, RawContent, RequestBody};
    match body {
        RequestBody::Json(value) => MorseBodyDto::Json {
            text: dit_core::morse_value_to_json(value),
        },
        RequestBody::Form(pairs) => MorseBodyDto::Form {
            fields: pairs_dto(pairs),
        },
        RequestBody::Raw {
            media_type,
            content,
        } => MorseBodyDto::Raw {
            media_type: media_type.clone(),
            text: match content {
                RawContent::Text(t) => Some(t.clone()),
                RawContent::File(_) => None,
            },
            file: match content {
                RawContent::File(f) => Some(f.clone()),
                RawContent::Text(_) => None,
            },
        },
        RequestBody::Multipart(parts) => MorseBodyDto::Multipart {
            parts: parts
                .iter()
                .map(|p| MorsePartDto {
                    name: p.name.clone(),
                    value: match &p.content {
                        PartContent::Value(v) => Some(pair_value(v)),
                        PartContent::File(_) => None,
                    },
                    file: match &p.content {
                        PartContent::File(f) => Some(f.clone()),
                        PartContent::Value(_) => None,
                    },
                    media_type: p.media_type.clone(),
                })
                .collect(),
        },
    }
}

/// A body from the page, checked as the fence reader checks one. An empty
/// JSON text means no body, as before there were shapes.
fn body_from(dto: &MorseBodyDto) -> Result<Option<dit_core::RequestBody>, String> {
    use dit_core::{PartContent, RawContent, RequestBody};
    let file = |path: &str| -> Result<String, String> {
        let path = path.trim();
        match dit_core::morse_file_path_problem(path) {
            None => Ok(path.to_owned()),
            Some(problem) => Err(format!("`{path}`: {problem}")),
        }
    };
    let media_type = |raw: &str| -> Result<String, String> {
        let raw = raw.trim();
        if dit_core::is_media_type(raw) {
            Ok(raw.to_owned())
        } else {
            Err(format!(
                "`{raw}` is not a media type — like `application/xml`"
            ))
        }
    };
    Ok(match dto {
        MorseBodyDto::Json { text } if text.trim().is_empty() => None,
        MorseBodyDto::Json { text } => Some(RequestBody::Json(
            dit_core::morse_value_from_json(text.trim()).map_err(|e| e.to_string())?,
        )),
        MorseBodyDto::Form { fields } => Some(RequestBody::Form(pairs_from(fields))),
        MorseBodyDto::Raw {
            media_type: kind,
            text,
            file: path,
        } => {
            let content = match (
                text,
                path.as_deref().map(str::trim).filter(|p| !p.is_empty()),
            ) {
                (_, Some(path)) => RawContent::File(file(path)?),
                (Some(text), None) => RawContent::Text(text.clone()),
                (None, None) => RawContent::Text(String::new()),
            };
            Some(RequestBody::Raw {
                media_type: media_type(kind)?,
                content,
            })
        }
        MorseBodyDto::Multipart { parts } => {
            let mut out = Vec::new();
            for part in parts {
                let name = part.name.trim();
                if name.is_empty() {
                    continue;
                }
                if name.contains(['"', '\r', '\n']) {
                    return Err(format!(
                        "part `{name}`: a name may not hold quotes or line breaks"
                    ));
                }
                let content = match part
                    .file
                    .as_deref()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                {
                    Some(path) => PartContent::File(file(path)?),
                    None => PartContent::Value(pair_from(part.value.as_deref().unwrap_or(""))),
                };
                let kind = match part
                    .media_type
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                {
                    Some(t) => Some(media_type(t)?),
                    None => None,
                };
                out.push(dit_core::MultipartPart {
                    name: name.to_owned(),
                    content,
                    media_type: kind,
                });
            }
            Some(RequestBody::Multipart(out))
        }
    })
}

pub fn morse_step_dto(step: &dit_core::MorseStep) -> MorseStepDto {
    let (operation, request) = match &step.operation {
        dit_core::StepTarget::Operation(op) => (Some(op.qualified()), None),
        dit_core::StepTarget::Inline(id) => (None, Some(id.clone())),
    };
    MorseStepDto {
        id: step.id.clone(),
        operation,
        request,
        params: pairs_dto(&step.params),
        query: pairs_dto(&step.query),
        headers: pairs_dto(&step.headers),
        body: step.body.as_ref().map(body_dto),
        status: step.expect.status,
        checks: step
            .expect
            .json
            .iter()
            .map(|c| match &c.rule {
                dit_core::ExpectRule::Exists => MorseCheckDto {
                    path: c.path.clone(),
                    rule: "exists".into(),
                    value: String::new(),
                },
                dit_core::ExpectRule::Equals(v) => MorseCheckDto {
                    path: c.path.clone(),
                    rule: "equals".into(),
                    value: v.clone(),
                },
            })
            .collect(),
        capture: step
            .capture
            .iter()
            .map(|c| MorseCaptureDto {
                name: c.name.clone(),
                from: match &c.from {
                    dit_core::Selector::Status => "status".into(),
                    dit_core::Selector::Header(h) => format!("header:{h}"),
                    dit_core::Selector::JsonPath(p) => p.clone(),
                },
            })
            .collect(),
    }
}

/// A step from the page, checked the way the fence reader would check it.
/// Every refusal names the field, so the tab can point at it.
pub fn morse_step_from(dto: &MorseStepDto) -> Result<dit_core::MorseStep, String> {
    let id = dto.id.trim();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return Err("a step id is one word".into());
    }
    let operation = match (&dto.operation, &dto.request) {
        (Some(op), None) => dit_core::StepTarget::Operation(
            dit_core::OperationRef::parse(op)
                .ok_or_else(|| format!("`{op}` is not `<spec>/<operationId>`"))?,
        ),
        (None, Some(request)) => dit_core::StepTarget::Inline(request.trim().to_owned()),
        _ => return Err("a step calls exactly one operation or one request".into()),
    };
    let body = match &dto.body {
        None => None,
        Some(shape) => body_from(shape)?,
    };
    let mut json = Vec::new();
    for check in &dto.checks {
        if check.path.trim().is_empty() {
            continue;
        }
        let rule = match check.rule.as_str() {
            "exists" => dit_core::ExpectRule::Exists,
            "equals" => dit_core::ExpectRule::Equals(check.value.clone()),
            other => return Err(format!("`{other}` is not a check — `exists` or `equals`")),
        };
        json.push(dit_core::JsonCheck {
            path: check.path.trim().to_owned(),
            rule,
        });
    }
    let mut capture = Vec::new();
    for c in &dto.capture {
        if c.name.trim().is_empty() {
            continue;
        }
        let from = dit_core::morse_selector(&c.from).ok_or_else(|| {
            format!(
                "capture `{}`: `{}` is not a selector — `$.path`, `header:Name` or `status`",
                c.name, c.from
            )
        })?;
        capture.push(dit_core::Capture {
            name: c.name.trim().to_owned(),
            from,
        });
    }
    if let Some(status) = dto.status {
        if !(100..=599).contains(&status) {
            return Err(format!("`{status}` is not an HTTP status"));
        }
    }
    Ok(dit_core::MorseStep {
        id: id.to_owned(),
        operation,
        params: pairs_from(&dto.params),
        query: pairs_from(&dto.query),
        headers: pairs_from(&dto.headers),
        body,
        expect: dit_core::Expect {
            status: dto.status,
            json,
        },
        capture,
    })
}

pub fn morse_scenario_detail_dto(detail: &dit_core::MorseScenarioDetail) -> MorseScenarioDetailDto {
    let s = &detail.scenario;
    MorseScenarioDetailDto {
        scenario: s.scenario.clone(),
        path: detail.path.clone(),
        line: detail.line,
        spec_id: s.spec.id.clone(),
        pin: s.spec.commit.clone(),
        env: s.env.clone(),
        requires: s.requires.clone(),
        requests: s
            .requests
            .iter()
            .map(|r| MorseInlineRequestDto {
                id: r.id.clone(),
                method: r.method.clone(),
                path: r.path.clone(),
                summary: r.summary.clone(),
            })
            .collect(),
        steps: s.steps.iter().map(morse_step_dto).collect(),
        fence: detail.fence.clone(),
        editable: detail.editable,
    }
}

/// The flow diagram (ADR 0019): every flow with its member count.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowSummaryDto {
    pub name: String,
    pub issues: usize,
}

/// One flow rendered as a diagram: nodes on a computed stage grid, edges
/// from `blocked_by`, the critical path highlighted.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowBoardDto {
    /// Absent for the union of every flow.
    pub name: Option<String>,
    pub lanes: Vec<FlowLaneDto>,
    /// The authored columns (ADR 0020); empty when this flow has no fence.
    pub phases: Vec<FlowPhaseDto>,
    pub groups: Vec<FlowGroupDto>,
    /// A trailing "Unphased" column is drawn.
    pub unphased: bool,
    /// Why the fence could not be used, when there is one and it could not.
    pub shape_problem: Option<FlowShapeProblemDto>,
    pub stages: usize,
    pub nodes: Vec<FlowNodeDto>,
    pub edges: Vec<FlowEdgeDto>,
    /// The critical path, root first, as issue ids.
    pub main_path: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowPhaseDto {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowGroupDto {
    pub id: String,
    pub label: String,
    /// `null` is the unlaned band.
    pub lane: Option<String>,
    /// Phase ids the frame covers.
    pub phases: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowShapeProblemDto {
    pub path: String,
    pub line: usize,
    pub detail: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowLaneDto {
    /// Absent for the Unlaned band.
    pub id: Option<String>,
    pub label: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowNodeDto {
    pub id: String,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    /// `task` | `bug` | `story` | `spike` | `chore`.
    pub kind: String,
    pub status: String,
    pub status_label: String,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub lane: Option<String>,
    /// The computed column.
    pub stage: usize,
    /// Draw order within the (lane, stage) cell.
    pub row: usize,
    /// Every phase this issue's labels claim; more than one is legal and is
    /// shown rather than hidden.
    pub phases: Vec<String>,
    /// `ready` | `not_pickable` | `blocked`.
    pub readiness: String,
    /// Blockers that are not members of this board: they gate the node but
    /// cannot be drawn, so they travel with it by name.
    pub outside_blockers: Vec<FlowOutsideBlockerDto>,
    pub claim: Option<FlowClaimDto>,
    /// Commits that have touched this issue — derived, never authored.
    pub commits: usize,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowOutsideBlockerDto {
    pub id: String,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status_label: String,
    /// Through the gate already — it no longer holds the node.
    pub satisfied: bool,
    /// Not in the index at all: a dangling reference.
    pub gone: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowClaimDto {
    pub claimed_by: String,
    pub claimed_at: String,
    pub stale: bool,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct FlowEdgeDto {
    pub from: String,
    pub to: String,
    /// `satisfied` | `unsatisfied` | `broken`.
    pub disposition: String,
    /// True for `blocked_by`, false for the non-gating `fed_by`.
    pub gating: bool,
    /// The blocker sits in a later column than what it blocks.
    pub backward: bool,
    /// What the fence says this arrow means.
    pub label: Option<String>,
}

pub fn flow_summary_dto(s: &dit_core::FlowSummary) -> FlowSummaryDto {
    FlowSummaryDto {
        name: s.name.clone(),
        issues: s.issues,
    }
}

pub fn flow_board_dto(board: &dit_core::FlowBoard) -> FlowBoardDto {
    FlowBoardDto {
        name: board.name.clone(),
        stages: board.stages,
        main_path: board
            .main_path
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
        lanes: board
            .lanes
            .iter()
            .map(|lane| FlowLaneDto {
                id: lane.id.clone(),
                label: lane.label.clone(),
            })
            .collect(),
        phases: board
            .phases
            .iter()
            .map(|p| FlowPhaseDto {
                id: p.id.clone(),
                label: p.label.clone(),
            })
            .collect(),
        groups: board
            .groups
            .iter()
            .map(|g| FlowGroupDto {
                id: g.id.clone(),
                label: g.label.clone(),
                lane: g.lane.clone(),
                phases: g.phases.clone(),
            })
            .collect(),
        unphased: board.unphased,
        shape_problem: board.shape_problem.as_ref().map(|p| FlowShapeProblemDto {
            path: p.path.clone(),
            line: p.line,
            detail: p.detail.clone(),
        }),
        nodes: board
            .nodes
            .iter()
            .map(|n| FlowNodeDto {
                id: n.id.as_str().to_owned(),
                short_ref: n.short_ref.clone(),
                number: n.number,
                title: n.title.clone(),
                kind: n.kind.as_str().to_owned(),
                status: n.status.clone(),
                status_label: n.status_label.clone(),
                category: n.category.map(|c| c.as_str().to_owned()),
                priority: n.priority.map(priority_str),
                lane: n.lane.clone(),
                stage: n.stage,
                row: n.row,
                phases: n.phases.clone(),
                readiness: match n.readiness {
                    dit_core::Readiness::Ready => "ready".into(),
                    dit_core::Readiness::NotPickable => "not_pickable".into(),
                    dit_core::Readiness::Blocked { .. } => "blocked".into(),
                    dit_core::Readiness::Unproven { .. } => "unproven".into(),
                },
                outside_blockers: n
                    .outside_blockers
                    .iter()
                    .map(|o| FlowOutsideBlockerDto {
                        id: o.id.as_str().to_owned(),
                        short_ref: o.short_ref.clone(),
                        number: o.number,
                        title: o.title.clone(),
                        status_label: o.status_label.clone(),
                        satisfied: o.satisfied,
                        gone: o.gone,
                    })
                    .collect(),
                commits: n.commits,
                claim: n.claim.as_ref().map(|c| FlowClaimDto {
                    claimed_by: c.claimed_by.clone(),
                    claimed_at: c.claimed_at.clone(),
                    stale: c.stale,
                }),
            })
            .collect(),
        edges: board
            .edges
            .iter()
            .map(|e| FlowEdgeDto {
                from: e.from.as_str().to_owned(),
                to: e.to.as_str().to_owned(),
                gating: e.gating,
                backward: e.backward,
                label: e.label.clone(),
                disposition: match e.disposition {
                    dit_core::EdgeDisposition::Satisfied => "satisfied".into(),
                    dit_core::EdgeDisposition::Unsatisfied => "unsatisfied".into(),
                    dit_core::EdgeDisposition::Broken => "broken".into(),
                },
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

/// A kind of document a page can be made from (ADR 0031).
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct DocTemplateDto {
    pub id: String,
    pub name: String,
    /// What the document answers, in a sentence.
    pub summary: String,
    /// Where a page of this kind is placed, e.g. `docs/business`.
    pub folder: String,
    pub built_in: bool,
    /// The workspace's own `docs/.templates/<id>.md` replaces the built-in.
    pub overridden: bool,
}

impl From<dit_core::DocTemplate> for DocTemplateDto {
    fn from(t: dit_core::DocTemplate) -> Self {
        DocTemplateDto {
            id: t.id,
            name: t.name,
            summary: t.summary,
            folder: t.folder,
            built_in: t.built_in,
            overridden: t.overridden,
        }
    }
}

/// Make a page from a document template.
#[derive(Debug, Deserialize, TS)]
#[ts(export)]
pub struct NewDocFromTemplateDto {
    pub kind: String,
    pub title: String,
}

/// Where an uploaded picture landed (ADR 0026), and the relative link to
/// write into the markdown that shows it.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct AttachedDto {
    pub path: String,
    pub link: String,
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

pub fn issue_dto(hit: &IndexedIssue) -> IssueDto {
    let issue = &hit.issue;
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
        fed_by: issue.fed_by.iter().map(|b| b.as_str().to_owned()).collect(),
        lane: issue.lane.clone(),
        flows: issue.flows.clone(),
        needs_scenarios: issue.needs_scenarios.clone(),
        proves: issue.proves.clone(),
        env: issue.env.clone(),
        claimed_by: issue.claimed_by.clone(),
        claimed_at: issue.claimed_at.clone(),
        created: issue.created.clone(),
        updated: issue.updated.clone(),
        body: issue.body.clone(),
        body_html: render_markdown(&issue.body),
        dir: issue_dir(&hit.path),
    }
}

pub fn indexed_dto(hit: &IndexedIssue) -> IssueDto {
    issue_dto(hit)
}

/// The folder of an issue body's repo path, relative to the content root.
/// Only the `.dit/` layout puts content below a prefix (ADR 0005), and in the
/// root layout no issue path starts with `.dit/`.
fn issue_dir(path: &str) -> String {
    let below = path.strip_prefix(".dit/").unwrap_or(path);
    below.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned()
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
    let flows = dto.flows.clone();
    let env = tri_state(ClearableField::Env, &dto.env, blank, &mut clear).cloned();
    let ids = |field: &Option<Vec<String>>| -> Result<Option<Vec<dit_core::IssueId>>, String> {
        match field {
            Some(ids) => Ok(Some(
                ids.iter()
                    .map(|text| {
                        dit_core::IssueId::parse(text)
                            .map_err(|e| format!("`{text}` is not an issue id: {e}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            None => Ok(None),
        }
    };
    let blocked_by = ids(&dto.blocked_by)?;
    let fed_by = ids(&dto.fed_by)?;
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
        fed_by,
        lane,
        flows,
        needs_scenarios: dto.needs_scenarios,
        proves: dto.proves,
        env,
        // Claims are `POST /api/issues/{id}/claim`'s to write (ADR 0015) —
        // never a generic field edit.
        claimed_by: None,
        claimed_at: None,
        clear,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod morse_boundary_tests {
    use super::*;

    fn step_with(body: MorseBodyDto) -> MorseStepDto {
        MorseStepDto {
            id: "one".into(),
            operation: Some("auth/x".into()),
            request: None,
            params: vec![],
            query: vec![],
            headers: vec![],
            body: Some(body),
            status: None,
            checks: vec![],
            capture: vec![],
        }
    }

    #[test]
    fn every_body_shape_survives_the_wire_both_ways() {
        let shapes = [
            // JSON comes back pretty-printed, the way the Body tab shows it.
            MorseBodyDto::Json {
                text: "{\n  \"a\": \"{{b}}\"\n}".into(),
            },
            MorseBodyDto::Form {
                fields: vec![MorsePairDto {
                    key: "grant_type".into(),
                    value: "password".into(),
                }],
            },
            MorseBodyDto::Raw {
                media_type: "application/xml".into(),
                text: Some("<a/>".into()),
                file: None,
            },
            MorseBodyDto::Raw {
                media_type: "image/png".into(),
                text: None,
                file: Some("fixtures/a.png".into()),
            },
            MorseBodyDto::Multipart {
                parts: vec![
                    MorsePartDto {
                        name: "t".into(),
                        value: Some("x".into()),
                        file: None,
                        media_type: None,
                    },
                    MorsePartDto {
                        name: "f".into(),
                        value: None,
                        file: Some("fixtures/a.png".into()),
                        media_type: Some("image/png".into()),
                    },
                ],
            },
        ];
        for shape in shapes {
            let step = morse_step_from(&step_with(shape.clone())).unwrap();
            let back = morse_step_dto(&step).body.unwrap();
            assert_eq!(
                serde_json::to_value(&back).unwrap(),
                serde_json::to_value(&shape).unwrap()
            );
        }
    }

    #[test]
    fn a_body_from_the_page_is_checked_like_a_fence() {
        let bad = [
            MorseBodyDto::Raw {
                media_type: "text/plain\r\nX-Evil: 1".into(),
                text: Some("a".into()),
                file: None,
            },
            MorseBodyDto::Raw {
                media_type: "application/octet-stream".into(),
                text: None,
                file: Some("../../.ssh/id_rsa".into()),
            },
            MorseBodyDto::Multipart {
                parts: vec![MorsePartDto {
                    name: "f".into(),
                    value: None,
                    file: Some(".dit/morse.local.yaml".into()),
                    media_type: None,
                }],
            },
            MorseBodyDto::Multipart {
                parts: vec![MorsePartDto {
                    name: "a\"b".into(),
                    value: Some("x".into()),
                    file: None,
                    media_type: None,
                }],
            },
        ];
        for shape in bad {
            assert!(
                morse_step_from(&step_with(shape.clone())).is_err(),
                "{shape:?}"
            );
        }
        // An empty JSON text is no body, as it was before shapes.
        let none = morse_step_from(&step_with(MorseBodyDto::Json { text: "  ".into() })).unwrap();
        assert!(none.body.is_none());
    }

    #[test]
    fn a_response_body_and_a_captured_value_never_reach_the_page() {
        let outcome = dit_core::RunOutcome {
            scenario: "send:auth/getUser".into(),
            refused: None,
            steps: vec![dit_core::StepOutcome {
                id: "getUser".into(),
                method: "GET".into(),
                url: "http://localhost/users/1".into(),
                status: Some(200),
                duration_ms: 3,
                bytes: Some(32),
                body: Some(r#"{"token":"body-secret"}"#.into()),
                failures: vec![],
                captured: vec![("token".into(), "captured-secret".into())],
                error: None,
            }],
        };
        let wire = serde_json::to_string(&morse_run_dto(&outcome)).unwrap();
        assert!(!wire.contains("body-secret"), "{wire}");
        assert!(!wire.contains("captured-secret"), "{wire}");
        assert!(
            wire.contains("captured token"),
            "that it happened does cross: {wire}"
        );
        assert!(
            wire.contains("\"bytes\":32"),
            "and so does the size: {wire}"
        );
    }
}

// -- the code map (ADR 0025) --------------------------------------------------

/// A code root the map screen can open.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeRootDto {
    pub id: String,
    /// The linked repository it reads, or `None` for this one.
    pub repo: Option<String>,
    /// The pinned branch or tag, when there is one.
    pub git_ref: Option<String>,
    pub files: usize,
}

/// The roots, after a refresh to HEAD.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeRootsDto {
    pub roots: Vec<CodeRootDto>,
    /// Files parsed by the refresh this answer ran.
    pub parsed: usize,
    /// Roots that could not be read, with why.
    pub problems: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeUnitDto {
    pub path: String,
    pub folder: bool,
    pub files: usize,
    pub generated: usize,
    pub inbound: usize,
    pub outbound: usize,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeUnitEdgeDto {
    pub from: String,
    pub to: String,
    pub imports: usize,
}

/// One folder of a root, drawn.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeOverviewDto {
    pub root: String,
    pub folder: String,
    pub units: Vec<CodeUnitDto>,
    pub edges: Vec<CodeUnitEdgeDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeNeighbourDto {
    pub path: String,
    pub generated: bool,
    pub users: usize,
    pub names: Vec<String>,
    pub via: Option<String>,
}

/// One file in focus, with both of its sides.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeNeighbourhoodDto {
    pub root: String,
    pub path: String,
    pub generated: bool,
    pub defines: Vec<String>,
    pub users: Vec<CodeNeighbourDto>,
    pub uses: Vec<CodeNeighbourDto>,
    pub external: Vec<String>,
    pub api_calls: Vec<CodeApiCallDto>,
}

/// One API path a file calls, and the operations it reaches.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeApiCallDto {
    pub line: u32,
    /// With the constants it is built from put back in.
    pub path: String,
    /// Empty for an orphan: no registered spec describes it.
    pub operations: Vec<CodeApiOperationDto>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeApiOperationDto {
    pub spec: String,
    pub operation_id: String,
    pub method: String,
    pub path: String,
    /// Environments where a scenario exercising it holds a fresh proof.
    pub proven: Vec<String>,
}

pub fn code_overview_dto(o: &dit_core::CodeOverview) -> CodeOverviewDto {
    CodeOverviewDto {
        root: o.root.clone(),
        folder: o.folder.clone(),
        units: o
            .units
            .iter()
            .map(|u| CodeUnitDto {
                path: u.path.clone(),
                folder: u.folder,
                files: u.files,
                generated: u.generated,
                inbound: u.inbound,
                outbound: u.outbound,
            })
            .collect(),
        edges: o
            .edges
            .iter()
            .map(|e| CodeUnitEdgeDto {
                from: e.from.clone(),
                to: e.to.clone(),
                imports: e.imports,
            })
            .collect(),
    }
}

fn code_neighbour_dto(n: &dit_core::CodeNeighbour) -> CodeNeighbourDto {
    CodeNeighbourDto {
        path: n.path.clone(),
        generated: n.generated,
        users: n.users,
        names: n.names.clone(),
        via: n.via.clone(),
    }
}

pub fn code_neighbourhood_dto(n: &dit_core::CodeNeighbourhood) -> CodeNeighbourhoodDto {
    CodeNeighbourhoodDto {
        root: n.root.clone(),
        path: n.path.clone(),
        generated: n.generated,
        defines: n.defines.clone(),
        users: n.users.iter().map(code_neighbour_dto).collect(),
        uses: n.uses.iter().map(code_neighbour_dto).collect(),
        external: n.external.clone(),
        api_calls: n
            .api_calls
            .iter()
            .map(|c| CodeApiCallDto {
                line: c.line,
                path: c.resolved.clone(),
                operations: c
                    .operations
                    .iter()
                    .map(|o| CodeApiOperationDto {
                        spec: o.spec.clone(),
                        operation_id: o.operation_id.clone(),
                        method: o.method.clone(),
                        path: o.path.clone(),
                        proven: o.proven.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// One file of the whole-root network.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeGraphFileDto {
    pub path: String,
    pub generated: bool,
    /// Files importing it.
    pub users: usize,
}

/// Every file of a root and every import between them. Edges are pairs of
/// indexes into `files`, importer first — the whole network is thousands of
/// edges, and paths repeated in each would multiply the payload.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct CodeGraphDto {
    pub root: String,
    pub files: Vec<CodeGraphFileDto>,
    pub edges: Vec<(u32, u32)>,
}

pub fn code_graph_dto(g: &dit_core::CodeGraph) -> CodeGraphDto {
    CodeGraphDto {
        root: g.root.clone(),
        files: g
            .files
            .iter()
            .map(|(path, generated, users)| CodeGraphFileDto {
                path: path.clone(),
                generated: *generated,
                users: *users,
            })
            .collect(),
        edges: g.edges.clone(),
    }
}
