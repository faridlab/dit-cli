---
id: 0018
title: "Reference resolution rejects ambiguity instead of silently picking the first hit"
status: accepted
date: 2026-09-21
supersedes: null
---

## Context

Every CLI and API reference — `#12`, a 7-char short ref, a full ULID — lands
in `Dit::get` (dit-core), which today takes `issues_with_number(n)?.next()`
and, for short refs, compiles a `limit 2` query and still takes `.next()`:
**duplicate numbers and duplicate short refs resolve silently to the first
hit.** Duplicates are not hypothetical; a real workspace in daily use has
five colliding numbers (a known CLI-era defect, flagged by `dit doctor`).

The coordination features make this worse, not better: `dit claim #12`, a
`blocked_by` entry, an `epic` link — all of them *act on* the resolved
issue. Acting on the wrong issue silently is a data-loss class of bug, the
same family §16.5 refuses to call an error.

Related: `epic=` is unusable from the CLI today because the patch parser
hands the raw string to `IssueId::parse`, which accepts only the full
26-char ULID. `#12` and short refs were never resolvable at that seam.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Keep first-hit resolution | None | One in a thousand commands edits, claims, or links the wrong issue with no trace — and coordination commands act on the result |
| Refuse duplicates at write time everywhere | Re-numbering existing data | ADR 0009 territory: numbers are append-only and never move; refusing writes strands workspaces that already carry duplicates |
| **Resolve every reference with ambiguity rejection that names the candidates** | A facade method, an error variant | Ambiguous commands fail loudly once, with everything needed to disambiguate; unambiguous refs behave exactly as before |

## Decision

**`Dit::resolve(needle) -> Result<IssueId, DitError>`** walks the same ladder
as `get` (full id → short ref → `#number`) but rejects ambiguity: more than
one issue holding the number, or the short-ref query returning two rows,
yields `DitError::Ambiguous { needle, candidates }` whose message lists each
candidate's full id and title and suggests the short ref or full id. `get`
keeps its display semantics; every *acting* path (CLI reference, `epic=`,
`blocked_by=`, `claim`, the server's issue resolver) switches to `resolve`.
Ambiguity maps to CLI exit 2 (NotFound's class: a human must restate the
reference) and HTTP 409 with the candidate list.

`#N` stays display sugar (ADR 0007); the short ref remains the script-safe
handle. The `epic=` seam stops requiring full ULIDs: reference-typed patch
values are collected raw and resolved through the facade after open, so
`epic=#12`, `epic=Q2R7VN8` and `blocked_by=#12,#13` all work, and
comma-separated `blocked_by=` sets each resolve or fail individually with
the field named.

## Consequences

- A command that used to "work" by accident now fails loudly on the five
  real collisions. That is the correct trade: the failure names both
  candidates, and the caller retries with a short ref — the same loop a
  human runs after `dit doctor` flags duplicates.
- No migration, no renumbering: resolution is read-time behavior.
- Server PATCH paths that patch by ambiguous reference change from
  "patched the wrong issue" to 409 — strictly better, and now observable.

## Verification

Pinned by `dit-core` tests building a workspace with two issues sharing
`number: 285`: `resolve("#285")` returns `Ambiguous` naming both ULIDs and
titles; a unique short ref picks the right issue; `epic=#N` and
`blocked_by=#N,#M` resolve end-to-end through the CLI patch path in the
integration tests.
