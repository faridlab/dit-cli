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

/// A declared parallel work stream (ADR 0015). `owners` is advisory — the
/// aliases expected to work this lane; routing reads it, nothing enforces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lane {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub owners: Vec<String>,
}

/// What a blocker must have reached for a dependent to be ready (ADR 0015).
/// Serialized as the bare string: `terminal`, or a status id meaning "that
/// status or later" in declaration order. A status reachable from any state
/// (the `from: ["*"]` exit, `cancelled` in the default workflow) satisfies
/// no gate — it is *broken*, not progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    Terminal,
    Until(String),
}

impl Gate {
    pub fn as_str(&self) -> &str {
        match self {
            Gate::Terminal => "terminal",
            Gate::Until(id) => id,
        }
    }
}

impl Serialize for Gate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Gate {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        if raw == "terminal" {
            Ok(Gate::Terminal)
        } else {
            Ok(Gate::Until(raw))
        }
    }
}

/// Tuning for the readiness derivation. Defaults mirror the seed workflow:
/// pick from `todo`, gate at `terminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessConfig {
    pub pick_from: StatusCategory,
    pub gate: Gate,
}

impl Default for ReadinessConfig {
    fn default() -> Self {
        ReadinessConfig {
            pick_from: StatusCategory::Todo,
            gate: Gate::Terminal,
        }
    }
}

/// The coordination knobs (ADR 0015). A claim older than `claim_ttl_minutes`
/// is stale: takable by another actor, renewable by its owner. The TTL is a
/// workspace-committed integer so every lane reads the same rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coordination {
    pub claim_ttl_minutes: u32,
    pub readiness: ReadinessConfig,
}

impl Default for Coordination {
    fn default() -> Self {
        Coordination {
            claim_ttl_minutes: 15,
            readiness: ReadinessConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub statuses: Vec<WorkflowStatus>,
    #[serde(default)]
    pub transitions: Vec<Transition>,
    #[serde(default)]
    pub derived: Vec<DerivedRule>,
    #[serde(default)]
    pub lanes: Vec<Lane>,
    #[serde(default)]
    pub coordination: Coordination,
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
            lanes: Vec::new(),
            coordination: Coordination::default(),
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

    pub fn lane(&self, id: &str) -> Option<&Lane> {
        self.lanes.iter().find(|l| l.id == id)
    }

    /// Statuses reachable from any state — the `from: ["*"]` escapes. They
    /// are exits (`cancelled` in the default workflow), not progress: a
    /// blocker sitting in one is *broken*, never satisfying a gate (ADR 0015).
    pub fn exit_statuses(&self) -> Vec<&str> {
        self.transitions
            .iter()
            .filter(|t| t.from.iter().any(|f| f == "*"))
            .map(|t| t.to.as_str())
            .collect()
    }

    /// The statuses that satisfy `gate`. `Terminal` = every terminal status
    /// except the exits; `Until(id)` = `id` and every status declared after
    /// it, except the exits. An unknown `Until` id yields an empty set —
    /// schema validation rejects it at load time; this is the defensive floor.
    pub fn gate_statuses(&self, gate: &Gate) -> Vec<&str> {
        let exits = self.exit_statuses();
        let not_exit = |s: &WorkflowStatus| !exits.contains(&s.id.as_str());
        match gate {
            Gate::Terminal => self
                .statuses
                .iter()
                .filter(|s| s.terminal)
                .filter(|s| not_exit(s))
                .map(|s| s.id.as_str())
                .collect(),
            Gate::Until(id) => match self.statuses.iter().position(|s| &s.id == id) {
                Some(from) => self.statuses[from..]
                    .iter()
                    .filter(|s| not_exit(s))
                    .map(|s| s.id.as_str())
                    .collect(),
                None => Vec::new(),
            },
        }
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
    fn the_default_gate_is_done_only_and_exits_never_satisfy() {
        let wf = Workflow::default_workflow();
        // Terminal completion only: `cancelled` is an exit, not progress.
        assert_eq!(wf.gate_statuses(&Gate::Terminal), vec!["done"]);
        // "review or later" in declaration order, still without the exit.
        assert_eq!(
            wf.gate_statuses(&Gate::Until("review".into())),
            vec!["review", "done"]
        );
        assert_eq!(
            wf.gate_statuses(&Gate::Until("cancelled".into())),
            Vec::<&str>::new(),
            "an Until pinned at the exit still excludes it"
        );
        assert_eq!(wf.exit_statuses(), vec!["cancelled"]);
    }

    #[test]
    fn an_unknown_until_id_is_an_empty_gate_set() {
        let wf = Workflow::default_workflow();
        assert!(wf.gate_statuses(&Gate::Until("nope".into())).is_empty());
    }

    #[test]
    fn default_workflow_has_no_lanes_and_default_coordination() {
        let wf = Workflow::default_workflow();
        assert!(wf.lanes.is_empty());
        assert_eq!(wf.lane("backend"), None);
        assert_eq!(wf.coordination.claim_ttl_minutes, 15);
        assert_eq!(
            wf.coordination.readiness.pick_from,
            crate::status::StatusCategory::Todo
        );
        assert_eq!(wf.coordination.readiness.gate, Gate::Terminal);
    }

    #[test]
    fn lanes_and_coordination_round_trip() {
        let wf = Workflow {
            lanes: vec![Lane {
                id: "backend".into(),
                label: "Backend".into(),
                owners: vec!["be-1".into()],
            }],
            coordination: Coordination {
                claim_ttl_minutes: 30,
                readiness: ReadinessConfig {
                    pick_from: crate::status::StatusCategory::Todo,
                    gate: Gate::Until("review".into()),
                },
            },
            ..Workflow::default_workflow()
        };
        let json = serde_json::to_value(&wf).unwrap();
        // The gate serializes as the bare string the yaml block shows.
        assert_eq!(json["coordination"]["readiness"]["gate"], "review");
        let back: Workflow = serde_json::from_value(json).unwrap();
        assert_eq!(back, wf);
        assert_eq!(
            back.lane("backend").map(|l| l.owners.clone()),
            Some(vec!["be-1".to_string()])
        );
    }

    #[test]
    fn a_workflow_without_the_new_blocks_defaults_them() {
        // The pre-ADR-0015 yaml shape: statuses, transitions, derived only.
        let json = serde_json::json!({
            "statuses": [
                { "id": "todo", "label": "To Do", "category": "todo" },
                { "id": "done", "label": "Done", "category": "done", "terminal": true }
            ],
            "transitions": [{ "from": ["todo"], "to": "done" }]
        });
        let wf: Workflow = serde_json::from_value(json).unwrap();
        assert!(wf.lanes.is_empty());
        assert_eq!(wf.coordination, Coordination::default());
        assert_eq!(wf.gate_statuses(&Gate::Terminal), vec!["done"]);
    }

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
            transitions: vec![],
            derived: vec![], // no derived rules configured
            ..Workflow::default_workflow()
        };
        let sigs = [DerivedStatusSignal {
            signal: DerivedSignal::PrMerged,
            implies: "done".into(),
        }];
        assert_eq!(resolve_status(&wf, "todo", &sigs), "todo");
    }
}
