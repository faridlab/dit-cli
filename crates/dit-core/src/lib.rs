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
    Comment, Config, DataLayout, DerivedSignal, DocEntry, DocPath, DocPathError, FieldPatch, Issue,
    IssueDraft, IssueId, IssueKind, Numbering, Priority, StoredFieldEvent, Workflow,
    WorkflowStatus, CONTENT_ROOTS, DOC_ROOTS, GENERATED_INDEX_MARKER,
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
        if ignore.exists() {
            let mut text = std::fs::read_to_string(&ignore)?;
            if !text.lines().any(|l| l.trim() == ".dit-cache/") {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str("\n# DIT's disposable index — never committed.\n.dit-cache/\n");
                dit_store::atomic::write(&ignore, &text)?;
            }
        } else {
            dit_store::atomic::write(&ignore, ".dit-cache/\n")?;
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
        // Issue templates (description / criteria / user acceptance test):
        // seeded once; hand edits survive every later init.
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
        Ok(self.store_tx.set_fields(id, patch)?)
    }

    /// Replace the markdown body; frontmatter is untouched down to the byte.
    pub fn set_body(&mut self, id: &IssueId, body: &str) -> Result<(), DitError> {
        Ok(self.store_tx.set_body(id, body)?)
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

    pub fn comment(&mut self, id: &IssueId, alias: &str, body: &str) -> Result<IssueId, DitError> {
        Ok(self.store_tx.add_comment(id, alias, body)?)
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
            if dit_model::looks_like_issue_body(new_path, layout) {
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
        let events = dit_vcs::walk_field_events(&self.repo, prev_head, layout)?;
        self.index.record_field_events(&events)?;
        self.index.set_watermark("events", head)?;
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

/// The default issue template: the three sections every issue owes a reader
/// — what is going on, what "done" means, and how a human verifies it.
const TEMPLATE_DEFAULT: &str = "\
## Description

<!-- What is going on? What did you expect instead? -->

## Criteria

<!-- What must be true when this is done? One bullet each. -->

- [ ]

## User acceptance test

<!-- How does a user verify this? Steps a real person can follow. -->

1.
";

/// A bug adds the reproduction steps that turn "it's broken" into a report.
const TEMPLATE_BUG: &str = "\
## Description

<!-- What is broken? What did you expect instead? -->

## Steps to reproduce

1.

## Criteria

- [ ]

## User acceptance test

1.
";

/// A story states the want in the user's words before the criteria.
const TEMPLATE_STORY: &str = "\
## Story

As a … , I want … , so that … .

## Criteria

- [ ]

## User acceptance test

1.
";

/// A spike is a question with a deadline, not a deliverable.
const TEMPLATE_SPIKE: &str = "\
## Question

<!-- What must this spike answer before time runs out? -->

## Findings

<!-- What was learned, with links to the evidence. -->

## Recommendation

<!-- What should happen next. -->
";

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
