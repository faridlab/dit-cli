---
id: 0021
title: "dit ai: one canonical agent document, thin pointers, spec printed by the binary"
status: accepted
date: 2026-09-22
supersedes: null
---

## Context

ADR 0015 designed the coordination plane around actors that are "human or AI
session" as equals. That assumption has a gap nothing fills: an AI session
arriving in a DIT workspace knows nothing about DIT. What it does next is
predictable, and every item is something DIT's invariants forbid — editing an
issue file with `sed` instead of going through a transaction, inventing a
frontmatter key, writing a derived fact into the file, treating a merge
conflict as an error. The rules exist and are enforced; nothing tells the agent
they exist before it breaks one.

Half a mechanism already ships. `dit workflow init` writes a marker-delimited
block into the workspace's `CLAUDE.md`, idempotently, touching only the text
between `<!-- dit:workflow-protocol -->` and its closing marker so hand-written
rules survive byte-for-byte (`crates/dit-core/src/lib.rs:2268-2310`). The
pattern is right. Its scope is not:

- It teaches the coordination protocol only — claims, lanes, `dit ready` —
  not what DIT is, how issues are written, or (once ADR 0020 lands) how a flow
  is shaped.
- It writes one file, for one vendor, and nothing else knows about it.
- It is a side effect of a command about `workflow.yaml`, so updating agent
  rules means running something that also rewrites schema.
- What it actually emits is damaged; see **Verification**.

Two forces shape the answer. A workspace pinned to a copied specification
freezes at the version it was copied from, and nothing notices when it starts
lying — DIT is a versioned binary, upgraded independently of the workspaces it
serves. And writing full rules into `CLAUDE.md`, `AGENTS.md`, `.cursor/rules`
and `.github/copilot-instructions.md` is four sources of truth for one fact,
which is the thing DIT refuses everywhere else.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Extend the existing `dit workflow init` block | None | One command acquires a second, unrelated reason to be run; updating agent rules touches `workflow.yaml` |
| Document a block for people to paste | None | Nothing keeps it matching the binary, and most workspaces never get one |
| **A `dit ai` command** | One command surface | Agent onboarding is its own thing, runnable on its own, and absorbs the existing protocol block |

| Option | Cost | Consequence |
|---|---|---|
| Write full rules into every tool file | N copies | They drift; every sentence change is a noisy diff in four places |
| Support `CLAUDE.md` only | None | Locks the design to one vendor; the next tool reopens it |
| **One canonical document + thin pointer blocks** | One document, N four-line blocks | One source of truth; supporting another tool adds a pointer, not a copy |

| Option | Cost | Consequence |
|---|---|---|
| Copy the whole specification into the workspace | None | Frozen at the version it was written by, with nothing to notice |
| Point at `dit ai spec` and nothing else | None | An agent that cannot run commands, or does not think to, learns nothing |
| **Both: durable rules inline, `dit ai spec` for the full grammar** | Two surfaces to keep coherent | An agent works from the file alone, and reaches for the binary when it needs the version-accurate detail |

## Decision

**`dit ai init` writes one canonical document and points every agent file at
it.** One idempotent command, following `dit workflow init`'s precedent — there
is no separate `update`; running it again is the update. `dit ai spec` prints
the full specification to stdout, from the binary, always matching the binary.

- **The document** is `docs/dit-for-agents.md`: visible in the tree, readable
  on GitHub, reviewed in pull requests, and present in DIT's own Docs screen, so
  the workspace documents itself. It uses marker blocks internally too, so a
  team's hand-written notes survive regeneration.
- **The pointers** are four-line marker blocks. `AGENTS.md` always gets one,
  as the cross-tool convention. Every other agent file that **already exists**
  in the repo gets one too. DIT never creates a file for a tool the team does
  not use; `--all` opts into that, `--only <tool>` narrows.
