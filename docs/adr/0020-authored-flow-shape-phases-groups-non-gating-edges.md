---
id: 0020
title: "Authored flow shape: phases as labels, a document fence for the shape, non-gating edges"
status: accepted
date: 2026-09-22
supersedes: null
---

## Context

ADR 0019 made the flow diagram entirely derived: membership is the only
authored fact, and stages, rows, readiness and the critical path are all
computed at read time. That was right about everything it could compute. Using
the screen showed three things derivation cannot produce, and one of them is
actively dangerous.

**1. A computed column has no name, and naming it would lie.** A stage index is
a fact about the graph — "nothing in this column depends on anything in a later
one". It is not a fact about the work. Readers want `Intake`, `Build + verify`,
`Ship`, and no algorithm can invent those words. Attaching names to computed
indices is worse than leaving them numbered: inserting one blocker mid-chain
shifts every index, and `stage 1 = Plan` silently becomes `stage 1 = Build`
with nobody having edited anything.

**2. Every arrow means "gates", so every arrow people want to draw becomes a
gate.** Real orchestrations carry arrows that pass a result rather than a
permission — "records the outcome", "returns for revision", "feeds the trace".
Today the only relation that draws is `blocked_by`, and `blocked_by` is
load-bearing: it decides `dit ready`, readiness and the critical path. Refusing
a second relation does not stop people drawing those arrows; it makes them draw
them with `blocked_by`, and from that moment `dit ready` lies to every actor
polling it. The damage lands in the coordination plane, not in the picture.

**3. There is no grouping below the lane.** A lane is a team or a surface. A
retry loop, an approval gate, an evidence path is a smaller thing inside one
lane that has its own name and its own boundary.

What must not move: `flows:` stays the only statement of membership (ADR 0019
rejected a per-flow membership file, and that rejection stands — it is about
membership, not about shape); one dependency is stated once; and nothing
derived is ever stored (I5).

What the storage layer permits is not a matter of opinion here. The frontmatter
parser, the merge driver and the index were measured rather than reasoned
about; see **Verification**. Three results shaped every option below: a new
scalar field loses one side's edit silently, a new list field merges as a clean
union, and multi-line text cannot live in frontmatter at all.

## Options considered

### Who states which phase an issue is in

| Option | Cost | Consequence |
|---|---|---|
| Nothing — keep stages computed, optionally name the indices | None | The name drifts from the work the first time the graph changes shape, and nobody is notified |
| A new `phase:` scalar field | New frontmatter vocabulary (I5 allow-list), a new merge policy written in Rust (`fields.yaml` does not exist), and it contradicts ADR 0019 on authored placement | Without that policy, two branches assigning different phases resolve silently to one — a decision disappears with no marker |
| **A reserved label, `phase/<id>`** | A label namespace DIT owns; UI must hide it from ordinary label surfaces | No new field, no schema movement, no merge policy. Two branches disagreeing produce **two labels** — visible on screen and reportable, instead of one edit vanishing |

### Where the shape (phase order, groups, arrow labels) is declared

| Option | Cost | Consequence |
|---|---|---|
| A section in `.dit/schema/workflow.yaml` | Parser + emitter work | Workspace-wide configuration for a fact that belongs to one flow; and the schema emitter has no unknown-key guarantee (I8 covers frontmatter only), so a full rewrite would drop hand-authored sections |
| Frontmatter on the issues themselves | Impossible | Multi-line text does not parse; the driver falls back to whole-file diff3, so every concurrent edit of that issue becomes a manual conflict |
| **A `dit-flow` fence in a document body** | One new index table; a new parse surface for pull-request input | Costs the file format nothing (§12.5: an info-string convention over legal CommonMark, "so §18 does not move"), merges as real line-level diff3 scoped to the body, and reads as an ordinary code block in GitHub, Obsidian or `cat` |

### How the diagram finds the fence

