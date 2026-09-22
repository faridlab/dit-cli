//! The flow diagram (ADR 0019): an orchestration rendered as a graph —
//! issues as nodes, `blocked_by` as edges, stages computed by longest-path
//! layering so every dependency points rightward, lanes as the horizontal
//! bands. Everything here is derived at read time from the index; membership
//! (`flows:` in the frontmatter) is the only authored fact.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use dit_index::IndexedIssue;
use dit_model::{
    ClaimLiveness, FlowGroup, FlowPhase, IssueId, Priority, Readiness, StatusCategory,
};

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
    /// The authored columns (ADR 0020), empty when this flow has no fence —
    /// in which case `stage` is the computed longest-path rank, as before.
    pub phases: Vec<FlowPhase>,
    /// Frames inside a lane, spanning a run of phases.
    pub groups: Vec<FlowGroup>,
    /// True when at least one member claims no phase and a trailing
    /// "Unphased" column is therefore drawn.
    pub unphased: bool,
    /// Why the fence could not be used, when there is one and it could not.
    /// The diagram still draws; this is what the screen reports over it.
    pub shape_problem: Option<FlowShapeProblem>,
    /// Number of columns.
    pub stages: usize,
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    /// The longest chain (by edge count), root first — the critical path.
    pub main_path: Vec<IssueId>,
}

/// A fence that is there but unreadable. Named rather than swallowed: a
/// typo in one document must never cost someone their diagram, and it must
/// never silently look like "this flow has no phases".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowShapeProblem {
    /// The document the fence lives in.
    pub path: String,
    /// The fence's opening line.
    pub line: usize,
    pub detail: String,
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
    /// The issue's type (`task` | `bug` | …) — one of the dimensions the
    /// screen can paint by without any new authored data.
    pub kind: dit_model::IssueKind,
    pub status: String,
    pub status_label: String,
    pub category: Option<StatusCategory>,
    pub priority: Option<Priority>,
    pub lane: Option<String>,
    /// The column this node draws in: the authored phase when the flow has a
    /// fence, otherwise the computed longest-path rank.
    pub stage: usize,
    /// Every phase this issue's labels claim, in label order. More than one
    /// is legal — a merge of two branches produces exactly that — and the
    /// node still draws once, in the earliest.
    pub phases: Vec<String>,
    /// Order within (lane, stage), priority-then-id — the draw row.
    pub row: usize,
    pub readiness: Readiness,
    /// Blockers that are not members of this board. They still gate the
    /// node, but no arrow can draw them, so they are named instead — the
    /// diagram would otherwise show a ready node that nothing can start.
    pub outside_blockers: Vec<FlowOutsideBlocker>,
    pub claim: Option<FlowClaim>,
    /// How many commits have touched this issue. Derived from the recorded
    /// history, never authored: the diagram can show which nodes have real
    /// work behind them without anyone maintaining a link.
    pub commits: usize,
}

