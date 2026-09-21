---
id: 0015
title: "Lanes, claims, and derived readiness: DIT as a coordination plane for parallel actors"
status: accepted
date: 2026-09-21
supersedes: null
---

## Context

DIT is increasingly driven by several parallel actors at once — humans, and AI
sessions running `dit` in separate processes (one per work stream: a backend
lane, a frontend lane). Today the coordination between them lives in prose:
`blocked by #12` in a body paragraph, "who is working on this" in nobody's
memory. Each actor that has to wait has no machine-readable signal for when to
start, and two actors can pick the same issue without ever noticing.

The constraints in play: Principle 3 / invariant I5 (nothing computable is
stored), invariant I7 (no workspace field may name an executable or URL —
coordination must not smuggle in hooks), and §4.5 (workflow.yaml is the
per-workspace process definition, already loaded by the facade and the merge
driver). `blocked_by` already exists end-to-end (§4.3, the index side table,
the merge driver's SET policy); what is missing is the rest of the
coordination vocabulary and any derivation on top of it.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Do nothing; actors coordinate out of band (chat, memory) | None | Every handoff costs each actor its context; waiting is either busy-polling by hand or stale knowledge |
| An orchestrator service assigns work and routes messages | A daemon, an API, a new trust boundary | Violates local-first; a crashed brain stalls every lane; the state it holds drifts from git |
| **Authored coordination fields on the issue + derivations computed at read time** | Three frontmatter keys, one workflow.yaml block, index/query work | Git stays the only coordination state; any actor can participate with the CLI alone |

## Decision

**Coordination state is authored on issues; every judgment about it is derived.**

Three new *authored* frontmatter fields (an actor's assertion, the same class
as `assignees` — not computable from git, so invariant I5 is untouched):

- `lane` — the work stream an issue belongs to. Values come from a registry;
  an issue without `lane` is valid and renders as *Unlaned*.
- `claimed_by` — the actor (`DIT_ME`/`--me` alias) asserting exclusive intent
  to work this issue.
- `claimed_at` — RFC3339 timestamp of the assertion. Written by `dit claim`
  only, never at creation.

Everything else is computed and never stored: *readiness* (is this issue
pickable now), *gate satisfaction* (has each blocker reached the required
status), *claim liveness* (is `claimed_at` within the TTL).

The registry and the tuning knobs live in `.dit/schema/workflow.yaml` (§4.5),
because a lane list is team process, not machinery, and the file is already
read by every component that needs it:

```yaml
lanes:
  - { id: backend,  label: Backend,  owners: [be-1] }
  - { id: frontend, label: Frontend, owners: [fe-1] }
coordination:
  claim_ttl_minutes: 15
  readiness:
    pick_from: todo    # status category an issue must sit in to be pickable
    gate: terminal     # what a blocker must have reached
```

Readiness derivation, stated once (dit-model, one engine — the same principle
as DQL §6.4):

- An issue is **ready** when its status is in the `pick_from` category and
  every `blocked_by` entry has reached the gate.
- The default gate `terminal` means the terminal completion status (`done`).
  A gate naming a status id means "that status or later" in declaration
  order — `--until review` is the per-call escape hatch, never written back.
- **A cancelled blocker never satisfies any gate.** It is reported as a
  *broken* dependency: the waiting issue stays blocked and the board shows
  why. The remedy is a human one — remove or re-point the `blocked_by` entry,
  an edit that is recorded in git — not a silent auto-unblock onto a
  dependency that was deliberately abandoned. (Cancelled is still terminal
  for the cancelled issue itself; this rule is only about what it does to its
  dependents.)

`dit claim` is the guardrail: it refuses to claim an issue in a terminal
status, an issue with unsatisfied or broken blockers, or an issue another
actor holds a live claim on; `--force`, `--takeover` and `--renew` are the
named ways out. Status writes gain membership validation against
workflow.yaml (today only a charset is checked, §4.5's admitted limit), with
`--force` as the escape.

`dit workflow init` scaffolds the registry and a marked protocol section in
the workspace's CLAUDE.md, idempotently: it appends the `lanes:`/
`coordination:` blocks only when those top-level keys are absent, and
replaces only the text between the `dit:workflow-protocol` markers. Hand
edits and comments survive, the same policy as templates (§4.3).

## Consequences

- Readiness queries need each blocker's status: an index join, cheap at the
  §8 target scale, and read-only — no write amplification.
- Claim liveness uses the wall clock of the *reader*. That is acceptable
  because a claim is advisory, not a lock: staleness only unlocks takeover,
  it never blocks work. The TTL is a workspace-committed integer, so every
  lane reads the same rule.
- `tests/invariants.rs` grows three authored keys (`KNOWN_ISSUE_KEYS`); the
  list's comment carries the authored-state justification so the next reader
  does not mistake them for I5 violations.
- Old binaries ignore the new workflow.yaml keys (unknown top-level keys are
  skipped by `parse_workflow`), so a mixed-version team degrades to
  "no coordination semantics", never to corruption.
- DQL gains `lane =` / `lane IN (...)`. `blocked_by` stays unfilterable in
  DQL v1 (it is a join, not a column predicate); `dit ready` is the
  dedicated surface.

## Verification

`parse_workflow` skipping unknown top-level keys is what makes the
forward-compatible claim safe; it is pinned by `dit-parse` schema tests (a
legacy workflow.yaml without the new blocks parses to empty lanes and
default coordination, and a file with lanes and coordination round-trips).
Claim liveness and gate semantics are pure functions with an injected clock,
pinned in `dit-model` tests: a claim 20 minutes old at TTL 15 is stale; a
cancelled blocker yields `broken`, not readiness.
