//! The disposable SQLite index: everything the read path needs, and nothing
//! that cannot be rebuilt from the files.
//!
//! The division of labor is strict. Files in git are the source of truth;
//! this database is a cache that happens to speak SQL. Writes arrive here
//! only after the corresponding files are committed — the index is updated to
//! *match* reality, never to record it. Delete `index.sqlite` and the worst
//! outcome is a rebuild.
//!
//! Consequences visible in the code below:
//! - FTS stays in sync through SQL triggers, not Rust bookkeeping — an
//!   external-content FTS table drifts *silently* if any write path forgets
//!   the delete-then-insert dance, and a cache that lies is worse than no
//!   cache.
//! - `field_events` rows are ordered by `seq`, a position in the commit
//!   graph. Ordering by timestamp would contradict itself on every merge
//!   commit, which carries several dates at once.
//! - Backfills are idempotent through a uniqueness key that includes
//!   `parent_sha`, with `''` — not NULL — for "no parent", because NULL
//!   values slip past SQLite's uniqueness check and double the history on a
//!   second run.

use std::path::Path;

use dit_model::{
    ChangeSummary, Comment, DayCount, FieldEvent, Issue, IssueId, Release, ReleaseStatus,
    StoredFieldEvent,
};
use dit_query::{compile, Compiled, Query, SqlVal};
use rusqlite::{params, Connection, OptionalExtension};
use time::OffsetDateTime;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("the index holds a value that no longer parses: {0}")]
    Corrupt(String),
}

/// An issue as stored: the typed aggregate plus the facts only the index
/// knows — where the file lives and which blob it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedIssue {
    pub issue: Issue,
    pub path: String,
    pub blob_sha: String,
}

/// A release plan as stored (DESIGN.md §15.2), plus where its file lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedRelease {
    pub release: Release,
    pub path: String,
}

/// One row of the workspace-wide comment feed: the comment plus enough of
/// its issue to render a feed line without a second lookup per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceComment {
    pub comment: Comment,
    pub issue_id: IssueId,
    /// `Some(12)` displays as `#12`; `None` until the issue is numbered, or
    /// when the issue is no longer indexed.
    pub number: Option<u32>,
    /// Empty when the issue is gone — a comment outlives its subject in the
    /// index only until the next state rebuild, but the feed must not die
    /// on that window.
    pub title: String,
}

/// One flow's authored shape as the index holds it: the fence's own bytes,
/// where they were found, and why they could not be used if they could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFlowShape {
    pub flow: String,
    pub path: String,
    pub line: usize,
    pub shape: String,
    pub problem: Option<String>,
}

/// One Morse scenario as the index holds it: the fence's own bytes, where
/// they were found, and why they could not be used if they could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMorseScenario {
    pub scenario: String,
    pub path: String,
    pub line: usize,
    /// Empty when the fence did not parse far enough to state them.
    pub spec_id: String,
    pub pin: String,
    pub env: Option<String>,
    pub body: String,
    /// Why the fence itself could not be read. Distinct from the two below:
    /// this one means DIT never got as far as a scenario.
    pub problem: Option<String>,
    /// How many commits have touched the spec since the pin. Computed at
    /// reindex and stored here rather than answered from git on the read
    /// path, so reads still come only from the index (I2).
    pub stale_by: Option<usize>,
    /// Why the scenario cannot be run as written — one reason per line.
    pub broken: Option<String>,
}

