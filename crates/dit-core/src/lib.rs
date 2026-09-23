//! The facade — the only API the CLI, the server and the merge driver use.
//!
//! Reading and writing are separate surfaces on purpose. Every read answers
//! from the SQLite index and never opens a file: that is what keeps queries
//! fast and, more importantly, what guarantees there is exactly one
//! interpretation of "what is in this workspace" — the one the index
//! computed from git. Every write goes through a [`Transaction`] that
//! stages changes, writes them through `dit fmt`, puts them in one git
//! commit, and only then updates the index to match.
//!
//! Everything here is blocking. The two slow things underneath — running
//! git and querying SQLite — are blocking operations, and wrapping them in
//! async would infect every signature for no real parallelism: there is one
//! writer per workspace, guarded by a lock file.

mod agent;
pub mod board;
pub mod diagnostics;
pub mod error;
pub mod flow;
pub mod morse;

/// Where Morse keeps environment values, server overrides and the host
/// allowlist — this machine only, gitignored from `dit init` onwards.
pub const MORSE_LOCAL_PATH: &str = ".dit/morse.local.yaml";
pub mod watch;
pub mod workflow;

use std::path::Path;

use dit_index::Index;
use dit_store::atomic::LockGuard;
use dit_store::{Changeset, Store};
use dit_vcs::{Repo, EMPTY_TREE};
use time::OffsetDateTime;

/// The sanctioned disk writer (invariant I1): every write outside a
/// `Transaction` — scaffolding, drafts — goes through here, never a direct
/// `fs::write`.
pub use dit_store::atomic;

pub use board::{Board, BoardColumn};
pub use diagnostics::{Diagnostic, DiagnosticLevel};
pub use error::DitError;
// Types from below the facade that appear in its signatures. Callers
// construct arguments out of these, so they must be reachable without a
// second dependency — the facade is the only crate delivery names.
pub use agent::{AgentDocOptions, AgentDocReport, AGENT_DOC_PATH};
pub use dit_index::{IndexedIssue, IndexedRelease, WorkspaceComment};
pub use dit_model::{
    claim_liveness, validate_date, validate_release_version, ChangeSummary, ClaimLiveness,
    ClearableField, Comment, Config, DataLayout, DayCount, DerivedSignal, DocEntry, DocPath,
    DocPathError, FieldPatch, Gate, Issue, IssueDraft, IssueId, IssueKind, Numbering, Priority,
    Readiness, Release, ReleasePatch, ReleaseStatus, StatusCategory, StoredFieldEvent, Workflow,
    WorkflowStatus, CONTENT_ROOTS, DOC_ROOTS, GENERATED_INDEX_MARKER,
};
pub use dit_model::{
    Capture, Expect, ExpectRule, JsonCheck, MorseScenario, MorseStep, MorseValue, OperationRef,
    Selector, SpecField, SpecOperation, SpecParam, SpecServer, StepTarget,
};
pub use dit_model::{FlowGroup, FlowPhase, FlowShape, PHASE_LABEL_PREFIX};
pub use dit_vcs::{SyncOptions, SyncReport};
pub use flow::{
    EdgeDisposition, FlowBoard, FlowClaim, FlowEdge, FlowLane, FlowNode, FlowOutsideBlocker,
    FlowSummary,
};
pub use morse::{morse_selector, morse_value_from_json, morse_value_to_json};
pub use morse::{
    LastRun, MorseEnvView, MorseEnvsView, MorseReport, MorseRunRecord, MorseScenarioDetail,
    MorseScenarioView, MorseSpecView, RunStepLine, ScenarioHealth, SendDraft, SyncOutcome,
    SEND_KEY_PREFIX,
};
// Delivery reports what a run did, so the shapes it reports come through
// the facade rather than making every caller depend on the adapter.
pub use dit_morse::{RunOutcome, StepOutcome};
pub use watch::spawn as spawn_watcher;
pub use workflow::InboxItem;

/// Which parts of the index to rebuild. The state half is cheap (read every
/// file at HEAD); the history half walks the whole commit graph, so it gets
/// its own mode and a watermark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexMode {
    /// Re-read every issue and comment at HEAD. Keeps history.
    State,
    /// Walk commits newer than the watermark and record their field events.
    Events,
    /// Start over: clear everything, rebuild state and history.
    All,
}

/// What a rebuild did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexReport {
    pub issues: usize,
    pub comments: usize,
    pub events: usize,
    /// Files that exist at HEAD but would not parse. They are skipped rather
    /// than fatal — one hand-broken file must not hide every other issue.
    pub skipped: usize,
    pub head: String,
}

/// Head/branch/dirty facts for a status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoStatus {
    pub branch: String,
    pub head: String,
    pub dirty: bool,
}

/// One ready issue with its derived readiness (ADR 0015).
#[derive(Debug, Clone, PartialEq)]
pub struct ReadyIssue {
    pub issue: IndexedIssue,
    pub readiness: Readiness,
}

/// What kind of claim operation to run (ADR 0015). The flags are mutually
/// exclusive at the CLI edge; the facade treats `release` first, then
/// `renew`, as the plain claim.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaimOptions {
    /// Skip the readiness guard and refresh an existing claim's timestamp.
    pub renew: bool,
    /// Take the issue over from another actor's live claim.
    pub takeover: bool,
    /// Clear the claim pair entirely.
    pub release: bool,
    /// The human escape hatch: write the claim regardless of any guard.
    pub force: bool,
}

/// What `claim` did — `wrote: false` means no commit was needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimReport {
    pub wrote: bool,
    pub note: String,
}

/// One lane for `init_workflow` to register: id (charset-checked like a
/// status id), display label, and the advisory owner aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSpec {
    pub id: String,
    pub label: String,
    pub owners: Vec<String>,
}

/// What `init_workflow` wrote. A second run reports all-false and writes
/// nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkflowInitReport {
    pub schema_created: bool,
    pub lanes_written: bool,
    pub coordination_written: bool,
    pub report_template_written: bool,
}

/// A flow's authored shape as the workspace holds it, with where it came
/// from — so a fence that does not parse can be named rather than ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowShapeRef {
    /// The document the fence was found in.
    pub path: String,
    /// The fence's opening line.
    pub line: usize,
    /// Why the fence could not be used, if it could not.
    pub problem: Option<String>,
    /// The parsed shape; `None` exactly when `problem` is set.
    pub shape: Option<dit_model::FlowShape>,
}

/// Whether a path is a document page — the only files a `dit-flow` fence
/// can live in. Deliberately loose: a fence is found by its info string,
/// not by where someone filed the page.
fn is_doc_path(path: &str) -> bool {
    path.ends_with(".md")
        && dit_model::DOC_ROOTS
            .iter()
            .any(|root| path == *root || path.contains(&format!("{root}/")))
}

/// The flow a fence names, read without parsing the rest. A fence that fails
/// on line nine still knows which diagram it was shaping on line one, and
/// that is what lets the screen report the failure instead of swallowing it.
fn name_in_fence(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("flow:"))
        .map(|name| name.trim().trim_matches('"').trim_matches('\'').to_owned())
        .filter(|name| !name.is_empty())
}

/// An open workspace: git repo + files on one side, the index on the other.
pub struct Dit {
    store: Store,
    index: Index,
    repo: Repo,
    workflow: Workflow,
    config: Config,
    /// Why `workflow` fell back to the default, if it did. A broken
    /// workflow.yaml must not stop the workspace from opening — reading and
    /// writing still work — but it must be visible in `doctor`.
    schema_problem: Option<String>,
}

impl std::fmt::Debug for Dit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The index connection and git handle don't print anything useful;
        // where the workspace lives is the identity that matters in a log.
        f.debug_struct("Dit")
            .field("root", &self.repo.root())
            .finish()
    }
}

impl Dit {
    /// Open a workspace. The directory must be a git repository; `.dit/`
    /// itself may be missing or empty (the first transaction bootstraps it).
    pub fn open(workspace: &Path) -> Result<Dit, DitError> {
        let repo = Repo::open(workspace)?;
        let store = Store::open(repo.root());
        let index = Index::open(&store.layout().cache_dir().join("index.sqlite"))?;
        let mut dit = Dit {
            store,
            index,
            repo,
            workflow: Workflow::default_workflow(),
            config: Config::default(),
            schema_problem: None,
        };
        dit.reload_schema();
        // Self-heal the cache before the first read: a version bump drops it,
        // a fresh clone lacks it, and another process's commits move HEAD
        // past the watermark — none of those should ever surface as an empty
        // board. Cheap when current (one git call, one sqlite read); a
        // failure is logged, never fatal — reads answer empty and `doctor`
        // says so, which is better than refusing to open the workspace.
        if let Err(e) = dit.refresh_state() {
            tracing::warn!("startup index refresh failed: {e} — run `dit reindex`");
        }
        Ok(dit)
    }

    /// Turn an empty directory into a workspace with the visible layout
    /// (ADR 0005): content roots at the tree root, machinery under `.dit/`.
    /// Delegates to [`Dit::init_with_layout`].
    pub fn init(path: &Path, driver: &Path) -> Result<Dit, DitError> {
        Self::init_with_layout(path, driver, DataLayout::Root)
    }