| Option | Cost | Consequence |
|---|---|---|
| Scan the document tree when drawing | None up front | Violates I2 — the read path would touch disk — and the cost grows with every document added |
| Path convention `docs/<something>/<flow>.md` | None | Binds a flow's name to a file path, so renaming a flow means `git mv` plus editing every member; and `docs/flows/` already means something else (§7.4, AI-generated business-flow pages) |
| **A small index table, filled at reindex** | Reindex walks the document tree; one table | I2 intact, live through the existing watcher, and the fence may sit in any document and name its own flow. Does **not** require the full document index ADR 0010 deferred — only fences are read |

### The second, non-gating relation

| Option | Cost | Consequence |
|---|---|---|
| None — keep only `blocked_by` | None | People express result-flow with `blocked_by` anyway, and readiness becomes wrong |
| A relation stated by the producer (`feeds:`) | Same as below | Points the opposite way to `blocked_by`; every reader and every call site must remember which relation runs which way, permanently |
| **A relation stated by the receiver (`fed_by:`)** | One list field | Same direction as `blocked_by`, so the index's existing reverse-map pattern is reused; merges as a clean union; never touches readiness, stages or the critical path |

## Decision

**A flow's *membership* stays derived-free and authored on the issue; a flow's
*shape* becomes authored, in one document, and nothing about either is
computed twice.**

**Phases are labels.** An issue states its phase with a reserved label,
`phase/<id>`. `labels` already merges as a set union, already round-trips, and
already lives in the I5 vocabulary, so this adds no field, no schema movement
and no merge policy. `phase/` becomes a namespace DIT owns: the label filter,
context chips and search hide it, and the Flow screen is the one place it is
shown.

**Shape lives in a `dit-flow` fence in a document body**, written in the YAML
subset `dit-parse` already speaks — no new grammar, no new tokenizer, no new
fuzz target. The fence declares only what cannot be derived:

```dit-flow
flow: register
phases:
  - { id: intake, label: Intake }
  - { id: build,  label: "Build + verify" }
  - { id: ship,   label: Ship }
groups:
  - { id: planning, label: "Planning loop", lane: backend, phases: [intake, build] }
labels:
  - { from: "#515", to: "#497", text: "record result" }
```

It names its own flow, so the document may live anywhere. It may **not** list
members (that is `flows:`), state status or dependencies (that is `status` and
`blocked_by`), or name anything to run or fetch — I7 forbids a `run:`,
`command:`, `url:` or `on_enter:` step, and a flow whose steps *do* things is
remote code execution by pull request.

**A small index table holds the parsed shapes**, filled at reindex by walking
the document tree for fences. The read path still answers from the index (I2),
and the existing watcher keeps the diagram live.

**Groups are one level deep**, scoped to exactly one lane and a span of phases.
This is the shape archify's own schema uses (`{id, label, lane, fromCol,
toCol}`). Nesting or spanning lanes turns a grid plus frames into a constraint
layout problem, which is a different class of work.

**`fed_by:` is a second, non-gating relation**, a list field stated by the
receiving issue like `blocked_by`. It draws as a neutral dashed arrow, may
carry a label from the fence, and **never** affects readiness, stage layering
or the critical path. It exists so that `blocked_by` never has to mean anything
except "this gates that".

**Stages stay computed.** This is where ADR 0019 is amended, and only here: a
flow with no fence draws exactly as it does today, by longest-path layering. A
flow with a fence draws authored phases as its columns. The critical path,
readiness, rows and every other derived fact remain derived.

**Disagreement is shown, never prevented.** Phase and `blocked_by` are both
human assertions and both legitimate, so where they contradict each other the
diagram reports rather than refuses:

- A blocker in a later phase draws a backward arrow in its own style and marks
  the node; the panel names the violation. Nothing is blocked.
- An issue with no phase label draws in a trailing **Unphased** column, the way
  an unlaned issue already draws in a trailing band.
- An issue that ended up with two phase labels (a merge of two branches does
  exactly this) draws once, in the earliest phase, marked.
- A fence that does not parse leaves the diagram drawn — falling back to
  computed stages — under a banner naming the document and line. Two fences for
  one flow: the first wins, the rest are named warnings. A typo in one document
  must never cost someone their diagram.

## Consequences

**Easier.** A flow reads as the work rather than as a graph: columns carry the
words the team uses. Arrows that pass a result no longer have to masquerade as
gates, so `dit ready` keeps meaning one thing. A group can be named without
inventing a lane for it. The fence is a plain code block, so a reviewer sees
the shape change in the pull request diff, in GitHub, without DIT installed.

**Harder.** Reindex now walks the document tree, which costs time in a large
workspace. `phase/` becomes reserved vocabulary — an existing label with that
prefix would be reinterpreted. A flow can now be *wrong* in a way it could not
be before, because two authored facts can disagree; we made that visible rather
than impossible, which means the screen must keep earning trust by reporting it
clearly. And the fence is a new parse surface for text that arrives by pull
request (§17): the YAML layer is already hardened, but every semantic check —
unknown phase id, reversed span, unknown lane — is new.

**No longer possible.** `blocked_by` can no longer be justified as "just for
drawing". Anything drawn but not gating is `fed_by`, and a reviewer may say so.

**Forward and backward.** A binary that predates this ADR reading a workspace
that uses it sees an unknown fence in a document body (ignored, rendered as a
code block) and an unrecognised label (preserved, shown as a label). It draws
computed stages — exactly today's diagram. Nothing is lost and nothing is
misread, so no schema bump is needed (§18: adding an optional field or a
configuration surface does not bump).

**Naming.** `docs/flows/` is already taken by §7.4's generated business-flow
pages; flow-shape documents must not be put there, and the fence's
self-declared `flow:` means they do not have to be.

## Verification

Every storage claim above was measured against the real merge driver
(`crates/dit-vcs/src/merge_driver.rs`) rather than reasoned about, using a
throwaway crate that depends on it. Same document, same three sides, four
candidate shapes:

```
$ cargo run -q          # probe against merge_documents(base, ours, theirs)

