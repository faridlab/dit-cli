---
id: 0007
title: Human-friendly issue numbers live in frontmatter, never in folder names
status: accepted
date: 2026-08-18
supersedes: null
---

## Context

Users ask for `001-DIT-fix-login-timeout` — and the ask is legitimate: ULID
folder names are machine soup, and humans count things. §4.2 already rejected
sequential numbers **as folder names**: they need a central coordinator, two
offline writers create the same number (Principle 6), and folder names must
never change after creation. §4.2 left one compromise on the table: "`dit-bot`
in CI can attach `number: 123` to the frontmatter on merge into the main
branch." This ADR adopts and extends that compromise.

The load-bearing asymmetry, which is why frontmatter numbers are safe where
folder numbers are not: **a number collision is an ambiguity you repair with a
field edit; a folder collision forces a rename — and the merge driver is never
invoked for rename/modify conflicts** (§4.2, verified). Frontmatter edits are
ordinary modifications the four-layer conflict machinery (§5.3) already
handles.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Sequential folder names | Offline writers collide; bulk import (3,000 issues in seconds, §4.2) collides systematically; display coupled to an immutable name | Fails Principle 6 |
| `number:` assigned by dit-bot on merge only (§4.2 as written) | Solo/small repos never see numbers until they push | Team-safe, but withholds the feature from Mode A's likely majority |
| **`number:` at creation for single-writer repos, dit-bot for teams** | Two machines can mint the same number offline — surfaced by `dit validate`, fixed by a field edit | **Chosen** |

## Decision

`number` is an **optional** positive integer in issue frontmatter.

- **`numbering: local`** (default for single-writer Mode A repos): `dit create`
  assigns `max(existing) + 1` at creation. Two offline machines can mint the
  same number; on merge, `dit validate` flags duplicates and a human renumbers
  one — a frontmatter edit, never a rename.
- **`numbering: on-merge`** (team default): numbers are assigned by `dit-bot`
  on merge into the main branch, where merge serialization guarantees
  uniqueness. Unmerged branches simply have no number yet.

Display and lookup: the CLI and UI show `#12 Fix login timeout`; DQL accepts
`#12` and `number: 12`. The ULID short ref (`#Q2R7VN8`) remains the
offline-unique handle and is unaffected. Ordering is **never** by `number` —
`field_events` stays ordered by `seq` (invariant 9); numbers are identifiers,
not sequence.

## Consequences

- `fields.yaml` gains the optional builtin `number` field; §18 records it as a
  schema addition.
- Duplicate numbers across unmerged branches are accepted state; duplicates on
  one branch are a `dit validate` error.
- The generated index (ADR 0008) can finally list issues the way humans expect
  — by number — which is half the reason it exists.

## Verification

No new git claim — the rename/modify behavior §4.2 relies on was already
verified there. The recovery story is ordinary frontmatter editing, covered by
existing merge machinery (§5.3).
