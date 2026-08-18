//! The one error type callers of the facade ever see.

use dit_index::IndexError;
use dit_parse::SchemaError;
use dit_query::QueryError;
use dit_store::StoreError;
use dit_vcs::VcsError;

/// Every failure the facade can produce.
///
/// The deliberate omission: a merge conflict. Conflicts come back as fields
/// on `SyncReport`, never as this enum — a conflict is a state the UI must
/// show somebody, and an `Err` invites callers to hide it.
#[derive(Debug, thiserror::Error)]
pub enum DitError {
    /// Another process holds the single-writer lock. Not retried silently:
    /// the holder's name goes to the user, who decides whether to wait.
    #[error("another DIT process is writing ({held_by}) — close it or retry shortly")]
    Busy { held_by: String },
    #[error(transparent)]
    Vcs(#[from] VcsError),
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Anything wrong here is recoverable with a rebuild — the index is a
    /// disposable copy of what git already holds.
    #[error(transparent)]
    Index(#[from] IndexError),
    /// A DQL string that does not parse, or parses into something the
    /// compiler rejects (e.g. `@me` with no known user).
    #[error(transparent)]
    Query(#[from] QueryError),
    /// The caller named an issue that is not in the workspace — a wrong
    /// short ref or id, not a broken workspace. Delivery maps it to its own
    /// "you asked for something that isn't there" signal (404, exit 2).
    #[error("no issue matches `{0}`")]
    NotFound(String),
    /// The action would take over something DIT does not own (e.g. `dit init`
    /// into a tree that already has an `issues/` directory). Nothing was
    /// written — the message names the way out.
    #[error("{0}")]
    Refuse(String),
    /// A named issue template is not in `.dit/templates/`. Distinct from
    /// `NotFound` (an issue ref) so the CLI can hint at `dit templates list`.
    #[error("no template named `{0}` in .dit/templates/")]
    TemplateMissing(String),
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
