//! The coordination board (ADR 0015): lanes as rows, workflow statuses as
//! columns, and the derived per-issue facts a parallel actor or a watching
//! human needs — readiness, per-blocker state, claim liveness. Everything
//! here is computed at read time from the index; nothing is stored.

use dit_index::IndexedIssue;
use dit_model::{ClaimLiveness, IssueId, Priority, Readiness, StatusCategory};

use crate::{Dit, DitError};
use time::OffsetDateTime;

/// The whole board: one row per registered lane, Unlaned last.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowBoard {
    pub lanes: Vec<WorkflowLane>,
    pub claim_ttl_minutes: u32,
}

/// One swimlane row.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowLane {
    /// `None` = the Unlaned row every workspace has.
    pub id: Option<String>,
    pub label: String,
    pub owners: Vec<String>,
    pub cards: Vec<WorkflowCard>,
}

/// One card. `readiness`, `blockers` and `claim` are the derived facts; the
/// rest is the issue's authored state.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowCard {
    pub id: IssueId,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    pub status_label: String,
    pub category: Option<StatusCategory>,
    pub priority: Option<Priority>,
    pub readiness: Readiness,
    /// Every blocker with its derived state.
    pub blockers: Vec<BlockerState>,
    pub claim: Option<ClaimState>,
}

/// One blocker of a card, with where it stands against the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockerState {
    pub id: IssueId,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    /// Satisfied = through the gate; Unsatisfied = not yet; Broken =
    /// cancelled or gone — never auto-unblocking (ADR 0015).
    pub state: BlockerDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockerDisposition {
    Satisfied,
    Unsatisfied,
    Broken,
}

/// A claim as the board shows it: who holds it and whether it is still worth
/// respecting. The age is computed, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimState {
    pub claimed_by: String,
    pub claimed_at: String,
    pub stale: bool,
}

impl Dit {
    /// The coordination board (ADR 0015). Lanes come from the registry in
    /// declaration order; issues with no lane (or a lane id that is no
    /// longer registered) land in the trailing Unlaned row. Reads only.
    pub fn workflow_board(&self) -> Result<WorkflowBoard, DitError> {
        let all = self.query("", None)?;
        let by_id: std::collections::HashMap<IssueId, &IndexedIssue> =
            all.iter().map(|h| (h.issue.id, h)).collect();
        let gate = &self.workflow.coordination.readiness.gate;
        let satisfying = self.workflow.gate_statuses(gate);
        let exits = self.workflow.exit_statuses();
        let now = OffsetDateTime::now_utc();
        let ttl = self.workflow.coordination.claim_ttl_minutes;

        let mut lanes: Vec<WorkflowLane> = self
            .workflow
            .lanes
            .iter()
            .map(|l| WorkflowLane {
                id: Some(l.id.clone()),
                label: l.label.clone(),
                owners: l.owners.clone(),
                cards: Vec::new(),
            })
            .collect();
        lanes.push(WorkflowLane {
            id: None,
            label: "Unlaned".into(),
            owners: Vec::new(),
            cards: Vec::new(),
        });

        // Urgent first, matching the classic board's sort.
        let mut sorted = all.clone();
        sorted.sort_by_key(|h| h.issue.priority);

        for hit in sorted {
            let issue = &hit.issue;
            let mut blockers = Vec::new();
            let mut unsatisfied = Vec::new();
            let mut broken = Vec::new();
            for b in &issue.blocked_by {
                match by_id.get(b) {
                    Some(blocker) => {
                        let status = blocker.issue.status.as_str();
                        let state = if satisfying.contains(&status) {
                            BlockerDisposition::Satisfied
                        } else if exits.contains(&status) {
                            BlockerDisposition::Broken
                        } else {
                            BlockerDisposition::Unsatisfied
                        };
                        match state {
                            BlockerDisposition::Satisfied => {}
                            BlockerDisposition::Unsatisfied => unsatisfied.push(*b),
                            BlockerDisposition::Broken => broken.push(*b),
                        }
                        blockers.push(BlockerState {
                            id: *b,
                            short_ref: b.short_ref().as_str().to_owned(),
                            number: blocker.issue.number,
                            title: blocker.issue.title.clone(),
                            status: status.to_owned(),
                            state,
                        });
                    }
                    // Gone from the index: a deleted dependency is broken.
                    None => {
                        broken.push(*b);
                        blockers.push(BlockerState {
                            id: *b,
                            short_ref: b.short_ref().as_str().to_owned(),
                            number: None,
                            title: String::new(),
                            status: String::new(),
                            state: BlockerDisposition::Broken,
                        });
                    }
                }
            }
            let readiness = if unsatisfied.is_empty() && broken.is_empty() {
                Readiness::Ready
            } else {
                Readiness::Blocked {
                    unsatisfied,
                    broken,
                }
            };
            let claim = match (issue.claimed_by.as_deref(), issue.claimed_at.as_deref()) {
                (Some(by), Some(at)) => Some(ClaimState {
                    claimed_by: by.to_owned(),
                    claimed_at: at.to_owned(),
                    stale: matches!(
                        dit_model::claim_liveness(Some(by), Some(at), now, ttl),
                        ClaimLiveness::Stale
                    ),
                }),
                _ => None,
            };
            let card = WorkflowCard {
                id: issue.id,
                short_ref: issue.id.short_ref().as_str().to_owned(),
                number: issue.number,
                title: issue.title.clone(),
                status: issue.status.clone(),
                status_label: self
                    .workflow
                    .status(&issue.status)
                    .map(|s| s.label.clone())
                    .unwrap_or_else(|| issue.status.clone()),
                category: self.workflow.status(&issue.status).map(|s| s.category),
                priority: issue.priority,
                readiness,
                blockers,
                claim,
            };
            let row = lanes
                .iter()
                .position(|l| l.id.as_deref() == issue.lane.as_deref())
                .unwrap_or(lanes.len() - 1);
            lanes[row].cards.push(card);
        }
        Ok(WorkflowBoard {
            lanes,
            claim_ttl_minutes: ttl,
        })
    }
}