--- A. new scalar field `phase:` ---
  phase: build
  conflicts = []
--- B. new list field `fed_by:` ---
  fed_by: [01X, 01Y, 01Z]
  conflicts = []
--- C. block scalar in frontmatter ---
  REFUSED: the base version does not parse: line 5: `  phase intake` is not a
  `key: value` entry, comment or blank — fix the indentation
--- D. same text in the body fence ---
  ```dit-flow
  phases:
    - id: intake
  <<<<<<< ours
    - id: build
  ||||||| base
  =======
    - id: ship
  >>>>>>> theirs
  ```
  conflicts = [FieldConflict { key: "body", detail: "both sides edited the same body text" }]
```

What each result decided:

- **A** is why phases are not a new scalar field. Base `intake`, ours `build`,
  theirs `ship` resolves to `build` with **no conflict and no marker**: one
  side's decision is gone and nothing says so. This is the default for every
  scalar (`CommitOrder`), and overriding it means writing a policy in Rust —
  `.dit/schema/fields.yaml` is documented in DESIGN.md §4 but is not
  implemented anywhere.
- **B** is why `fed_by:` is a list. Both additions survive, every time, with no
  conflict — the same behaviour `flows:` and `labels:` already have.
- **C** is why the shape cannot live in frontmatter. The parser has no block
  scalar (`|`, `>`) by design, and the driver's caller turns a parse refusal
  into **whole-file** diff3 markers — so one multi-line field would make every
  concurrent edit of that issue, even to an unrelated key, a manual conflict.
- **D** is what the fence actually buys, stated precisely: **not** an automatic
  merge. Concurrent edits to the same fence region produce a real conflict —
  but a *localised* one, with both intents preserved in the body, while every
  frontmatter key on the same file merged cleanly. A conflict a human resolves
  is the correct outcome for two people redesigning the same diagram; silence
  (A) is not.

The index claim was read rather than run. There is no documents table:
`crates/dit-index/src/lib.rs:73-162` creates `issues`, `issue_assignees`,
`issue_labels`, `issue_blocked_by`, `issue_flows`, `releases`,
`release_includes`, `comments`, `field_events`, `state` — and nothing else.
`Dit::list_docs` says so outright at `crates/dit-core/src/lib.rs:1120-1127`:
"The one read here that answers from the filesystem instead of the index —
deliberately, per ADR 0010: pages have no index rows yet, so the file tree *is*
the list." That is why this ADR adds a table instead of assuming one.