- **The content** is what an agent cannot guess: the data model (issue, flow,
  lane, phase), the flow-fence grammar from ADR 0020, the rules that carry
  consequences — never edit an issue file directly, never store a derived fact,
  a merge conflict is a state and not a failure — and the coordination
  protocol. It does not restate `--help`: the agent can read that itself, and
  copying it would create a second place to keep in sync.
- **It is stamped** with the version that generated it, and `dit doctor`
  reports a block written by an older binary. There is no separate `dit ai
  check`; staleness is a health question and `dit doctor` is where health
  questions are answered.
- **The existing block is absorbed.** `dit ai init` recognises
  `<!-- dit:workflow-protocol -->`, replaces it with a pointer, and
  `dit workflow init` stops writing it. The defects in Verification go with it.
- **`dit ai init` commits its own work**, as `dit workflow init` does.

Nothing here is executed or fetched by DIT. The document tells a human or an
agent what to run; DIT never runs it. That is the same line `remote` already
sits on the right side of under I7.

## Consequences

**Easier.** A workspace can state, in one reviewed file, how it expects to be
worked in — and an AI session reads it before its first mistake instead of
after. Supporting another tool is a pointer. The flow-fence grammar from ADR
0020 reaches agents the day it ships, without anyone writing documentation by
hand.

**Harder.** `docs/dit-for-agents.md` is generated but lives where people edit
things, so the marker discipline has to hold or someone will lose a paragraph.
And there are now two places describing DIT to a reader — this document for
agents working *in* a workspace, and `DESIGN.md`/`ARCHITECTURE.md` for people
building DIT itself. They must not drift into contradicting each other; they do
not overlap today, because one is about using the tool and the other about
building it.

**No longer true.** `dit workflow init` no longer touches `CLAUDE.md`. A
workspace that was scaffolded before this and never runs `dit ai init` keeps
the old block until it does — the marker is recognised, not orphaned.

## Verification

The mechanism this ADR builds on works, and what it currently emits is broken.
Both were run, not read.

A fresh workspace, scaffolded with the binary built from `main`:

```
$ git init -q . && dit init && dit workflow init
$ cat -n CLAUDE.md
     1	<!-- dit:workflow-protocol -->
     2	
     3	## DIT peer protocol for parallel actors
     4	
     5	Each actor (human or AI session) works one lane. Identity: `export DIT_ME=<alias>`
     6	(or `--me`) before any command; every claim, comment and commit is attributed to it.
     7	         Lanes are free-form (`dit issue set REF lane=any-name`); orchestrations are flows
     8	         (`dit issue set REF flows=launch,audit`) and one issue may join several at once â
     9	         watch one with `dit flow show <name>`.
```

Two defects, both shipping to every workspace scaffolded so far:

1. **Lines 7-9 carry nine spaces of stray indentation.** Three lines of the
   `format!` literal at `crates/dit-core/src/lib.rs:2279-2281` are missing the
   `\n\` continuation their neighbours have, so the source's own indentation is
   emitted verbatim. It does not become a code block — CommonMark does not let
   an indented chunk interrupt a paragraph — but agents read `CLAUDE.md` as raw
   text, so the mangled bytes are exactly what reaches the model.

2. **Line 8 ends in mojibake.** An em-dash, double-encoded:

```
$ sed -n 2279,2280p crates/dit-core/src/lib.rs | hexdump -C | grep 'c3 a2'
00000110  65 20 c3 a2 c2 80 c2 94  0a 20 20 20 20 20 20 20  |e .......       |
```

`c3 a2 c2 80 c2 94` is UTF-8 `e2 80 94` (—) re-encoded through Latin-1. It
renders as `â` followed by two invisible control characters.

What this verifies for the decision: the marker mechanism itself is sound —
the block is delimited, idempotent, and everything outside it survives — so
this ADR reuses it rather than inventing one. And hand-formatted prose inside a
Rust `format!` literal is how both defects got in, which is why the canonical
document is generated as a whole file from one source string rather than
assembled line by line across four tool files.
