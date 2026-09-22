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
    /// Human-friendly number (ADR 0007). Assigned — at creation under
    /// `numbering: local`, by dit-bot on merge under `on-merge` — never chosen
    /// by hand and never part of the folder name. Optional: issues created
    /// before the field existed, and freshly filed branches in `on-merge`
    /// repos, carry None until assigned.
    pub number: Option<u32>,
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
    /// When the work is planned to begin — `YYYY-MM-DD`, or None. Optional
    /// on purpose: most issues never need one, and a scheduled bar can be
    /// inferred from `due` and the estimate without writing anything back.
    pub start: Option<String>,
    pub blocked_by: Vec<IssueId>,
    /// Issues that feed this one (ADR 0020): a result, an outcome, a return
    /// path. It draws an arrow and may carry a caption from the flow's
    /// fence, and it touches nothing derived — never readiness, never a
    /// stage, never the critical path. That separation is the whole point:
    /// `blocked_by` gets to mean exactly one thing.
    pub fed_by: Vec<IssueId>,
    /// A free-form lane name (ADR 0015, 0019): the band an issue renders in
    /// inside a flow diagram. None = Unlaned; valid, not an error. The
    /// workflow.yaml registry only hints order and labels — never a gate.
    pub lane: Option<String>,
    /// The orchestrations this issue belongs to (ADR 0019), by name.
    /// Many-to-many: one issue, any number of concurrent flows. A flow with
    /// no members is nothing — there is deliberately no file to keep alive.
    pub flows: Vec<String>,
    /// The actor asserting exclusive intent (ADR 0015). Written by
    /// `dit claim` only, never at creation. An assertion like `assignees`,
    /// not a derived fact — its liveness is computed from `claimed_at` +
    /// the TTL, never stored.
    pub claimed_by: Option<String>,
    /// RFC3339, minted alongside `claimed_by`.
    pub claimed_at: Option<String>,
    /// The markdown body below the frontmatter.
    pub body: String,
}

/// Input to `Transaction::create_issue` — identity and timestamps are minted
/// by the store, never trusted from the caller.
#[derive(Debug, Clone)]
pub struct IssueDraft {
    /// Set by the facade from the index (max + 1), not by the caller — the
    /// CLI and API have no `--number` flag on purpose.
    pub number: Option<u32>,
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
    pub start: Option<String>,
    pub blocked_by: Vec<IssueId>,
    /// Issues that feed this one (ADR 0020): a result, an outcome, a return
    /// path. It draws an arrow and may carry a caption from the flow's
    /// fence, and it touches nothing derived — never readiness, never a
    /// stage, never the critical path. That separation is the whole point:
    /// `blocked_by` gets to mean exactly one thing.
    pub fed_by: Vec<IssueId>,
    /// Claims never ride creation (ADR 0015): an issue is claimable once it
    /// exists, by an actor, through `dit claim`.
    pub lane: Option<String>,
    /// Flows may ride creation (ADR 0019) — membership is ordinary
    /// authorship, unlike a claim.
    pub flows: Vec<String>,
    pub body: String,
}

/// The optional fields a patch may clear (remove from the file). Required
/// fields are absent on purpose: an issue without a title or status is not
/// an issue, and the list fields clear by being set to `[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClearableField {
    Priority,
    Epic,
    Estimate,
    Sprint,
    Due,
    Start,
    Lane,
    /// `claim --release` clears the pair together; they are separate variants
    /// so `touched_keys` reports each line the merge driver must reason about.
    ClaimedBy,
    ClaimedAt,
}

impl ClearableField {
    pub const ALL: [ClearableField; 9] = [
        ClearableField::Priority,
        ClearableField::Epic,
        ClearableField::Estimate,
        ClearableField::Sprint,
        ClearableField::Due,
        ClearableField::Start,
        ClearableField::Lane,
        ClearableField::ClaimedBy,
        ClearableField::ClaimedAt,
    ];

    /// The frontmatter key this field lives under.
    pub fn key(self) -> &'static str {
        match self {
            ClearableField::Priority => "priority",
            ClearableField::Epic => "epic",
            ClearableField::Estimate => "estimate",
            ClearableField::Sprint => "sprint",
            ClearableField::Due => "due",
            ClearableField::Start => "start",
            ClearableField::Lane => "lane",
            ClearableField::ClaimedBy => "claimed_by",
            ClearableField::ClaimedAt => "claimed_at",
        }
    }
}