/// One registered spec as last read (§20.2). `head` is the commit of the
/// repo holding it at the time the catalogue was built — in Mode A that is
/// the linked code repo, not this workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMorseSpec {
    pub spec_id: String,
    pub repo: Option<String>,
    pub path: String,
    pub head: Option<String>,
    pub title: Option<String>,
    pub version: Option<String>,
    pub problem: Option<String>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS issues (
  id          TEXT PRIMARY KEY,
  number      INTEGER,
  path        TEXT NOT NULL,
  blob_sha    TEXT NOT NULL,
  short_ref   TEXT NOT NULL,
  title       TEXT NOT NULL,
  type        TEXT NOT NULL,
  status      TEXT NOT NULL,
  priority    TEXT,
  reporter    TEXT,
  epic        TEXT,
  estimate    INTEGER,
  sprint      TEXT,
  due         TEXT,
  start       TEXT,
  lane        TEXT,
  claimed_by  TEXT,
  claimed_at  TEXT,
  created     TEXT NOT NULL,
  updated     TEXT NOT NULL,
  body        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_assignees (
  issue_id TEXT NOT NULL,
  pos      INTEGER NOT NULL,
  alias    TEXT NOT NULL,
  PRIMARY KEY (issue_id, alias)
);
CREATE TABLE IF NOT EXISTS issue_labels (
  issue_id TEXT NOT NULL,
  pos      INTEGER NOT NULL,
  label    TEXT NOT NULL,
  PRIMARY KEY (issue_id, label)
);
CREATE TABLE IF NOT EXISTS issue_blocked_by (
  issue_id      TEXT NOT NULL,
  pos           INTEGER NOT NULL,
  blocked_by_id TEXT NOT NULL,
  PRIMARY KEY (issue_id, blocked_by_id)
);
CREATE TABLE IF NOT EXISTS issue_fed_by (
  issue_id  TEXT NOT NULL,
  pos       INTEGER NOT NULL,
  fed_by_id TEXT NOT NULL,
  PRIMARY KEY (issue_id, fed_by_id)
);
CREATE TABLE IF NOT EXISTS issue_flows (
  issue_id TEXT NOT NULL,
  pos      INTEGER NOT NULL,
  flow     TEXT NOT NULL,
  PRIMARY KEY (issue_id, flow)
);

CREATE TABLE IF NOT EXISTS releases (
  version    TEXT PRIMARY KEY,
  path       TEXT NOT NULL,
  status     TEXT NOT NULL,
  target_ref TEXT,
  repo       TEXT,
  target     TEXT
);
CREATE TABLE IF NOT EXISTS release_includes (
  version  TEXT NOT NULL,
  pos      INTEGER NOT NULL,
  issue_id TEXT NOT NULL,
  PRIMARY KEY (version, issue_id)
);

CREATE TABLE IF NOT EXISTS comments (
  id       TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL,
  author   TEXT NOT NULL,
  at       TEXT NOT NULL,
  reply_to TEXT,
  body     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_comments_issue ON comments(issue_id, at);

CREATE TABLE IF NOT EXISTS field_events (
  seq        INTEGER PRIMARY KEY,
  issue_id   TEXT NOT NULL,
  field      TEXT NOT NULL,
  old_value  TEXT,
  new_value  TEXT,
  author     TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  parent_sha TEXT NOT NULL DEFAULT '',
  ts         TEXT NOT NULL,
  source     TEXT NOT NULL,
  UNIQUE (commit_sha, parent_sha, issue_id, field, source)
);
CREATE INDEX IF NOT EXISTS idx_events_issue_field ON field_events(issue_id, field, seq);

-- The authored half of a flow diagram (ADR 0020), parsed from a `dit-flow`
-- fence at reindex. Stored so the read path still answers from the index
-- (I2) instead of walking the document tree every time a diagram is drawn.
-- `problem` is set when the fence did not parse: the flow still draws, under
-- a banner naming the document and line.
CREATE TABLE IF NOT EXISTS flow_shapes (
  flow     TEXT PRIMARY KEY,
  path     TEXT NOT NULL,
  line     INTEGER NOT NULL,
  shape    TEXT NOT NULL,
  problem  TEXT
);

-- Morse (§20, ADR 0022). Both tables are a catalogue, not a copy: the
-- operations are rebuilt from the OpenAPI document at every reindex and the
-- document stays the only source of truth for them (I5). Nothing here is
-- ever written back to a file, and reading either table never fetches
-- anything (I11).
CREATE TABLE IF NOT EXISTS morse_specs (
  spec_id  TEXT PRIMARY KEY,
  repo     TEXT,
  path     TEXT NOT NULL,
  head     TEXT,
  title    TEXT,
  version  TEXT,
  problem  TEXT
);

CREATE TABLE IF NOT EXISTS morse_operations (
  spec_id      TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  method       TEXT NOT NULL,
  path         TEXT NOT NULL,
  summary      TEXT,
  PRIMARY KEY (spec_id, operation_id)
);

-- One scenario per name, like flow shapes: a second fence for the same
-- scenario is a warning rather than a merge, because two chains under one
-- name have no defined resolution.
CREATE TABLE IF NOT EXISTS morse_scenarios (
  scenario TEXT PRIMARY KEY,
  path     TEXT NOT NULL,
  line     INTEGER NOT NULL,
  spec_id  TEXT NOT NULL,
  pin      TEXT NOT NULL,
  env      TEXT,
  body     TEXT NOT NULL,
  problem  TEXT,
  stale_by INTEGER,
  broken   TEXT
);

CREATE TABLE IF NOT EXISTS state (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS issues_fts USING fts5(
  title, body, content='issues', content_rowid='rowid', tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS issues_fts_ai AFTER INSERT ON issues BEGIN
  INSERT INTO issues_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
CREATE TRIGGER IF NOT EXISTS issues_fts_ad AFTER DELETE ON issues BEGIN
  INSERT INTO issues_fts(issues_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
END;
CREATE TRIGGER IF NOT EXISTS issues_fts_au AFTER UPDATE OF title, body ON issues BEGIN
  INSERT INTO issues_fts(issues_fts, rowid, title, body)
    VALUES ('delete', old.rowid, old.title, old.body);
  INSERT INTO issues_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
END;
"#;

/// Bumped whenever the schema below changes shape. The index is disposable —
/// an on-disk file stamped with an older version is dropped and rebuilt from
/// git rather than migrated in place (§6: SQLite is only an index).
const INDEX_VERSION: i64 = 6;

/// The column list every issue SELECT shares, in a fixed order. Hand-written
/// SELECTs drifting out of step with the schema is the known failure mode of
/// this file, so there is exactly one list — pinned against `SCHEMA` by test.
const ISSUE_SELECT: &str = "path, blob_sha, title, type, status, priority, reporter, epic, \
     estimate, sprint, due, start, lane, claimed_by, claimed_at, created, updated, body, number";

/// The default row cap when a query names no limit. A cap exists because the
/// API serves people, not exports; a workspace that genuinely holds more
/// matching issues pages with an explicit limit.
pub const DEFAULT_LIMIT: u32 = 500;

/// Debug shows the connection's path only — a full dump of a Connection
/// would say nothing useful and would grow with every SQLite internal field.
impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Index")
            .field("path", &self.conn.path())
            .finish()
    }
}

pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open (creating if needed) the index file at `path`. The parent
    /// directory is created too — `.dit-cache/` may not exist on first run.
    pub fn open(path: &Path) -> Result<Index, IndexError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| IndexError::Corrupt(format!("cannot create index directory: {e}")))?;
        }
        Index::init(Connection::open(path)?)
    }

    /// An in-memory index — tests, and any caller that wants a throwaway.
    pub fn in_memory() -> Result<Index, IndexError> {
        Index::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Index, IndexError> {
        // WAL lets the server read while a write is in flight; NORMAL sync is
        // the documented pairing for WAL, and this file is a rebuildable
        // cache, so extra durability spending buys nothing.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        // A stamp can lie: a binary that adopted an old table and stamped it
        // current (the pre-probe guard) leaves version == INDEX_VERSION over
        // a stale shape. Probe the one column every schema bump so far has
        // touched, so the check is over what the table IS, not what a
        // previous writer claimed. `prepare`, not `query_row`: compiling the
        // statement fails exactly when the table or column is absent, while
        // an executed probe would also fail on an empty (healthy) table.
        let stale_shape = conn.prepare("SELECT number FROM issues").is_err();
        if version != INDEX_VERSION || stale_shape {
            // A cache this binary did not write: older (pre-`number`), from
            // before version stamps existed at all (version 0 with tables),
            // falsely stamped, or newer (a binary we cannot out-guess). Drop
            // everything and let the caller reindex from git — cheaper and
            // safer than ALTER TABLE on a file whose whole reason to exist is
            // being rebuildable. On a fresh database the drops are no-ops.
            conn.execute_batch(
                "DROP TABLE IF EXISTS issues_fts;
                 DROP TABLE IF EXISTS issue_assignees;
                 DROP TABLE IF EXISTS issue_labels;
                 DROP TABLE IF EXISTS issue_blocked_by;
                 DROP TABLE IF EXISTS issue_fed_by;
                 DROP TABLE IF EXISTS releases;
                 DROP TABLE IF EXISTS release_includes;
                 DROP TABLE IF EXISTS comments;
                 DROP TABLE IF EXISTS field_events;
                 DROP TABLE IF EXISTS state;
                 DROP TABLE IF EXISTS issues;",
            )?;
        }
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", INDEX_VERSION)?;
        Ok(Index { conn })
    }

    // -- writes --------------------------------------------------------------

    /// Insert or replace one issue row, keeping the side tables and the
    /// full-text index in step. Only called after the file exists in a
    /// commit — the index mirrors, it does not originate.
    pub fn upsert_issue(
        &mut self,
        issue: &Issue,
        path: &str,
        blob_sha: &str,
    ) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        // Delete-then-insert rather than an upsert clause: the interaction
        // between `ON CONFLICT DO UPDATE` and update triggers has varied
        // across SQLite releases, and every path here must fire the FTS
        // triggers identically.
        tx.execute(
            "DELETE FROM issues WHERE id = ?1",
            params![issue.id.as_str()],
        )?;
        tx.execute(
            "INSERT INTO issues (id, number, path, blob_sha, short_ref, title, type, status, \
             priority, reporter, epic, estimate, sprint, due, start, lane, claimed_by, claimed_at, \
             created, updated, body) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
            params![
                issue.id.as_str(),
                issue.number,
                path,
                blob_sha,
                issue.id.short_ref().as_str(),
                issue.title,
                issue.kind.as_str(),
                issue.status,
                issue.priority.map(|p| p.as_str()),
                issue.reporter,
                issue.epic.map(|e| e.as_str().to_owned()),
                issue.estimate,
                issue.sprint,
                issue.due,
                issue.start,
                issue.lane,
                issue.claimed_by,
                issue.claimed_at,
                issue.created,
                issue.updated,
                issue.body,
            ],
        )?;
        replace_set(
            &tx,
            "issue_assignees",
            "alias",
            issue.id.as_str(),
            &issue.assignees,
        )?;
        replace_set(
            &tx,
            "issue_labels",
            "label",
            issue.id.as_str(),
            &issue.labels,
        )?;
        let blockers: Vec<String> = issue
            .blocked_by
            .iter()
            .map(|b| b.as_str().to_owned())
            .collect();
        replace_set(
            &tx,
            "issue_blocked_by",
            "blocked_by_id",
            issue.id.as_str(),
            &blockers,
        )?;
        let feeders: Vec<String> = issue.fed_by.iter().map(|b| b.as_str().to_owned()).collect();
        replace_set(
            &tx,
            "issue_fed_by",
            "fed_by_id",
            issue.id.as_str(),
            &feeders,
        )?;
        replace_set(&tx, "issue_flows", "flow", issue.id.as_str(), &issue.flows)?;
        tx.commit()?;
        Ok(())
    }

    /// Drop an issue entirely — the file is gone from the repo.
    pub fn remove_issue(&mut self, id: &IssueId) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM issues WHERE id = ?1", params![id.as_str()])?;
        tx.execute(
            "DELETE FROM issue_assignees WHERE issue_id = ?1",
            params![id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM issue_labels WHERE issue_id = ?1",
            params![id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM issue_blocked_by WHERE issue_id = ?1",
            params![id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM issue_fed_by WHERE issue_id = ?1",
            params![id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM issue_flows WHERE issue_id = ?1",
            params![id.as_str()],
        )?;
        tx.execute(
            "DELETE FROM comments WHERE issue_id = ?1",
            params![id.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_comment(
        &mut self,
        issue_id: &IssueId,
        comment: &Comment,
    ) -> Result<(), IndexError> {
        self.conn.execute(
            "INSERT INTO comments (id, issue_id, author, at, reply_to, body) \
             VALUES (?1,?2,?3,?4,?5,?6) \
             ON CONFLICT(id) DO UPDATE SET author=?3, at=?4, reply_to=?5, body=?6",
            params![
                comment.id.as_str(),
                issue_id.as_str(),
                comment.author,
                comment.created,
                comment.reply_to.map(|r| r.as_str().to_owned()),
                comment.body
            ],
        )?;
        Ok(())
    }

    /// Record one flow's authored shape, or the reason its fence could not
    /// be read. The first fence for a flow wins: a second one is a warning,
    /// not a merge, because two shapes for one diagram have no defined
    /// resolution and guessing would be worse than saying so.
    pub fn upsert_flow_shape(
        &mut self,
        flow: &str,
        path: &str,
        line: usize,
        shape: &str,
        problem: Option<&str>,
    ) -> Result<bool, IndexError> {
        let taken: Option<String> = self
            .conn
            .query_row(
                "SELECT path FROM flow_shapes WHERE flow = ?1",
                params![flow],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(taken) = taken {
            if taken != path {
                return Ok(false);
            }
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO flow_shapes (flow, path, line, shape, problem) \
             VALUES (?1,?2,?3,?4,?5)",
            params![flow, path, line as i64, shape, problem],
        )?;
        Ok(true)
    }

    /// How many distinct commits have touched each issue, by id. Derived
    /// from the recorded field events — the same walk the history layer
    /// already does — so the diagram can mark which nodes have real work
    /// behind them without anyone authoring a link (ADR 0021).
    pub fn commit_counts(&self) -> Result<std::collections::HashMap<String, usize>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT issue_id, COUNT(DISTINCT commit_sha) FROM field_events GROUP BY issue_id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, count) = row?;
            out.insert(id, count as usize);
        }
        Ok(out)
    }

    /// Replace one spec's catalogue: the spec row and every operation it
    /// describes. Called once per spec per reindex, so the catalogue can
    /// only ever say what the document at `head` says.
    pub fn replace_morse_spec(
        &mut self,
        spec: &StoredMorseSpec,
        operations: &[dit_model::SpecOperation],
    ) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM morse_operations WHERE spec_id = ?1",
            params![spec.spec_id],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO morse_specs \
             (spec_id, repo, path, head, title, version, problem) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                spec.spec_id,
                spec.repo,
                spec.path,
                spec.head,
                spec.title,
                spec.version,
                spec.problem
            ],
        )?;
        for op in operations {
            tx.execute(
                "INSERT OR REPLACE INTO morse_operations \
                 (spec_id, operation_id, method, path, summary) VALUES (?1,?2,?3,?4,?5)",
                params![
                    spec.spec_id,
                    op.operation_id,
                    op.method,
                    op.path,
                    op.summary
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Forget every spec and its operations, ahead of a rebuild.
    pub fn clear_morse_specs(&mut self) -> Result<(), IndexError> {
        self.conn.execute("DELETE FROM morse_operations", [])?;
        self.conn.execute("DELETE FROM morse_specs", [])?;
        Ok(())
    }

    /// Every registered spec, by id.
    pub fn morse_specs(&self) -> Result<Vec<StoredMorseSpec>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT spec_id, repo, path, head, title, version, problem \
             FROM morse_specs ORDER BY spec_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(StoredMorseSpec {
                spec_id: r.get(0)?,
                repo: r.get(1)?,
                path: r.get(2)?,
                head: r.get(3)?,
                title: r.get(4)?,
                version: r.get(5)?,
                problem: r.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(IndexError::from)
    }

    /// One spec's operations, in the order a person reads them: by path,
    /// then by method.
    pub fn morse_operations(
        &self,
        spec_id: &str,
    ) -> Result<Vec<dit_model::SpecOperation>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT operation_id, method, path, summary FROM morse_operations \
             WHERE spec_id = ?1 ORDER BY path, method",
        )?;
        let rows = stmt.query_map(params![spec_id], |r| {
            Ok(dit_model::SpecOperation {
                operation_id: r.get(0)?,
                method: r.get(1)?,
                path: r.get(2)?,
                summary: r.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(IndexError::from)
    }

    /// Record one scenario, or the reason its fence could not be read. The
    /// first fence for a scenario wins, exactly as for flow shapes.
    pub fn upsert_morse_scenario(
        &mut self,
        scenario: &StoredMorseScenario,
    ) -> Result<bool, IndexError> {
        let taken: Option<String> = self
            .conn
            .query_row(
                "SELECT path FROM morse_scenarios WHERE scenario = ?1",
                params![scenario.scenario],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(taken) = taken {
            if taken != scenario.path {
                return Ok(false);
            }
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO morse_scenarios \
             (scenario, path, line, spec_id, pin, env, body, problem, stale_by, broken) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                scenario.scenario,
                scenario.path,
                scenario.line as i64,
                scenario.spec_id,
                scenario.pin,
                scenario.env,
                scenario.body,
                scenario.problem,
                scenario.stale_by.map(|n| n as i64),
                scenario.broken
            ],
        )?;
        Ok(true)
    }

    /// Record what the health pass worked out for one scenario. Separate
    /// from the upsert because the fence is read before the catalogue it is
    /// judged against exists.
    pub fn set_morse_health(
        &mut self,
        scenario: &str,
        stale_by: Option<usize>,
        broken: &[String],
    ) -> Result<(), IndexError> {
        let joined = (!broken.is_empty()).then(|| broken.join("\n"));
        self.conn.execute(
            "UPDATE morse_scenarios SET stale_by = ?2, broken = ?3 WHERE scenario = ?1",
            params![scenario, stale_by.map(|n| n as i64), joined],
        )?;
        Ok(())
    }

    pub fn clear_morse_scenarios(&mut self) -> Result<(), IndexError> {
        self.conn.execute("DELETE FROM morse_scenarios", [])?;
        Ok(())
    }

    /// Forget the scenarios one document declared — what editing or deleting
    /// it means, before its current fences are re-read.
    pub fn clear_morse_scenarios_at(&mut self, path: &str) -> Result<(), IndexError> {
        self.conn
            .execute("DELETE FROM morse_scenarios WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Every scenario, by name.
    pub fn morse_scenarios(&self) -> Result<Vec<StoredMorseScenario>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT scenario, path, line, spec_id, pin, env, body, problem, stale_by, broken \
             FROM morse_scenarios ORDER BY scenario",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(StoredMorseScenario {
                scenario: r.get(0)?,
                path: r.get(1)?,
                line: r.get::<_, i64>(2)? as usize,
                spec_id: r.get(3)?,
                pin: r.get(4)?,
                env: r.get(5)?,
                body: r.get(6)?,
                problem: r.get(7)?,
                stale_by: r.get::<_, Option<i64>>(8)?.map(|n| n as usize),
                broken: r.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(IndexError::from)
    }

    /// Forget every shape, ahead of a rebuild.
    pub fn clear_flow_shapes(&mut self) -> Result<(), IndexError> {
        self.conn.execute("DELETE FROM flow_shapes", [])?;
        Ok(())
    }

    /// Forget the shapes one document declared — what a document being
    /// edited or deleted means, before its current fences are re-read.
    pub fn clear_flow_shapes_at(&mut self, path: &str) -> Result<(), IndexError> {
        self.conn
            .execute("DELETE FROM flow_shapes WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// One flow's stored shape: the fence's text, where it came from, and
    /// the problem if it did not parse.
    pub fn flow_shape(&self, flow: &str) -> Result<Option<StoredFlowShape>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT flow, path, line, shape, problem FROM flow_shapes WHERE flow = ?1",
                params![flow],
                |r| {
                    Ok(StoredFlowShape {
                        flow: r.get(0)?,
                        path: r.get(1)?,
                        line: r.get::<_, i64>(2)? as usize,
                        shape: r.get(3)?,
                        problem: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Insert or replace one release plan, keeping `release_includes` in
    /// step. Like issues: only called after the file exists in a commit.
    pub fn upsert_release(&mut self, release: &Release, path: &str) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM releases WHERE version = ?1",
            params![release.version],
        )?;
        tx.execute(
            "INSERT INTO releases (version, path, status, target_ref, repo, target) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                release.version,
                path,
                release.status.as_str(),
                release.target_ref,
                release.repo,
                release.target,
            ],
        )?;
        tx.execute(
            "DELETE FROM release_includes WHERE version = ?1",
            params![release.version],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO release_includes (version, pos, issue_id) VALUES (?1, ?2, ?3)",
            )?;
            for (pos, id) in release.includes.iter().enumerate() {
                stmt.execute(params![release.version, pos as i64, id.as_str()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drop a release entirely — the plan file is gone from the repo.
    pub fn remove_release(&mut self, version: &str) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM releases WHERE version = ?1", params![version])?;
        tx.execute(
            "DELETE FROM release_includes WHERE version = ?1",
            params![version],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn remove_comment(&mut self, comment_id: &IssueId) -> Result<(), IndexError> {
        self.conn.execute(
            "DELETE FROM comments WHERE id = ?1",
            params![comment_id.as_str()],
        )?;
        Ok(())
    }

    /// Record observed field changes. Events are stamped with consecutive
    /// `seq` values in the order given — callers pass a walk in commit-graph
    /// order, which is what makes the numbering reproducible across rebuilds.
    /// Already-known events are skipped (a re-run backfill is a no-op), and
    /// the return value says how many rows actually landed.
    pub fn record_field_events(&mut self, events: &[FieldEvent]) -> Result<usize, IndexError> {
        let tx = self.conn.transaction()?;
        let mut seq: i64 = tx
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM field_events", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        let mut inserted = 0usize;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO field_events \
                 (seq, issue_id, field, old_value, new_value, author, commit_sha, parent_sha, ts, source) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            )?;
            for e in events {
                seq += 1;
                inserted += stmt.execute(params![
                    seq,
                    e.issue_id,
                    e.field,
                    e.old_value,
                    e.new_value,
                    e.author,
                    e.commit_sha,
                    e.parent_sha,
                    e.ts,
                    e.source.as_str(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(inserted)
    }

    /// Drop every stored fact (issues, comments, events, watermarks). The
    /// schema survives. Used when the index must be rebuilt from scratch —
    /// stale watermarks would silently skip half of that rebuild, so they go
    /// too.
    pub fn wipe(&mut self) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        for table in [
            "issues",
            "issue_assignees",
            "issue_labels",
            "issue_blocked_by",
            "issue_fed_by",
            "releases",
            "release_includes",
            "comments",
            "field_events",
            "state",
        ] {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Clear the current-state tables only — the issue/comment rows — while
    /// keeping the field history and the watermarks. A state rebuild reads
    /// the same commits either way, but the history walk is the expensive
    /// half and has nothing to gain from being redone.
    pub fn wipe_state(&mut self) -> Result<(), IndexError> {
        let tx = self.conn.transaction()?;
        for table in [
            "issues",
            "issue_assignees",
            "issue_labels",
            "issue_blocked_by",
            "issue_fed_by",
            "releases",
            "release_includes",
            "comments",
        ] {
            tx.execute(&format!("DELETE FROM {table}"), [])?;
        }
        tx.commit()?;
        Ok(())
    }

    // -- reads ---------------------------------------------------------------

    /// One issue by id.
    pub fn get_issue(&self, id: &IssueId) -> Result<Option<IndexedIssue>, IndexError> {
        let Some(cols) = self
            .conn
            .query_row(
                &format!("SELECT {ISSUE_SELECT} FROM issues WHERE id = ?1"),
                params![id.as_str()],
                issue_columns,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let assignees = self.set_for("issue_assignees", "alias", id)?;
        let labels = self.set_for("issue_labels", "label", id)?;
        let blocked_by = self.set_for("issue_blocked_by", "blocked_by_id", id)?;
        let fed_by = self.set_for("issue_fed_by", "fed_by_id", id)?;
        let flows = self.set_for("issue_flows", "flow", id)?;
        Ok(Some(hydrate(
            id, cols, assignees, labels, blocked_by, fed_by, flows,
        )?))
    }

    /// Every issue matching a compiled DQL query. Reads never touch the
    /// filesystem — this method is the whole read path.
    pub fn list_issues(&self, compiled: &Compiled) -> Result<Vec<IndexedIssue>, IndexError> {
        let limit = compiled.limit.unwrap_or(DEFAULT_LIMIT) as i64;
        let mut sql = format!(
            "SELECT id, {ISSUE_SELECT} FROM issues WHERE {}",
            compiled.where_sql
        );
        if compiled.order_sql.is_empty() {
            // A stable default: newest first, id as the tiebreaker.
            sql.push_str(" ORDER BY issues.created DESC, issues.id");
        } else {
            sql.push_str(" ORDER BY ");
            sql.push_str(&compiled.order_sql);
            // The tiebreaker keeps pages deterministic when the ordered
            // column holds equal values.
            sql.push_str(", issues.id");
        }
        sql.push_str(&format!(" LIMIT {limit}"));

        let mut stmt = self.conn.prepare(&sql)?;
        let binds = bindable(&compiled.params);
        let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), |r| {
            Ok((r.get::<_, String>(0)?, issue_columns_from_offset(r)))
        })?;

        let mut found = Vec::new();
        for row in rows {
            let (id_str, cols) = row?;
            let id = IssueId::parse(&id_str)
                .map_err(|e| IndexError::Corrupt(format!("issue id `{id_str}`: {e}")))?;
            let assignees = self.set_for("issue_assignees", "alias", &id)?;
            let labels = self.set_for("issue_labels", "label", &id)?;
            let blocked_by = self.set_for("issue_blocked_by", "blocked_by_id", &id)?;
            let fed_by = self.set_for("issue_fed_by", "fed_by_id", &id)?;
            let flows = self.set_for("issue_flows", "flow", &id)?;
            found.push(hydrate(
                &id, cols, assignees, labels, blocked_by, fed_by, flows,
            )?);
        }
        Ok(found)
    }

    /// Convenience for callers holding a parsed DQL query: parse → compile →
    /// list, with the same injected user and clock the query compiler needs.
    pub fn search(
        &self,
        query: &Query,
        me: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<Vec<IndexedIssue>, IndexError> {
        let compiled = compile(query, me, now)
            .map_err(|e| IndexError::Corrupt(format!("query rejected: {e}")))?;
        self.list_issues(&compiled)
    }

    /// The highest assigned `number:` — the next number under
    /// `numbering: local` is this + 1 (ADR 0007).
    pub fn max_number(&self) -> Result<Option<u32>, IndexError> {
        let max = self
            .conn
            .query_row("SELECT MAX(number) FROM issues", [], |r| {
                r.get::<_, Option<i64>>(0)
            })?;
        Ok(max.map(|m| m as u32))
    }

    /// How many indexed issues carry no `number` — the backfillable set
    /// (ADR 0009). What `dit doctor` counts before suggesting `dit renumber`.
    pub fn unnumbered_count(&self) -> Result<usize, IndexError> {
        let n = self.conn.query_row(
            "SELECT COUNT(*) FROM issues WHERE number IS NULL",
            [],
            |r| r.get::<_, i64>(0),
        )?;
        Ok(n as usize)
    }

    /// Issues holding a given `#number`. 0 or 1 entries in a healthy
    /// workspace; more than one is the duplicate `dit doctor` reports.
    pub fn issues_with_number(&self, number: u32) -> Result<Vec<IndexedIssue>, IndexError> {
        let ids: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM issues WHERE number = ?1 ORDER BY id")?;
            let rows = stmt.query_map(params![i64::from(number)], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        ids.iter()
            .map(|id_str| {
                let id = IssueId::parse(id_str)
                    .map_err(|e| IndexError::Corrupt(format!("issue id `{id_str}`: {e}")))?;
                self.get_issue(&id)?
                    .ok_or_else(|| IndexError::Corrupt(format!("issue `{id_str}` vanished")))
            })
            .collect()
    }

    /// Every number held by more than one issue, with the holders' ids —
    /// the raw material for `dit doctor`'s duplicate-number diagnostic.
    pub fn duplicate_numbers(&self) -> Result<Vec<(u32, Vec<String>)>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT number, id FROM issues WHERE number IS NOT NULL AND number IN \
             (SELECT number FROM issues WHERE number IS NOT NULL \
              GROUP BY number HAVING COUNT(*) > 1) ORDER BY number, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?))
        })?;
        let mut out: Vec<(u32, Vec<String>)> = Vec::new();
        for row in rows {
            let (number, id) = row?;
            match out.last_mut() {
                Some((n, holders)) if *n == number => holders.push(id),
                _ => out.push((number, vec![id])),
            }
        }
        Ok(out)
    }

    fn set_for(&self, table: &str, col: &str, id: &IssueId) -> Result<Vec<String>, IndexError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {col} FROM {table} WHERE issue_id = ?1 ORDER BY pos"
        ))?;
        let values = stmt
            .query_map(params![id.as_str()], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    /// All comments of one issue, oldest first.
    pub fn comments_for(&self, issue_id: &IssueId) -> Result<Vec<Comment>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, author, at, reply_to, body FROM comments WHERE issue_id = ?1 \
             ORDER BY at, id",
        )?;
        let rows = stmt.query_map(params![issue_id.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, author, at, reply_to, body) = row?;
            out.push(Comment {
                id: IssueId::parse(&id)
                    .map_err(|e| IndexError::Corrupt(format!("comment id `{id}`: {e}")))?,
                author,
                created: at,
                reply_to: reply_to
                    .map(|r| {
                        IssueId::parse(&r).map_err(|e| {
                            IndexError::Corrupt(format!("comment `{id}` reply_to `{r}`: {e}"))
                        })
                    })
                    .transpose()?,
                body,
            });
        }
        Ok(out)
    }

    /// One release plan by version.
    pub fn release(&self, version: &str) -> Result<Option<IndexedRelease>, IndexError> {
        let Some(cols) = self
            .conn
            .query_row(
                "SELECT version, path, status, target_ref, repo, target FROM releases \
                 WHERE version = ?1",
                params![version],
                release_columns,
            )
            .optional()?
        else {
            return Ok(None);
        };
        Ok(Some(self.hydrate_release(cols)?))
    }

    /// Every release plan: dated ones first, soonest first, then the undated
    /// ones by version — the order a roadmap draws them in.
    pub fn releases(&self) -> Result<Vec<IndexedRelease>, IndexError> {
        let rows: Vec<ReleaseCols> = {
            let mut stmt = self.conn.prepare(
                "SELECT version, path, status, target_ref, repo, target FROM releases \
                 ORDER BY target IS NULL, target, version",
            )?;
            let rows = stmt.query_map([], release_columns)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        rows.into_iter()
            .map(|cols| self.hydrate_release(cols))
            .collect()
    }

    fn hydrate_release(&self, cols: ReleaseCols) -> Result<IndexedRelease, IndexError> {
        let status = ReleaseStatus::parse(&cols.status).ok_or_else(|| {
            IndexError::Corrupt(format!("field `status` holds `{}`", cols.status))
        })?;
        let includes = {
            let mut stmt = self
                .conn
                .prepare("SELECT issue_id FROM release_includes WHERE version = ?1 ORDER BY pos")?;
            let ids = stmt
                .query_map(params![cols.version], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.iter()
                .map(|id| {
                    IssueId::parse(id)
                        .map_err(|e| IndexError::Corrupt(format!("field `includes`: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(IndexedRelease {
            release: Release {
                version: cols.version,
                status,
                target_ref: cols.target_ref,
                repo: cols.repo,
                target: cols.target,
                includes,
            },
            path: cols.path,
        })
    }

    /// The most recent comments across every issue, newest first by their
    /// `created` stamp (comment order is a wall-clock question by definition —
    /// comments are append-only files with no `seq`). The issue is joined in
    /// so a feed line renders without a lookup per row; a comment whose issue
    /// is not indexed still appears, with an empty title.
    pub fn recent_comments(&self, limit: usize) -> Result<Vec<WorkspaceComment>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.issue_id, c.author, c.at, c.reply_to, c.body, i.number, i.title \
             FROM comments c LEFT JOIN issues i ON i.id = c.issue_id \
             ORDER BY c.at DESC, c.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, issue_id, author, at, reply_to, body, number, title) = row?;
            out.push(WorkspaceComment {
                comment: Comment {
                    id: IssueId::parse(&id)
                        .map_err(|e| IndexError::Corrupt(format!("comment id `{id}`: {e}")))?,
                    author,
                    created: at,
                    reply_to: reply_to
                        .map(|r| {
                            IssueId::parse(&r).map_err(|e| {
                                IndexError::Corrupt(format!("comment `{id}` reply_to `{r}`: {e}"))
                            })
                        })
                        .transpose()?,
                    body,
                },
                issue_id: IssueId::parse(&issue_id)
                    .map_err(|e| IndexError::Corrupt(format!("issue id `{issue_id}`: {e}")))?,
                number: number.map(|n| n as u32),
                title: title.unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// One page of the workspace's field history, newest first. `before_seq`
    /// is the cursor: pass the `seq` of the last row you received to get the
    /// next page. Paging by `seq` rather than by offset means a backfill
    /// landing mid-scroll cannot make rows repeat or vanish.
    pub fn activity(
        &self,
        before_seq: Option<i64>,
        limit: usize,
    ) -> Result<Vec<StoredFieldEvent>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, issue_id, field, old_value, new_value, author, commit_sha, parent_sha, \
                    ts, source \
             FROM field_events \
             WHERE (?1 IS NULL OR seq < ?1) \
             ORDER BY seq DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![before_seq, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, issue_id, field, old, new, author, commit, parent, ts, source_raw) = row?;
            let source = dit_model::EventSource::parse(&source_raw).ok_or_else(|| {
                IndexError::Corrupt(format!("unknown event source `{source_raw}`"))
            })?;
            out.push(StoredFieldEvent {
                seq,
                issue_id,
                field,
                old_value: old,
                new_value: new,
                author,
                commit_sha: commit,
                parent_sha: parent,
                ts,
                source,
            });
        }
        Ok(out)
    }

    /// Events per calendar day from `since_day` (a `YYYY-MM-DD`) onward,
    /// oldest first. This is the one place a timestamp is allowed to decide
    /// anything, and only because "which day did this land on" is a question
    /// about wall clocks by definition.
    pub fn activity_days(&self, since_day: &str) -> Result<Vec<DayCount>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT substr(ts, 1, 10) AS day, COUNT(*) FROM field_events \
             WHERE substr(ts, 1, 10) >= ?1 GROUP BY day ORDER BY day",
        )?;
        let rows = stmt.query_map(params![since_day], |r| {
            Ok(DayCount {
                day: r.get::<_, String>(0)?,
                count: r.get::<_, i64>(1)? as usize,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The status of every issue that existed at `cutoff_seq`, as it stood
    /// then (DESIGN.md §14.3b). Three things make this correct, and all three
    /// are lessons from running it:
    ///
    ///  - The latest `seq` decides, never the latest timestamp: a merge
    ///    commit diffed per parent produces rows with identical timestamps,
    ///    and ordering by those returns two contradictory current values.
    ///  - `source = 'file'` only: mixing file and derived events into one
    ///    status timeline silently applies last-writer-wins, whereas
    ///    effective status is `resolve(file, derived)`.
    ///  - Issues not yet born simply have no events at or below the cutoff,
    ///    so they never appear.
    pub fn status_as_of(&self, cutoff_seq: i64) -> Result<Vec<(String, String)>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT e.issue_id, e.new_value FROM field_events e \
             WHERE e.field = 'status' AND e.source = 'file' AND e.seq <= ?1 \
               AND e.seq = (SELECT MAX(i.seq) FROM field_events i \
                            WHERE i.issue_id = e.issue_id AND i.field = 'status' \
                              AND i.source = 'file' AND i.seq <= ?1) \
             ORDER BY e.issue_id",
        )?;
        let rows = stmt.query_map(params![cutoff_seq], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, status) = row?;
            // A status set to nothing is a deleted issue, not a board entry.
            if let Some(status) = status {
                out.push((id, status));
            }
        }
        Ok(out)
    }

    /// What changed after `cutoff_seq`, counted. `terminal_statuses` names
    /// what this workspace calls done — the workflow is configurable, so the
    /// caller supplies it rather than this layer guessing.
    pub fn changes_since(
        &self,
        cutoff_seq: i64,
        terminal_statuses: &[String],
    ) -> Result<ChangeSummary, IndexError> {
        let touched: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT issue_id) FROM field_events WHERE seq > ?1",
            params![cutoff_seq],
            |r| r.get(0),
        )?;
        // Born since: every event this issue has is after the cutoff.
        let created: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM (SELECT issue_id FROM field_events \
             GROUP BY issue_id HAVING MIN(seq) > ?1)",
            params![cutoff_seq],
            |r| r.get(0),
        )?;
        let reprioritized: i64 = self.conn.query_row(
            "SELECT COUNT(DISTINCT issue_id) FROM field_events \
             WHERE field = 'priority' AND seq > ?1",
            params![cutoff_seq],
            |r| r.get(0),
        )?;
        let mut finished = 0i64;
        if !terminal_statuses.is_empty() {
            // One parameter per status: a workflow has a handful of them, and
            // building the list by hand keeps the values bound rather than
            // interpolated.
            let placeholders = (2..terminal_statuses.len() + 2)
                .map(|n| format!("?{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT COUNT(DISTINCT issue_id) FROM field_events \
                 WHERE field = 'status' AND seq > ?1 AND new_value IN ({placeholders})"
            );
            let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(cutoff_seq)];
            for status in terminal_statuses {
                values.push(Box::new(status.clone()));
            }
            let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
            finished = self.conn.query_row(&sql, refs.as_slice(), |r| r.get(0))?;
        }
        Ok(ChangeSummary {
            touched: touched as usize,
            created: created as usize,
            finished: finished as usize,
            reprioritized: reprioritized as usize,
        })
    }

    /// The last `seq` the index has recorded — "now" for time travel. Zero
    /// when nothing has been indexed yet.
    pub fn max_event_seq(&self) -> Result<i64, IndexError> {
        Ok(self
            .conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM field_events", [], |r| {
                r.get(0)
            })?)
    }

    /// Stored events for one issue (optionally one field), in event order.
    /// Ordered by `seq` — the position in the commit graph — because merge
    /// commits carry several identical timestamps and any timestamp ordering
    /// contradicts itself there.
    pub fn field_events(
        &self,
        issue_id: &IssueId,
        field: Option<&str>,
    ) -> Result<Vec<StoredFieldEvent>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, field, old_value, new_value, author, commit_sha, parent_sha, ts, source \
             FROM field_events WHERE issue_id = ?1 AND (?2 IS NULL OR field = ?2) ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![issue_id.as_str(), field], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seq, f, old, new, author, commit, parent, ts, source_raw) = row?;
            let source = dit_model::EventSource::parse(&source_raw).ok_or_else(|| {
                IndexError::Corrupt(format!("unknown event source `{source_raw}`"))
            })?;
            out.push(StoredFieldEvent {
                seq,
                issue_id: issue_id.as_str().to_owned(),
                field: f,
                old_value: old,
                new_value: new,
                author,
                commit_sha: commit,
                parent_sha: parent,
                ts,
                source,
            });
        }
        Ok(out)
    }

    /// Every indexed (id, path, blob) — what an incremental sync diffs
    /// against the current commit.
    pub fn all_blobs(&self) -> Result<Vec<(String, String, String)>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, blob_sha FROM issues ORDER BY path")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(IndexError::Db)
    }

    // -- watermarks ----------------------------------------------------------

    pub fn watermark(&self, key: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM state WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_watermark(&mut self, key: &str, value: &str) -> Result<(), IndexError> {
        self.conn.execute(
            "INSERT INTO state (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    /// True when the full-text index agrees with the issues table. External-
    /// content FTS drifts silently, so the check is worth its milliseconds.
    pub fn fts_integrity_ok(&self) -> bool {
        self.conn
            .execute_batch("INSERT INTO issues_fts(issues_fts) VALUES ('integrity-check');")
            .is_ok()
    }
}

/// Replace a side-table's rows for one issue with the given list. The file's
/// order is preserved through `pos`, because `assignees: [a, b]` and
/// `[b, a]` are different files even though they are the same set. Table and
/// column names come from this file, never from data.
fn replace_set(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    col: &str,
    id: &str,
    values: &[String],
) -> Result<(), IndexError> {
    tx.execute(
        &format!("DELETE FROM {table} WHERE issue_id = ?1"),
        params![id],
    )?;
    let mut stmt = tx.prepare(&format!(
        "INSERT OR IGNORE INTO {table} (issue_id, pos, {col}) VALUES (?1, ?2, ?3)"
    ))?;
    for (pos, v) in values.iter().enumerate() {
        stmt.execute(params![id, pos as i64, v])?;
    }
    Ok(())
}

/// Adapt the query compiler's parameter values to rusqlite's binder. Owned
/// Vec because `params_from_iter` borrows the iterator it is given.
fn bindable(params: &[SqlVal]) -> Vec<rusqlite::types::Value> {
    params
        .iter()
        .map(|p| match p {
            SqlVal::Text(s) => rusqlite::types::Value::Text(s.clone()),
            SqlVal::Real(f) => rusqlite::types::Value::Real(*f),
            SqlVal::Int(i) => rusqlite::types::Value::Integer(*i),
        })
        .collect()
}

/// The `releases` row, before the includes are joined in.
struct ReleaseCols {
    version: String,
    path: String,
    status: String,
    target_ref: Option<String>,
    repo: Option<String>,
    target: Option<String>,
}

fn release_columns(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReleaseCols> {
    Ok(ReleaseCols {
        version: r.get(0)?,
        path: r.get(1)?,
        status: r.get(2)?,
        target_ref: r.get(3)?,
        repo: r.get(4)?,
        target: r.get(5)?,
    })
}

/// Column bundle shared by the two read shapes.
struct IssueCols {
    path: String,
    blob_sha: String,
    title: String,
    kind: String,
    status: String,
    priority: Option<String>,
    reporter: Option<String>,
    epic: Option<String>,
    estimate: Option<i64>,
    sprint: Option<String>,
    due: Option<String>,
    start: Option<String>,
    lane: Option<String>,
    claimed_by: Option<String>,
    claimed_at: Option<String>,
    created: String,
    updated: String,
    body: String,
    /// Selected last in every read shape. `None` = unassigned (ADR 0007).
    number: Option<u32>,
}

/// Row mapper when `path` is the first selected column.
fn issue_columns(r: &rusqlite::Row<'_>) -> rusqlite::Result<IssueCols> {
    Ok(IssueCols {
        path: r.get(0)?,
        blob_sha: r.get(1)?,
        title: r.get(2)?,
        kind: r.get(3)?,
        status: r.get(4)?,
        priority: r.get(5)?,
        reporter: r.get(6)?,
        epic: r.get(7)?,
        estimate: r.get(8)?,
        sprint: r.get(9)?,
        due: r.get(10)?,
        start: r.get(11)?,
        lane: r.get(12)?,
        claimed_by: r.get(13)?,
        claimed_at: r.get(14)?,
        created: r.get(15)?,
        updated: r.get(16)?,
        body: r.get(17)?,
        number: r.get::<_, Option<i64>>(18)?.map(|n| n as u32),
    })
}

/// Row mapper when an extra `id` column sits in front (the list shape).
fn issue_columns_from_offset(r: &rusqlite::Row<'_>) -> IssueCols {
    issue_columns_with_skip(r, 1)
}

fn issue_columns_with_skip(r: &rusqlite::Row<'_>, n: usize) -> IssueCols {
    let get = |i: usize| -> rusqlite::Result<String> { r.get(n + i) };
    let get_opt = |i: usize| -> rusqlite::Result<Option<String>> { r.get(n + i) };
    IssueCols {
        path: get(0).unwrap_or_default(),
        blob_sha: get(1).unwrap_or_default(),
        title: get(2).unwrap_or_default(),
        kind: get(3).unwrap_or_default(),
        status: get(4).unwrap_or_default(),
        priority: get_opt(5).unwrap_or(None),
        reporter: get_opt(6).unwrap_or(None),
        epic: get_opt(7).unwrap_or(None),
        estimate: r.get(n + 8).unwrap_or(None),
        sprint: get_opt(9).unwrap_or(None),
        due: get_opt(10).unwrap_or(None),
        start: get_opt(11).unwrap_or(None),
        lane: get_opt(12).unwrap_or(None),
        claimed_by: get_opt(13).unwrap_or(None),
        claimed_at: get_opt(14).unwrap_or(None),
        created: get(15).unwrap_or_default(),
        updated: get(16).unwrap_or_default(),
        body: get(17).unwrap_or_default(),
        number: r
            .get::<_, Option<i64>>(n + 18)
            .unwrap_or(None)
            .map(|n| n as u32),
    }
}

/// Rebuild the typed aggregate from stored text. Every parse failure means
/// the index and the files disagree — which is a rebuild situation, not a
/// situation to paper over with a default.
fn hydrate(
    id: &IssueId,
    cols: IssueCols,
    assignees: Vec<String>,
    labels: Vec<String>,
    blocked_by: Vec<String>,
    fed_by: Vec<String>,
    flows: Vec<String>,
) -> Result<IndexedIssue, IndexError> {
    let corrupt =
        |field: &str, raw: &str| IndexError::Corrupt(format!("field `{field}` holds `{raw}`"));
    let blocked_by = blocked_by
        .iter()
        .map(|b| {
            IssueId::parse(b).map_err(|e| IndexError::Corrupt(format!("field `blocked_by`: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fed_by = fed_by
        .iter()
        .map(|b| IssueId::parse(b).map_err(|e| IndexError::Corrupt(format!("field `fed_by`: {e}"))))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(IndexedIssue {
        issue: Issue {
            id: *id,
            number: cols.number,
            title: cols.title,
            kind: dit_model::IssueKind::parse(&cols.kind)
                .ok_or_else(|| corrupt("type", &cols.kind))?,
            status: cols.status,
            priority: match cols.priority.as_deref() {
                None => None,
                Some(p) => {
                    Some(dit_model::Priority::parse(p).ok_or_else(|| corrupt("priority", p))?)
                }
            },
            reporter: cols.reporter,
            assignees,
            labels,
            epic: match cols.epic.as_deref() {
                None => None,
                Some(e) => Some(
                    IssueId::parse(e)
                        .map_err(|e| IndexError::Corrupt(format!("field `epic`: {e}")))?,
                ),
            },
            estimate: cols.estimate.map(|e| e as u32),
            sprint: cols.sprint,
            created: cols.created,
            updated: cols.updated,
            due: cols.due,
            start: cols.start,
            blocked_by,
            fed_by,
            lane: cols.lane,
            flows,
            claimed_by: cols.claimed_by,
            claimed_at: cols.claimed_at,
            body: cols.body,
        },
        path: cols.path,
        blob_sha: cols.blob_sha,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use dit_model::{EventSource, IssueKind, Priority, Release, ReleaseStatus};

    fn sample_issue(id: &str, status: &str) -> Issue {
        Issue {
            id: IssueId::parse(id).unwrap(),
            number: None,
            title: "Login timeout".into(),
            kind: IssueKind::Bug,
            status: status.into(),
            priority: Some(Priority::P1),
            reporter: Some("farid".into()),
            assignees: vec!["farid".into(), "budi".into()],
            labels: vec!["auth".into()],
            epic: None,
            estimate: Some(3),
            sprint: None,
            created: "2026-08-16T09:12:00Z".into(),
            updated: "2026-08-16T11:40:00Z".into(),
            due: None,
            start: None,
            blocked_by: vec![],
            fed_by: vec![],
            lane: None,
            flows: Vec::new(),
            claimed_by: None,
            claimed_at: None,
            body: "Users on 3G get logged out.".into(),
        }
    }

    fn dql(text: &str) -> Query {
        dit_query::parse(text).unwrap()
    }

    #[test]
    fn every_selected_issue_column_exists_in_the_schema() {
        // ISSUE_SELECT is the one shared column list; this pins it against the
        // CREATE TABLE so adding a column without updating the list (or vice
        // versa) fails here instead of as a runtime SQL error.
        let table = SCHEMA
            .split("CREATE TABLE IF NOT EXISTS issues (")
            .nth(1)
            .and_then(|rest| rest.split(");").next())
            .expect("the issues CREATE TABLE is present");
        for col in ISSUE_SELECT.split(", ") {
            assert!(
                table.contains(&format!(" {col} ")),
                "ISSUE_SELECT names `{col}` but the issues table does not define it"
            );
        }
    }

    #[test]
    fn lane_and_claims_round_trip_through_the_index() {
        let mut idx = Index::in_memory().unwrap();
        let mut issue = sample_issue(ID, "todo");
        issue.lane = Some("frontend".into());
        issue.claimed_by = Some("fe-1".into());
        issue.claimed_at = Some("2026-08-16T11:38:00Z".into());
        idx.upsert_issue(&issue, "p", "s").unwrap();

        let got = idx.get_issue(&issue.id).unwrap().unwrap();
        assert_eq!(got.issue.lane.as_deref(), Some("frontend"));
        assert_eq!(got.issue.claimed_by.as_deref(), Some("fe-1"));
        assert_eq!(
            got.issue.claimed_at.as_deref(),
            Some("2026-08-16T11:38:00Z")
        );

        // DQL filters on the lane column end to end.
        let hits = idx
            .search(&dql("lane = frontend"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(hits.len(), 1);
        let none = idx
            .search(&dql("lane = backend"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn comment_threads_round_trip_through_the_index() {
        let mut idx = Index::in_memory().unwrap();
        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        let parent_id = IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        idx.upsert_comment(
            &issue.id,
            &Comment {
                id: parent_id,
                author: "fe-1".into(),
                created: "2026-08-16T10:00:00Z".into(),
                reply_to: None,
                body: "expected JSON, got HTML".into(),
            },
        )
        .unwrap();
        idx.upsert_comment(
            &issue.id,
            &Comment {
                id: IssueId::parse("01K3MA9ZC2HJ5M8PQRTVWXYZK1").unwrap(),
                author: "be-1".into(),
                created: "2026-08-16T10:20:00Z".into(),
                reply_to: Some(parent_id),
                body: "payload hits the 500 path".into(),
            },
        )
        .unwrap();

        let comments = idx.comments_for(&issue.id).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].reply_to, None);
        assert_eq!(comments[1].reply_to, Some(parent_id));

        let feed = idx.recent_comments(10).unwrap();
        assert_eq!(feed.len(), 2);
        assert_eq!(feed[0].comment.reply_to, Some(parent_id), "newest first");
    }

    fn event(
        field: &str,
        old: Option<&str>,
        new: Option<&str>,
        commit: &str,
        parent: &str,
    ) -> FieldEvent {
        FieldEvent {
            issue_id: "01K3M9ZXQ2R7VN8P4TDBCEFGHJ".into(),
            field: field.into(),
            old_value: old.map(str::to_owned),
            new_value: new.map(str::to_owned),
            author: "farid".into(),
            commit_sha: commit.into(),
            parent_sha: parent.into(),
            ts: "2026-08-16T10:00:00Z".into(),
            source: EventSource::File,
        }
    }

    const ID: &str = "01K3M9ZXQ2R7VN8P4TDBCEFGHJ";
    const OTHER: &str = "01K3M9ZXQ2ZZZZZZZZZZZZZZZZ";

    #[test]
    fn a_spec_catalogue_is_replaced_whole_rather_than_merged() {
        let mut index = Index::in_memory().unwrap();
        let spec = |problem: Option<&str>| StoredMorseSpec {
            spec_id: "auth".into(),
            repo: Some("backend".into()),
            path: "services/auth/openapi.yaml".into(),
            head: Some("a3f9c2d".into()),
            title: Some("Acme Auth".into()),
            version: Some("1.4.0".into()),
            problem: problem.map(str::to_owned),
        };
        let op = |id: &str, path: &str| dit_model::SpecOperation {
            operation_id: id.into(),
            method: "POST".into(),
            path: path.into(),
            summary: None,
        };
        index
            .replace_morse_spec(
                &spec(None),
                &[op("createUser", "/users"), op("loginUser", "/s")],
            )
            .unwrap();
        assert_eq!(index.morse_operations("auth").unwrap().len(), 2);

        // The document dropped an operation. The catalogue must drop it too:
        // it is derived, so a leftover row would be the index asserting
        // something the source of truth no longer says.
        index
            .replace_morse_spec(&spec(None), &[op("createUser", "/users")])
            .unwrap();
        let ops = index.morse_operations("auth").unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation_id, "createUser");

        let stored = index.morse_specs().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].title.as_deref(), Some("Acme Auth"));
        assert_eq!(stored[0].repo.as_deref(), Some("backend"));

        index.clear_morse_specs().unwrap();
        assert!(index.morse_specs().unwrap().is_empty());
        assert!(index.morse_operations("auth").unwrap().is_empty());
    }

    #[test]
    fn the_first_document_to_name_a_scenario_keeps_it() {
        let mut index = Index::in_memory().unwrap();
        let at = |path: &str, line: usize, problem: Option<&str>| StoredMorseScenario {
            scenario: "register".into(),
            path: path.into(),
            line,
            spec_id: "auth".into(),
            pin: "a3f9c2d".into(),
            env: Some("local".into()),
            body: "scenario: register".into(),
            problem: problem.map(str::to_owned),
            stale_by: Some(0),
            broken: None,
        };
        assert!(index
            .upsert_morse_scenario(&at("docs/a.md", 3, None))
            .unwrap());
        assert!(
            !index
                .upsert_morse_scenario(&at("docs/b.md", 1, None))
                .unwrap(),
            "a second document naming the same scenario is a warning, not a merge"
        );
        let stored = index.morse_scenarios().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].path, "docs/a.md");

        // The owning document may move its own fence.
        assert!(index
            .upsert_morse_scenario(&at("docs/a.md", 9, None))
            .unwrap());
        assert_eq!(index.morse_scenarios().unwrap()[0].line, 9);

        // A fence that did not parse is still recorded, so the screen can
        // say which document and line to fix rather than showing nothing.
        assert!(index
            .upsert_morse_scenario(&at("docs/a.md", 9, Some("line 4: bad")))
            .unwrap());
        assert_eq!(
            index.morse_scenarios().unwrap()[0].problem.as_deref(),
            Some("line 4: bad")
        );

        index.clear_morse_scenarios_at("docs/a.md").unwrap();
        assert!(index.morse_scenarios().unwrap().is_empty());
    }

    #[test]
    fn the_first_fence_for_a_flow_wins_and_a_second_document_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut index = Index::open(&tmp.path().join("index.sqlite")).unwrap();
        assert!(index
            .upsert_flow_shape("register", "docs/a.md", 3, "flow: register", None)
            .unwrap());
        // A second document claiming the same flow is refused rather than
        // merged: two shapes for one diagram have no defined resolution.
        assert!(!index
            .upsert_flow_shape("register", "docs/b.md", 1, "flow: register", None)
            .unwrap());
        let stored = index.flow_shape("register").unwrap().unwrap();
        assert_eq!(stored.path, "docs/a.md");
        assert_eq!(stored.line, 3);
        assert_eq!(stored.problem, None);

        // The same document may change its mind as often as it likes.
        assert!(index
            .upsert_flow_shape(
                "register",
                "docs/a.md",
                9,
                "flow: register\nphases: []",
                None
            )
            .unwrap());
        assert_eq!(index.flow_shape("register").unwrap().unwrap().line, 9);

        // A fence that did not parse is stored with its reason, so the
        // screen can name the file and line instead of drawing nothing.
        index
            .upsert_flow_shape(
                "broken",
                "docs/c.md",
                2,
                "flow: broken",
                Some("line 4: bad"),
            )
            .unwrap();
        assert_eq!(
            index
                .flow_shape("broken")
                .unwrap()
                .unwrap()
                .problem
                .as_deref(),
            Some("line 4: bad")
        );

        assert_eq!(index.flow_shape("never-written").unwrap(), None);
        index.clear_flow_shapes().unwrap();
        assert_eq!(index.flow_shape("register").unwrap(), None);
    }

    #[test]
    fn upsert_then_read_round_trips_every_column() {
        let mut idx = Index::in_memory().unwrap();
        let issue = sample_issue(ID, "in_progress");
        idx.upsert_issue(&issue, ".dit/issues/2026/08/xxx-issue/issue.md", "ab12")
            .unwrap();

        let got = idx.get_issue(&issue.id).unwrap().unwrap();
        assert_eq!(got.issue, issue);
        assert_eq!(got.issue.assignees, vec!["farid", "budi"]);
        assert_eq!(got.path, ".dit/issues/2026/08/xxx-issue/issue.md");
        assert_eq!(got.blob_sha, "ab12");
        assert_eq!(got.issue.id.short_ref().as_str(), "R7VN8P4");
    }

    #[test]
    fn blocked_by_round_trips_in_file_order() {
        let mut idx = Index::in_memory().unwrap();
        let mut issue = sample_issue(ID, "todo");
        let a = IssueId::parse(OTHER).unwrap();
        let b = IssueId::parse("01K3M9ZXQ2YYYYYYYYYYYYYYYY").unwrap();
        issue.blocked_by = vec![b, a];
        idx.upsert_issue(&issue, "p", "s1").unwrap();

        let got = idx.get_issue(&issue.id).unwrap().unwrap();
        assert_eq!(
            got.issue.blocked_by,
            vec![b, a],
            "order is the file's order"
        );
        assert_eq!(got.issue, issue);

        // The list read shape sees the same blockers.
        let listed = idx
            .search(&dql(""), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(listed[0].issue.blocked_by, vec![b, a]);

        // Clearing the list clears the side table too.
        issue.blocked_by = vec![];
        idx.upsert_issue(&issue, "p", "s2").unwrap();
        let got = idx.get_issue(&issue.id).unwrap().unwrap();
        assert!(got.issue.blocked_by.is_empty());
    }

    #[test]
    fn recent_comments_are_newest_first_and_carry_their_issue() {
        let mut idx = Index::in_memory().unwrap();
        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        let comment = |id: &str, at: &str| Comment {
            id: IssueId::parse(id).unwrap(),
            author: "budi".into(),
            created: at.into(),
            reply_to: None,
            body: "note".into(),
        };
        idx.upsert_comment(
            &issue.id,
            &comment("01K3MA1F7XQW8N2V5RTGBCDEF0", "2026-08-16T10:00:00Z"),
        )
        .unwrap();
        idx.upsert_comment(
            &issue.id,
            &comment("01K3MA1F7XQW8N2V5RTGBCDEF1", "2026-08-17T10:00:00Z"),
        )
        .unwrap();
        // A comment whose issue was never indexed still shows up — with the
        // handle derived from its id and no title, like the activity feed.
        let orphan_issue = IssueId::parse(OTHER).unwrap();
        idx.upsert_comment(
            &orphan_issue,
            &comment("01K3MA1F7XQW8N2V5RTGBCDEF2", "2026-08-18T10:00:00Z"),
        )
        .unwrap();

        let feed = idx.recent_comments(10).unwrap();
        assert_eq!(feed.len(), 3);
        assert_eq!(feed[0].comment.created, "2026-08-18T10:00:00Z");
        assert_eq!(feed[0].issue_id, orphan_issue);
        assert_eq!(feed[0].title, "");
        assert_eq!(feed[0].number, None);
        assert_eq!(feed[1].comment.created, "2026-08-17T10:00:00Z");
        assert_eq!(feed[1].issue_id, issue.id);
        assert_eq!(feed[1].title, "Login timeout");
        assert_eq!(feed[2].comment.id.as_str(), "01K3MA1F7XQW8N2V5RTGBCDEF0");

        // The limit is honoured.
        assert_eq!(idx.recent_comments(1).unwrap().len(), 1);
    }

    fn sample_release(version: &str, target: Option<&str>) -> Release {
        Release {
            version: version.into(),
            status: ReleaseStatus::Planned,
            target_ref: Some(format!("release/{version}")),
            repo: Some("api".into()),
            target: target.map(str::to_owned),
            includes: vec![],
        }
    }

    #[test]
    fn releases_round_trip_and_order_by_target_then_version() {
        let mut idx = Index::in_memory().unwrap();
        // Nothing indexed: an empty list, not an error.
        assert!(idx.releases().unwrap().is_empty());
        assert!(idx.release("v0.2.0").unwrap().is_none());

        let mut dated = sample_release("v0.2.0", Some("2026-10-01"));
        let a = IssueId::parse(ID).unwrap();
        let b = IssueId::parse(OTHER).unwrap();
        dated.includes = vec![b, a];
        idx.upsert_release(&dated, ".dit/releases/v0.2.0/release.md")
            .unwrap();
        idx.upsert_release(
            &sample_release("v0.1.0", Some("2026-09-01")),
            ".dit/releases/v0.1.0/release.md",
        )
        .unwrap();
        idx.upsert_release(
            &sample_release("v0.9.0", None),
            ".dit/releases/v0.9.0/release.md",
        )
        .unwrap();
        idx.upsert_release(
            &sample_release("v0.3.0", None),
            ".dit/releases/v0.3.0/release.md",
        )
        .unwrap();

        let got = idx.release("v0.2.0").unwrap().unwrap();
        assert_eq!(got.release, dated, "includes keep the file's order");
        assert_eq!(got.path, ".dit/releases/v0.2.0/release.md");

        // Dated releases first, soonest first; undated ones after, by version.
        let versions: Vec<String> = idx
            .releases()
            .unwrap()
            .into_iter()
            .map(|r| r.release.version)
            .collect();
        assert_eq!(versions, ["v0.1.0", "v0.2.0", "v0.3.0", "v0.9.0"]);

        // Re-upserting replaces the includes rather than appending.
        dated.includes = vec![a];
        dated.status = ReleaseStatus::Released;
        idx.upsert_release(&dated, ".dit/releases/v0.2.0/release.md")
            .unwrap();
        let got = idx.release("v0.2.0").unwrap().unwrap();
        assert_eq!(got.release.includes, vec![a]);
        assert_eq!(got.release.status, ReleaseStatus::Released);

        idx.remove_release("v0.2.0").unwrap();
        assert!(idx.release("v0.2.0").unwrap().is_none());
        assert_eq!(idx.releases().unwrap().len(), 3);

        // A state wipe clears releases with the issues.
        idx.wipe_state().unwrap();
        assert!(idx.releases().unwrap().is_empty());
    }

    #[test]
    fn upsert_replaces_sets_and_keeps_fts_in_sync() {
        let mut idx = Index::in_memory().unwrap();
        let id = IssueId::parse(ID).unwrap();
        let mut issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s1").unwrap();

        // Retitle to words absent from the old title, clear labels, change
        // assignees.
        issue.title = "Session drops on slow networks".into();
        issue.labels = vec![];
        issue.assignees = vec!["budi".into()];
        idx.upsert_issue(&issue, "p", "s2").unwrap();

        let got = idx.get_issue(&id).unwrap().unwrap();
        assert_eq!(got.issue.title, "Session drops on slow networks");
        assert!(got.issue.labels.is_empty());
        assert_eq!(got.issue.assignees, vec!["budi"]);

        // FTS must reflect the newest title only — external-content FTS
        // drifts silently if the delete half of the update is skipped.
        assert!(idx.fts_integrity_ok());
        let hits = idx
            .search(&dql("title ~ session"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(hits.len(), 1);
        let stale = idx
            .search(&dql("title ~ login"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(stale.len(), 0, "old title text must leave the FTS index");
    }

    #[test]
    fn remove_issue_drops_the_row_and_its_sets() {
        let mut idx = Index::in_memory().unwrap();
        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        idx.remove_issue(&issue.id).unwrap();
        assert!(idx.get_issue(&issue.id).unwrap().is_none());
        assert!(idx
            .list_issues(&Compiled {
                where_sql: "1=1".into(),
                params: vec![],
                order_sql: "".into(),
                limit: None,
            })
            .unwrap()
            .is_empty());
    }

    #[test]
    fn dql_filters_and_orders_through_bound_parameters() {
        let mut idx = Index::in_memory().unwrap();
        for (id, status, est) in [
            (ID, "todo", 3u32),
            (OTHER, "done", 8),
            (ID, "in_progress", 1),
        ] {
            if id == ID && status == "in_progress" {
                // Third distinct id: reuse the shape with a different tail.
            }
            let _ = (id, status, est);
        }
        let cases = [
            ("01K3M9ZXQ2R7VN8P4TDBCEFGHJ", "todo", 3u32),
            ("01K3M9ZXQ2ZZZZZZZZZZZZZZZZ", "done", 8),
            ("01K3M9ZXQ2R7VN8P4TDBCEFFFF", "in_progress", 1),
        ];
        for (id, status, est) in cases {
            let mut issue = sample_issue(id, status);
            issue.estimate = Some(est);
            idx.upsert_issue(&issue, "p", "s").unwrap();
        }

        let hits = idx
            .search(
                &dql("status != done AND estimate >= 1 ORDER BY estimate DESC"),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();
        let ests: Vec<u32> = hits.iter().filter_map(|h| h.issue.estimate).collect();
        assert_eq!(ests, vec![3, 1], "ordered by estimate DESC, done excluded");
    }

    #[test]
    fn assignee_and_label_queries_go_through_the_side_tables() {
        let mut idx = Index::in_memory().unwrap();
        let mut a = sample_issue(ID, "todo");
        a.assignees = vec!["budi".into()];
        a.labels = vec!["auth".into()];
        let mut b = sample_issue(OTHER, "todo");
        b.assignees = vec!["farid".into()];
        b.labels = vec!["api".into()];
        idx.upsert_issue(&a, "a", "s").unwrap();
        idx.upsert_issue(&b, "b", "s").unwrap();

        let hits = idx
            .search(
                &dql("assignee = budi OR label = api"),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();
        assert_eq!(hits.len(), 2, "each issue matches its own condition");

        let hits = idx
            .search(&dql("label IN (auth)"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].issue.id.as_str(), ID);
    }

    #[test]
    fn full_text_search_finds_body_text_and_phrases_stay_safe() {
        let mut idx = Index::in_memory().unwrap();
        let mut a = sample_issue(ID, "todo");
        a.body = "The session cookie expires too early on 3G.".into();
        let mut b = sample_issue(OTHER, "todo");
        b.title = "Printer jams".into();
        b.body = "Hardware.".into();
        idx.upsert_issue(&a, "a", "s").unwrap();
        idx.upsert_issue(&b, "b", "s").unwrap();

        let hits = idx
            .search(&dql("body ~ session"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].issue.id.as_str(), ID);

        // FTS operators in user text must be treated as literal words.
        let hits = idx
            .search(
                &dql("body ~ \"session OR NEAR(a b)\""),
                None,
                OffsetDateTime::UNIX_EPOCH,
            )
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn comments_round_trip_and_sort_oldest_first() {
        let mut idx = Index::in_memory().unwrap();
        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        let early = Comment {
            id: IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap(),
            author: "farid".into(),
            created: "2026-08-16T10:00:00Z".into(),
            reply_to: None,
            body: "reproduced".into(),
        };
        let late = Comment {
            id: IssueId::parse("01K3MA1F7XZZZZZZZZZZZZZZZZ").unwrap(),
            author: "budi".into(),
            created: "2026-08-16T11:00:00Z".into(),
            reply_to: None,
            body: "same here".into(),
        };
        // Insert late first: read order must not follow insert order.
        idx.upsert_comment(&issue.id, &late).unwrap();
        idx.upsert_comment(&issue.id, &early).unwrap();

        let comments = idx.comments_for(&issue.id).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].body, "reproduced");
        assert_eq!(comments[1].body, "same here");

        idx.remove_comment(&late.id).unwrap();
        assert_eq!(idx.comments_for(&issue.id).unwrap().len(), 1);
    }

    #[test]
    fn activity_pages_newest_first_and_hands_back_a_cursor() {
        let mut idx = Index::in_memory().unwrap();
        for n in 1..=5 {
            idx.record_field_events(&[event(
                "status",
                Some("todo"),
                Some("in_progress"),
                &format!("c{n}"),
                &format!("c{}", n - 1),
            )])
            .unwrap();
        }

        let first = idx.activity(None, 2).unwrap();
        assert_eq!(first.len(), 2);
        // Newest first: a feed reads downward from now.
        assert!(first[0].seq > first[1].seq);

        let cursor = first[1].seq;
        let second = idx.activity(Some(cursor), 2).unwrap();
        assert_eq!(second.len(), 2);
        assert!(second[0].seq < cursor);

        // The whole history is reachable by walking the cursor.
        let all = idx.activity(None, 100).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn the_histogram_counts_events_per_day() {
        let mut idx = Index::in_memory().unwrap();
        let mut at = |ts: &str, commit: &str| {
            let mut e = event("status", Some("todo"), Some("done"), commit, "");
            e.ts = ts.into();
            idx.record_field_events(&[e]).unwrap();
        };
        at("2026-09-01T09:00:00Z", "c1");
        at("2026-09-01T23:59:59Z", "c2");
        at("2026-09-03T10:00:00Z", "c3");

        let days = idx.activity_days("2026-09-01").unwrap();
        assert_eq!(
            days,
            vec![
                DayCount {
                    day: "2026-09-01".into(),
                    count: 2
                },
                DayCount {
                    day: "2026-09-03".into(),
                    count: 1
                },
            ]
        );

        // The window is a floor, not a filter on everything.
        assert_eq!(idx.activity_days("2026-09-02").unwrap().len(), 1);
    }

    #[test]
    fn status_as_of_reads_the_board_at_a_point_in_history() {
        let mut idx = Index::in_memory().unwrap();
        let one = "01K3M9ZXQ2R7VN8P4TDBCEFGHJ";
        let two = "01K3M5QQQQ0000000000ZZZZZZ";
        let mut push = |issue: &str, old: Option<&str>, new: &str, commit: &str| {
            let mut e = event("status", old, Some(new), commit, "");
            e.issue_id = issue.into();
            idx.record_field_events(&[e]).unwrap();
        };
        push(one, None, "todo", "c1");
        push(two, None, "todo", "c2");
        push(one, Some("todo"), "in_progress", "c3");
        push(one, Some("in_progress"), "done", "c4");

        // After the second commit only two issues exist, both todo.
        let early = idx.status_as_of(2).unwrap();
        assert_eq!(early.len(), 2);
        assert_eq!(early.iter().filter(|(_, s)| s == "todo").count(), 2);

        // After the third, one has moved on.
        let mid: Vec<_> = idx.status_as_of(3).unwrap();
        assert_eq!(
            mid.iter()
                .find(|(id, _)| id == one)
                .map(|(_, s)| s.as_str()),
            Some("in_progress")
        );

        // Before anything happened, the board is empty — issues that were
        // not yet born must not appear.
        assert!(idx.status_as_of(0).unwrap().is_empty());
    }

    #[test]
    fn status_as_of_ignores_derived_events() {
        // §14.3: mixing `file` and `derived` into one status timeline applies
        // last-writer-wins, which is not how effective status is resolved.
        let mut idx = Index::in_memory().unwrap();
        let mut file = event("status", None, Some("todo"), "c1", "");
        file.source = EventSource::File;
        let mut derived = event("status", Some("todo"), Some("done"), "c2", "");
        derived.source = EventSource::Derived;
        idx.record_field_events(&[file, derived]).unwrap();

        let board = idx.status_as_of(99).unwrap();
        assert_eq!(board.len(), 1);
        assert_eq!(board[0].1, "todo");
    }

    #[test]
    fn changes_since_counts_what_a_person_would_call_progress() {
        let mut idx = Index::in_memory().unwrap();
        let old_issue = "01K3M9ZXQ2R7VN8P4TDBCEFGHJ";
        let new_issue = "01K3M5QQQQ0000000000ZZZZZZ";
        let mut push = |issue: &str, field: &str, old: Option<&str>, new: &str, commit: &str| {
            let mut e = event(field, old, Some(new), commit, "");
            e.issue_id = issue.into();
            idx.record_field_events(&[e]).unwrap();
        };
        push(old_issue, "status", None, "todo", "c1"); // seq 1 — before the cutoff
        push(old_issue, "status", Some("todo"), "done", "c2"); // seq 2
        push(old_issue, "priority", Some("p2"), "p1", "c3"); // seq 3
        push(new_issue, "status", None, "todo", "c4"); // seq 4 — born after

        let since = idx.changes_since(1, &["done".to_string()]).unwrap();
        assert_eq!(since.touched, 2);
        assert_eq!(
            since.created, 1,
            "only the issue whose first event is later"
        );
        assert_eq!(since.finished, 1);
        assert_eq!(since.reprioritized, 1);

        // From the very beginning, everything counts as new.
        let all = idx.changes_since(0, &["done".to_string()]).unwrap();
        assert_eq!(all.created, 2);
    }

    #[test]
    fn max_event_seq_reports_the_end_of_history() {
        let mut idx = Index::in_memory().unwrap();
        assert_eq!(idx.max_event_seq().unwrap(), 0);
        idx.record_field_events(&[event("status", None, Some("todo"), "c1", "")])
            .unwrap();
        assert_eq!(idx.max_event_seq().unwrap(), 1);
    }

    #[test]
    fn field_events_are_ordered_by_seq_and_survive_a_rerun() {
        let mut idx = Index::in_memory().unwrap();
        let id = IssueId::parse(ID).unwrap();
        let first = event("status", Some("todo"), Some("in_progress"), "c2", "c1");
        let second = event("status", Some("in_progress"), Some("done"), "c3", "c2");

        let inserted = idx.record_field_events(&[first.clone(), second]).unwrap();
        assert_eq!(inserted, 2);

        // The same backfill again: nothing new, no duplicates, same seqs.
        assert_eq!(idx.record_field_events(&[first]).unwrap(), 0);
        let events = idx.field_events(&id, None).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].new_value.as_deref(), Some("in_progress"));
        assert_eq!(events[1].new_value.as_deref(), Some("done"));
        assert!(events[0].seq < events[1].seq);

        // New commits continue after the existing ones.
        idx.record_field_events(&[event("priority", Some("p1"), Some("p0"), "c4", "c3")])
            .unwrap();
        let events = idx.field_events(&id, None).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events[2].seq > events[1].seq);
        assert_eq!(idx.field_events(&id, Some("status")).unwrap().len(), 2);
    }

    #[test]
    fn an_empty_parent_sha_cannot_duplicate_on_a_rerun() {
        // The stored default is '' precisely because NULL parents are
        // considered distinct by SQLite's uniqueness check and would double
        // the history on every backfill re-run.
        let mut idx = Index::in_memory().unwrap();
        let e = FieldEvent {
            parent_sha: String::new(),
            ..event("status", Some("todo"), Some("done"), "c9", "ignored")
        };
        idx.record_field_events(std::slice::from_ref(&e)).unwrap();
        assert_eq!(idx.record_field_events(&[e]).unwrap(), 0);
    }

    #[test]
    fn watermarks_persist_and_wipe_clears_everything() {
        let mut idx = Index::in_memory().unwrap();
        idx.set_watermark("history_head", "abc123").unwrap();
        assert_eq!(
            idx.watermark("history_head").unwrap().as_deref(),
            Some("abc123")
        );
        assert_eq!(idx.watermark("missing").unwrap(), None);

        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        idx.record_field_events(&[event("status", None, Some("todo"), "c1", "")])
            .unwrap();
        idx.wipe().unwrap();
        assert_eq!(idx.watermark("history_head").unwrap(), None);
        assert!(idx.get_issue(&issue.id).unwrap().is_none());
        assert_eq!(idx.field_events(&issue.id, None).unwrap().len(), 0);
        assert!(idx.fts_integrity_ok());
    }

    #[test]
    fn open_creates_parent_directories_and_a_real_file() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("nested/deeper/index.sqlite");
        let mut idx = Index::open(&db).unwrap();
        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        assert!(db.exists());
        // Reopening sees the same data — it is a real file, not a memory toy.
        let reopened = Index::open(&db).unwrap();
        assert!(reopened.get_issue(&issue.id).unwrap().is_some());
    }

    #[test]
    fn all_blobs_supports_incremental_sync() {
        let mut idx = Index::in_memory().unwrap();
        let a = sample_issue(ID, "todo");
        let b = sample_issue(OTHER, "todo");
        idx.upsert_issue(&a, "a.md", "sha-a").unwrap();
        idx.upsert_issue(&b, "b.md", "sha-b").unwrap();
        let blobs = idx.all_blobs().unwrap();
        assert_eq!(blobs.len(), 2);
        assert!(blobs.contains(&(OTHER.to_owned(), "b.md".into(), "sha-b".into())));
    }

    #[test]
    fn wipe_state_clears_issues_but_keeps_history_and_watermarks() {
        let mut idx = Index::in_memory().unwrap();
        let issue = sample_issue(ID, "todo");
        idx.upsert_issue(&issue, "p", "s").unwrap();
        idx.record_field_events(&[event("status", Some("todo"), Some("done"), "c1", "")])
            .unwrap();
        idx.set_watermark("events", "c1").unwrap();

        idx.wipe_state().unwrap();

        assert!(idx.get_issue(&issue.id).unwrap().is_none());
        // The expensive half of a rebuild survives a state-only refresh.
        assert_eq!(idx.field_events(&issue.id, None).unwrap().len(), 1);
        assert_eq!(idx.watermark("events").unwrap().as_deref(), Some("c1"));
    }

    #[test]
    fn number_round_trips_through_upsert_and_both_read_shapes() {
        let mut idx = Index::in_memory().unwrap();
        let mut numbered = sample_issue(ID, "todo");
        numbered.number = Some(12);
        idx.upsert_issue(&numbered, "p", "s").unwrap();

        assert_eq!(
            idx.get_issue(&numbered.id).unwrap().unwrap().issue.number,
            Some(12)
        );
        // The list shape reads `number` through the offset mapper.
        let all = idx
            .search(&dql(""), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert!(all.iter().any(|i| i.issue.number == Some(12)), "{all:?}");

        // And DQL can select on it (SQLite compares INTEGER to REAL 12.0 fine).
        let by_number = idx
            .search(&dql("number = 12"), None, OffsetDateTime::UNIX_EPOCH)
            .unwrap();
        assert_eq!(by_number.len(), 1);
        assert_eq!(by_number[0].issue.number, Some(12));

        // max_number drives the next `numbering: local` assignment.
        assert_eq!(idx.max_number().unwrap(), Some(12));
    }

    #[test]
    fn unnumbered_count_counts_exactly_the_backfillable_set() {
        let mut idx = Index::in_memory().unwrap();
        let mut numbered = sample_issue(ID, "todo");
        numbered.number = Some(4);
        idx.upsert_issue(&numbered, "p", "s").unwrap();
        idx.upsert_issue(&sample_issue(OTHER, "todo"), "q", "s")
            .unwrap();

        assert_eq!(idx.unnumbered_count().unwrap(), 1);

        let mut legacy = sample_issue(OTHER, "done");
        legacy.number = Some(5);
        idx.upsert_issue(&legacy, "q", "s").unwrap();
        assert_eq!(
            idx.unnumbered_count().unwrap(),
            0,
            "a backfilled issue leaves the set"
        );
    }

    #[test]
    fn max_number_is_none_when_nothing_is_numbered() {
        let mut idx = Index::in_memory().unwrap();
        idx.upsert_issue(&sample_issue(ID, "todo"), "p", "s")
            .unwrap();
        assert_eq!(idx.max_number().unwrap(), None);
    }

    #[test]
    fn duplicate_numbers_are_reported_with_their_holders() {
        let mut idx = Index::in_memory().unwrap();
        let mut a = sample_issue(ID, "todo");
        a.number = Some(12);
        let mut b = sample_issue(OTHER, "todo");
        b.number = Some(12);
        let mut c = sample_issue("01K3M9ZXQ2R7VN8P4TDBCEFGHK", "todo");
        c.number = Some(13);
        idx.upsert_issue(&a, "a", "sa").unwrap();
        idx.upsert_issue(&b, "b", "sb").unwrap();
        idx.upsert_issue(&c, "c", "sc").unwrap();

        let dupes = idx.duplicate_numbers().unwrap();
        assert_eq!(dupes.len(), 1, "{dupes:?}");
        assert_eq!(dupes[0].0, 12);
        assert_eq!(dupes[0].1.len(), 2);

        let holders = idx.issues_with_number(12).unwrap();
        assert_eq!(holders.len(), 2);
        assert!(holders.iter().all(|h| h.issue.number == Some(12)));
    }

    #[test]
    fn an_index_from_an_older_schema_is_dropped_and_rebuilt() {
        // Simulate a `.dit-cache` written before the `number` column: stamp it
        // with the old version and a table shape this binary no longer makes.
        let file = tempfile::NamedTempFile::new().unwrap();
        let old = rusqlite::Connection::open(file.path()).unwrap();
        old.execute_batch(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, path TEXT, blob_sha TEXT, short_ref TEXT, \
             title TEXT, type TEXT, status TEXT, priority TEXT, reporter TEXT, epic TEXT, \
             estimate INTEGER, sprint TEXT, due TEXT, created TEXT, updated TEXT, body TEXT);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(old);

        // Opening with this binary silently rebuilds the schema.
        let idx = Index::open(file.path()).unwrap();
        assert!(idx.max_number().is_ok());
    }

    #[test]
    fn an_unstamped_index_from_a_pre_versioning_binary_is_rebuilt_too() {
        // The pilot case: a `.dit-cache` written before `user_version`
        // existed at all — old tables present, version stamp 0. Opening it
        // must rebuild, not adopt the stale columns and stamp them current.
        let file = tempfile::NamedTempFile::new().unwrap();
        let old = rusqlite::Connection::open(file.path()).unwrap();
        old.execute_batch(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, path TEXT, blob_sha TEXT, short_ref TEXT, \
             title TEXT, type TEXT, status TEXT, priority TEXT, reporter TEXT, epic TEXT, \
             estimate INTEGER, sprint TEXT, due TEXT, created TEXT, updated TEXT, body TEXT);",
        )
        .unwrap();
        drop(old);

        // The failure this guards against is on the write path, not open:
        // adopting the old table makes every numbered upsert fail with
        // "table issues has no column named number".
        let mut idx = Index::open(file.path()).unwrap();
        let mut issue = sample_issue(ID, "todo");
        issue.number = Some(7);
        idx.upsert_issue(&issue, "p", "s").unwrap();
        assert_eq!(idx.max_number().unwrap(), Some(7));
    }

    #[test]
    fn a_falsely_stamped_index_is_detected_by_shape_not_stamp() {
        // What a buggy open leaves behind: old tables carrying today's
        // version stamp. The stamp says current; the missing column says
        // otherwise — the probe must win.
        let file = tempfile::NamedTempFile::new().unwrap();
        let old = rusqlite::Connection::open(file.path()).unwrap();
        old.execute_batch(
            "CREATE TABLE issues (id TEXT PRIMARY KEY, path TEXT, blob_sha TEXT, short_ref TEXT, \
             title TEXT, type TEXT, status TEXT, priority TEXT, reporter TEXT, epic TEXT, \
             estimate INTEGER, sprint TEXT, due TEXT, created TEXT, updated TEXT, body TEXT);
             PRAGMA user_version = 2;",
        )
        .unwrap();
        drop(old);

        let mut idx = Index::open(file.path()).unwrap();
        let mut issue = sample_issue(ID, "todo");
        issue.number = Some(3);
        idx.upsert_issue(&issue, "p", "s").unwrap();
        assert_eq!(idx.max_number().unwrap(), Some(3));
    }
}