    /// `init` with an explicit layout — `dotdir` keeps everything under
    /// `.dit/` for guest repos (Mode C) and anyone who prefers the hidden
    /// style.
    ///
    /// Scaffolding: git init, the ignore entry that keeps the index database
    /// out of history, a README so a fresh workspace does not look empty, the
    /// merge-driver routing at the mode-correct place, the config that states
    /// where data goes, seeded issue templates, the five content roots, and
    /// the merge driver pointed at `driver` (the absolute path of this
    /// binary), all in one bootstrap commit so the first real write is never
    /// a root commit.
    ///
    /// `driver` is passed in rather than read from `current_exe()` because a
    /// test process is not the binary users run — the caller knows which
    /// executable actually serves `merge-driver`.
    pub fn init_with_layout(
        path: &Path,
        driver: &Path,
        layout: DataLayout,
    ) -> Result<Dit, DitError> {
        // The guards run before `Repo::init` so a refusal leaves the
        // directory exactly as it was — not even a .git/ behind.
        let re_init = path.join(".dit").join("config.yaml").exists();
        if !re_init {
            if path.join(".dit").join("issues").is_dir() {
                return Err(DitError::Refuse(
                    "this workspace already has DIT data under .dit/issues/ — run \
                     `dit migrate-layout root` to move it to the tree root"
                        .into(),
                ));
            }
            if layout == DataLayout::Root {
                let clash: Vec<String> = CONTENT_ROOTS
                    .iter()
                    .filter(|root| path.join(*root).exists())
                    .map(|root| format!("`{root}/`"))
                    .collect();
                if !clash.is_empty() {
                    return Err(DitError::Refuse(format!(
                        "{} already exist here and DIT reserves those names for its \
                         content roots — use `dit init --layout dotdir` to keep data \
                         under .dit/ instead",
                        clash.join(", ")
                    )));
                }
            }
        }
        let repo = Repo::init(path)?;
        // Commits fail outright without an identity. A machine-local default
        // keeps init working on a fresh computer; anyone who cares replaces
        // it with their real name afterwards.
        if !repo.has_identity() {
            let who = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "dit".to_owned());
            repo.set_identity(&who, &format!("{who}@dit.local"))?;
        }
        // Scaffolding files land through the atomic writer like everything
        // else: a half-written .gitignore is how the cache ends up committed.
        // But init also runs in directories that already hold a project —
        // joining one must never colonize it, so every piece is written only
        // when missing, never replaced.
        let dit_dir = path.join(".dit");
        let content_dir = |name: &str| match layout {
            DataLayout::Root => path.join(name),
            DataLayout::DotDir => dit_dir.join(name),
        };
        let ignore = path.join(".gitignore");
        // Two entries, for two different reasons. `.dit-cache/` is
        // disposable. `.dit/morse.local.yaml` holds environment values and
        // secrets (§20.6), and a secret that reaches git history cannot be
        // taken back by deleting the file — so the guard is here, at init,
        // and not at review time.
        let wanted = [
            ("# DIT's disposable index — never committed.", ".dit-cache/"),
            (
                "# Morse environments and their secrets — this machine only.",
                MORSE_LOCAL_PATH,
            ),
        ];
        if ignore.exists() {
            let mut text = std::fs::read_to_string(&ignore)?;
            let mut changed = false;
            for (why, entry) in wanted {
                if text.lines().any(|l| l.trim() == entry) {
                    continue;
                }
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&format!("\n{why}\n{entry}\n"));
                changed = true;
            }
            if changed {
                dit_store::atomic::write(&ignore, &text)?;
            }
        } else {
            let text = wanted
                .iter()
                .map(|(why, entry)| format!("{why}\n{entry}\n"))
                .collect::<Vec<_>>()
                .join("\n");
            dit_store::atomic::write(&ignore, &text)?;
        }
        let readme = path.join("README.md");
        let wrote_readme = !readme.exists();
        if wrote_readme {
            // Only for repos that would otherwise look empty — a project
            // that wrote its own README keeps it.
            dit_store::atomic::write(&readme, INIT_README)?;
        }
        // The config states where data goes (ADR 0005) — it is also what
        // `Layout::detect` resolves on every later open.
        let config_path = dit_dir.join("config.yaml");
        if !config_path.exists() {
            let config = Config {
                layout,
                ..Config::default()
            };
            dit_store::atomic::write(&config_path, &dit_parse::write_config(&config))?;
        }
        // Merge-driver routing at the mode-correct place (ADR 0005).
        let attrs_path = match layout {
            DataLayout::Root => path.join(".gitattributes"),
            DataLayout::DotDir => dit_dir.join(".gitattributes"),
        };
        if !attrs_path.exists() {
            dit_store::atomic::write(&attrs_path, GIT_ATTRIBUTES)?;
        }
        // Issue templates (the evidence-first shape: summary grounded in
        // code pointers, plan, rejected alternative, assertable criteria,
        // tests, scope fence): seeded once; hand edits survive every later
        // init.
        let templates = [
            ("default", TEMPLATE_DEFAULT),
            ("bug", TEMPLATE_BUG),
            ("story", TEMPLATE_STORY),
            ("spike", TEMPLATE_SPIKE),
        ];
        for (name, text) in templates {
            let file = dit_dir.join("templates").join(format!("{name}.md"));
            if !file.exists() {
                dit_store::atomic::write(&file, text)?;
            }
        }
        // The five content roots exist locally from the first `ls` — git has
        // no empty directories, so a clone regrows them on first write.
        for name in CONTENT_ROOTS {
            let dir = content_dir(name);
            if !dir.is_dir() {
                std::fs::create_dir_all(&dir)?;
            }
        }
        repo.add(".gitignore")?;
        if wrote_readme {
            repo.add("README.md")?;
        }
        // `.dit` carries config, templates, and — in dotdir — the roots and
        // the attributes file. In root layout the attributes file is staged
        // on its own; the empty content roots have nothing to stage.
        repo.add(".dit")?;
        if layout == DataLayout::Root && attrs_path.exists() {
            repo.add(".gitattributes")?;
        }
        // In an already-initialized workspace nothing changed, and commit
        // treats "nothing to commit" as a skip, not a failure.
        repo.commit("dit init")?;
        repo.configure_merge_driver(&driver_command(driver))?;
        Dit::open(path)
    }

    /// Point this repository's merge driver at `driver`. Safe to re-run.
    pub fn install_merge_driver(&self, driver: &Path) -> Result<(), DitError> {
        self.repo.configure_merge_driver(&driver_command(driver))?;
        Ok(())
    }

    /// Re-read workflow.yaml and config.yaml from disk. Called when opening
    /// and after any rebuild, so hand edits to the schema take effect on the
    /// next open instead of being masked by a stale copy.
    fn reload_schema(&mut self) {
        self.schema_problem = None;
        let layout = self.store.layout();
        match std::fs::read_to_string(layout.workflow_yaml()) {
            Ok(text) => match dit_parse::parse_workflow(&text) {
                Ok(wf) => self.workflow = wf,
                Err(e) => {
                    self.workflow = Workflow::default_workflow();
                    self.schema_problem = Some(format!("workflow.yaml: {e}"));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.schema_problem = Some(format!("workflow.yaml: {e}")),
        }
        match std::fs::read_to_string(layout.config_yaml()) {
            Ok(text) => match dit_parse::parse_config(&text) {
                Ok(cfg) => self.config = cfg,
                Err(e) => {
                    self.config = Config::default();
                    self.schema_problem = Some(self.schema_problem.take().map_or_else(
                        || format!("config.yaml: {e}"),
                        |prior| format!("{prior}; config.yaml: {e}"),
                    ));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.schema_problem = Some(format!("config.yaml: {e}")),
        }
    }

    // -- reads ----------------------------------------------------------------

    /// Where this workspace lives — the repo root, not the directory passed
    /// to `open` (which may have been a subdirectory).
    pub fn root(&self) -> &Path {
        self.repo.root()
    }

    /// The workflow this workspace runs on.
    pub fn workflow(&self) -> &Workflow {
        &self.workflow
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Where this workspace's content roots live (ADR 0005) — the one bit
    /// every consumer branches on.
    pub fn layout(&self) -> DataLayout {
        self.store.layout().kind()
    }

    /// The workspace's issue-template names, alphabetical — what `dit
    /// templates list` shows and the UI offers.
    pub fn templates(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.store.layout().templates_dir())
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        name.strip_suffix(".md").map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        names.sort_unstable();
        names
    }

    /// Path of a named issue template. None when the name could not be a
    /// template file (shape check only — existence is the caller's question).
    pub fn template_path(&self, name: &str) -> Option<std::path::PathBuf> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return None;
        }
        Some(
            self.store
                .layout()
                .templates_dir()
                .join(format!("{name}.md")),
        )
    }

    /// A template's body text, when it exists.
    fn template_text(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.template_path(name)?).ok()
    }

    /// The alias this clone attributes writes to, if one was saved with
    /// [`Dit::set_me`]. Callers layer their own precedence on top (an
    /// explicit flag or `DIT_ME` wins; `$USER` is the last guess).
    pub fn me(&self) -> Option<String> {
        self.repo.alias()
    }

    /// Save the alias later writes are attributed to. It lives in the
    /// clone's git config — the same place as the git identity, and the
    /// honest one: it is a fact about who sits at this clone, not workspace
    /// data, so it is never committed and never shared. The shape is the
    /// comment-file rule (lowercase letters, digits, dashes), refused here so
    /// no later comment fails on a name that cannot be a file name.
    pub fn set_me(&self, alias: &str) -> Result<(), DitError> {
        let alias = alias.trim();
        if alias.is_empty() {
            return Err(DitError::Refuse("an alias is required".into()));
        }
        if !alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(DitError::Refuse(format!(
                "`{alias}` cannot be an alias — use lowercase letters, digits and dashes"
            )));
        }
        self.repo.set_alias(alias)?;
        Ok(())
    }

    /// Branch, head and dirtiness for status displays. Best effort: an
    /// unborn branch (no commits yet) still reports, with an empty head.
    pub fn status(&self) -> RepoStatus {
        let branch = self
            .repo
            .current_branch()
            .unwrap_or_else(|_| "(unborn)".into());
        let head = self.repo.head().unwrap_or_default();
        // A git status that cannot run must not read as "everything is
        // committed" — reporting a dirty tree is the safe direction.
        let dirty = !self.repo.is_clean().unwrap_or(false);
        RepoStatus {
            branch,
            head,
            dirty,
        }
    }

    /// One issue by full id, 7-character short ref, or `#N` number handle
    /// (ADR 0007 — `#12` is display sugar over the frontmatter number, and
    /// `#Q2R7VN8` the same sugar over a short ref). Answers from the index
    /// like every read; the filesystem is never consulted.
    pub fn get(&self, needle: &str) -> Result<Option<IndexedIssue>, DitError> {
        let stripped = needle.strip_prefix('#').unwrap_or(needle);
        if needle.starts_with('#') {
            if let Ok(n) = stripped.parse::<u32>() {
                if n > 0 {
                    return Ok(self.index.issues_with_number(n)?.into_iter().next());
                }
            }
        }
        if stripped.len() == 26 {
            if let Ok(id) = IssueId::parse(stripped) {
                return self.index.get_issue(&id).map_err(DitError::Index);
            }
        }
        // Short refs go through the same compiled-query path as user queries,
        // so there is one resolution mechanism, not two.
        let query = dit_query::Query {
            filter: Some(dit_query::Expr::Cmp {
                field: dit_query::Field::ShortRef,
                op: dit_query::Op::Eq,
                value: dit_query::Val::Str(stripped.to_owned()),
            }),
            order: vec![],
            limit: Some(2),
        };
        let compiled = dit_query::compile(&query, None, OffsetDateTime::now_utc())
            .map_err(dit_query::QueryError::from)?;
        let hits = self.index.list_issues(&compiled)?;
        Ok(hits.into_iter().next())
    }

    /// Run a DQL query. `me` is the current user's alias — the one piece of
    /// context `@me` needs that cannot be derived from the workspace.
    pub fn query(&self, dql: &str, me: Option<&str>) -> Result<Vec<IndexedIssue>, DitError> {
        let compiled = dit_query::compile_str(dql, me, OffsetDateTime::now_utc())?;
        Ok(self.index.list_issues(&compiled)?)
    }

    /// Resolve a reference to exactly one issue, or refuse (ADR 0018). The
    /// same ladder as [`Dit::get`] — `#N`, full id, short ref — but a
    /// reference matching several issues (duplicate numbers exist in real
    /// workspaces) is [`DitError::Ambiguous`] naming the candidates, never a
    /// silently chosen first hit. Every *acting* path resolves through here;
    /// `get` keeps its display semantics.
    pub fn resolve(&self, needle: &str) -> Result<IssueId, DitError> {
        let stripped = needle.strip_prefix('#').unwrap_or(needle);
        if needle.starts_with('#') {
            if let Ok(n) = stripped.parse::<u32>() {
                if n > 0 {
                    let holders = self.index.issues_with_number(n)?;
                    return match holders.as_slice() {
                        [one] => Ok(one.issue.id),
                        [] => Err(DitError::NotFound(needle.to_owned())),
                        many => Err(ambiguous(
                            needle,
                            many.iter()
                                .map(|h| (h.issue.id.as_str().to_owned(), h.issue.title.clone()))
                                .collect(),
                        )),
                    };
                }
            }
        }
        if stripped.len() == 26 {
            if let Ok(id) = IssueId::parse(stripped) {
                return match self.index.get_issue(&id)? {
                    Some(_) => Ok(id),
                    None => return Err(DitError::NotFound(needle.to_owned())),
                };
            }
        }
        let query = dit_query::Query {
            filter: Some(dit_query::Expr::Cmp {
                field: dit_query::Field::ShortRef,
                op: dit_query::Op::Eq,
                value: dit_query::Val::Str(stripped.to_owned()),
            }),
            order: vec![],
            limit: Some(2),
        };
        let compiled = dit_query::compile(&query, None, OffsetDateTime::now_utc())
            .map_err(dit_query::QueryError::from)?;
        match self.index.list_issues(&compiled)?.as_slice() {
            [one] => Ok(one.issue.id),
            [] => Err(DitError::NotFound(needle.to_owned())),
            many => Err(ambiguous(
                needle,
                many.iter()
                    .map(|h| (h.issue.id.as_str().to_owned(), h.issue.title.clone()))
                    .collect(),
            )),
        }
    }

    /// Issues this actor may pick up right now (ADR 0015): `pick_from`
    /// status, lane-matching, every blocker through the gate. `until`
    /// overrides the configured gate for this call only ("review-or-later"),
    /// never writing anything back. Readiness is derived — nothing here
    /// touches a file.
    pub fn ready(
        &self,
        lane: Option<&str>,
        until: Option<&str>,
    ) -> Result<Vec<ReadyIssue>, DitError> {
        let gate = match until {
            Some(s) => {
                if !self.workflow.contains_status(s) {
                    return Err(DitError::Refuse(format!(
                        "`{s}` is not one of this workflow's statuses — --until takes a status id"
                    )));
                }
                Some(Gate::Until(s.to_owned()))
            }
            None => None,
        };
        let all = self.query("", None)?;
        let by_id: std::collections::HashMap<IssueId, &str> = all
            .iter()
            .map(|h| (h.issue.id, h.issue.status.as_str()))
            .collect();
        let mut out = Vec::new();
        for hit in &all {
            if let Some(want) = lane {
                if hit.issue.lane.as_deref() != Some(want) {
                    continue;
                }
            }
            let blockers: Vec<(IssueId, String)> = hit
                .issue
                .blocked_by
                .iter()
                .map(|b| {
                    // A blocker that is not in the index (deleted) is broken:
                    // the dependency needs a human to re-point it, not a
                    // silent unblock.
                    let status = by_id
                        .get(b)
                        .map(|s| (*s).to_owned())
                        .unwrap_or_else(|| "cancelled".to_owned());
                    (*b, status)
                })
                .collect();
            let r =
                dit_model::readiness(&hit.issue.status, &blockers, &self.workflow, gate.as_ref());
            if matches!(r, Readiness::Ready) {
                out.push(ReadyIssue {
                    issue: hit.clone(),
                    readiness: r,
                });
            }
        }
        Ok(out)
    }

    /// Claim an issue as an actor's exclusive intent, renew a claim, take one
    /// over, or release it (ADR 0015) — one commit when it writes. The CLI
    /// guards live here, not in docs: claiming a terminal issue, an issue
    /// with unsatisfied or broken blockers, or an issue another actor holds
    /// live is refused with the way out named. A claim is advisory, never a
    /// lock: staleness only unlocks takeover.
    pub fn claim(
        &mut self,
        id: &IssueId,
        actor: &str,
        opts: ClaimOptions,
    ) -> Result<ClaimReport, DitError> {
        let target = self
            .index
            .get_issue(id)?
            .ok_or_else(|| DitError::NotFound(id.as_str().to_owned()))?;
        let issue = &target.issue;
        let ttl = self.workflow.coordination.claim_ttl_minutes;
        let liveness = claim_liveness(
            issue.claimed_by.as_deref(),
            issue.claimed_at.as_deref(),
            OffsetDateTime::now_utc(),
            ttl,
        );
        let holder = issue.claimed_by.clone();

        if opts.release {
            if let (Some(holder), ClaimLiveness::Live) = (&holder, liveness) {
                if holder != actor && !opts.force {
                    return Err(DitError::Refuse(format!(
                        "{} is claimed live by `{holder}` — only they may release it, or pass \
                         --force",
                        id.short_ref().as_str()
                    )));
                }
            }
            let mut tx = self.transaction(actor)?;
            tx.set_fields(
                id,
                FieldPatch {
                    clear: vec![ClearableField::ClaimedBy, ClearableField::ClaimedAt],
                    ..FieldPatch::default()
                },
            )?;
            tx.commit(&format!("release {}: {}", id.short_ref().as_str(), actor))?;
            return Ok(ClaimReport {
                wrote: true,
                note: "released".into(),
            });
        }

        // The readiness guard: no claiming work that cannot be picked up.
        // `--renew` refreshes an existing claim and skips it (the issue may
        // legitimately have moved on since).
        if !opts.renew {
            if self.workflow.is_terminal(&issue.status) {
                return Err(DitError::Refuse(format!(
                    "`{}` is a terminal status — a finished issue cannot be claimed",
                    issue.status
                )));
            }
            let blockers: Vec<(IssueId, String)> = issue
                .blocked_by
                .iter()
                .map(|b| {
                    let status = self
                        .index
                        .get_issue(b)?
                        .map(|t| t.issue.status.clone())
                        // A blocker missing from the index is broken.
                        .unwrap_or_else(|| "cancelled".to_owned());
                    Ok((*b, status))
                })
                .collect::<Result<_, DitError>>()?;
            if let Readiness::Blocked {
                unsatisfied,
                broken,
            } = dit_model::readiness(&issue.status, &blockers, &self.workflow, None)
            {
                if !opts.force {
                    let unsatisfied_list: Vec<String> = unsatisfied
                        .iter()
                        .map(|b| b.short_ref().as_str().to_owned())
                        .collect();
                    let broken_list: Vec<String> = broken
                        .iter()
                        .map(|b| b.short_ref().as_str().to_owned())
                        .collect();
                    let mut why = String::new();
                    if !unsatisfied_list.is_empty() {
                        why.push_str(&format!(
                            "blockers not through the gate: {}",
                            unsatisfied_list.join(", ")
                        ));
                    }
                    if !broken_list.is_empty() {
                        if !why.is_empty() {
                            why.push_str("; ");
                        }
                        why.push_str(&format!(
                            "broken dependencies (cancelled or gone): {}",
                            broken_list.join(", ")
                        ));
                    }
                    return Err(DitError::Refuse(format!(
                        "blocked — {why}. Clear the blockers, or pass --force to claim anyway"
                    )));
                }
            }
        }

        // The exclusivity guard.
        match (&holder, liveness) {
            (Some(other), ClaimLiveness::Live) if other != actor => {
                if !opts.takeover && !opts.force {
                    return Err(DitError::Refuse(format!(
                        "claimed live by `{other}` (within the last {ttl} min) — pass \
                         --takeover to take it over"
                    )));
                }
            }
            (Some(other), ClaimLiveness::Stale) if other != actor => {
                // A stale claim is takable by plain `claim`; reaching for
                // `--renew` on someone else's claim reads like a mistake, so
                // name the right move.
                if opts.renew && !opts.takeover && !opts.force {
                    return Err(DitError::Refuse(format!(
                        "`{other}`'s claim is stale — claim it plainly (no --renew), or \
                         --takeover if you want the intent recorded as a takeover"
                    )));
                }
            }
            (Some(own), ClaimLiveness::Live) if own == actor && !opts.renew && !opts.force => {
                return Ok(ClaimReport {
                    wrote: false,
                    note: format!(
                        "already claimed by you (live, TTL {ttl} min) — --renew refreshes it"
                    ),
                });
            }
            _ => {}
        }

        let now = dit_model::format_rfc3339(OffsetDateTime::now_utc());
        let mut tx = self.transaction(actor)?;
        tx.set_fields(
            id,
            FieldPatch {
                claimed_by: Some(actor.to_owned()),
                claimed_at: Some(now),
                ..FieldPatch::default()
            },
        )?;
        tx.commit(&format!("claim {} as {}", id.short_ref().as_str(), actor))?;
        Ok(ClaimReport {
            wrote: true,
            note: format!("claimed as {actor}"),
        })
    }

    /// Bring the state index up to date when HEAD moved past the watermark
    /// (ADR 0017): a write made by another process. Own-process writes move
    /// the watermark in `absorb_commit`, so this is a no-op after them. The
    /// history tier keeps its own watermark; nothing here touches it.
    pub fn refresh_state(&mut self) -> Result<bool, DitError> {
        let Ok(head) = self.repo.head() else {
            return Ok(false);
        };
        if self.index.watermark("state")?.as_deref() == Some(head.as_str()) {
            return Ok(false);
        }
        self.reindex(ReindexMode::State)?;
        self.index.set_watermark("state", &head)?;
        Ok(true)
    }

    /// Scaffold the coordination plane in this workspace (ADR 0015): the
    /// lane registry + coordination block in workflow.yaml, and the peer
    /// protocol section in CLAUDE.md. Idempotent — a second run writes
    /// nothing. Existing workflow.yaml hand edits and comments survive: the
    /// new blocks are appended textually, only when their top-level keys are
    /// absent.
    pub fn init_workflow(&mut self, lanes: &[LaneSpec]) -> Result<WorkflowInitReport, DitError> {
        let mut report = WorkflowInitReport::default();

        // 1. workflow.yaml — create with the canonical default when absent,
        //    append the blocks when present but without them.
        let schema_path = self.store.layout().workflow_yaml();
        let aux = Workflow {
            lanes: lanes
                .iter()
                .map(|l| dit_model::Lane {
                    id: l.id.clone(),
                    label: l.label.clone(),
                    owners: l.owners.clone(),
                })
                .collect(),
            coordination: self.workflow.coordination.clone(),
            ..Workflow::default_workflow()
        };
        let aux_text = dit_parse::write_workflow(&aux);
        let (lanes_block, coordination_block) = split_coordination_blocks(&aux_text);
        // The emitter skips a default coordination block; the scaffold
        // writes it anyway, so the knobs are visible from day one.
        let coordination_block = if coordination_block.is_empty() {
            DEFAULT_COORDINATION_BLOCK.to_owned()
        } else {
            coordination_block
        };
        let text = match std::fs::read_to_string(&schema_path) {
            Ok(text) => {
                let has_lanes = text.lines().any(|l| l.starts_with("lanes:"));
                let has_coordination = text.lines().any(|l| l.starts_with("coordination:"));
                if has_lanes && has_coordination {
                    text
                } else {
                    let mut text = text;
                    if !text.ends_with('\n') {
                        text.push('\n');
                    }
                    if !has_lanes {
                        text.push_str(&lanes_block);
                        report.lanes_written = true;
                    }
                    if !has_coordination {
                        text.push_str(&coordination_block);
                        report.coordination_written = true;
                    }
                    text
                }
            }
            Err(_) => {
                report.schema_created = true;
                report.lanes_written = true;
                report.coordination_written = true;
                // Seed the whole canonical file: the workspace gets a
                // materialized workflow, not just the new blocks.
                let mut seeded = dit_parse::write_workflow(&Workflow::default_workflow());
                if !seeded.ends_with('\n') {
                    seeded.push('\n');
                }
                seeded.push_str(&lanes_block);
                seeded.push_str(&coordination_block);
                seeded
            }
        };
        if report.lanes_written || report.coordination_written || report.schema_created {
            let parent = schema_path.parent().ok_or_else(|| {
                DitError::Refuse("workflow.yaml must live inside a directory".into())
            })?;
            std::fs::create_dir_all(parent)?;
            atomic::write(&schema_path, &text)?;
        }

        // Agent-facing rules are no longer written here: they live in one
        // generated document and are installed by `dit ai` (ADR 0021).

        // 3. The evidence-report template, seeded like every other template:
        // only when absent, hand edits survive.
        let report_template = self
            .store
            .layout()
            .templates_dir()
            .join("integration-report.md");
        if !report_template.exists() {
            std::fs::create_dir_all(report_template.parent().ok_or_else(|| {
                DitError::Refuse("templates dir must live inside a directory".into())
            })?)?;
            atomic::write(&report_template, TEMPLATE_INTEGRATION_REPORT)?;
            report.report_template_written = true;
        }

        self.reload_schema();

        let root = self.repo.root().to_owned();
        if report.lanes_written || report.coordination_written || report.schema_created {
            let rel = rel_to_root(&root, &schema_path);
            self.repo.add(&rel)?;
        }
        if report.report_template_written {
            let rel = rel_to_root(&root, &report_template);
            self.repo.add(&rel)?;
        }
        if report.lanes_written
            || report.coordination_written
            || report.schema_created
            || report.report_template_written
        {
            self.repo
                .commit("init workflow coordination: lanes and claim settings")?;
        }
        Ok(report)
    }

    /// The specification an agent needs to work in this workspace (ADR
    /// 0021): generated from this binary, so it can never disagree with the
    /// binary, and filled in with this workspace's own statuses and lanes.
    pub fn agent_spec(&self) -> String {
        let statuses: Vec<String> = self
            .workflow
            .statuses
            .iter()
            .map(|s| s.id.clone())
            .collect();
        let lanes: Vec<String> = self.workflow.lanes.iter().map(|l| l.id.clone()).collect();
        agent::agent_spec(env!("CARGO_PKG_VERSION"), &statuses, &lanes)
    }

    /// Install the agent document and point every agent file at it (ADR
    /// 0021). Idempotent — a second run writes nothing and reports
    /// `changed: false`. Only the marked blocks are ever touched, so
    /// hand-written rules in the same files survive byte-for-byte.
    pub fn write_agent_docs(&mut self, opts: &AgentDocOptions) -> Result<AgentDocReport, DitError> {
        let unknown = agent::unknown_tools(opts);
        if !unknown.is_empty() {
            return Err(DitError::Refuse(format!(
                "no agent file is known for {} — the tools DIT can point are {}",
                unknown.join(", "),
                agent::tool_keys().join(", ")
            )));
        }
        let mut report = AgentDocReport::default();
        let root = self.repo.root().to_owned();

        // 1. The canonical document.
        let doc_path = root.join(AGENT_DOC_PATH);
        let existing = std::fs::read_to_string(&doc_path).unwrap_or_default();
        let updated = agent::render_document(&existing, &self.agent_spec());
        if updated != existing {
            let parent = doc_path.parent().ok_or_else(|| {
                DitError::Refuse("the agent document must live inside a directory".into())
            })?;
            std::fs::create_dir_all(parent)?;
            atomic::write(&doc_path, &updated)?;
            self.repo.add(AGENT_DOC_PATH)?;
            report.document_written = true;
            report.changed = true;
        }

        // 2. A pointer in each agent file this repo actually uses.
        for rel in agent::targets(&root, opts) {
            let path = root.join(&rel);
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            let (updated, had_legacy) = agent::render_pointer(&existing);
            if had_legacy {
                report.legacy_replaced.push(rel.clone());
            }
            if updated != existing {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                atomic::write(&path, &updated)?;
                self.repo.add(&rel)?;
                report.changed = true;
            }
            report.pointers.push(rel);
        }

        if report.changed {
            self.repo
                .commit("install the DIT agent guide and point the agent files at it")?;
        }
        Ok(report)
    }

    /// The board: workflow columns in declaration order, issues sorted most
    /// urgent first. Issues whose status is not in the workflow land in a
    /// trailing unnamed column rather than vanishing.
    pub fn board(&self) -> Result<Board, DitError> {
        let compiled = dit_query::compile(
            &dit_query::Query {
                filter: None,
                order: vec![],
                limit: Some(10_000),
            },
            None,
            OffsetDateTime::now_utc(),
        )
        .map_err(dit_query::QueryError::from)?;
        let mut issues = self.index.list_issues(&compiled)?;

        // Urgent first; among equals, newest first. Issues without a
        // priority sort last rather than first.
        issues.sort_by(|a, b| {
            b.issue
                .priority
                .unwrap_or(dit_model::Priority::P4)
                .cmp(&a.issue.priority.unwrap_or(dit_model::Priority::P4))
                .then_with(|| b.issue.created.cmp(&a.issue.created))
        });

        let mut columns: Vec<BoardColumn> = self
            .workflow
            .board_columns()
            .map(|s| BoardColumn {
                status: s.id.clone(),
                label: s.label.clone(),
                wip_limit: s.wip_limit,
                issues: vec![],
            })
            .collect();
        let mut strays = Vec::new();
        for issue in issues {
            match columns.iter_mut().find(|c| c.status == issue.issue.status) {
                Some(col) => col.issues.push(issue),
                None => strays.push(issue),
            }
        }
        if !strays.is_empty() {
            columns.push(BoardColumn {
                status: String::new(),
                label: "not in workflow".into(),
                wip_limit: None,
                issues: strays,
            });
        }
        Ok(Board { columns })
    }

    /// One page of the whole workspace's field history, newest first.
    /// `before_seq` is the cursor from the previous page's last row.
    pub fn activity(
        &self,
        before_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<StoredFieldEvent>, DitError> {
        Ok(self.index.activity(before_seq, limit.clamp(1, 500))?)
    }

    /// The workspace as it stood at a point in its history, next to how it
    /// stands now (DESIGN.md §14.3). `cutoff_seq` is a position in the commit
    /// graph, not a date: a tag or a row in the feed resolves to one exactly,
    /// while a date only maps to one through an author's clock.
    ///
    /// Nothing here is stored. Every number is recomputed from `field_events`
    /// on each call, which is what makes "the board as of v0.1.0" answerable
    /// at all (invariant 5).
    pub fn activity_summary(
        &self,
        cutoff_seq: Option<i64>,
        histogram_days: u32,
    ) -> Result<ActivitySummary, DitError> {
        let max_seq = self.index.max_event_seq()?;
        let cutoff = cutoff_seq.unwrap_or(max_seq).clamp(0, max_seq);

        let terminal: Vec<String> = self
            .workflow
            .statuses
            .iter()
            .filter(|s| s.category == StatusCategory::Done)
            .map(|s| s.id.clone())
            .collect();

        // The histogram is the one place a wall clock decides anything, and
        // only because "which day did this land on" is a question about
        // clocks by definition.
        let floor = OffsetDateTime::now_utc()
            - time::Duration::days(i64::from(histogram_days.clamp(1, 366)));
        let since_day = format!(
            "{:04}-{:02}-{:02}",
            floor.year(),
            u8::from(floor.month()),
            floor.day()
        );

        Ok(ActivitySummary {
            seq: cutoff,
            max_seq,
            days: self.index.activity_days(&since_day)?,
            at_cutoff: self.counts_at(cutoff)?,
            now: self.counts_at(max_seq)?,
            since: self.index.changes_since(cutoff, &terminal)?,
        })
    }

    /// Board counts by workflow category at one point in history. A status
    /// the workflow no longer defines counts as `todo`: it is work that is
    /// not finished, and dropping it would silently shrink the board.
    fn counts_at(&self, seq: i64) -> Result<CategoryCounts, DitError> {
        let mut counts = CategoryCounts::default();
        for (_, status) in self.index.status_as_of(seq)? {
            match self.workflow.status(&status).map(|s| s.category) {
                Some(StatusCategory::Done) => counts.done += 1,
                Some(StatusCategory::Doing) => counts.doing += 1,
                _ => counts.todo += 1,
            }
        }
        Ok(counts)
    }

    /// Field history of one issue, oldest first.
    pub fn history(
        &self,
        id: &IssueId,
        field: Option<&str>,
    ) -> Result<Vec<StoredFieldEvent>, DitError> {
        Ok(self.index.field_events(id, field)?)
    }

    /// Comments of one issue, oldest first.
    pub fn comments(&self, id: &IssueId) -> Result<Vec<Comment>, DitError> {
        Ok(self.index.comments_for(id)?)
    }

    /// The most recent comments across the whole workspace, newest first,
    /// each joined to its issue — what a timeline shows.
    pub fn recent_comments(&self, limit: usize) -> Result<Vec<WorkspaceComment>, DitError> {
        Ok(self.index.recent_comments(limit.clamp(1, 500))?)
    }

    /// Every release plan (DESIGN.md §15.2), dated ones first by target date,
    /// then the undated by version — the roadmap's lane order. Answers from
    /// the index like every read.
    pub fn releases(&self) -> Result<Vec<IndexedRelease>, DitError> {
        Ok(self.index.releases()?)
    }

    /// One release plan by version. A version that could not be a folder
    /// name is simply not there — never an error, never a path.
    pub fn release(&self, version: &str) -> Result<Option<IndexedRelease>, DitError> {
        if validate_release_version(version).is_err() {
            return Ok(None);
        }
        Ok(self.index.release(version)?)
    }

    /// Every doc page (§13) under the four doc roots, path-sorted.
    ///
    /// The one read here that answers from the filesystem instead of the
    /// index — deliberately, per ADR 0010: pages have no index rows yet, so
    /// the file tree *is* the list. When the doc index lands (labels,
    /// wiki-links), this moves under I2 with it. Files whose names fall
    /// outside `DocPath` rules (hand-made uppercase, dotfiles) are skipped,
    /// not fatal: the listing must not die on one odd file.
    pub fn list_docs(&self) -> Vec<DocEntry> {
        let root = self.store.layout().root().to_owned();
        let mut out = Vec::new();
        for name in DOC_ROOTS {
            let dir = self.store.layout().content_dir(name);
            let mut stack = vec![dir];
            while let Some(dir) = stack.pop() {
                let entries = match std::fs::read_dir(&dir) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().into_owned();
                    if file_name.starts_with('.') {
                        continue;
                    }
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    if !file_name.ends_with(".md") {
                        continue;
                    }
                    let Ok(rel) = path.strip_prefix(&root) else {
                        continue;
                    };
                    // Layout-safe on every platform git runs: forward slashes.
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    let Ok(doc_path) = DocPath::parse(&rel) else {
                        continue;
                    };
                    let (updated_ms, bytes) = match entry.metadata() {
                        Ok(meta) => (
                            meta.modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_millis() as i64)
                                .unwrap_or(0),
                            meta.len(),
                        ),
                        Err(_) => continue,
                    };
                    out.push(DocEntry {
                        path: doc_path,
                        updated_ms,
                        bytes,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// Read one doc page. File-backed like [`Dit::list_docs`] — the page on
    /// disk is the source of truth, and git holds its history.
    pub fn read_doc(&self, path: &str) -> Result<String, DitError> {
        let doc_path = DocPath::parse(path)?;
        let file = self.store.layout().doc_file(&doc_path);
        match std::fs::read_to_string(&file) {
            Ok(text) => Ok(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(DitError::NotFound(doc_path.as_str().to_owned()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Health checks — what `dit doctor` prints.
    pub fn doctor(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        if let Some(problem) = &self.schema_problem {
            out.push(Diagnostic::error("schema", problem.clone()));
        } else {
            out.push(Diagnostic::ok(
                "schema",
                "workflow.yaml and config.yaml parse",
            ));
        }
        // The agent guide is generated, so a workspace can silently drift a
        // version behind the binary reading it (ADR 0021).
        let guide = self.repo.root().join(AGENT_DOC_PATH);
        match std::fs::read_to_string(&guide)
            .ok()
            .as_deref()
            .map(agent::agent_doc_stamp)
        {
            None => out.push(Diagnostic::warn(
                "agent-guide",
                format!("no {AGENT_DOC_PATH} — run `dit ai init` so AI sessions know the rules"),
            )),
            Some(Some(stamp)) if stamp == env!("CARGO_PKG_VERSION") => out.push(Diagnostic::ok(
                "agent-guide",
                "the agent guide matches this binary",
            )),
            Some(stamp) => out.push(Diagnostic::warn(
                "agent-guide",
                format!(
                    "{AGENT_DOC_PATH} was written by DIT {} — run `dit ai init` to refresh it",
                    stamp.as_deref().unwrap_or("an unknown version")
                ),
            )),
        }
        if self.repo.has_identity() {
            out.push(Diagnostic::ok("git-identity", "git user is configured"));
        } else {
            out.push(Diagnostic::error(
                "git-identity",
                "no git user configured — every commit this tool makes would fail",
            ));
        }
        if self.repo.merge_driver_ready() {
            out.push(Diagnostic::ok(
                "merge-driver",
                "frontmatter merge driver is configured",
            ));
        } else {
            out.push(Diagnostic::warn(
                "merge-driver",
                "merge driver not configured — sync will refuse to rebase until it is",
            ));
        }
        match (self.repo.head(), self.index.watermark("events")) {
            (Ok(head), Ok(wm)) if wm.as_deref() == Some(head.as_str()) => {
                out.push(Diagnostic::ok("index", "index is up to date with HEAD"));
            }
            (Ok(head), _) => out.push(Diagnostic::warn(
                "index",
                format!(
                    "index is behind {} — run `dit reindex`",
                    &head[..7.min(head.len())]
                ),
            )),
            (Err(_), _) => out.push(Diagnostic::warn(
                "index",
                "no commits yet — nothing to index",
            )),
        }
        if !self.config.writable() {
            out.push(Diagnostic::error(
                "schema-version",
                format!(
                    "files use schema {} but this client writes at most {} — upgrade before editing",
                    self.config.schema_version,
                    dit_model::SCHEMA_MAX
                ),
            ));
        }
        if !self.repo.is_clean().unwrap_or(false) {
            out.push(Diagnostic::warn(
                "dirty",
                "uncommitted changes — the index reflects HEAD, not the working tree",
            ));
        }
        // Duplicate numbers are an ambiguity in the human handle (ADR 0007):
        // repairable by a field edit, but only once somebody can see it.
        match self.index.duplicate_numbers() {
            Ok(dupes) if dupes.is_empty() => {
                // Unnumbered issues are by design under `on-merge` (the bot
                // assigns at merge); under `local` they are legacy and
                // backfillable (ADR 0009) — degraded display, not data risk.
                let backfillable = matches!(self.config.numbering, Numbering::Local)
                    && self.index.unnumbered_count().unwrap_or(0) > 0;
                if backfillable {
                    out.push(Diagnostic::warn(
                        "numbers",
                        "some issues have no number — run `dit renumber` to backfill them",
                    ));
                } else {
                    out.push(Diagnostic::ok("numbers", "issue numbers are unique"));
                }
            }
            Ok(dupes) => {
                for (n, ids) in dupes {
                    let refs: Vec<String> = ids
                        .iter()
                        .filter_map(|id| {
                            IssueId::parse(id)
                                .ok()
                                .map(|parsed| format!("#{}", parsed.short_ref().as_str()))
                        })
                        .collect();
                    out.push(Diagnostic::error(
                        "numbers",
                        format!(
                            "#{n} is held by {} — renumber all but one (`dit edit`)",
                            refs.join(", ")
                        ),
                    ));
                }
            }
            Err(_) => out.push(Diagnostic::warn(
                "numbers",
                "cannot check issue numbers — run `dit reindex`",
            )),
        }
        // A stale generated index is a publication artifact lying about the
        // data (ADR 0008) — visible, never fatal. Absent means never built:
        // the file is optional, not owed.
        let docs_index = self.store.layout().content_dir("issues").join("README.md");
        if docs_index.is_file() {
            if let Ok(rendered) = self.render_issue_index() {
                let on_disk = std::fs::read_to_string(&docs_index).unwrap_or_default();
                if on_disk == rendered {
                    out.push(Diagnostic::ok(
                        "docs-index",
                        "the generated issues index is current",
                    ));
                } else {
                    out.push(Diagnostic::warn(
                        "docs-index",
                        "issues/README.md is stale — run `dit docs build --index`",
                    ));
                }
            }
        }
        // Both `.dit-cache` and `.dit-cache/` ignore the directory; matching
        // either keeps the check in step with what init writes.
        let ignored = std::fs::read_to_string(self.repo.root().join(".gitignore"))
            .map(|text| {
                text.lines()
                    .any(|l| l.trim().trim_end_matches('/') == ".dit-cache")
            })
            .unwrap_or(false);
        if ignored {
            out.push(Diagnostic::ok("cache-ignored", ".dit-cache is gitignored"));
        } else {
            out.push(Diagnostic::warn(
                "cache-ignored",
                ".gitignore does not list .dit-cache — the index database would get committed",
            ));
        }
        out.extend(self.morse_diagnostics());
        out
    }

    /// Morse's two health questions (§20.6), both about secrets rather than
    /// about scenarios — `dit morse check` answers the rest.
    fn morse_diagnostics(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        // The environment file must never be tracked. This is an error, not
        // a warning: by the time it is in a commit the secret is in history
        // and deleting the file does not remove it.
        let tracked = self
            .repo
            .ls_tree(MORSE_LOCAL_PATH)
            .map(|files| !files.is_empty())
            .unwrap_or(false);
        if tracked {
            out.push(Diagnostic::error(
                "morse-env",
                format!(
                    "{MORSE_LOCAL_PATH} is tracked by git — it holds environment values and                      secrets. Remove it from the index (`git rm --cached {MORSE_LOCAL_PATH}`)                      and treat anything it held as leaked"
                ),
            ));
        } else {
            let ignored = std::fs::read_to_string(self.repo.root().join(".gitignore"))
                .map(|text| text.lines().any(|l| l.trim() == MORSE_LOCAL_PATH))
                .unwrap_or(false);
            if ignored {
                out.push(Diagnostic::ok(
                    "morse-env",
                    "the Morse environment file is gitignored",
                ));
            } else if self.repo.root().join(MORSE_LOCAL_PATH).exists() {
                out.push(Diagnostic::error(
                    "morse-env",
                    format!(
                        "{MORSE_LOCAL_PATH} exists but .gitignore does not list it — one                          `git add .` away from committing a secret"
                    ),
                ));
            }
        }
        // And a scenario that wrote a credential into the repository rather
        // than naming it.
        let scenarios = self.index.morse_scenarios().unwrap_or_default();
        let mut leaks = Vec::new();
        for row in &scenarios {
            let Ok(scenario) = dit_parse::parse_morse_scenario(&row.body) else {
                continue;
            };
            for found in scenario.suspected_secrets() {
                leaks.push(format!(
                    "{}:{} — scenario `{}`, step `{}`, field `{}`: {}",
                    row.path, row.line, row.scenario, found.step, found.field, found.reason
                ));
            }
        }
        if leaks.is_empty() {
            if !scenarios.is_empty() {
                out.push(Diagnostic::ok(
                    "morse-secrets",
                    "no scenario writes a credential into the repository",
                ));
            }
        } else {
            out.push(Diagnostic::error(
                "morse-secrets",
                format!(
                    "a scenario carries what looks like a real credential — a fence states                      variable names, never values:\n  {}",
                    leaks.join("\n  ")
                ),
            ));
        }
        out
    }

    // -- writes -----------------------------------------------------------------

    /// Begin a write. Fails with [`DitError::Busy`] when another process
    /// holds the workspace lock. The author is named here, not read from
    /// config, because a server may commit on behalf of the person who
    /// clicked — attribution must follow the action, not the machine.
    pub fn transaction(&mut self, author: &str) -> Result<Transaction<'_>, DitError> {
        let lock_path = self.store.layout().write_lock();
        let lock = acquire_lock_or_busy(&lock_path, author)?;
        let prev_head = self.repo.head().ok();
        let store_tx = self.store.transaction(OffsetDateTime::now_utc(), author);
        Ok(Transaction {
            dit: self,
            store_tx,
            lock,
            prev_head,
            number_cursor: None,
        })
    }

    /// Fetch + rebase + push through the merge driver, then rebuild the
    /// index if anything arrived. Conflicts are a field on the report, not
    /// an error — the caller has to show them to somebody.
    pub fn sync(&mut self, opts: SyncOptions) -> Result<SyncReport, DitError> {
        let mut opts = opts;
        let report = dit_vcs::sync(&self.repo, &mut opts)?;
        // A pull moved HEAD (or a rebase rewrote it), and a conflicted
        // rebase leaves the repo mid-flight — both mean the index no longer
        // describes reality. Rebuild only when the tree is usable; during a
        // conflict stop, the index stays at the last good point.
        if report.pulled > 0 && report.needs_human.is_empty() {
            self.reindex(ReindexMode::All)?;
        }
        Ok(report)
    }

    /// Rebuild the index from git.
    pub fn reindex(&mut self, mode: ReindexMode) -> Result<IndexReport, DitError> {
        self.reload_schema();
        let Ok(head) = self.repo.head() else {
            // No commits yet: an empty index is the truthful state.
            return Ok(IndexReport {
                issues: 0,
                comments: 0,
                events: 0,
                skipped: 0,
                head: String::new(),
            });
        };
        if mode == ReindexMode::All {
            self.index.wipe()?;
        }

        let mut report = IndexReport {
            issues: 0,
            comments: 0,
            events: 0,
            skipped: 0,
            head: head.clone(),
        };
        if matches!(mode, ReindexMode::State | ReindexMode::All) {
            self.index.wipe_state()?;
            // The issues root is everything the index reads: bodies and
            // comments both live under it, whichever side of `.dit/` the
            // layout puts it (ADR 0005).
            let layout = self.store.layout().kind();
            let issues_root = self.store.layout().content_root_rel("issues");
            for (path, blob) in self.repo.ls_tree(&issues_root)? {
                let Some(text) = self.repo.show_text(&format!("HEAD:{path}")) else {
                    continue;
                };
                // Loose on purpose: a moved or archived issue is still an
                // issue, and the generated index is excluded by the same
                // classification (ADR 0008 — an output, never an input).
                if dit_model::looks_like_issue_body(&path, layout) {
                    match dit_parse::parse_issue(&text) {
                        Ok((issue, _)) => {
                            self.index.upsert_issue(&issue, &path, &blob)?;
                            report.issues += 1;
                        }
                        Err(_) => report.skipped += 1,
                    }
                } else if path.contains("/comments/") && path.ends_with(".md") {
                    match dit_parse::parse_comment(&text) {
                        Ok(comment) => {
                            if let Some(parent) = self.issue_owning(&path)? {
                                self.index.upsert_comment(&parent, &comment)?;
                                report.comments += 1;
                            } else {
                                report.skipped += 1;
                            }
                        }
                        Err(_) => report.skipped += 1,
                    }
                }
            }
            // The authored shape of a flow (ADR 0020): a `dit-flow` fence
            // in any document. Only the fences are read — this is not the
            // document index ADR 0010 deferred — and the result lands in
            // the index so the read path never walks the tree (I2).
            //
            // Morse scenarios (§20, ADR 0022) ride the same walk: both are
            // fences in a document, and reading either one is reading, never
            // fetching (I11). The catalogue is rebuilt first, because a
            // scenario is judged against it.
            self.index.clear_flow_shapes()?;
            self.index.clear_morse_scenarios()?;
            self.index.clear_morse_runs()?;
            self.refresh_morse_specs()?;
            for root in dit_model::DOC_ROOTS {
                let rel = self.store.layout().content_root_rel(root);
                for (path, _) in self.repo.ls_tree(&rel)? {
                    if !path.ends_with(".md") {
                        continue;
                    }
                    let Some(text) = self.repo.show_text(&format!("HEAD:{path}")) else {
                        continue;
                    };
                    report.skipped += self.absorb_flow_fences(&path, &text)?;
                    report.skipped += self.absorb_morse_fences(&path, &text)?;
                }
            }
            self.judge_morse_scenarios()?;
            // Release plans (§15.2) live under `.dit/releases/` in every
            // layout. A workspace without the directory lists nothing —
            // `ls-tree` over a missing prefix is empty, not an error.
            for (path, _) in self.repo.ls_tree(dit_model::RELEASES_DIR)? {
                if !dit_model::is_release_file(&path) {
                    continue;
                }
                let Some(text) = self.repo.show_text(&format!("HEAD:{path}")) else {
                    continue;
                };
                match dit_parse::parse_release(&text) {
                    Ok((release, _)) => self.index.upsert_release(&release, &path)?,
                    Err(_) => report.skipped += 1,
                }
            }
            // Where the state tier currently stands — the watcher's dedupe
            // key (ADR 0017).
            self.index.set_watermark("state", &head)?;
        }
        if matches!(mode, ReindexMode::Events | ReindexMode::All) {
            let watermark = self.index.watermark("events")?;
            let layout = self.store.layout().kind();
            let events = dit_vcs::walk_field_events(&self.repo, watermark.as_deref(), layout)?;
            report.events = self.index.record_field_events(&events)?;
            self.index.set_watermark("events", &head)?;
        }
        Ok(report)
    }

    /// Read one document's `dit-flow` fences into the index. Returns how
    /// many were passed over: a fence too broken to even name its flow has
    /// nowhere to report itself, and a second fence for a flow another
    /// document already shapes is a warning, not a merge.
    fn absorb_flow_fences(&mut self, path: &str, text: &str) -> Result<usize, DitError> {
        let mut skipped = 0;
        for fence in dit_parse::flow_fences(text) {
            let (flow, problem) = match dit_parse::parse_flow_shape(&fence.body) {
                Ok(shape) => (shape.flow, None),
                Err(err) => match name_in_fence(&fence.body) {
                    Some(flow) => (flow, Some(err.to_string())),
                    None => {
                        skipped += 1;
                        continue;
                    }
                },
            };
            let kept = self.index.upsert_flow_shape(
                &flow,
                path,
                fence.line,
                &fence.body,
                problem.as_deref(),
            )?;
            if !kept {
                skipped += 1;
            }
        }
        Ok(skipped)
    }

    /// One flow's authored shape, as stored at reindex (ADR 0020). `None`
    /// means the flow has no fence and the board falls back to computed
    /// stages; a shape carrying a `problem` means the fence is there but
    /// unreadable, and the screen says which document and line to fix.
    pub fn flow_shape(&self, flow: &str) -> Result<Option<FlowShapeRef>, DitError> {
        let Some(stored) = self.index.flow_shape(flow)? else {
            return Ok(None);
        };
        let shape = match &stored.problem {
            Some(_) => None,
            None => dit_parse::parse_flow_shape(&stored.shape).ok(),
        };
        Ok(Some(FlowShapeRef {
            path: stored.path,
            line: stored.line,
            problem: stored.problem,
            shape,
        }))
    }

    /// The issue a comment file belongs to: the body file living in the
    /// folder above `comments/` — `README.md` since ADR 0006, the legacy
    /// `issue.md` for folders that predate it. Derived from the tree, never
    /// stored, so a moved folder cannot orphan a comment.
    fn issue_owning(&self, comment_path: &str) -> Result<Option<IssueId>, DitError> {
        let Some(marker) = comment_path.find("/comments/") else {
            return Ok(None);
        };
        let dir = &comment_path[..marker];
        if dir.is_empty() {
            return Ok(None);
        }
        for name in [
            dit_model::ISSUE_BODY_FILE,
            dit_model::LEGACY_ISSUE_BODY_FILE,
        ] {
            if let Some(text) = self.repo.show_text(&format!("HEAD:{dir}/{name}")) {
                return match dit_parse::parse_issue(&text) {
                    Ok((issue, _)) => Ok(Some(issue.id)),
                    Err(_) => Ok(None),
                };
            }
        }
        Ok(None)
    }
}

/// A write in progress. Everything it stages is formatted, written, put in
/// ONE git commit by [`Transaction::commit`], and mirrored into the index —
/// or, with [`Transaction::abort`] or a drop, leaves nothing behind.
pub struct Transaction<'a> {
    dit: &'a mut Dit,
    store_tx: dit_store::Transaction,
    lock: LockGuard,
    /// HEAD before this transaction commits. The diff from here to the new
    /// HEAD is exactly what this transaction did, which is what the index
    /// needs to absorb.
    prev_head: Option<String>,
    /// The last number this transaction assigned (ADR 0007). The index knows
    /// nothing about staged-but-uncommitted issues, so a second `create`
    /// inside the same transaction counts up from here instead of re-reading
    /// a stale max.
    number_cursor: Option<u32>,
}

impl std::fmt::Debug for Transaction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("staged", &self.store_tx.staged_len())
            .finish()
    }
}

impl<'a> Transaction<'a> {
    /// Create an issue. An empty body is seeded from the workspace's template
    /// for the draft's kind (falling back to `default.md`), so every delivery
    /// surface — CLI, server, bot — writes the same shape. Under
    /// `numbering: local` (ADR 0007) a missing number is assigned
    /// `max(existing) + 1` from the index; callers never choose it.
    pub fn create_issue(&mut self, mut draft: IssueDraft) -> Result<IssueId, DitError> {
        if let Some(s) = &draft.status {
            if !self.dit.workflow.contains_status(s) {
                return Err(DitError::Refuse(format!(
                    "`{s}` is not one of this workflow's statuses — create the issue, then \
                     `dit issue set --force status={s}` if you truly mean it"
                )));
            }
        }
        self.assign_number(&mut draft)?;
        if draft.body.trim().is_empty() {
            let kind = draft.kind.as_str().to_owned();
            if let Some(text) = self
                .dit
                .template_text(&kind)
                .or_else(|| self.dit.template_text("default"))
            {
                draft.body = text;
            }
        }
        Ok(self.store_tx.create_issue(draft)?)
    }

    /// [`Transaction::create_issue`] with an explicit template name (`dit
    /// issue new --template bug`). A missing template is an error, not a
    /// silent fallback — the user asked for a shape by name.
    pub fn create_issue_from_template(
        &mut self,
        mut draft: IssueDraft,
        template: &str,
    ) -> Result<IssueId, DitError> {
        match self.dit.template_path(template) {
            Some(path) if path.is_file() => {
                if draft.body.trim().is_empty() {
                    draft.body = std::fs::read_to_string(&path)?;
                }
            }
            _ => return Err(DitError::TemplateMissing(template.to_owned())),
        }
        self.assign_number(&mut draft)?;
        Ok(self.store_tx.create_issue(draft)?)
    }

    /// Fill in `number` per the workspace's numbering policy. Explicitly-set
    /// numbers and `on-merge` workspaces are left alone — the bot owns
    /// assignment there (ADR 0007).
    fn assign_number(&mut self, draft: &mut IssueDraft) -> Result<(), DitError> {
        if draft.number.is_some() || !matches!(self.dit.config.numbering, Numbering::Local) {
            return Ok(());
        }
        let next = match self.number_cursor {
            Some(n) => n + 1,
            None => self
                .dit
                .index
                .max_number()?
                .map_or(1, |n| n.saturating_add(1)),
        };
        self.number_cursor = Some(next);
        draft.number = Some(next);
        Ok(())
    }

    pub fn set_fields(&mut self, id: &IssueId, patch: FieldPatch) -> Result<(), DitError> {
        self.set_fields_opts(id, patch, false)
    }

    /// [`Transaction::set_fields`] with the `--force` escape for the status
    /// membership check (ADR 0015): today only a charset is validated and a
    /// typo silently drops the issue off the board; from now on the value
    /// must name a workflow status unless `force_status` says otherwise.
    pub fn set_fields_opts(
        &mut self,
        id: &IssueId,
        patch: FieldPatch,
        force_status: bool,
    ) -> Result<(), DitError> {
        if let Some(s) = &patch.status {
            if !force_status && !self.dit.workflow.contains_status(s) {
                let legal: Vec<&str> = self
                    .dit
                    .workflow
                    .board_columns()
                    .map(|st| st.id.as_str())
                    .collect();
                return Err(DitError::Refuse(format!(
                    "`{s}` is not one of this workflow's statuses ({}) — pass --force to write \
                     it anyway",
                    legal.join(", ")
                )));
            }
        }
        Ok(self.store_tx.set_fields(id, patch)?)
    }

    /// Replace the markdown body; frontmatter is untouched down to the byte.
    pub fn set_body(&mut self, id: &IssueId, body: &str) -> Result<(), DitError> {
        Ok(self.store_tx.set_body(id, body)?)
    }

    /// Delete an issue: its body and its comments, in one commit. The index
    /// drops the row when the commit is absorbed; its `field_events` stay —
    /// history outlives its subject, and the deletion is itself the last
    /// event. Nothing here is undoable except through git, which is the
    /// point: the file is gone from HEAD, not from history.
    pub fn delete_issue(&mut self, id: &IssueId) -> Result<(), DitError> {
        match self.store_tx.remove_issue(id) {
            Ok(()) => Ok(()),
            Err(dit_store::StoreError::NotFound(name)) => Err(DitError::NotFound(name)),
            Err(e) => Err(e.into()),
        }
    }

    /// Patch a release plan's status or target date (DESIGN.md §15.2) —
    /// surgical, like an issue patch. The plan must already exist:
    /// creating one is `dit release plan`'s job (v0.9).
    pub fn set_release(&mut self, version: &str, patch: ReleasePatch) -> Result<(), DitError> {
        match self.store_tx.set_release(version, &patch) {
            Ok(()) => Ok(()),
            Err(dit_store::StoreError::NotFound(name)) => Err(DitError::NotFound(name)),
            Err(e) => Err(e.into()),
        }
    }

    /// Write a doc page (§13), formatted by `dit fmt` like every other
    /// write. The §16 name; the path string is validated into a `DocPath`
    /// here so no caller below the facade ever re-derives the sandbox.
    pub fn write_doc(&mut self, path: &str, content: &str) -> Result<(), DitError> {
        let doc_path = DocPath::parse(path)?;
        Ok(self.store_tx.write_doc(&doc_path, content)?)
    }

    /// Delete a doc page. Mapping the store's "not there" onto the facade's
    /// `NotFound` keeps delivery able to 404 uniformly.
    pub fn delete_doc(&mut self, path: &str) -> Result<(), DitError> {
        let doc_path = DocPath::parse(path)?;
        match self.store_tx.remove_doc(&doc_path) {
            Ok(()) => Ok(()),
            Err(dit_store::StoreError::NotFound(name)) => Err(DitError::NotFound(name)),
            Err(e) => Err(e.into()),
        }
    }

    pub fn comment(
        &mut self,
        id: &IssueId,
        alias: &str,
        reply_to: Option<&IssueId>,
        body: &str,
    ) -> Result<IssueId, DitError> {
        Ok(self.store_tx.add_comment(id, alias, reply_to, body)?)
    }

    /// Move (rename) a doc page in one commit. Byte-identical content at the
    /// new path plus removal of the old one is what makes git record a
    /// rename, so the page's history follows it. An occupied target is a
    /// `Refuse`, not an error in the write path — nothing is ever partially
    /// moved.
    pub fn move_doc(&mut self, from: &str, to: &str) -> Result<(), DitError> {
        let from_path = DocPath::parse(from)?;
        let to_path = DocPath::parse(to)?;
        match self.store_tx.move_doc(&from_path, &to_path) {
            Ok(()) => Ok(()),
            Err(dit_store::StoreError::NotFound(name)) => Err(DitError::NotFound(name)),
            Err(dit_store::StoreError::Conflict(name)) => Err(DitError::Refuse(format!(
                "a page already exists at `{name}` — nothing was moved"
            ))),
            Err(e) => Err(e.into()),
        }
    }

    /// Write everything staged, commit it as one git commit, and bring the
    /// index up to date. `Ok(None)` means nothing was staged — not an error,
    /// the caller's "nothing to do" is already true.
    pub fn commit(self, message: &str) -> Result<Option<String>, DitError> {
        let Transaction {
            dit,
            store_tx,
            lock,
            prev_head,
            number_cursor: _,
        } = self;
        let changeset = store_tx.finish()?;
        if changeset.paths().next().is_none() {
            drop(lock);
            return Ok(None);
        }
        let result = Self::commit_and_absorb(dit, changeset, prev_head.as_deref(), message);
        drop(lock);
        result
    }

    /// Stage the changeset into git, commit, and mirror the result into the
    /// index. On any failure the files are rolled back so the working tree
    /// is exactly as it was — a failed write must not leave half a change
    /// lying around for the next writer to trip over.
    fn commit_and_absorb(
        dit: &mut Dit,
        changeset: Changeset,
        prev_head: Option<&str>,
        message: &str,
    ) -> Result<Option<String>, DitError> {
        // The DIT alias rides as a commit trailer so history can attribute
        // events to the person who acted even when a whole team shares one
        // machine identity (a server, CI, or a shared box).
        let message = if changeset.author.is_empty() {
            message.to_owned()
        } else {
            format!("{message}\n\nDit-Author: {}", changeset.author)
        };
        let root = dit.repo.root().to_owned();
        // Collect first: the iterator borrows the changeset, and rollback
        // (inside the loop) consumes it.
        let rel_paths: Vec<String> = changeset.paths().map(|p| rel_to_root(&root, p)).collect();
        for rel in rel_paths {
            if let Err(e) = dit.repo.add(&rel) {
                changeset.rollback();
                return Err(e.into());
            }
        }
        match dit.repo.commit(&message) {
            Ok(Some(head)) => {
                if let Err(e) = dit.absorb_commit(prev_head, &head) {
                    // The commit itself succeeded. Git is the source of truth
                    // and the index is a rebuildable cache, so a failed absorb
                    // degrades to a stale index, never to a lost write.
                    tracing::warn!(
                        "index not updated after commit {head}: {e} — run `dit reindex`"
                    );
                }
                Ok(Some(head))
            }
            Ok(None) => Ok(None),
            Err(e) => {
                changeset.rollback();
                Err(e.into())
            }
        }
    }

    /// Forget everything: staged writes never reached disk, so dropping the
    /// store transaction undoes the whole thing. The lock releases on drop.
    pub fn abort(self) {
        tracing::warn!(
            "transaction aborted — {} staged writes discarded",
            self.store_tx.staged_len()
        );
    }
}

impl Dit {
    /// Bring the index up to date with one fresh commit: re-read every file
    /// the commit touched, and record the field events it implies. The diff
    /// range is `prev_head..head` (or the whole history for a first commit),
    /// which keeps deletions from old commits out of the work.
    fn absorb_commit(&mut self, prev_head: Option<&str>, head: &str) -> Result<(), DitError> {
        let base = prev_head.unwrap_or(EMPTY_TREE);
        let layout = self.store.layout().kind();
        let mut argv = vec!["diff", "--name-status", "-M", base, head, "--"];
        argv.extend(layout.diff_pathspecs());
        let diff = self.repo.git(&argv)?;
        for line in diff.lines() {
            let mut fields = line.split('\t');
            let Some(status) = fields.next() else {
                continue;
            };
            let Some(new_path) = fields.next_back() else {
                continue;
            };
            // A deletion names a file that is no longer at `head`; its id
            // is read from the side that still has it. Deletions arriving
            // through sync rebuild the whole index anyway, so this only has
            // to be right for this tool's own `delete_issue`.
            if status.starts_with('D') {
                let Some(old_text) = self.repo.show_text(&format!("{base}:{new_path}")) else {
                    continue;
                };
                if dit_model::looks_like_issue_body(new_path, layout) {
                    if let Ok((issue, _)) = dit_parse::parse_issue(&old_text) {
                        self.index.remove_issue(&issue.id)?;
                    }
                } else if is_doc_path(new_path) {
                    // A deleted document takes its flow shapes with it.
                    self.index.clear_flow_shapes_at(new_path)?;
                    self.index.clear_morse_scenarios_at(new_path)?;
                } else if new_path.contains("/comments/") {
                    if let Ok(comment) = dit_parse::parse_comment(&old_text) {
                        self.index.remove_comment(&comment.id)?;
                    }
                }
                continue;
            }
            let Some(text) = self.repo.show_text(&format!("{head}:{new_path}")) else {
                continue;
            };
            let blob = self
                .repo
                .git(&["rev-parse", &format!("{head}:{new_path}")])?;
            if dit_model::looks_like_issue_body(new_path, layout) {
                if let Ok((issue, _)) = dit_parse::parse_issue(&text) {
                    self.index.upsert_issue(&issue, new_path, &blob)?;
                }
            } else if dit_model::is_release_file(new_path) {
                if let Ok((release, _)) = dit_parse::parse_release(&text) {
                    self.index.upsert_release(&release, new_path)?;
                }
            } else if new_path.contains("/comments/") {
                if let Ok(comment) = dit_parse::parse_comment(&text) {
                    if let Some(parent) = self.issue_owning(new_path)? {
                        self.index.upsert_comment(&parent, &comment)?;
                    }
                }
            } else if is_doc_path(new_path) {
                // A `dit-flow` fence (ADR 0020) reaches the diagram on the
                // same write that saved it, rather than waiting for a full
                // reindex — the screen is live on any process's write, and
                // shaping a flow is a write like any other.
                self.index.clear_flow_shapes_at(new_path)?;
                self.absorb_flow_fences(new_path, &text)?;
                self.index.clear_morse_scenarios_at(new_path)?;
                self.absorb_morse_fences(new_path, &text)?;
                self.judge_morse_scenarios()?;
            }
        }
        let events = dit_vcs::walk_field_events(&self.repo, prev_head, layout)?;
        self.index.record_field_events(&events)?;
        self.index.set_watermark("events", head)?;
        // The state watermark is what the watcher (ADR 0017) compares HEAD
        // against: own-process writes move it here, so the watcher no-ops
        // and a write announces exactly once.
        self.index.set_watermark("state", head)?;
        Ok(())
    }

    /// The generated `issues/README.md` content (ADR 0008): every issue
    /// grouped by workflow column, ordered by number, linking into the
    /// folder that holds its `README.md` body. The offline-unique short ref
    /// stands in for issues with no number yet. A pure function of the index
    /// and this build's version — an unchanged repo renders byte-identically,
    /// so CI never commits churn.
    fn render_issue_index(&self) -> Result<String, DitError> {
        let mut issues = self.query("", None)?;
        // Numbers are identifiers, not sequence (ADR 0007) — but a listing
        // for humans reads best counted up, with unnumbered issues last in
        // stable id order.
        issues.sort_by(|a, b| {
            match (&a.issue.number, &b.issue.number) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.issue.id.as_str().cmp(b.issue.id.as_str()))
        });

        let columns: Vec<&WorkflowStatus> = self.workflow.board_columns().collect();
        let mut groups: Vec<(&str, Vec<&IndexedIssue>)> = columns
            .iter()
            .map(|s| (s.label.as_str(), Vec::new()))
            .collect();
        let mut strays: Vec<&IndexedIssue> = Vec::new();
        for hit in &issues {
            match columns.iter().position(|s| s.id == hit.issue.status) {
                Some(i) => groups[i].1.push(hit),
                None => strays.push(hit),
            }
        }
        if !strays.is_empty() {
            groups.push(("not in workflow", strays));
        }

        let mut out = format!(
            "{GENERATED_INDEX_MARKER} {VERSION} — do not edit; run dit docs build --index -->\n\n\
             # Issues\n"
        );
        for (label, group) in groups {
            if group.is_empty() {
                continue;
            }
            out.push_str(&format!("\n## {label}\n\n"));
            for hit in group {
                // The hash belongs to numbers alone — prefixing it to a short
                // ref would read as a number handle (every pre-ADR-0007
                // workspace would render `#06M5683`).
                let handle = match hit.issue.number {
                    Some(n) => format!("#{n}"),
                    None => hit.issue.id.short_ref().as_str().to_owned(),
                };
                let link = folder_link(&hit.path);
                out.push_str(&format!("- **{handle}** [{}]({link})\n", hit.issue.title));
            }
        }
        Ok(out)
    }

    /// Write the generated issues index (ADR 0008). `Ok(false)` means the
    /// file was already current — the caller's "nothing to do" is already
    /// true. CI/dit-bot is the intended writer; nothing else writes it as a
    /// side effect of issue writes.
    pub fn build_docs_index(&mut self) -> Result<bool, DitError> {
        let rendered = self.render_issue_index()?;
        let path = self.store.layout().content_dir("issues").join("README.md");
        let prior = std::fs::read_to_string(&path).ok();
        if prior.as_deref() == Some(rendered.as_str()) {
            return Ok(false);
        }
        let lock_path = self.store.layout().write_lock();
        let lock = acquire_lock_or_busy(&lock_path, "")?;
        let result = (|| {
            dit_store::atomic::write(&path, &rendered)?;
            let rel = rel_to_root(self.repo.root(), &path);
            if let Err(e) = self
                .repo
                .add(&rel)
                .and_then(|_| self.repo.commit("dit docs build --index").map(|_| ()))
            {
                // The tree goes back to exactly what it was — a failed
                // generation must not leave a stray file for the next writer
                // to commit by accident.
                match &prior {
                    Some(old) => dit_store::atomic::write(&path, old)?,
                    None => {
                        let _ = std::fs::remove_file(&path);
                    }
                }
                return Err(e.into());
            }
            Ok(true)
        })();
        drop(lock);
        result
    }

    /// Change the numbering policy (ADR 0007). A config-only write: it takes
    /// the single-writer lock, rewrites `.dit/config.yaml`, and commits —
    /// no issue file moves and no number is assigned or taken away. The
    /// in-memory config flips too, so the very next transaction obeys the
    /// new policy without a reopen.
    pub fn set_numbering(&mut self, numbering: Numbering) -> Result<(), DitError> {
        if self.config.numbering == numbering {
            return Ok(());
        }
        let lock_path = self.store.layout().write_lock();
        let lock = acquire_lock_or_busy(&lock_path, "set-numbering")?;
        let result = (|| {
            let mut config = self.config.clone();
            config.numbering = numbering;
            dit_store::atomic::write(
                &self.store.layout().config_yaml(),
                &dit_parse::write_config(&config),
            )?;
            self.repo.add(".dit")?;
            self.repo
                .commit(&format!("dit set numbering: {}", numbering.as_str()))?;
            self.config = config;
            Ok(())
        })();
        drop(lock);
        result
    }

    /// Backfill numbers onto unnumbered issues (ADR 0009): append-only —
    /// unnumbered issues take `max+1, max+2, …` in creation order, so an
    /// existing number never moves and nothing already pointing at `#N`
    /// re-points. One commit over a clean tree; returns how many issues
    /// gained a number (`Ok(0)` = nothing to do, no commit). Refused on
    /// `numbering: on-merge`, where merge serialization owns assignment.
    pub fn renumber(&mut self) -> Result<usize, DitError> {
        if !matches!(self.config.numbering, Numbering::Local) {
            return Err(DitError::Refuse(format!(
                "numbers are assigned on merge here (`numbering: {}`) — merge \
                 serialization owns assignment, so this command is for `local` \
                 workspaces",
                self.config.numbering.as_str()
            )));
        }
        if !self.repo.is_clean().unwrap_or(false) {
            return Err(DitError::Refuse(
                "the working tree is not clean — commit or stash before renumbering".into(),
            ));
        }
        // Creation order is ULID order (ADR 0001: Crockford base32 preserves
        // time order); same-millisecond ties break by random bits — either
        // way the id string decides, never a wall clock.
        let mut hits = self.query("", None)?;
        hits.sort_by(|a, b| a.issue.id.as_str().cmp(b.issue.id.as_str()));
        let targets: Vec<IssueId> = hits
            .iter()
            .filter(|h| h.issue.number.is_none())
            .map(|h| h.issue.id)
            .collect();
        if targets.is_empty() {
            return Ok(0);
        }
        let mut next = self.index.max_number()?.map_or(1, |n| n.saturating_add(1));
        let count = targets.len();
        let mut tx = self.transaction("dit")?;
        for id in &targets {
            tx.set_fields(
                id,
                FieldPatch {
                    number: Some(next),
                    ..FieldPatch::default()
                },
            )?;
            next = next.saturating_add(1);
        }
        tx.commit(&format!("dit renumber: {count} issue(s)"))?;
        Ok(count)
    }

    /// Move a workspace between layouts (ADR 0005): `git mv` every content
    /// root, rename legacy `issue.md` bodies to `README.md` (ADR 0006), put
    /// the merge-driver routing at the mode-correct place, flip
    /// `config.yaml`, and rebuild the index — one commit. Rename detection
    /// is what keeps field history alive across the move, so anything but a
    /// clean tree is refused.
    pub fn migrate_layout(&mut self, to: DataLayout) -> Result<MigrationReport, DitError> {
        let from = self.store.layout().kind();
        if from == to {
            return Err(DitError::Refuse(format!(
                "this workspace is already on the `{}` layout",
                to.as_str()
            )));
        }
        if !self.repo.is_clean().unwrap_or(false) {
            return Err(DitError::Refuse(
                "the working tree is not clean — commit or stash before migrating".into(),
            ));
        }
        let lock_path = self.store.layout().write_lock();
        let lock = acquire_lock_or_busy(&lock_path, "migrate-layout")?;
        let result = self.migrate_layout_locked(from, to);
        drop(lock);
        result
    }

    fn migrate_layout_locked(
        &mut self,
        from: DataLayout,
        to: DataLayout,
    ) -> Result<MigrationReport, DitError> {
        let mut report = MigrationReport::default();

        // 1. Content roots move first; everything else keys off their new
        //    location. Tracked trees go through `git mv` so the move stays a
        //    rename; empty untracked roots (init leftovers) just rename.
        for name in CONTENT_ROOTS {
            let old_rel = from.content_root(name);
            let new_rel = to.content_root(name);
            let old_dir = self.repo.root().join(&old_rel);
            if !old_dir.is_dir() {
                continue;
            }
            let new_dir = self.repo.root().join(&new_rel);
            if new_dir.exists() {
                return Err(DitError::Refuse(format!(
                    "`{new_rel}` already exists — move it aside before migrating"
                )));
            }
            if self.repo.ls_tree(&old_rel)?.is_empty() {
                if let Some(parent) = new_dir.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&old_dir, &new_dir)?;
            } else {
                self.repo.mv(&old_rel, &new_rel)?;
            }
            report.roots_moved += 1;
        }

        // 2. Legacy `issue.md` bodies become `README.md` in their new home
        //    (ADR 0006). Scanned on disk, not from HEAD — the move above is
        //    staged but uncommitted, so the tree at HEAD still shows the old
        //    paths. Folders holding both keep both; reads prefer the new
        //    name either way.
        let issues_dir = self.repo.root().join(to.content_root("issues"));
        let mut stack = vec![issues_dir];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.file_name() != Some(std::ffi::OsStr::new(dit_model::LEGACY_ISSUE_BODY_FILE))
                {
                    continue;
                }
                let readme = path.with_file_name(dit_model::ISSUE_BODY_FILE);
                if readme.exists() {
                    continue;
                }
                self.repo.mv(
                    &rel_to_root(self.repo.root(), &path),
                    &rel_to_root(self.repo.root(), &readme),
                )?;
                report.bodies_renamed += 1;
            }
        }

        // 3. The merge-driver routing follows the tree it protects. Most
        //    workspaces predate the file entirely — the migration is the
        //    moment it appears, at the mode-correct path.
        let attrs_rel = |kind: DataLayout| match kind {
            DataLayout::Root => ".gitattributes",
            DataLayout::DotDir => ".dit/.gitattributes",
        };
        let old_attrs = self.repo.root().join(attrs_rel(from));
        let new_attrs_rel = attrs_rel(to);
        let new_attrs = self.repo.root().join(new_attrs_rel);
        let mut attrs_needs_staging = false;
        if !new_attrs.exists() {
            if !self.repo.ls_tree(attrs_rel(from))?.is_empty() {
                self.repo.mv(attrs_rel(from), new_attrs_rel)?;
            } else if old_attrs.exists() {
                if let Some(parent) = new_attrs.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&old_attrs, &new_attrs)?;
                attrs_needs_staging = true;
            } else {
                dit_store::atomic::write(&new_attrs, GIT_ATTRIBUTES)?;
                attrs_needs_staging = true;
            }
        }

        // 4. The config flips last — this is what a re-opened Store detects.
        let mut config = self.config.clone();
        config.layout = to;
        dit_store::atomic::write(
            &self.store.layout().config_yaml(),
            &dit_parse::write_config(&config),
        )?;

        // 5. One commit: everything above is staged by the moves themselves
        //    plus `.dit` for the config (and, in dotdir, the routing file).
        self.repo.add(".dit")?;
        if to == DataLayout::Root && attrs_needs_staging {
            self.repo.add(".gitattributes")?;
        }
        self.repo.commit(&format!(
            "dit migrate layout: {} -> {}",
            from.as_str(),
            to.as_str()
        ))?;

        // 6. The store re-opens on the new layout, and the index rebuilds
        //    from the moved tree.
        self.store = Store::open(self.repo.root());
        self.reload_schema();
        self.reindex(ReindexMode::All)?;
        Ok(report)
    }
}

