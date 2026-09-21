//! Derived coordination judgments: readiness and claim liveness (ADR 0015).
//!
//! Pure functions over authored state. Nothing here is ever stored — the
//! moment any of it was, it would go stale against the git facts it derives
//! from (Principle 3 / I5). The clock is injected: `now()` is I/O.

use time::OffsetDateTime;

use crate::ids::IssueId;
use crate::time::parse_rfc3339;
use crate::workflow::{Gate, Workflow};

/// Is this issue pickable right now? Computed from the issue's own status and
/// its blockers' statuses — one engine, read by `dit ready` and the workflow
/// board alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// `pick_from` status and every blocker through the gate.
    Ready,
    /// The issue's own status is outside `pick_from`: already being worked,
    /// finished, or not yet pulled in.
    NotPickable,
    /// Still waiting. `unsatisfied` names blockers that have not reached the
    /// gate; `broken` names blockers sitting in an exit status (`cancelled`) —
    /// abandoned, needing a human to remove or re-point the dependency, never
    /// an auto-unblock.
    Blocked {
        unsatisfied: Vec<IssueId>,
        broken: Vec<IssueId>,
    },
}

/// Derive readiness. `blockers` pairs each `blocked_by` entry with that
/// issue's current status. `gate_override` is the per-call escape hatch
/// (`dit ready --until review`); `None` uses the workflow's configured gate.
pub fn readiness(
    status: &str,
    blockers: &[(IssueId, String)],
    workflow: &Workflow,
    gate_override: Option<&Gate>,
) -> Readiness {
    let cfg = &workflow.coordination.readiness;
    let gate = gate_override.unwrap_or(&cfg.gate);
    let pickable = workflow
        .status(status)
        .is_some_and(|s| s.category == cfg.pick_from);
    if !pickable {
        return Readiness::NotPickable;
    }
    let satisfying = workflow.gate_statuses(gate);
    let exits = workflow.exit_statuses();
    let mut unsatisfied = Vec::new();
    let mut broken = Vec::new();
    for (id, blocker_status) in blockers {
        if satisfying.contains(&blocker_status.as_str()) {
            continue;
        }
        if exits.contains(&blocker_status.as_str()) {
            broken.push(*id);
        } else {
            unsatisfied.push(*id);
        }
    }
    if unsatisfied.is_empty() && broken.is_empty() {
        Readiness::Ready
    } else {
        Readiness::Blocked {
            unsatisfied,
            broken,
        }
    }
}

/// Is a claim worth respecting? A claim is advisory, never a lock: staleness
/// only unlocks takeover, it never blocks work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimLiveness {
    /// No claim, or a malformed one (an author without a timestamp, a
    /// timestamp that is not RFC3339). Nothing to respect.
    None,
    /// `claimed_at` within the TTL.
    Live,
    /// Older than the TTL: takable by another actor, renewable by its owner.
    Stale,
}

