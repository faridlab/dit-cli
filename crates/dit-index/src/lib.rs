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

use dit_model::{Comment, FieldEvent, Issue, IssueId, StoredFieldEvent};
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

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS issues (
  id        TEXT PRIMARY KEY,
  number    INTEGER,
  path      TEXT NOT NULL,
  blob_sha  TEXT NOT NULL,
  short_ref TEXT NOT NULL,
  title     TEXT NOT NULL,
  type      TEXT NOT NULL,
  status    TEXT NOT NULL,
  priority  TEXT,
  reporter  TEXT,
  epic      TEXT,
  estimate  INTEGER,
  sprint    TEXT,
  due       TEXT,
  created   TEXT NOT NULL,
  updated   TEXT NOT NULL,
  body      TEXT NOT NULL
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

CREATE TABLE IF NOT EXISTS comments (
  id       TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL,
  author   TEXT NOT NULL,
  at       TEXT NOT NULL,
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
const INDEX_VERSION: i64 = 2;

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
             priority, reporter, epic, estimate, sprint, due, created, updated, body) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
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
            "INSERT INTO comments (id, issue_id, author, at, body) VALUES (?1,?2,?3,?4,?5) \
             ON CONFLICT(id) DO UPDATE SET author=?3, at=?4, body=?5",
            params![
                comment.id.as_str(),
                issue_id.as_str(),
                comment.author,
                comment.created,
                comment.body
            ],
        )?;
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
        for table in ["issues", "issue_assignees", "issue_labels", "comments"] {
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
                "SELECT path, blob_sha, title, type, status, priority, reporter, epic, estimate, \
                 sprint, due, created, updated, body, number FROM issues WHERE id = ?1",
                params![id.as_str()],
                issue_columns,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let assignees = self.set_for("issue_assignees", "alias", id)?;
        let labels = self.set_for("issue_labels", "label", id)?;
        Ok(Some(hydrate(id, cols, assignees, labels)?))
    }

    /// Every issue matching a compiled DQL query. Reads never touch the
    /// filesystem — this method is the whole read path.
    pub fn list_issues(&self, compiled: &Compiled) -> Result<Vec<IndexedIssue>, IndexError> {
        let limit = compiled.limit.unwrap_or(DEFAULT_LIMIT) as i64;
        let mut sql = format!(
            "SELECT id, path, blob_sha, title, type, status, priority, reporter, epic, estimate, \
             sprint, due, created, updated, body, number FROM issues WHERE {}",
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
            found.push(hydrate(&id, cols, assignees, labels)?);
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
            "SELECT id, author, at, body FROM comments WHERE issue_id = ?1 ORDER BY at, id",
        )?;
        let rows = stmt.query_map(params![issue_id.as_str()], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, author, at, body) = row?;
            out.push(Comment {
                id: IssueId::parse(&id)
                    .map_err(|e| IndexError::Corrupt(format!("comment id `{id}`: {e}")))?,
                author,
                created: at,
                body,
            });
        }
        Ok(out)
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
        created: r.get(11)?,
        updated: r.get(12)?,
        body: r.get(13)?,
        number: r.get::<_, Option<i64>>(14)?.map(|n| n as u32),
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
        created: get(11).unwrap_or_default(),
        updated: get(12).unwrap_or_default(),
        body: get(13).unwrap_or_default(),
        number: r
            .get::<_, Option<i64>>(n + 14)
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
) -> Result<IndexedIssue, IndexError> {
    let corrupt =
        |field: &str, raw: &str| IndexError::Corrupt(format!("field `{field}` holds `{raw}`"));
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
            blocked_by: Vec::new(),
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
    use dit_model::{EventSource, IssueKind, Priority};

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
            blocked_by: vec![],
            body: "Users on 3G get logged out.".into(),
        }
    }

    fn dql(text: &str) -> Query {
        dit_query::parse(text).unwrap()
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
            body: "reproduced".into(),
        };
        let late = Comment {
            id: IssueId::parse("01K3MA1F7XZZZZZZZZZZZZZZZZ").unwrap(),
            author: "budi".into(),
            created: "2026-08-16T11:00:00Z".into(),
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
