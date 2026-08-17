//! The issue aggregate — the shape of `issue.md`.
//!
//! What is deliberately NOT here: related commits, PR links, comment counts,
//! activity, time-in-status, `repo:`. All of that is computed from git
//! during indexing. Storing computed data in the file would go stale the
//! moment the underlying git facts change, so the file only carries what a
//! human actually edits.

use serde::{Deserialize, Serialize};

use crate::ids::{IdError, IssueId};

/// `type:` in the frontmatter. The set is fixed by the file format; adding a
/// variant means every older client sees an unknown value, so it is a
/// deliberate, versioned change — not an everyday edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    Task,
    Bug,
    Story,
    Spike,
    Chore,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::Task => "task",
            IssueKind::Bug => "bug",
            IssueKind::Story => "story",
            IssueKind::Spike => "spike",
            IssueKind::Chore => "chore",
        }
    }

    /// Parse the frontmatter wire form. Lockstep with the serde rename
    /// (snake_case) — a mismatch means files and the API disagree.
    pub fn parse(s: &str) -> Option<IssueKind> {
        match s {
            "task" => Some(IssueKind::Task),
            "bug" => Some(IssueKind::Bug),
            "story" => Some(IssueKind::Story),
            "spike" => Some(IssueKind::Spike),
            "chore" => Some(IssueKind::Chore),
            _ => None,
        }
    }
}

/// p0 (urgent) .. p4 (never). Ordering derives from the enum, not the string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Priority {
    P0 = 0,
    P1 = 1,
    P2 = 2,
    P3 = 3,
    P4 = 4,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Priority::P0 => "p0",
            Priority::P1 => "p1",
            Priority::P2 => "p2",
            Priority::P3 => "p3",
            Priority::P4 => "p4",
        }
    }

    pub fn parse(s: &str) -> Option<Priority> {
        match s {
            "p0" => Some(Priority::P0),
            "p1" => Some(Priority::P1),
            "p2" => Some(Priority::P2),
            "p3" => Some(Priority::P3),
            "p4" => Some(Priority::P4),
            _ => None,
        }
    }
}

/// The aggregate as it lives in the file plus what the index carries beside it.
/// Computed data never appears here — see the module docs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub id: IssueId,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: IssueKind,
    /// Must exist in `schema/workflow.yaml`; validated by the store, not the type.
    pub status: String,
    pub priority: Option<Priority>,
    pub reporter: Option<String>,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub epic: Option<IssueId>,
    pub estimate: Option<u32>,
    pub sprint: Option<String>,
    /// RFC3339. Parsed/validated in `dit-parse`; kept as a string so the pure
    /// core stays copy-cheap and the wire format is byte-faithful.
    pub created: String,
    pub updated: String,
    /// `YYYY-MM-DD`, or None.
    pub due: Option<String>,
    pub blocked_by: Vec<IssueId>,
    /// The markdown body below the frontmatter.
    pub body: String,
}

/// Input to `Transaction::create_issue` — identity and timestamps are minted
/// by the store, never trusted from the caller.
#[derive(Debug, Clone)]
pub struct IssueDraft {
    pub title: String,
    pub kind: IssueKind,
    pub status: Option<String>,
    pub priority: Option<Priority>,
    pub reporter: Option<String>,
    pub assignees: Vec<String>,
    pub labels: Vec<String>,
    pub epic: Option<IssueId>,
    pub estimate: Option<u32>,
    pub sprint: Option<String>,
    pub due: Option<String>,
    pub blocked_by: Vec<IssueId>,
    pub body: String,
}

/// An additive patch: `None` means "don't touch". A whole `Issue` must never
/// be written back — writing fields nobody changed is what produces spurious
/// merge conflicts when two people edit different parts of the same issue.
/// Unknown frontmatter fields are not representable here, on purpose: the
/// patch only speaks the fields DIT knows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldPatch {
    pub title: Option<String>,
    pub kind: Option<IssueKind>,
    pub status: Option<String>,
    pub priority: Option<Priority>,
    pub reporter: Option<String>,
    /// Replaces the set. To add one label, send the union you want.
    pub assignees: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
    pub epic: Option<IssueId>,
    pub estimate: Option<u32>,
    pub sprint: Option<String>,
    pub due: Option<String>,
    pub blocked_by: Option<Vec<IssueId>>,
}