pub fn claim_liveness(
    claimed_by: Option<&str>,
    claimed_at: Option<&str>,
    now: OffsetDateTime,
    ttl_minutes: u32,
) -> ClaimLiveness {
    let (Some(_by), Some(at)) = (claimed_by, claimed_at) else {
        return ClaimLiveness::None;
    };
    let Ok(ts) = parse_rfc3339(at) else {
        return ClaimLiveness::None;
    };
    let ttl = time::Duration::minutes(i64::from(ttl_minutes));
    if now > ts + ttl {
        ClaimLiveness::Stale
    } else {
        ClaimLiveness::Live
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn blocker(suffix: char, status: &str) -> (IssueId, String) {
        // Same valid 26-char ULID shape, varied in the random tail so ids differ.
        let mut raw = "01K3M5QQQQ00000000000ZZZZW".to_string();
        raw.pop();
        raw.push(suffix);
        (IssueId::parse(&raw).unwrap(), status.to_owned())
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse(
            "2026-09-21T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    }

    #[test]
    fn todo_with_all_blockers_done_is_ready() {
        let wf = Workflow::default_workflow();
        let r = readiness(
            "todo",
            &[blocker('A', "done"), blocker('B', "done")],
            &wf,
            None,
        );
        assert_eq!(r, Readiness::Ready);
    }

    #[test]
    fn a_blocker_in_flight_keeps_the_issue_blocked() {
        let wf = Workflow::default_workflow();
        let r = readiness(
            "todo",
            &[blocker('A', "done"), blocker('B', "in_progress")],
            &wf,
            None,
        );
        match r {
            Readiness::Blocked {
                unsatisfied,
                broken,
            } => {
                assert_eq!(unsatisfied, vec![blocker('B', "").0]);
                assert!(broken.is_empty());
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn a_cancelled_blocker_is_broken_not_satisfying() {
        let wf = Workflow::default_workflow();
        // The default gate and even an Until pinned past it: an exit never
        // satisfies, and the dependent is not silently unblocked.
        for gate in [None, Some(&Gate::Until("review".into()))] {
            let r = readiness("todo", &[blocker('A', "cancelled")], &wf, gate);
            match r {
                Readiness::Blocked {
                    unsatisfied,
                    broken,
                } => {
                    assert!(unsatisfied.is_empty());
                    assert_eq!(broken, vec![blocker('A', "").0]);
                }
                other => panic!("expected Blocked, got {other:?}"),
            }
        }
    }

    #[test]
    fn until_review_admits_a_review_blocker_the_default_gate_rejects() {
        let wf = Workflow::default_workflow();
        let blockers = &[blocker('A', "review")];
        assert_eq!(
            readiness("todo", blockers, &wf, None),
            Readiness::Blocked {
                unsatisfied: vec![blocker('A', "").0],
                broken: vec![]
            }
        );
        assert_eq!(
            readiness("todo", blockers, &wf, Some(&Gate::Until("review".into()))),
            Readiness::Ready
        );
    }

    #[test]
    fn a_status_outside_pick_from_is_not_pickable() {
        let wf = Workflow::default_workflow();
        for status in ["in_progress", "review", "done", "cancelled", "unknown"] {
            assert_eq!(
                readiness(status, &[], &wf, None),
                Readiness::NotPickable,
                "status {status}"
            );
        }
    }

    #[test]
    fn no_blockers_and_todo_is_ready() {
        let wf = Workflow::default_workflow();
        assert_eq!(readiness("todo", &[], &wf, None), Readiness::Ready);
        assert_eq!(readiness("backlog", &[], &wf, None), Readiness::Ready);
    }

    #[test]
    fn liveness_respects_the_ttl_and_ignores_malformed_claims() {
        let n = now();
        // 5 minutes old, TTL 15: live.
        assert_eq!(
            claim_liveness(Some("fe-1"), Some("2026-09-21T11:55:00Z"), n, 15),
            ClaimLiveness::Live
        );
        // 20 minutes old, TTL 15: stale.
        assert_eq!(
            claim_liveness(Some("fe-1"), Some("2026-09-21T11:40:00Z"), n, 15),
            ClaimLiveness::Stale
        );
        // Exactly at the TTL boundary: still live — staleness is strictly past.
        assert_eq!(
            claim_liveness(Some("fe-1"), Some("2026-09-21T11:45:00Z"), n, 15),
            ClaimLiveness::Live
        );
        // Nothing to respect.
        assert_eq!(claim_liveness(None, None, n, 15), ClaimLiveness::None);
        assert_eq!(
            claim_liveness(Some("fe-1"), None, n, 15),
            ClaimLiveness::None
        );
        assert_eq!(
            claim_liveness(None, Some("2026-09-21T11:00:00Z"), n, 15),
            ClaimLiveness::None
        );
        // A malformed timestamp is not a claim anyone must honor.
        assert_eq!(
            claim_liveness(Some("fe-1"), Some("yesterday"), n, 15),
            ClaimLiveness::None
        );
    }
}
