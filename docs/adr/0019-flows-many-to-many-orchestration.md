---
id: 0019
title: "Flows: many-to-many orchestration membership, blocked_by as the diagram's edges"
status: accepted
date: 2026-09-22
supersedes: null
---

## Context

ADR 0015 gave a workspace one static grouping axis: `lane:`, a single value per
issue, rendered as swimlane rows. Two things it got wrong in real use:

1. **Lanes were one axis, orchestration needs another.** Several orchestrations
   run at the same time (a launch, an audit, a migration), and the same issue
   belongs to more than one of them at once. A single-valued field cannot say
   that, and a workspace-wide swimlane board cannot show one orchestration at
   a time.
2. **The kanban-shaped screen duplicated the board.** Showing the same status
   columns again, arranged by lane, answered no question the board could not;
   what an orchestration actually needs to see is the *flow*: what blocks
   what, in what order the work can proceed, and where the critical path runs.
   Archify (github.com/tt-a1i/archify) demonstrates the shape users expect:
   lanes as horizontal bands, stages as columns, orthogonal edges, a
   highlighted main path — a diagram, not a board with sections.

The constraint: `blocked_by` already exists as the dependency relation, and it
must be reused, not duplicated — one dependency, stated once, visible in every
orchestration the issue belongs to.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Keep `lane:` as the only grouping, add board filters | None | Still one axis; cannot express concurrent orchestrations or shared membership |
| A membership file per flow (`flows/<name>.md` listing issues) | A second source of truth for the same fact | Membership merges badly and drifts from the issue files; `dit issue set` cannot touch it surgically |
| **A `flows:` list in the issue frontmatter** | One more list field, one side table | Membership is authored where the issue is, merges as a set like `labels`, filters in DQL, and an issue joins any number of flows |

## Decision

**A flow is an orchestration with no file of its own.** Its identity is the
string members carry in their frontmatter (`flows: [hr-launch, audit-2027]`);
its lanes are whatever those issues' `lane:` values say (free-form,
charset-validated; the workflow.yaml registry remains purely an ordering and
label hint, never a gate); its edges are the members' `blocked_by` entries
(blocker → dependent); its stages are computed, not authored — the
longest-path rank over the flow's edges, so a dependency always points
rightward; its main path is the longest chain. All derived, nothing stored
beyond membership (invariant I5 intact).

- **Field**: `flows: Vec<String>` on `Issue` and `FieldPatch`, set-replace like
  `labels`, cleared with `flows=`; merge policy SET (union) as with the other
  list fields. DQL gains `flow = <name>` over a side table.
- **API**: `GET /api/flow` (every flow with counts) and `GET /api/flow/{name}`
  (nodes, edges, stages, lanes, main path, per-node readiness and claim) —
  this replaces `/api/workflow`, which duplicated the board.
- **UI**: the Workflow swimlane screen is replaced by the Flow screen — an
   archify-style SVG diagram: lanes as horizontal bands, computed stages as
   columns, orthogonal edges (dashed for broken dependencies, muted for
   satisfied ones, emphasized on the main path), a flow selector, live via the
   existing event channel. The classic board gains a lane filter, which is
   the only thing the swimlane screen did that the board lacked.
- **CLI**: `dit flow list`, `dit flow show <name>` (stage-by-stage text tree,
  ready/blocked marks — the polling surface for headless sessions);
  membership through `dit issue set '#N' flows=a,b`. `dit workflow init`
  stops defaulting to `backend,frontend` lanes — lane names are examples, not
  vocabulary; nothing in the product may hardcode them.

## Consequences

- An issue in two flows renders as the same node in two diagrams, each
  computing its own stages from the edges that exist inside that flow; a
  dependency held with a non-member still counts for readiness, it just does
  not draw inside that flow.
- Unlaned members form a trailing band; a flow with no members yet does not
  exist (there is no empty-flow file to keep alive).
- The kanban-with-sections screen and `/api/workflow` are gone; anything that
  needed lane-shaped grouping uses the board's new lane filter.
- Naming is glossary-clean: `workflow.yaml` remains the status machine; an
  orchestration is always a *flow*.
