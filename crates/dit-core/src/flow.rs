//! The flow diagram (ADR 0019): an orchestration rendered as a graph —
//! issues as nodes, `blocked_by` as edges, stages computed by longest-path
//! layering so every dependency points rightward, lanes as the horizontal
//! bands. Everything here is derived at read time from the index; membership
//! (`flows:` in the frontmatter) is the only authored fact.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use dit_index::IndexedIssue;
use dit_model::{ClaimLiveness, IssueId, Priority, Readiness, StatusCategory};

use crate::{Dit, DitError};
use time::OffsetDateTime;

/// One known flow, by name, with how many issues carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSummary {
    pub name: String,
    pub issues: usize,
}

/// The whole diagram for one flow (or the union of all of them).
#[derive(Debug, Clone, PartialEq)]
pub struct FlowBoard {
    /// The flow this board renders; None = every flow's members together.
    pub name: Option<String>,
    /// Horizontal bands in draw order; the trailing band is Unlaned.
    pub lanes: Vec<FlowLane>,
    /// Number of columns — the deepest stage plus one.
    pub stages: usize,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    /// The longest chain (by edge count), root first — the critical path.
    pub main_path: Vec<IssueId>,
}

/// One horizontal band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowLane {
    /// `None` = the Unlaned band.
    pub id: Option<String>,
    pub label: String,
}

/// One node: the issue's authored facts plus its derived placement and
/// coordination state.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowNode {
    pub id: IssueId,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    pub status_label: String,
    pub category: Option<StatusCategory>,
    pub priority: Option<Priority>,
    pub lane: Option<String>,
    /// The computed column: 0 for roots, else deepest predecessor + 1.
    pub stage: usize,
    /// Order within (lane, stage), priority-then-id — the draw row.
    pub row: usize,
    pub readiness: Readiness,
    /// Blockers that live outside this board still gate the node; they are
    /// summarized as a count rather than drawn.
    pub outside_blockers: usize,
    pub claim: Option<FlowClaim>,
}

/// A claim as the diagram shows it: who holds it and whether it is still
/// worth respecting. The age is computed, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowClaim {
    pub claimed_by: String,
    pub claimed_at: String,
    pub stale: bool,
}

/// One edge: `from` blocks `to` (the arrow of the flow, left to right).
#[derive(Debug, Clone, PartialEq)]
pub struct FlowEdge {
    pub from: IssueId,
    pub to: IssueId,
    pub disposition: EdgeDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeDisposition {
    /// The blocker is through the gate — work can flow.
    Satisfied,
    /// The blocker is still in flight.
    Unsatisfied,
    /// Cancelled or gone from the index — needs a human, never auto-clears.
    Broken,
}

impl Dit {
    /// Every flow in the workspace, most-populated first. A flow exists
    /// exactly while at least one issue carries its name.
    pub fn flows(&self) -> Result<Vec<FlowSummary>, DitError> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for hit in self.query("", None)? {
            for flow in &hit.issue.flows {
                *counts.entry(flow.clone()).or_default() += 1;
            }
        }
        let mut out: Vec<FlowSummary> = counts
            .into_iter()
            .map(|(name, issues)| FlowSummary { name, issues })
            .collect();
        out.sort_by(|a, b| b.issues.cmp(&a.issues).then_with(|| a.name.cmp(&b.name)));
        Ok(out)
    }