/// An additive patch: `None` means "don't touch". A whole `Issue` must never
/// be written back — writing fields nobody changed is what produces spurious
/// merge conflicts when two people edit different parts of the same issue.
/// Unknown frontmatter fields are not representable here, on purpose: the
/// patch only speaks the fields DIT knows. Removing an optional field is a
/// third state, `clear` — separate from the setters so a `Some` always means
/// a value and never a sentinel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldPatch {
    /// Repair hatch for duplicate numbers flagged by `dit doctor` — not a
    /// normal edit surface. Merge-relevant: two sides assigning different
    /// numbers to the same issue is a real conflict.
    pub number: Option<u32>,
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
    pub start: Option<String>,
    pub blocked_by: Option<Vec<IssueId>>,
    /// Replaces the set, like every other list field.
    pub fed_by: Option<Vec<IssueId>>,
    pub lane: Option<String>,
    /// Replaces the whole membership set, like `labels` (ADR 0019).
    pub flows: Option<Vec<String>>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    /// Optional fields to remove from the file. A field both set and cleared
    /// in one patch is a contradiction the parser refuses.
    pub clear: Vec<ClearableField>,
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
            (self.number.is_some(), "number"),
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
            (self.start.is_some(), "start"),
            (self.blocked_by.is_some(), "blocked_by"),
            (self.fed_by.is_some(), "fed_by"),
            (self.lane.is_some(), "lane"),
            (self.flows.is_some(), "flows"),
            (self.claimed_by.is_some(), "claimed_by"),
            (self.claimed_at.is_some(), "claimed_at"),
        ] {
            if present {
                keys.push(key);
            }
        }
        for field in &self.clear {
            if !keys.contains(&field.key()) {
                keys.push(field.key());
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
        if let Some(n) = patch.number {
            self.number = Some(n);
        }
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
        if let Some(d) = &patch.start {
            self.start = Some(d.clone());
        }
        if let Some(f) = &patch.fed_by {
            self.fed_by = f.clone();
        }
        if let Some(b) = &patch.blocked_by {
            self.blocked_by = b.clone();
        }
        if let Some(l) = &patch.lane {
            self.lane = Some(l.clone());
        }
        if let Some(f) = &patch.flows {
            self.flows = f.clone();
        }
        if let Some(c) = &patch.claimed_by {
            self.claimed_by = Some(c.clone());
        }
        if let Some(c) = &patch.claimed_at {
            self.claimed_at = Some(c.clone());
        }
        for field in &patch.clear {
            match field {
                ClearableField::Priority => self.priority = None,
                ClearableField::Epic => self.epic = None,
                ClearableField::Estimate => self.estimate = None,
                ClearableField::Sprint => self.sprint = None,
                ClearableField::Due => self.due = None,
                ClearableField::Start => self.start = None,
                ClearableField::Lane => self.lane = None,
                ClearableField::ClaimedBy => self.claimed_by = None,
                ClearableField::ClaimedAt => self.claimed_at = None,
            }
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
            number: None,
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
            start: None,
            blocked_by: vec![],
            fed_by: vec![],
            lane: None,
            flows: Vec::new(),
            claimed_by: None,
            claimed_at: None,
            body: "## Context\n\nUsers on 3G get logged out.".into(),
        }
    }

    #[test]
    fn lane_and_claim_patch_set_clear_and_touch_their_keys() {
        let mut issue = sample_issue();
        let patch = FieldPatch {
            lane: Some("frontend".into()),
            claimed_by: Some("fe-1".into()),
            claimed_at: Some("2026-08-16T11:38:00Z".into()),
            ..FieldPatch::default()
        };
        assert_eq!(
            patch.touched_keys(),
            vec!["lane", "claimed_by", "claimed_at"]
        );
        issue.apply(&patch).unwrap();
        assert_eq!(issue.lane.as_deref(), Some("frontend"));
        assert_eq!(issue.claimed_by.as_deref(), Some("fe-1"));
        assert_eq!(issue.claimed_at.as_deref(), Some("2026-08-16T11:38:00Z"));

        // Release clears the claim pair; each cleared key counts as touched.
        let release = FieldPatch {
            clear: vec![ClearableField::ClaimedBy, ClearableField::ClaimedAt],
            ..FieldPatch::default()
        };
        assert_eq!(release.touched_keys(), vec!["claimed_by", "claimed_at"]);
        issue.apply(&release).unwrap();
        assert_eq!(issue.claimed_by, None);
        assert_eq!(issue.claimed_at, None);
        assert_eq!(
            issue.lane.as_deref(),
            Some("frontend"),
            "lane survives a release"
        );
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
    fn a_patch_can_clear_optional_fields() {
        let mut issue = sample_issue();
        issue.due = Some("2026-09-01".into());
        issue.epic = Some(IssueId::parse("01K3M9ZXQ2ZZZZZZZZZZZZZZZZ").unwrap());
        let patch = FieldPatch {
            clear: vec![
                ClearableField::Priority,
                ClearableField::Estimate,
                ClearableField::Sprint,
                ClearableField::Due,
                ClearableField::Epic,
            ],
            ..FieldPatch::default()
        };
        assert!(!patch.is_empty());
        // Every cleared key counts as touched — the merge driver must see
        // "both sides changed priority" whether one side set it or unset it.
        assert_eq!(
            patch.touched_keys(),
            vec!["priority", "estimate", "sprint", "due", "epic"]
        );
        issue.apply(&patch).unwrap();
        assert_eq!(issue.priority, None);
        assert_eq!(issue.estimate, None);
        assert_eq!(issue.sprint, None);
        assert_eq!(issue.due, None);
        assert_eq!(issue.epic, None);
        // Untouched fields survive verbatim.
        assert_eq!(issue.title, "Login timeout on slow networks");
        assert_eq!(issue.labels, vec!["auth"]);
        // Every clearable field names its frontmatter key.
        for field in ClearableField::ALL {
            assert!(!field.key().is_empty());
        }
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

    #[test]
    fn number_is_patchable_and_touches_its_key() {
        let mut issue = sample_issue();
        assert_eq!(
            issue.number, None,
            "issues created before ADR 0007 have no number"
        );
        let patch = FieldPatch {
            number: Some(12),
            ..FieldPatch::default()
        };
        issue.apply(&patch).unwrap();
        assert_eq!(issue.number, Some(12));
        assert_eq!(patch.touched_keys(), vec!["number"]);
    }

    #[test]
    fn number_survives_a_round_trip_through_json() {
        let mut issue = sample_issue();
        issue.number = Some(12);
        let json = serde_json::to_value(&issue).unwrap();
        assert_eq!(json.get("number").and_then(|n| n.as_u64()), Some(12));
        let back: Issue = serde_json::from_value(json).unwrap();
        assert_eq!(back.number, Some(12));
    }
}