/// A blocker the board cannot draw, named so the reader can follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowOutsideBlocker {
    pub id: IssueId,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status_label: String,
    /// Through the gate already — it no longer holds the node.
    pub satisfied: bool,
    /// Not in the index at all: a dangling reference that needs a human.
    pub gone: bool,
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
    /// True for `blocked_by`, false for `fed_by`. A non-gating arrow draws
    /// and may carry a caption, and touches nothing derived — not readiness,
    /// not the stage layering, not the critical path.
    pub gating: bool,
    /// The blocker sits in a *strictly* later column than the issue it
    /// blocks, so the arrow runs against the authored order. Reported, never
    /// refused: both facts are human assertions and the contradiction is the
    /// finding. A dependency inside one phase is not a contradiction.
    pub backward: bool,
    /// What the fence says this arrow means, if it says anything.
    pub label: Option<String>,
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
                    gating: true,
                    // Both filled in once the columns are known.
                    backward: false,
                    label: None,
                });
            }
        }

        // Non-gating arrows (ADR 0020). They are added after the predecessor
        // and successor maps are built, and deliberately never enter them:
        // `preds` is what the stage layering and the critical path read, and
        // a `fed_by` must not move either of them.
        for hit in &members {
            for feeder in &hit.issue.fed_by {
                if !member_ids.contains(feeder) {
                    continue;
                }
                edges.push(FlowEdge {
                    from: *feeder,
                    to: hit.issue.id,
                    disposition: EdgeDisposition::Unsatisfied,
                    gating: false,
                    backward: false,
                    label: None,
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
        let computed_stages = stage.values().copied().max().unwrap_or(0).saturating_add(1);

        // The authored shape (ADR 0020). The union of every flow has none by
        // definition — it spans flows, and a column named by one of them
        // would be a claim about the others.
        let shaped = match name {
            Some(name) => self.flow_shape(name)?,
            None => None,
        };
        let shape = shaped.as_ref().and_then(|s| s.shape.clone());
        let shape_problem = shaped.as_ref().and_then(|s| {
            s.problem.as_ref().map(|detail| FlowShapeProblem {
                path: s.path.clone(),
                line: s.line,
                detail: detail.clone(),
            })
        });

        // Nodes, ordered urgent-first; row is the draw order within the
        // (lane, stage) cell.
        let commits = self.index.commit_counts()?;
        let ttl = self.workflow.coordination.claim_ttl_minutes;
        let now = OffsetDateTime::now_utc();
        let satisfying = self
            .workflow
            .gate_statuses(&self.workflow.coordination.readiness.gate);
        let exits = self.workflow.exit_statuses();
        let mut members_sorted = members.clone();
        members_sorted.sort_by_key(|h| h.issue.priority);
        let lane_index = |lane: Option<&str>| -> usize {
            lanes
                .iter()
                .position(|l| l.id.as_deref() == lane)
                .unwrap_or(lanes.len().saturating_sub(1))
        };
        let mut nodes = Vec::with_capacity(members_sorted.len());
        for hit in &members_sorted {
            let issue = &hit.issue;
            // A fence turns the columns into authored phases; without one
            // the computed rank stands, exactly as it did before ADR 0020.
            let claims: Vec<String> = dit_model::phases_of(&issue.labels)
                .into_iter()
                .map(str::to_owned)
                .collect();
            let st = match &shape {
                Some(shape) => shape.column_of(&issue.labels).unwrap_or(shape.phases.len()),
                None => *stage.get(&issue.id).unwrap_or(&0),
            };

            let mut unsatisfied = Vec::new();
            let mut broken = Vec::new();
            let mut outside = Vec::new();
            for blocker in &issue.blocked_by {
                let known = by_id.get(blocker);
                let satisfied = known
                    .map(|b| satisfying.contains(&b.issue.status.as_str()))
                    .unwrap_or(false);
                match known {
                    Some(_) if satisfied => {
                        // through the gate
                    }
                    Some(b) if exits.contains(&b.issue.status.as_str()) => broken.push(*blocker),
                    Some(_) => unsatisfied.push(*blocker),
                    // A reference the index cannot resolve: it never clears
                    // on its own, so it is broken, not merely unsatisfied.
                    None => broken.push(*blocker),
                }
                // Anything that is not a member of this board cannot draw an
                // arrow here, so the node carries it as named context.
                if !member_ids.contains(blocker) {
                    outside.push(FlowOutsideBlocker {
                        id: *blocker,
                        short_ref: blocker.short_ref().as_str().to_owned(),
                        number: known.and_then(|b| b.issue.number),
                        title: known
                            .map(|b| b.issue.title.clone())
                            .unwrap_or_else(|| blocker.short_ref().as_str().to_owned()),
                        status_label: known
                            .and_then(|b| self.workflow.status(&b.issue.status))
                            .map(|s| s.label.clone())
                            .unwrap_or_else(|| "gone".into()),
                        satisfied,
                        gone: known.is_none(),
                    });
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
                kind: issue.kind,
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
                phases: claims,
                // Filled in below, once every node's cell is known.
                row: 0,
                readiness,
                outside_blockers: outside,
                commits: commits.get(issue.id.as_str()).copied().unwrap_or(0),
                claim,
            });
        }

        // Rows: the draw order inside a (lane, stage) cell. Priority alone
        // makes the arrows cross whenever a successor's priority disagrees
        // with its predecessor's row, so each stage is ordered by the mean
        // row of its predecessors (the barycenter heuristic) and priority
        // only breaks the tie. Stage 0 has no predecessors, so it stays
        // priority-ordered.
        let node_ix: HashMap<IssueId, usize> =
            nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let lane_of: Vec<usize> = nodes
            .iter()
            .map(|n| lane_index(n.lane.as_deref().filter(|l| !l.is_empty())))
            .collect();
        // A node's vertical place, coarse enough that the lane dominates:
        // a cross-lane predecessor pulls toward its lane, not past it.
        let vpos = |lane_ix: usize, row: usize| (lane_ix * 1024 + row) as f64;
        // However the columns were decided — computed rank or authored
        // phase — the rows are ordered column by column, left to right.
        let columns = nodes.iter().map(|n| n.stage).max().map_or(0, |m| m + 1);
        for s in 0..columns {
            let mut cells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for (i, n) in nodes.iter().enumerate() {
                if n.stage == s {
                    cells.entry(lane_of[i]).or_default().push(i);
                }
            }
            for (_, cell) in cells {
                let mut ranked: Vec<(Option<f64>, usize)> = cell
                    .iter()
                    .map(|&i| {
                        let bars: Vec<f64> = preds
                            .get(&nodes[i].id)
                            .map(|ps| {
                                ps.iter()
                                    .filter_map(|p| node_ix.get(p))
                                    .filter(|&&j| nodes[j].stage < s)
                                    .map(|&j| vpos(lane_of[j], nodes[j].row))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let bary = if bars.is_empty() {
                            None
                        } else {
                            Some(bars.iter().sum::<f64>() / bars.len() as f64)
                        };
                        (bary, i)
                    })
                    .collect();
                // `cell` already arrives in priority-then-creation order, so a
                // stable sort keeps that as the tie-break. Nodes the
                // barycenter cannot place (only reachable under a dependency
                // cycle) keep their priority order at the bottom of the cell.
                ranked.sort_by(|a, b| match (a.0, b.0) {
                    (Some(x), Some(y)) => x.total_cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                });
                for (row, (_, i)) in ranked.into_iter().enumerate() {
                    nodes[i].row = row;
                }
            }
        }

        // Now that every node has a column, the arrows can be read against
        // it: a blocker sitting in a later column than what it blocks means
        // the authored order and the dependency disagree. That is a finding
        // for the reader, not an error for the writer, so it is marked.
        let column: HashMap<IssueId, usize> = nodes.iter().map(|n| (n.id, n.stage)).collect();
        for edge in &mut edges {
            let (Some(from), Some(to)) = (column.get(&edge.from), column.get(&edge.to)) else {
                continue;
            };
            // Strictly later, not merely equal: two issues in the same phase
            // depending on each other is ordinary — a phase groups work, it
            // does not forbid an order inside the group. Flagging those would
            // bury the real contradictions in noise.
            edge.backward = edge.gating && from > to;
        }

        // Arrow captions from the fence. Its two ends are written the way
        // people write them — `#12` or a short ref — so they resolve through
        // the same ambiguity-rejecting lookup everything else uses; a pair
        // that does not resolve is simply not captioned.
        if let Some(shape) = &shape {
            for label in &shape.labels {
                let (Ok(from), Ok(to)) = (self.resolve(&label.from), self.resolve(&label.to))
                else {
                    continue;
                };
                for edge in &mut edges {
                    if edge.from == from && edge.to == to {
                        edge.label = Some(label.text.clone());
                    }
                }
            }
        }

        // Main path: the chain that still has the most work left in it.
        // Depth alone would nominate a finished chain, which tells the
        // reader nothing about when this flow lands; the count of nodes that
        // are neither done nor abandoned does. Ties fall back to the longer
        // chain, then to the id, so the answer is stable between reads.
        let unfinished = |id: &IssueId| -> usize {
            match by_id.get(id) {
                Some(hit) => {
                    let status = hit.issue.status.as_str();
                    let done = exits.contains(&status)
                        || self
                            .workflow
                            .status(status)
                            .map(|s| s.category == StatusCategory::Done)
                            .unwrap_or(false);
                    usize::from(!done)
                }
                None => 0,
            }
        };
        // (remaining, hops, the predecessor that produced them).
        type Chain = (usize, usize, Option<IssueId>);
        fn best_chain(
            id: IssueId,
            preds: &BTreeMap<IssueId, BTreeSet<IssueId>>,
            unfinished: &dyn Fn(&IssueId) -> usize,
            memo: &mut HashMap<IssueId, Chain>,
        ) -> Chain {
            if let Some(seen) = memo.get(&id) {
                return *seen;
            }
            // Cycle guard, the same shape as the layering walk: claim the
            // node before recursing so a back edge terminates.
            memo.insert(id, (unfinished(&id), 0, None));
            let mut best: Chain = (unfinished(&id), 0, None);
            if let Some(ps) = preds.get(&id) {
                for p in ps {
                    let (remaining, hops, _) = best_chain(*p, preds, unfinished, memo);
                    let candidate = (remaining + unfinished(&id), hops + 1, Some(*p));
                    let rank = |c: &Chain| {
                        (
                            c.0,
                            c.1,
                            c.2.map(|from| from.as_str().to_owned()).unwrap_or_default(),
                        )
                    };
                    if rank(&candidate) > rank(&best) {
                        best = candidate;
                    }
                }
            }
            memo.insert(id, best);
            best
        }
        let mut memo: HashMap<IssueId, Chain> = HashMap::new();
        let mut main_path: Vec<IssueId> = Vec::new();
        let mut tip: Option<(usize, usize, IssueId)> = None;
        for id in &ordered {
            let (remaining, hops, _) = best_chain(*id, &preds, &unfinished, &mut memo);
            let candidate = (remaining, hops, *id);
            if tip
                .map(|(r, h, t)| (remaining, hops, id.as_str()) > (r, h, t.as_str()))
                .unwrap_or(true)
            {
                tip = Some(candidate);
            }
        }
        if let Some((_, _, tip)) = tip {
            let mut cursor = Some(tip);
            let mut guard = 0;
            while let Some(id) = cursor {
                guard += 1;
                if guard > 10_000 {
                    break;
                }
                main_path.push(id);
                cursor = memo.get(&id).and_then(|(_, _, from)| *from);
            }
            main_path.reverse();
        }

        // With a fence, the columns are the phases plus a trailing Unphased
        // band for members nobody has placed yet — the same courtesy the
        // Unlaned band already extends, and the reason a workspace's first
        // fence does not make most of its issues disappear.
        let unphased = shape
            .as_ref()
            .map(|shape| nodes.iter().any(|n| n.stage >= shape.phases.len()))
            .unwrap_or(false);
        let stages = match &shape {
            Some(shape) => shape.phases.len() + usize::from(unphased),
            None => computed_stages,
        };

        Ok(FlowBoard {
            name: name.map(str::to_owned),
            lanes,
            phases: shape.as_ref().map(|s| s.phases.clone()).unwrap_or_default(),
            groups: shape.as_ref().map(|s| s.groups.clone()).unwrap_or_default(),
            unphased,
            shape_problem,
            stages,
            nodes,
            edges,
            main_path,
        })
    }
}
