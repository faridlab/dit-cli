---
id: 0009
title: "`dit renumber` backfills numbers append-only, never shifting an existing one"
status: accepted
date: 2026-08-18
amends: 0007
---

## Context

ADR 0007 assigned `number:` **going forward** — at creation under `numbering:
local`, on merge under `on-merge`. That leaves two classes of issue permanently
unnumbered:

- every issue in a workspace that predates ADR 0007 (the serpa-dit pilot: 30
  issues, zero numbers), and
- every issue created before a workspace switched `on-merge` → `local`.

They display as bare short refs — correct, but the `#N` sugar and the
generated index's number ordering (ADR 0008) never engage for them, and the
older an issue is, the more a human wants to say "what's #3?" about it.

A backfill assigns numbers to issues that already exist. That is a different
act from assigning at creation, because the numbers it picks can collide with
references people have already made.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| No backfill | None | Legacy issues stay handle-less forever; the pilot — the flagship for the root layout — never shows a `#N` |
| Full chronological renumber (all issues become 1..N by creation) | Number correlates with age | Every existing `#N` in a comment, commit message, or a teammate's memory silently re-points to a different issue — the exact harm class invariant 6 exists to prevent |
| **Append-only backfill** | In mixed workspaces the oldest issues can carry the highest numbers | **Chosen** — nothing that already points somewhere moves |

## Decision

`dit renumber` assigns numbers to unnumbered issues:

- **Append-only.** Unnumbered issues receive `max(existing) + 1`, `+2`, …
  An existing number is never moved; renumbering someone else's handle is the
  one thing this command must not do. In the common case — a legacy workspace
  with no numbered issues at all — the result is `#1..#N` in creation order,
  which is what everyone expects anyway.
- **Creation order** within the unnumbered set: ULID ascending. A ULID's
  leading bits are its timestamp and Crockford base32 preserves order (ADR
  0001), and minting is monotonic: ids minted inside one transaction share
  that transaction's single clock reading, and each mint steps above the
  previous id (through the short-ref window, so folder and comment file
  names move with it) instead of drawing fresh entropy. Id-string order is
  therefore creation order — not by encoding alone, which shuffles a
  same-millisecond burst, but because minting never emits an id that sorts
  below one minted before it. No wall clock is consulted.
- **Same cursor as creation.** The sequence continues from the same
  `max(living) + 1` the creation path uses (ADR 0007). Consequence inherited
  from there, stated plainly: deleting the highest-numbered issue frees its
  number and the next assignment reuses it. Backfill adds no new reuse; it
  shares the existing rule. A true high-water mark would be stored derived
  state — a different decision, not smuggled in here.
- **One commit, clean tree required.** Every number lands as a single
  `dit renumber` commit — reviewable and revertible as a unit — so the working
  tree must be clean first, exactly like `migrate-layout`.
- **Local numbering only.** On `numbering: on-merge` the command refuses:
  merge serialization owns assignment there, and a local backfill would bypass
  the uniqueness guarantee the policy exists to provide.

## Consequences

- Each backfilled issue gets an ordinary `number: - -> N` field event and an
  `updated` bump — the assignment is visible in history and revertible with
  the tools every other field edit has. 30 issues means 30 events, one commit.
- `dit doctor` reports unnumbered issues under `numbering: local` with the
  command as the way out (`renumber` code, `Warn` level — degraded display,
  not data risk).
- The facade method is `Dit::renumber() -> Result<usize, DitError>` (count of
  issues numbered; `Ok(0)` = nothing to do, no commit). Number assignment
  stays facade-owned: neither the server nor the CLI picks numbers, same as
  creation (ADR 0007).
- Follow-up, not in scope: a "Backfill existing issues" button in the
  settings panel calling the same facade method.

## Verification

No new git claim — the mechanism is ordinary frontmatter edits through the
existing Transaction + merge machinery (§5.3), already verified. The ordering
claim (ULID string order == creation order) needs more than the encoding ADR
0001 adopted Crockford base32 for: the encoding alone sorts a
same-millisecond burst randomly, which is exactly how the original backfill
fixture caught this ADR overclaiming. `IssueId::from_parts_after` (dit-model)
plus the per-transaction mint cursor (dit-store) close the gap, with fixtures
on both sides — `ids_minted_in_one_transaction_keep_creation_order`,
`comments_added_in_one_transaction_read_back_in_addition_order`, and this
ADR's own backfill test. The indexer's month shards already rely on the
timestamp half of the property. The pilot is itself the end-to-end fixture:
30 unnumbered issues → `dit renumber` → `#1..#30` in creation order, one
commit.