    /// The diagram for one flow, or for the union of every flow when `name`
    /// is None. Terminal issues stay on the board — a diagram tells the whole
    /// story of the orchestration, including what already finished; the UI
    /// dims them.
    pub fn flow_board(&self, name: Option<&str>) -> Result<FlowBoard, DitError> {
        let all = self.query("", None)?;
        let by_id: HashMap<IssueId, &IndexedIssue> = all.iter().map(|h| (h.issue.id, h)).collect();

        // Members of this board.
        let members: Vec<&IndexedIssue> = all
            .iter()
            .filter(|h| match name {
                Some(want) => h.issue.flows.iter().any(|f| f == want),
                None => !h.issue.flows.is_empty(),
            })
            .collect();
        let member_ids: BTreeSet<IssueId> = members.iter().map(|h| h.issue.id).collect();

        // Lanes: registry order first (the hint), then any lanes the data
        // carries that the registry never mentioned, Unlaned always last.
        let mut lanes: Vec<FlowLane> = self
            .workflow
            .lanes
            .iter()
            .map(|l| FlowLane {
                id: Some(l.id.clone()),
                label: l.label.clone(),
            })
            .collect();
        let mut data_lanes: BTreeSet<String> = members
            .iter()
            .filter_map(|h| h.issue.lane.clone())
            .collect();
        for lane in &lanes {
            if let Some(id) = &lane.id {
                data_lanes.remove(id);
            }
        }
        for extra in data_lanes {
            lanes.push(FlowLane {
                label: extra.clone(),
                id: Some(extra),
            });
        }
        if members
            .iter()
            .any(|h| h.issue.lane.as_deref().is_none_or(str::is_empty))
        {
            lanes.push(FlowLane {
                id: None,
                label: "Unlaned".into(),
            });
        }

        // Edges inside the board. A dependency on a non-member is outside:
        // it still gates readiness, but it does not draw here.
        let mut edges = Vec::new();
        let mut preds: BTreeMap<IssueId, BTreeSet<IssueId>> = BTreeMap::new();
        let mut succs: BTreeMap<IssueId, BTreeSet<IssueId>> = BTreeMap::new();
        for hit in &members {
            for blocker in &hit.issue.blocked_by {
                if !member_ids.contains(blocker) {
                    continue;
                }
                let disposition = match by_id.get(blocker) {
                    Some(blocker) => {
                        let status = blocker.issue.status.as_str();
                        if self
                            .workflow
                            .gate_statuses(&self.workflow.coordination.readiness.gate)
                            .contains(&status)
                        {
                            EdgeDisposition::Satisfied
                        } else if self.workflow.exit_statuses().contains(&status) {
                            EdgeDisposition::Broken
                        } else {
                            EdgeDisposition::Unsatisfied
                        }
                    }
                    None => EdgeDisposition::Broken,
                };
                preds.entry(hit.issue.id).or_default().insert(*blocker);
                succs.entry(*blocker).or_default().insert(hit.issue.id);
                edges.push(FlowEdge {
                    from: *blocker,
                    to: hit.issue.id,
                    disposition,
                });
            }
        }

        // Stages: longest-path layering, cycle-safe by capping the walk.
        let mut stage: HashMap<IssueId, usize> = HashMap::new();
        fn depth_of(
            id: IssueId,
            preds: &BTreeMap<IssueId, BTreeSet<IssueId>>,
            stage: &mut HashMap<IssueId, usize>,
        ) -> usize {
            if let Some(seen) = stage.get(&id) {
                return *seen;
            }
            // Guard against cycles: mark as visiting at depth 0 so a back
            // edge cannot recurse forever.
            stage.insert(id, 0);
            let d = preds
                .get(&id)
                .map(|ps| {
                    ps.iter()
                        .map(|p| depth_of(*p, preds, stage).saturating_add(1))
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            stage.insert(id, d);
            d
        }
        let mut ordered: Vec<IssueId> = members.iter().map(|h| h.issue.id).collect();
        ordered.sort();
        for id in &ordered {
            depth_of(*id, &preds, &mut stage);
        }
        let stages = stage.values().copied().max().unwrap_or(0).saturating_add(1);

        // Nodes, ordered urgent-first; row is the draw order within the
        // (lane, stage) cell.
        let ttl = self.workflow.coordination.claim_ttl_minutes;
        let now = OffsetDateTime::now_utc();
        let satisfying = self
            .workflow
            .gate_statuses(&self.workflow.coordination.readiness.gate);
        let exits = self.workflow.exit_statuses();
        let mut members_sorted = members.clone();
        members_sorted.sort_by_key(|h| h.issue.priority);
        let mut cell_counts: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let lane_index = |lane: Option<&str>| -> usize {
            lanes
                .iter()
                .position(|l| l.id.as_deref() == lane)
                .unwrap_or(lanes.len().saturating_sub(1))
        };
        let mut nodes = Vec::with_capacity(members_sorted.len());
        for hit in &members_sorted {
            let issue = &hit.issue;
            let lane_ix = lane_index(
                issue
                    .lane
                    .as_deref()
                    .map(str::to_owned)
                    .as_deref()
                    .filter(|l| !l.is_empty()),
            );
            let st = *stage.get(&issue.id).unwrap_or(&0);
            let row = *cell_counts.entry((lane_ix, st)).or_insert(0);
            cell_counts.insert((lane_ix, st), row + 1);

            let mut unsatisfied = Vec::new();
            let mut broken = Vec::new();
            let mut outside = 0usize;
            for blocker in &issue.blocked_by {
                let status = by_id
                    .get(blocker)
                    .map(|b| b.issue.status.as_str())
                    .map(str::to_owned);
                match status {
                    Some(status) => {
                        if satisfying.contains(&status.as_str()) {
                            // through the gate
                        } else if exits.contains(&status.as_str()) {
                            broken.push(*blocker);
                        } else {
                            unsatisfied.push(*blocker);
                        }
                    }
                    None => {
                        if member_ids.contains(blocker) {
                            broken.push(*blocker);
                        } else {
                            outside += 1;
                        }
                    }
                }
            }
            let readiness = match (
                self.workflow
                    .status(&issue.status)
                    .map(|s| s.category)
                    .unwrap_or(StatusCategory::Doing),
                unsatisfied.is_empty() && broken.is_empty(),
            ) {
                (StatusCategory::Todo, true) => Readiness::Ready,
                (StatusCategory::Todo, false) => Readiness::Blocked {
                    unsatisfied,
                    broken,
                },
                _ => Readiness::NotPickable,
            };
            let claim = match (issue.claimed_by.as_deref(), issue.claimed_at.as_deref()) {
                (Some(by), Some(at)) => Some(FlowClaim {
                    claimed_by: by.to_owned(),
                    claimed_at: at.to_owned(),
                    stale: matches!(
                        dit_model::claim_liveness(Some(by), Some(at), now, ttl),
                        ClaimLiveness::Stale
                    ),
                }),
                _ => None,
            };
            nodes.push(FlowNode {
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
                lane: issue.lane.clone(),
                stage: st,
                row,
                readiness,
                outside_blockers: outside,
                claim,
            });
        }

        // Main path: walk predecessors from the deepest node, root first.
        let mut main_path: Vec<IssueId> = Vec::new();
        if let Some(deepest) = stage
            .iter()
            .max_by_key(|(id, d)| (**d, id.as_str().to_owned()))
            .map(|(id, _)| *id)
        {
            let mut cursor = Some(deepest);
            let mut guard = 0;
            while let Some(id) = cursor {
                guard += 1;
                if guard > 10_000 {
                    break;
                }
                main_path.push(id);
                cursor = preds.get(&id).and_then(|ps| {
                    ps.iter()
                        .max_by_key(|p| (*stage.get(p).unwrap_or(&0), p.as_str().to_owned()))
                        .copied()
                });
            }
            main_path.reverse();
        }

        Ok(FlowBoard {
            name: name.map(str::to_owned),
            lanes,
            stages,
            nodes,
            edges,
            main_path,
        })
    }
}