/// A repo-relative, forward-slash path for git arguments — git on every
/// platform accepts this form, and quoting the wrong separator would fail
/// only on Windows, which is exactly where nobody is looking.
fn rel_to_root(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

/// Take the single-writer lock, or say who holds it. The lock file's content
/// is the holder's name — the exact thing a "workspace is busy" message
/// wants to show.
fn acquire_lock_or_busy(lock_path: &Path, author: &str) -> Result<LockGuard, DitError> {
    atomic::acquire_lock(lock_path, author).map_err(|_| {
        let held_by = std::fs::read_to_string(lock_path)
            .unwrap_or_default()
            .trim()
            .to_owned();
        DitError::Busy { held_by }
    })
}

/// What a layout migration did — the receipt `dit migrate-layout` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MigrationReport {
    pub roots_moved: usize,
    pub bodies_renamed: usize,
}

/// Relative link from the generated `issues/README.md` to an issue's folder:
/// strip the issues-root prefix and the body file name, keep the trailing
/// slash so a forge renders the folder's `README.md` body (ADR 0006/0008).
fn folder_link(repo_path: &str) -> String {
    let Some((_, rest)) = repo_path.split_once("issues/") else {
        return repo_path.to_owned();
    };
    let Some((dir, _)) = rest.rsplit_once('/') else {
        return rest.to_owned();
    };
    format!("{dir}/")
}

