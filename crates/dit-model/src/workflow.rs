//! Configurable workflow and the rules for an issue's *effective* status.
//!
//! The status a board shows is `resolve(status_in_file, derived_signals)` —
//! computed at read time, never written back to the file. Writing it back
//! would make every code commit touch the issue file again, which is exactly
//! the churn this split exists to prevent.

use serde::{Deserialize, Serialize};

use crate::status::StatusCategory;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStatus {
    pub id: String,
    pub label: String,
    pub category: StatusCategory,
    #[serde(default)]
    pub wip_limit: Option<u32>,
    #[serde(default)]
    pub terminal: bool,
}

/// `from` may contain `"*"` meaning any non-terminal status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub from: Vec<String>,
    pub to: String,
    #[serde(default)]
    pub requires: Vec<String>,
}

/// A durable git signal that implies a status. `branch_exists` is deliberately
/// absent: branches are refs, hosts delete them after a merge, and a signal
/// that can vanish later would make the board flicker on every rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerivedSignal {
    /// A `Closes`/`Refs` trailer on a code commit (durable: the commit stays
    /// reachable forever).
    CommitTrailer,
    /// A PR merge, when host credentials exist. Degrades gracefully offline.
    PrMerged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedRule {
    pub signal: DerivedSignal,
    pub implies: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub statuses: Vec<WorkflowStatus>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub derived: Vec<DerivedRule>,
}

/// One resolved derived signal at query time. Computed, never stored — see
/// the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedStatusSignal {
    pub signal: DerivedSignal,
    pub implies: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WorkflowError {
    #[error("unknown status `{0}` — is it in schema/workflow.yaml?")]
    UnknownStatus(String),
    #[error(
        "illegal transition `{from}` → `{to}` (allowed transitions come from schema/workflow.yaml)"
    )]
    IllegalTransition { from: String, to: String },
}

impl Workflow {
    /// The workflow shipped by `dit init`. Projects edit
    /// `.dit/schema/workflow.yaml`; this is only the seed.
    pub fn default_workflow() -> Workflow {
        Workflow {
            statuses: vec![
                status("backlog", "Backlog", StatusCategory::Todo, None, false),
                status("todo", "To Do", StatusCategory::Todo, None, false),
                status(
                    "in_progress",
                    "In Progress",
                    StatusCategory::Doing,
                    Some(3),
                    false,
                ),
                status("review", "In Review", StatusCategory::Doing, None, false),
                status("done", "Done", StatusCategory::Done, None, true),
                status("cancelled", "Cancelled", StatusCategory::Done, None, true),
            ],
            transitions: vec![
                Transition {
                    from: vec!["backlog".into(), "todo".into()],
                    to: "in_progress".into(),
                    requires: vec![],
                },
                Transition {
                    from: vec!["in_progress".into()],
                    to: "review".into(),
                    requires: vec![],
                },
                Transition {
                    from: vec!["review".into()],
                    to: "done".into(),
                    requires: vec![],
                },
                Transition {
                    from: vec!["*".into()],
                    to: "cancelled".into(),
                    requires: vec![],
                },
            ],
            derived: vec![
                DerivedRule {
                    signal: DerivedSignal::CommitTrailer,
                    implies: "review".into(),
                },
                DerivedRule {
                    signal: DerivedSignal::PrMerged,
                    implies: "done".into(),
                },
            ],
        }
    }

    pub fn status(&self, id: &str) -> Option<&WorkflowStatus> {
        self.statuses.iter().find(|s| s.id == id)
    }

    pub fn contains_status(&self, id: &str) -> bool {
        self.status(id).is_some()
    }

    /// Is `to` reachable from `from`? `*` in a transition's `from` matches
    /// any non-terminal source. The previous status is not stored anywhere,
    /// so full checking is impossible for direct edits — this is used by
    /// `dit validate` on PR diffs and by the merge driver.
    pub fn is_legal_transition(&self, from: &str, to: &str) -> bool {
        self.transitions.iter().any(|t| {
            t.to == to
                && (t.from.iter().any(|f| f == from)
                    || (t.from.iter().any(|f| f == "*") && !self.is_terminal(from)))
        })
    }

