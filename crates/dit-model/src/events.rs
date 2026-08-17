//! Field history events — what `git log` on an issue file would tell you, in
//! a shape the index can store and the UI can page through.
//!
//! Events are never written into the files. They are *observed*: from
//! per-commit frontmatter diffs in the DIT repo, or from commit trailers in a
//! code repo. That is what makes the history honest — someone editing a file
//! with Vim produces exactly the same events as someone using the UI.
//!
//! The ordering rule that shapes this type: events are ordered by `seq`, a
//! topological position in the commit graph, never by `ts`. Wall-clock
//! timestamps come from machines with skewed clocks and merge commits that
//! carry several dates at once; `ts` exists for display ("2 hours ago"), not
//! for deciding what happened after what.

use serde::{Deserialize, Serialize};

/// Where an event was observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// A frontmatter diff between a commit and one of its parents.
    File,
    /// A commit trailer or host-API fact that implies a field change without
    /// touching the file.
    Derived,
}

impl EventSource {
    pub fn as_str(self) -> &'static str {
        match self {
            EventSource::File => "file",
            EventSource::Derived => "derived",
        }
    }

    pub fn parse(s: &str) -> Option<EventSource> {
        match s {
            "file" => Some(EventSource::File),
            "derived" => Some(EventSource::Derived),
            _ => None,
        }
    }
}

/// One observed field change, not yet stored. `seq` is assigned by the index
/// when the event is recorded, in the order events arrive — so callers must
/// feed a walk in topological commit order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldEvent {
    pub issue_id: String,
    pub field: String,
    /// None = the field was absent before; Some("") = present but empty.
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    /// Git author of the commit the change rode in on.
    pub author: String,
    pub commit_sha: String,
    /// The parent the diff was taken against. Empty for a root commit or a
    /// derived event — the empty string, not NULL, because NULLs evade
    /// uniqueness checks and let a backfill run twice.
    pub parent_sha: String,
    /// Author date of the commit, for display only.
    pub ts: String,
    pub source: EventSource,
}

/// A stored event: everything above plus its assigned position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFieldEvent {
    pub seq: i64,
    pub issue_id: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub author: String,
    pub commit_sha: String,
    pub parent_sha: String,
    pub ts: String,
    pub source: EventSource,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn source_roundtrips_through_the_stored_spelling() {
        assert_eq!(EventSource::parse("file"), Some(EventSource::File));
        assert_eq!(EventSource::parse("derived"), Some(EventSource::Derived));
        assert_eq!(EventSource::parse("hunch"), None);
        assert_eq!(EventSource::File.as_str(), "file");
    }

    #[test]
    fn event_serializes_flat_for_the_api() {
        let e = FieldEvent {
            issue_id: "01K3M9ZXQ2R7VN8P4TDBCEFGHJ".into(),
            field: "status".into(),
            old_value: Some("todo".into()),
            new_value: Some("in_progress".into()),
            author: "farid".into(),
            commit_sha: "a3f9c2d".into(),
            parent_sha: "b7e1f45".into(),
            ts: "2026-08-16T09:00:00Z".into(),
            source: EventSource::File,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["source"], "file");
        let back: FieldEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }
}