/// Build the ambiguity error (ADR 0018): every candidate named with its id
/// and title, so the caller can restate the reference without a round trip.
fn ambiguous(needle: &str, candidates: Vec<(String, String)>) -> DitError {
    let listing = candidates
        .iter()
        .map(|(id, title)| format!("{id} ({title})"))
        .collect::<Vec<_>>()
        .join(", ");
    DitError::Ambiguous {
        needle: needle.to_owned(),
        count: candidates.len(),
        listing,
    }
}

/// Split `write_workflow` output into (lanes block, coordination block) so
/// `init_workflow` can append exactly those sections to an existing file.
/// Blocks absent from the input come back empty.
fn split_coordination_blocks(text: &str) -> (String, String) {
    let mut lanes = String::new();
    let mut coordination = String::new();
    let mut target: Option<&mut String> = None;
    for line in text.lines() {
        if line.starts_with("lanes:") {
            lanes.push_str("lanes:\n");
            target = Some(&mut lanes);
            continue;
        }
        if line.starts_with("coordination:") {
            coordination.push_str("coordination:\n");
            target = Some(&mut coordination);
            continue;
        }
        if let Some(t) = target.as_deref_mut() {
            if line.starts_with(' ') {
                t.push_str(line);
                t.push('\n');
            } else {
                target = None;
            }
        }
    }
    (lanes, coordination)
}