impl FieldPatch {
    pub fn is_empty(&self) -> bool {
        *self == FieldPatch::default()
    }

    /// The set of frontmatter keys this patch will touch. Used by the merge
    /// driver to detect "both sides changed the same field".
    pub fn touched_keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        for (present, key) in [
            (self.title.is_some(), "title"),
            (self.kind.is_some(), "type"),
            (self.status.is_some(), "status"),
            (self.priority.is_some(), "priority"),
            (self.reporter.is_some(), "reporter"),
            (self.assignees.is_some(), "assignees"),
            (self.labels.is_some(), "labels"),
            (self.epic.is_some(), "epic"),
            (self.estimate.is_some(), "estimate"),
            (self.sprint.is_some(), "sprint"),
            (self.due.is_some(), "due"),
            (self.blocked_by.is_some(), "blocked_by"),
        ] {
            if present {
                keys.push(key);
            }
        }
        keys
    }
}

impl Issue {
    /// Apply a patch in place. Timestamps are the store's business, not this
    /// method's — `updated` is bumped by the transaction, so that a Vim edit
    /// and a DIT edit bump it through the same single path.
    pub fn apply(&mut self, patch: &FieldPatch) -> Result<(), IdError> {
        if let Some(t) = &patch.title {
            self.title = t.clone();
        }
        if let Some(k) = patch.kind {
            self.kind = k;
        }
        if let Some(s) = &patch.status {
            self.status = s.clone();
        }
        if let Some(p) = patch.priority {
            self.priority = Some(p);
        }
        if let Some(r) = &patch.reporter {
            self.reporter = Some(r.clone());
        }
        if let Some(a) = &patch.assignees {
            self.assignees = a.clone();
        }
        if let Some(l) = &patch.labels {
            self.labels = l.clone();
        }
        if let Some(e) = patch.epic {
            self.epic = Some(e);
        }
        if let Some(est) = patch.estimate {
            self.estimate = Some(est);
        }
        if let Some(s) = &patch.sprint {
            self.sprint = Some(s.clone());
        }
        if let Some(d) = &patch.due {
            self.due = Some(d.clone());
        }
        if let Some(b) = &patch.blocked_by {
            self.blocked_by = b.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn sample_issue() -> Issue {
        Issue {
            id: IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap(),
            title: "Login timeout on slow networks".into(),
            kind: IssueKind::Bug,
            status: "in_progress".into(),
            priority: Some(Priority::P1),
            reporter: Some("farid".into()),
            assignees: vec!["farid".into()],
            labels: vec!["auth".into()],
            epic: None,
            estimate: Some(3),
            sprint: Some("2026-W33".into()),
            created: "2026-08-16T09:12:00Z".into(),
            updated: "2026-08-16T11:40:00Z".into(),
            due: None,
            blocked_by: vec![],
            body: "## Context\n\nUsers on 3G get logged out.".into(),
        }
    }

    #[test]
    fn patch_is_additive_only() {
        let mut issue = sample_issue();
        let patch = FieldPatch {
            status: Some("review".into()),
            labels: Some(vec!["auth".into(), "regression".into()]),
            ..FieldPatch::default()
        };
        issue.apply(&patch).unwrap();

        assert_eq!(issue.status, "review");
        assert_eq!(issue.labels, vec!["auth", "regression"]);
        // Untouched fields survive verbatim.
        assert_eq!(issue.title, "Login timeout on slow networks");
        assert_eq!(issue.priority, Some(Priority::P1));
    }

    #[test]
    fn empty_patch_touches_no_keys() {
        assert!(FieldPatch::default().is_empty());
        assert!(FieldPatch::default().touched_keys().is_empty());
    }

    #[test]
    fn kind_serializes_as_type() {
        let json = serde_json::to_value(sample_issue()).unwrap();
        // The file key is `type`, never `kind` — the file format is the
        // contract, and the API mirrors it.
        assert!(json.get("type").is_some());
        assert!(json.get("kind").is_none());
    }

    #[test]
    fn priority_orders_p0_first() {
        assert!(Priority::P0 < Priority::P4);
    }
}
