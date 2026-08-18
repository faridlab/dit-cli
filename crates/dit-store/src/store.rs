//! The store: file-level reads and the write transaction that stages,
//! applies, and can undo every change the workspace sees.
//!
//! A transaction stages all writes in memory; `finish` moves them to disk
//! atomically one file at a time and returns a `Changeset` describing what
//! happened. The caller (dit-core) commits to git afterwards; if that commit
//! fails, `Changeset::rollback` restores the previous bytes and deletes
//! freshly created files. Dropping an un-`finish`ed transaction touches
//! nothing at all.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use dit_model::{format_rfc3339, Comment, FieldPatch, Issue, IssueDraft, IssueId, ISSUE_BODY_FILE};
use dit_parse::Document;
use time::OffsetDateTime;

use crate::atomic;
use crate::layout::Layout;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] dit_parse::IssueParseError),
    #[error(transparent)]
    Comment(#[from] dit_parse::CommentError),
    #[error(transparent)]
    Frontmatter(#[from] dit_parse::FrontmatterError),
    #[error(transparent)]
    Fmt(#[from] dit_parse::fmt::FmtError),
    #[error("`{0}` does not exist in this workspace")]
    NotFound(String),
    #[error("`{0}` matches several issues ({1}) — use the full 26-character id")]
    Ambiguous(String, String),
    #[error(
        "the alias `{0}` cannot be used in a file name — use lowercase letters, digits and dashes"
    )]
    BadAlias(String),
    #[error("`{field}` may only contain lowercase letters, digits, dashes and underscores")]
    BadFieldValue { field: &'static str },
    #[error("cannot mint an id, the system entropy source failed: {0}")]
    Entropy(String),
}

/// A single issue's file, opened for editing or reading.
#[derive(Debug)]
pub struct IssueFile {
    pub path: PathBuf,
    pub issue: Issue,
    pub doc: Document,
}

#[derive(Debug)]
pub struct Store {
    layout: Layout,
}

impl Store {
    /// Open a workspace without validating it — `doctor` does that. An empty
    /// or missing `.dit/` is legal here because transactions may bootstrap.
    pub fn open(root: impl Into<PathBuf>) -> Store {
        Store {
            layout: Layout::detect(root),
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Resolve a full id or a 7-character short ref to a full id.
    ///
    /// This walks the filesystem, which makes it the right tool for
    /// bootstrapping (index builds, `dit doctor`) and the write path —
    /// never for queries, which must answer from the index.
    pub fn locate_issue(&self, needle: &str) -> Result<Option<IssueId>, StoreError> {
        if needle.len() == 26 {
            let id = IssueId::parse(needle).map_err(|_| StoreError::NotFound(needle.to_owned()))?;
            return Ok(self.layout.issue_dir_for(&id)?.map(|_| id));
        }
        let mut hits: Vec<IssueId> = Vec::new();
        self.walk_issue_files(|text| {
            if let Ok(doc) = Document::parse(&text) {
                if let Some(id) = doc.get_str("id").flatten() {
                    if let Ok(id) = IssueId::parse(&id) {
                        if id.as_str() == needle || id.short_ref().as_str() == needle {
                            hits.push(id);
                        }
                    }
                }
            }
        })?;
        match hits.len() {
            0 => Ok(None),
            1 => Ok(Some(hits.remove(0))),
            _ => Err(StoreError::Ambiguous(
                needle.to_owned(),
                hits.iter()
                    .map(|i| i.as_str().to_owned())
                    .collect::<Vec<_>>()
                    .join(", "),
            )),
        }
    }

    /// Read one issue's file from disk (write path / bootstrap only).
    pub fn read_issue(&self, id: &IssueId) -> Result<IssueFile, StoreError> {
        let path = self.layout.issue_body(id)?;
        let text = fs::read_to_string(&path)?;
        let (issue, doc) = dit_parse::parse_issue(&text)?;
        Ok(IssueFile { path, issue, doc })
    }

    /// All comments of an issue, oldest first (by file name, which starts
    /// with the comment's time-sortable id).
    pub fn read_comments(&self, id: &IssueId) -> Result<Vec<Comment>, StoreError> {
        let Some(dir) = self.layout.issue_dir_for(id)? else {
            return Err(StoreError::NotFound(id.as_str().to_owned()));
        };
        let comments_dir = dir.join("comments");
        let mut files: Vec<PathBuf> = match fs::read_dir(&comments_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "md"))
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        // Lexical order of the file names is creation order, because names
        // begin with the ULID's timestamp component.
        files.sort();
        let mut out = Vec::new();
        for f in files {
            let text = fs::read_to_string(&f)?;
            // An unparsable comment must surface, not be silently skipped:
            // it is user-visible data someone wrote by hand.
            out.push(dit_parse::parse_comment(&text)?);
        }
        Ok(out)
    }

    /// Begin staging writes. `now` is injected so tests are deterministic;
    /// `author` rides along on the changeset for commit attribution.
    ///
    /// The returned transaction owns what it needs (a copy of the layout) so
    /// it can outlive this borrow — the facade keeps one open across several
    /// operations while the rest of the workspace stays usable.
    pub fn transaction(&self, now: OffsetDateTime, author: &str) -> Transaction {
        Transaction {
            store: Store {
                layout: self.layout.clone(),
            },
            now,
            author: author.to_owned(),
            staged: Vec::new(),
            dirs: HashMap::new(),
        }
    }

    /// Visit every issue body under the issues root, cheapest-first. The walk
    /// only descends `year/month/folder`, so the generated `issues/README.md`
    /// index (ADR 0008) is never visited — shape excludes it. Unparsable
    /// files are skipped here — walking is for discovery, and one broken
    /// hand-edited file should not hide every other issue.
    fn walk_issue_files(&self, mut visit: impl FnMut(String)) -> Result<(), StoreError> {
        let years = match fs::read_dir(self.layout.issues_dir()) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for year in years {
            let year = year?;
            let months = match fs::read_dir(year.path()) {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            for month in months {
                let month = month?;
                for dir in fs::read_dir(month.path())? {
                    let dir = dir?;
                    if let Some(md) = Layout::body_file_in(&dir.path()) {
                        if let Ok(text) = fs::read_to_string(&md) {
                            visit(text);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// A staged write: the final path and the bytes it should hold.
#[derive(Debug)]
struct Staged {
    path: PathBuf,
    contents: String,
}

/// An applied write, remembering what was there before (if anything).
#[derive(Debug)]
struct Applied {
    path: PathBuf,
    previous: Option<Vec<u8>>,
}

/// The result of finishing a transaction: which files changed, and the power
/// to undo exactly those changes.
#[derive(Debug)]
pub struct Changeset {
    pub author: String,
    applied: Vec<Applied>,
    issues_root: PathBuf,
}

impl Changeset {
    /// The files that were written, in application order.
    pub fn paths(&self) -> impl Iterator<Item = &std::path::Path> {
        self.applied.iter().map(|a| a.path.as_path())
    }

    /// Undo every applied write: restore previous bytes, delete files that
    /// did not exist before, and clean up directories left empty.
    pub fn rollback(mut self) {
        for a in self.applied.drain(..).rev() {
            match a.previous {
                Some(bytes) => {
                    // Lossy restore is still better than leaving the new
                    // bytes in place; the odds of non-UTF-8 previous content
                    // are nil for files DIT itself wrote.
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let _ = atomic::write(&a.path, &text);
                }
                None => {
                    let _ = atomic::remove_file(&a.path);
                    if let Some(parent) = a.path.parent() {
                        atomic::prune_empty_dirs_up_to(parent, &self.issues_root);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Transaction {
    store: Store,
    now: OffsetDateTime,
    author: String,
    /// Staged writes in staging order; a repeated path replaces its entry so
    /// one transaction can never write a file twice.
    staged: Vec<Staged>,
    /// Issue dirs this transaction created, so later operations in the same
    /// transaction can find files that are not on disk yet.
    dirs: HashMap<IssueId, PathBuf>,
}

impl Transaction {
    /// Mint an id from the injected clock plus real entropy. Two calls in
    /// the same millisecond still differ — the 80 random bits carry the
    /// uniqueness, never the timestamp.
    fn mint_id(&self) -> Result<IssueId, StoreError> {
        let mut entropy = [0u8; 10];
        getrandom::fill(&mut entropy).map_err(|e| StoreError::Entropy(e.to_string()))?;
        Ok(IssueId::from_parts(
            crate::layout::datetime_to_ms(self.now).max(0) as u64,
            entropy,
        ))
    }

    fn push_staged(&mut self, path: PathBuf, contents: String) {
        if let Some(existing) = self.staged.iter_mut().find(|s| s.path == path) {
            existing.contents = contents;
        } else {
            self.staged.push(Staged { path, contents });
        }
    }

    /// The current bytes for a path: staged contents if this transaction
    /// already touched it, otherwise the file on disk.
    fn current_bytes(&self, path: &std::path::Path) -> Result<String, StoreError> {
        if let Some(s) = self.staged.iter().find(|s| s.path == path) {
            return Ok(s.contents.clone());
        }
        fs::read_to_string(path).map_err(|e| e.into())
    }

    /// Resolve an issue's directory, preferring dirs created in this
    /// transaction over a disk scan.
    fn issue_dir(&self, id: &IssueId) -> Result<PathBuf, StoreError> {
        if let Some(d) = self.dirs.get(id) {
            return Ok(d.clone());
        }
        self.store
            .layout
            .issue_dir_for(id)?
            .ok_or_else(|| StoreError::NotFound(id.as_str().to_owned()))
    }

    /// Resolve an issue's body file. Issues created in this transaction hold
    /// `README.md`; pre-existing ones keep whatever they have on disk (the
    /// legacy `issue.md` until migrated).
    fn issue_body(&self, id: &IssueId) -> Result<PathBuf, StoreError> {
        if let Some(d) = self.dirs.get(id) {
            return Ok(d.join(ISSUE_BODY_FILE));
        }
        self.store.layout.issue_body(id)
    }

    pub fn create_issue(&mut self, draft: IssueDraft) -> Result<IssueId, StoreError> {
        if let Some(s) = &draft.status {
            validate_value("status", s)?;
        }
        let id = self.mint_id()?;
        let slug = dit_model::Slug::from_title(&draft.title);
        let contents = dit_parse::serialize_new_issue(&id, &draft, &format_rfc3339(self.now))?;
        let dir = self.store.layout.issue_dir(&id, &slug);
        self.push_staged(dir.join(ISSUE_BODY_FILE), contents);
        self.dirs.insert(id, dir);
        Ok(id)
    }

    pub fn set_fields(&mut self, id: &IssueId, patch: FieldPatch) -> Result<(), StoreError> {
        if let Some(s) = &patch.status {
            validate_value("status", s)?;
        }
        let path = self.issue_body(id)?;
        let current = self.current_bytes(&path)?;
        let mut doc = Document::parse(&current)?;
        dit_parse::apply_patch(&mut doc, &patch, &format_rfc3339(self.now))?;
        self.push_staged(path, doc.to_string());
        Ok(())
    }

    /// Replace the markdown body (formatted canonically), leaving frontmatter
    /// untouched down to the byte.
    pub fn set_body(&mut self, id: &IssueId, body: &str) -> Result<(), StoreError> {
        let path = self.issue_body(id)?;
        let current = self.current_bytes(&path)?;
        let mut doc = Document::parse(&current)?;
        let body = dit_parse::fmt::format_body(body)?;
        let body = if body.is_empty() {
            String::new()
        } else if body.starts_with('\n') {
            body
        } else {
            format!("\n{body}")
        };
        doc.set_body(body);
        self.push_staged(path, doc.to_string());
        Ok(())
    }

    pub fn add_comment(
        &mut self,
        id: &IssueId,
        alias: &str,
        body: &str,
    ) -> Result<IssueId, StoreError> {
        let comment_id = self.mint_id()?;
        let name = Layout::comment_file_name(&comment_id, alias)?;
        let contents =
            dit_parse::serialize_comment(&comment_id, alias, &format_rfc3339(self.now), body)?;
        let dir = self.issue_dir(id)?;
        self.push_staged(dir.join("comments").join(name), contents);
        Ok(comment_id)
    }

    /// How many writes are staged. Purely informational — the abort path
    /// logs it so a discarded transaction is visible in the trace.
    pub fn staged_len(&self) -> usize {
        self.staged.len()
    }

    /// Apply every staged write. All-or-nothing: if a write fails partway,
    /// the already-applied prefix is rolled back before the error returns.
    pub fn finish(self) -> Result<Changeset, StoreError> {
        let mut applied: Vec<Applied> = Vec::new();
        for s in &self.staged {
            let previous = fs::read(&s.path).ok();
            match atomic::write(&s.path, &s.contents) {
                Ok(()) => applied.push(Applied {
                    path: s.path.clone(),
                    previous,
                }),
                Err(e) => {
                    let issues_root = self.store.layout.issues_dir();
                    Changeset {
                        author: self.author,
                        applied,
                        issues_root,
                    }
                    .rollback();
                    return Err(e.into());
                }
            }
        }
        Ok(Changeset {
            author: self.author,
            applied,
            issues_root: self.store.layout.issues_dir(),
        })
    }
}

/// Enum-like field values (status) are written into files that other tools —
/// the merge driver, the indexer, queries — treat as opaque tokens. A value
/// containing spaces, slashes or capitals would leak into paths and SQL as
/// structure, so it is refused here at the write boundary.
fn validate_value(field: &'static str, value: &str) -> Result<(), StoreError> {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if plain {
        Ok(())
    } else {
        Err(StoreError::BadFieldValue { field })
    }
}