/// The coordination block `init_workflow` scaffolds when the workspace's
/// settings are still the defaults. The canonical emitter skips a default
/// block (byte-stable round-trips), but scaffolding exists to be read by
/// humans — the knobs should be visible, not implied. Pinned to parse back
/// to `Coordination::default()` by the core tests.
const DEFAULT_COORDINATION_BLOCK: &str = "\
coordination:
  claim_ttl_minutes: 15
  readiness:
    pick_from: todo
    gate: terminal
";

/// This build's version, stamped into the generated index marker so a reader
/// can tell which dit wrote the file (ADR 0008).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The merge-driver routing `init` writes and `migrate-layout` places
/// (ADR 0005). Both patterns are load-bearing: `*.md` routes the bodies, and
/// the anchored `**/comments/*.md` reaches the per-comment files a single
/// star would miss (§5.3, re-verified in ADR 0005's throwaway repo).
const GIT_ATTRIBUTES: &str = "\
# DIT: route markdown through the frontmatter-aware merge driver.
*.md merge=dit-md
**/comments/*.md merge=dit-md
";

/// The default issue template: the evidence-first shape every issue owes a
/// reader — a summary grounded in code pointers, the plan, the recorded
/// rejected alternative, assertable criteria, load-bearing tests, and a
/// scope fence. Each stock template lives as a plain markdown file under
/// templates/ so a template change is a file swap, not string-constant
/// surgery; include_str! embeds it at compile time so the single-binary
/// install stays self-contained (a runtime path would make `init` depend
/// on this checkout being present).
const TEMPLATE_DEFAULT: &str = include_str!("../templates/default.md");

/// A bug report that can be acted on: exact reproduction, the mechanism
/// rather than the symptom, and a guard so the class does not return.
const TEMPLATE_BUG: &str = include_str!("../templates/bug.md");

/// A story carries its data contract (field tables, enums) and the numbered
/// behaviour rules before its criteria, and records the shape it rejected.
const TEMPLATE_STORY: &str = include_str!("../templates/story.md");

/// A spike is a question with a deadline, not a deliverable - and it ends in
/// one named outcome, not in time running out.
const TEMPLATE_SPIKE: &str = include_str!("../templates/spike.md");

/// The evidence report a waiting actor posts on the blocker's issue (ADR
/// 0015's phase 2): expectation vs actual, with the request and the response
/// verbatim. Seeded by `dit workflow init`, not `dit init` — it belongs to
/// the coordination plane, not to issue authoring.
const TEMPLATE_INTEGRATION_REPORT: &str = include_str!("../templates/integration-report.md");

/// How many issues sat in each workflow category at a point in history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CategoryCounts {
    pub todo: usize,
    pub doing: usize,
    pub done: usize,
}

/// The answer to "what did this workspace look like then, and what has
/// happened since?" — computed from `field_events`, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySummary {
    /// The cutoff this summary was taken at.
    pub seq: i64,
    /// The end of recorded history — the `seq` that means "now".
    pub max_seq: i64,
    /// Events per day, for the scrubber's density strip.
    pub days: Vec<DayCount>,
    pub at_cutoff: CategoryCounts,
    pub now: CategoryCounts,
    pub since: ChangeSummary,
}

/// Render markdown to safe HTML — the only rendering path the UI uses, so
/// the sanitizer and the wire format can never drift apart.
pub fn render_markdown(text: &str) -> String {
    dit_parse::render_html(text)
}

/// A random hex token for the local server's session auth. Same entropy
/// source as issue ids: the OS, not a clock.
pub fn generate_token() -> Result<String, DitError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| DitError::Io(std::io::Error::other(e.to_string())))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The git-config string for the merge driver: an absolute binary path plus
/// the placeholders git substitutes (base, ours, theirs, marker size, label).
/// Git runs this string through a shell, so the path is written with forward
/// slashes — a shell eats backslashes as escapes before finding the binary.
fn driver_command(driver: &Path) -> String {
    let driver = driver.to_string_lossy().replace('\\', "/");
    format!("{driver} merge-driver %O %A %B %L %P")
}

/// The entry point git invokes on this binary: `dit merge-driver %O %A %B %L %P`.
/// Exposed here so the registered driver command and the merge logic ship in
/// one artifact — anything that registers the driver calls this with whatever
/// git hands over. Returns the process exit code: 0 clean, nonzero conflict.
pub fn run_merge_driver(args: &[String]) -> i32 {
    match args {
        [base, ours, theirs, marker_size, label] => dit_vcs::drive(
            Path::new(base.as_str()),
            Path::new(ours.as_str()),
            Path::new(theirs.as_str()),
            marker_size.parse().unwrap_or(7),
            label,
        ),
        // Git could not have invoked us with the wrong arity; if it somehow
        // did, refuse loudly rather than guess which file is which.
        _ => 2,
    }
}

/// Written at the root by `init`: a fresh clone of a DIT workspace greets
/// its reader with a map of what they are looking at (ADR 0005). Hand-owned
/// and hand-editable — never generated (ADR 0008 keeps that class to the
/// issues index alone).
const INIT_README: &str = "\
# This repository is a DIT workspace

Project data lives as plain Markdown: issues under `issues/` (grouped by
year and month, one folder per issue with its `README.md` body), longer
documents under `docs/`, notes under `notes/`, changelogs under
`changelogs/`, epics under `epics/`. Machinery — the workflow definition,
config, people, templates — stays under `.dit/`.

`issues/README.md` is generated by `dit docs build --index`; do not edit it.

Git is the source of truth; everything under `.dit-cache/` is a local,
disposable index and is never committed.

Run `dit doctor` to check that this clone is set up correctly.
";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn repo_relative_paths_use_forward_slashes() {
        let rel = rel_to_root(Path::new("/w"), Path::new("/w/.dit/issues/x/issue.md"));
        assert_eq!(rel, ".dit/issues/x/issue.md");
    }

    #[test]
    fn the_driver_command_uses_forward_slashes() {
        // Git runs the configured command through a shell, and a shell reads
        // a backslash as an escape — a Windows path would arrive at the
        // driver with its separators eaten before the binary is ever found.
        assert_eq!(
            driver_command(Path::new(r"D:\a\dit-cli\target\debug\dit.exe")),
            "D:/a/dit-cli/target/debug/dit.exe merge-driver %O %A %B %L %P"
        );
    }
}
