---
id: 0016
title: "Merge policy for the coordination fields: divergent claims conflict, dependencies union"
status: accepted
date: 2026-09-21
supersedes: null
---

## Context

ADR 0015 adds three scalar-ish fields to the issue frontmatter (`lane`,
`claimed_by`, `claimed_at`) next to the existing SET field `blocked_by`.
§5.3 assigns every field a merge policy; the policy map was designed to come
from `.dit/schema/fields.yaml` read off the merge base — but that file does
not exist and the map is never populated. `blocked_by`'s set-union already
works through the generic sequence path, which is correct.

The new fields arrive precisely in the workflow where concurrent edits are
the norm, not the exception: two lanes claiming issues on their own branches
before a sync. The default SCALAR policy is `commit_order`, which §5.3
already distrusts for critical fields.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Leave claims on the `commit_order` default | None | Two actors claim the same issue on two branches; git picks a winner silently; the loser's exclusive intent disappears without a trace — the exact "silent state" class §16.5 exists to prevent |
| Introduce `fields.yaml` now to carry the policy | A second schema file, its loader, its I7 vocabulary, base-side plumbing | A whole config surface for one block of three keys; every workspace that never syncs still carries it |
| **Builtin policies in the driver, consulted before the (future) map** | A small match in `dit-vcs` | The policy is code where every clone reads the same rule with zero config; a future `fields.yaml` can still override, since builtins are consulted first only where the map is silent |

## Decision

**Policies for the coordination fields are builtins in the merge driver.**

- `claimed_by`, `claimed_at` → **conflict** when both sides changed them
  divergently. A claim is an assertion of exclusive intent; if two actors
  asserted concurrently, that is a fact both must see, not a race to be
  resolved by commit order. Identical values (both sides renewed the *same*
  actor's claim) stay clean through the existing equal-values path.
- `lane` → the SCALAR `commit_order` default. Moving an issue between lanes
  is a rare administrative edit; two concurrent moves picking a winner is
  acceptable and visible in history.
- `blocked_by` → unchanged: SET, set-union (§5.3). Two lanes adding
  different dependencies to the same issue is additive information, and the
  generic sequence path already implements it.

## Consequences

- Two clones that claim the same issue and sync now produce a field conflict
  with markers — intended, and the pilot's acceptance test asserts it. The
  conflict is on two lines of frontmatter, the cheapest possible surface for
  a human (or a session) to resolve deliberately.
- Renews do not turn every lane sync red: the equal-values path keeps
  same-actor renews clean, pinned by test.
- When `fields.yaml` is ever built (§5.3 as written), the builtin is the
  documented default the map may override; the lookup order is
  `builtin_policy(key).or_else(|| ctx.policies.get(key))`.

## Verification

Pinned by `dit-vcs` merge-driver tests constructing the three sides inline
(base without a claim; ours and theirs claiming different actors): the
result carries conflict markers on `claimed_by` and reports the field
conflict, while the same-actor renew variant merges clean, a one-sided
claim travels without conflict, and `lane` divergence still resolves by
commit order.