    pub fn is_terminal(&self, id: &str) -> bool {
        self.status(id).is_some_and(|s| s.terminal)
    }

    /// Statuses ordered as the board renders them (declaration order).
    pub fn board_columns(&self) -> impl Iterator<Item = &WorkflowStatus> {
        self.statuses.iter()
    }
}

fn status(
    id: &str,
    label: &str,
    category: StatusCategory,
    wip_limit: Option<u32>,
    terminal: bool,
) -> WorkflowStatus {
    WorkflowStatus {
        id: id.into(),
        label: label.into(),
        category,
        wip_limit,
        terminal,
    }
}

/// The domain service behind the board: effective status =
/// `resolve(status_in_file, derived_signals)`.
///
/// Rules:
/// - a terminal status in the file always wins — a merged PR must not drag a
///   cancelled issue back to `done`;
/// - otherwise the strongest signal wins. Precedence is fixed, not
///   configuration: `pr_merged` > `commit_trailer`. Signals the workflow does
///   not declare are ignored.
pub fn resolve_status(
    workflow: &Workflow,
    file_status: &str,
    signals: &[DerivedStatusSignal],
) -> String {
    if workflow.is_terminal(file_status) {
        return file_status.to_owned();
    }
    let strength = |s: &DerivedSignal| match s {
        DerivedSignal::PrMerged => 2,
        DerivedSignal::CommitTrailer => 1,
    };
    let known: Vec<&DerivedStatusSignal> = signals
        .iter()
        .filter(|sig| workflow.derived.iter().any(|r| r.signal == sig.signal))
        .collect();
    known
        .into_iter()
        .max_by_key(|sig| strength(&sig.signal))
        .map(|sig| sig.implies.clone())
        .unwrap_or_else(|| file_status.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_transition_matches_any_non_terminal_source() {
        let wf = Workflow::default_workflow();
        assert!(wf.is_legal_transition("todo", "cancelled"));
        assert!(wf.is_legal_transition("review", "cancelled"));
        // `*` does not rescue a transition out of a terminal status.
        assert!(!wf.is_legal_transition("done", "cancelled"));
    }

    #[test]
    fn transition_must_be_declared() {
        let wf = Workflow::default_workflow();
        assert!(wf.is_legal_transition("todo", "in_progress"));
        // Skipping straight from backlog to done is not declared.
        assert!(!wf.is_legal_transition("backlog", "done"));
    }

    #[test]
    fn file_status_wins_when_no_signals() {
        let wf = Workflow::default_workflow();
        assert_eq!(resolve_status(&wf, "todo", &[]), "todo");
    }

    #[test]
    fn commit_trailer_implies_review() {
        let wf = Workflow::default_workflow();
        let sig = [DerivedStatusSignal {
            signal: DerivedSignal::CommitTrailer,
            implies: "review".into(),
        }];
        assert_eq!(resolve_status(&wf, "todo", &sig), "review");
    }

    #[test]
    fn pr_merged_is_stronger_than_commit_trailer() {
        let wf = Workflow::default_workflow();
        let sigs = [
            DerivedStatusSignal {
                signal: DerivedSignal::CommitTrailer,
                implies: "review".into(),
            },
            DerivedStatusSignal {
                signal: DerivedSignal::PrMerged,
                implies: "done".into(),
            },
        ];
        assert_eq!(resolve_status(&wf, "todo", &sigs), "done");
    }

    #[test]
    fn terminal_file_status_is_never_overridden() {
        let wf = Workflow::default_workflow();
        let sigs = [DerivedStatusSignal {
            signal: DerivedSignal::PrMerged,
            implies: "done".into(),
        }];
        // cancelled is terminal: a stray merged PR cannot resurrect it.
        assert_eq!(resolve_status(&wf, "cancelled", &sigs), "cancelled");
    }

    #[test]
    fn signals_the_workflow_does_not_declare_are_ignored() {
        let wf = Workflow {
            statuses: Workflow::default_workflow().statuses,
            transitions: vec![],
            derived: vec![], // no derived rules configured
        };
        let sigs = [DerivedStatusSignal {
            signal: DerivedSignal::PrMerged,
            implies: "done".into(),
        }];
        assert_eq!(resolve_status(&wf, "todo", &sigs), "todo");
    }
}
