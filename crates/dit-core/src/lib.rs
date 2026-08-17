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

pub mod board;
pub mod diagnostics;
pub mod error;

use std::path::Path;

use dit_index::Index;
use dit_store::atomic::{self, LockGuard};
use dit_store::{Changeset, Store};
use dit_vcs::{Repo, EMPTY_TREE};
use time::OffsetDateTime;

pub use board::{Board, BoardColumn};
pub use diagnostics::{Diagnostic, DiagnosticLevel};
pub use error::DitError;
// Types from below the facade that appear in its signatures. Callers
// construct arguments out of these, so they must be reachable without a
// second dependency — the facade is the only crate delivery names.
pub use dit_index::IndexedIssue;
pub use dit_model::{
    Comment, Config, DerivedSignal, FieldPatch, Issue, IssueDraft, IssueId, IssueKind, Priority,
    StoredFieldEvent, Workflow, WorkflowStatus,
};
pub use dit_vcs::{SyncOptions, SyncReport};

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
        Ok(dit)
    }

    /// Turn an empty directory into a workspace: git init, the ignore entry
    /// that keeps the index database out of history, a README so the hidden
    /// `.dit/` folder does not look like an empty repo, the merge driver
    /// pointed at `driver` (the absolute path of this binary), and one
    /// bootstrap commit so the first real write is never a root commit.
    ///
    /// `driver` is passed in rather than read from `current_exe()` because a
    /// test process is not the binary users run — the caller knows which
    /// executable actually serves `merge-driver`.
    pub fn init(path: &Path, driver: &Path) -> Result<Dit, DitError> {
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
        dit_store::atomic::write(&path.join(".gitignore"), ".dit-cache/\n")?;
        dit_store::atomic::write(&path.join("README.md"), INIT_README)?;
        repo.add(".gitignore")?;
        repo.add("README.md")?;
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

    /// One issue by full id or 7-character short ref. Answers from the index
    /// like every read; the filesystem is never consulted.
    pub fn get(&self, needle: &str) -> Result<Option<IndexedIssue>, DitError> {
        if needle.len() == 26 {
            if let Ok(id) = IssueId::parse(needle) {
                return self.index.get_issue(&id).map_err(DitError::Index);
            }
        }
        // Short refs go through the same compiled-query path as user queries,
        // so there is one resolution mechanism, not two.
        let query = dit_query::Query {
            filter: Some(dit_query::Expr::Cmp {
                field: dit_query::Field::ShortRef,
                op: dit_query::Op::Eq,
                value: dit_query::Val::Str(needle.to_owned()),
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
        out
    }

    // -- writes -----------------------------------------------------------------

    /// Begin a write. Fails with [`DitError::Busy`] when another process
    /// holds the workspace lock. The author is named here, not read from
    /// config, because a server may commit on behalf of the person who
    /// clicked — attribution must follow the action, not the machine.
    pub fn transaction(&mut self, author: &str) -> Result<Transaction<'_>, DitError> {
        let lock_path = self.store.layout().write_lock();
        let lock = match atomic::acquire_lock(&lock_path, author) {
            Ok(lock) => lock,
            Err(_) => {
                // The error text is a sentence for logs; callers want the bare
                // name to show ("locked by farid"), and the lock file holds
                // exactly that.
                let held_by = std::fs::read_to_string(&lock_path)
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                return Err(DitError::Busy { held_by });
            }
        };
        let prev_head = self.repo.head().ok();
        let store_tx = self.store.transaction(OffsetDateTime::now_utc(), author);
        Ok(Transaction {
            dit: self,
            store_tx,
            lock,
            prev_head,
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
            for (path, blob) in self.repo.ls_tree(".dit/")? {
                let Some(text) = self.repo.show_text(&format!("HEAD:{path}")) else {
                    continue;
                };
                if path.ends_with("issue.md") {
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
        }
        if matches!(mode, ReindexMode::Events | ReindexMode::All) {
            let watermark = self.index.watermark("events")?;
            let events = dit_vcs::walk_field_events(&self.repo, watermark.as_deref())?;
            report.events = self.index.record_field_events(&events)?;
            self.index.set_watermark("events", &head)?;
        }
        Ok(report)
    }

    /// The issue a comment file belongs to: the `issue.md` living in the
    /// folder above `comments/`. Derived from the tree, never stored, so a
    /// moved folder cannot orphan a comment.
    fn issue_owning(&self, comment_path: &str) -> Result<Option<IssueId>, DitError> {
        let Some(marker) = comment_path.find("/comments/") else {
            return Ok(None);
        };
        let dir = &comment_path[..marker];
        if dir.is_empty() {
            return Ok(None);
        }
        let Some(text) = self.repo.show_text(&format!("HEAD:{dir}/issue.md")) else {
            return Ok(None);
        };
        match dit_parse::parse_issue(&text) {
            Ok((issue, _)) => Ok(Some(issue.id)),
            Err(_) => Ok(None),
        }
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
}

impl std::fmt::Debug for Transaction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("staged", &self.store_tx.staged_len())
            .finish()
    }
}

impl<'a> Transaction<'a> {
    pub fn create_issue(&mut self, draft: IssueDraft) -> Result<IssueId, DitError> {
        Ok(self.store_tx.create_issue(draft)?)
    }

    pub fn set_fields(&mut self, id: &IssueId, patch: FieldPatch) -> Result<(), DitError> {
        Ok(self.store_tx.set_fields(id, patch)?)
    }

    /// Replace the markdown body; frontmatter is untouched down to the byte.
    pub fn set_body(&mut self, id: &IssueId, body: &str) -> Result<(), DitError> {
        Ok(self.store_tx.set_body(id, body)?)
    }

    pub fn comment(&mut self, id: &IssueId, alias: &str, body: &str) -> Result<IssueId, DitError> {
        Ok(self.store_tx.add_comment(id, alias, body)?)
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
        let diff = self
            .repo
            .git(&["diff", "--name-status", "-M", base, head, "--", ".dit/"])?;
        for line in diff.lines() {
            let mut fields = line.split('\t');
            let Some(status) = fields.next() else {
                continue;
            };
            let Some(new_path) = fields.next_back() else {
                continue;
            };
            // Deletions do not originate from this tool's writes; they
            // arrive through sync, which rebuilds the whole index anyway.
            if status.starts_with('D') {
                continue;
            }
            let Some(text) = self.repo.show_text(&format!("{head}:{new_path}")) else {
                continue;
            };
            let blob = self
                .repo
                .git(&["rev-parse", &format!("{head}:{new_path}")])?;
            if new_path.ends_with("issue.md") {
                if let Ok((issue, _)) = dit_parse::parse_issue(&text) {
                    self.index.upsert_issue(&issue, new_path, &blob)?;
                }
            } else if new_path.contains("/comments/") {
                if let Ok(comment) = dit_parse::parse_comment(&text) {
                    if let Some(parent) = self.issue_owning(new_path)? {
                        self.index.upsert_comment(&parent, &comment)?;
                    }
                }
            }
        }
        let events = dit_vcs::walk_field_events(&self.repo, prev_head)?;
        self.index.record_field_events(&events)?;
        self.index.set_watermark("events", head)?;
        Ok(())
    }
}

/// A repo-relative, forward-slash path for git arguments — git on every
/// platform accepts this form, and quoting the wrong separator would fail
/// only on Windows, which is exactly where nobody is looking.
fn rel_to_root(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
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
fn driver_command(driver: &Path) -> String {
    format!("{} merge-driver %O %A %B %L %P", driver.to_string_lossy())
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

/// Written at the root by `init`: a fresh clone of a DIT workspace looks
/// like an empty repository, and the first thing anyone runs is `ls`.
const INIT_README: &str = "\
# This repository is a DIT workspace

Project data lives as Markdown files under `.dit/` — issues in
`.dit/issues/`, the workflow definition in `.dit/schema/workflow.yaml`.
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
}
