---
id: 0025
title: "Code map: the import graph is derived into the index, and the intent behind it is authored and pinned"
status: accepted
date: 2026-09-27
supersedes: null
---

## Context

Agents working the serpa workspace read code structure through graphify on every
task. Measured on `serpa-webapp-admin` (2026-09-26): 6,199 files, 33,131 nodes,
74,529 edges, of which the useful ones are almost all mechanical —

| Relation | Edges | What it answers |
|---|---|---|
| `contains` | 25,189 | which symbols a file defines |
| `imports` | 23,458 | which files a file depends on |
| `imports_from` | 12,819 | which named symbols it takes from each |
| `calls` | 4,373 | which functions it calls |
| `re_exports` | 4,328 | which barrels pass symbols through |
| `inherits` / `implements` | 2,863 | type relationships |
| `references`, `method`, `indirect_call` | 1,447 | the rest |

In other words: the code telling its own story — what it imports, from whom, and
what it uses. That is derivable from the source alone, every time, and graphify
derives it. Three things about how it does so cost the agents that use it:

1. **It is a snapshot.** `graph.json` is written by `graphify update` and says
   "Built from commit 5847ce71"; nothing makes it current, so an agent reads a
   graph of code that no longer exists unless someone remembered to rebuild it.
2. **It is a file in the repository, and a large one** — 27 to 46 MB per
   snapshot, eight snapshots kept. It is derived data committed to the source of
   truth, which DIT's Principle 3 (I5) forbids for exactly this reason: it goes
   stale against the facts it was derived from, and it conflicts on every merge
   (graphify ships a union merge driver for its own file).
3. **It stops at the code.** It cannot say which of the 33,131 nodes is the place
   to change for a given task, which file is the pattern to copy, or which
   directory is generated and must not be edited. Those facts live in each
   repository's `CLAUDE.md`, as prose that rots when files move — the webapp's
   table of "customise an entity here" points at paths nothing checks.

DIT already holds the machinery both halves need. The index is rebuilt from git
and keyed by blob sha, so it can derive a graph incrementally and never commit
it. §7.4 records a document's sources as `{ path, commit }` pairs and computes
staleness at query time; ADR 0022 does the same for a scenario's spec. ADR 0024
registered consumer repositories for the seam map and read them through `dit-vcs`
without a checkout. What is missing is a structural extractor and a place for
the authored intent.

## Options considered

| How the graph is extracted | Cost | Consequence |
|---|---|---|
| Keep graphify, point DIT at its `graph.json` | None | Inherits the snapshot, the committed file and the rebuild step — the three costs above |
| Hand-written import scanners per language | Small, no dependency | Imports and exports are regular enough, but `contains`, `calls` and `inherits` need a real parse; the scanner becomes a bad parser |
| A language server per language | Heavy, a process per language | Precise, but a runtime dependency on toolchains DIT does not own, and slow on a cold index |
| **tree-sitter grammars in one adapter crate** | One C-backed dependency plus a grammar per language, pinned | The same AST extraction graphify uses, run by DIT at reindex, incremental by blob, stored in the index |

| Where the graph lives | Cost | Consequence |
|---|---|---|
| A committed file, like graphify | Merge conflicts, a 30 MB diff | Violates I5 |
| **The index, keyed by blob sha** | Reindex work, bounded to changed files | Derived, never committed, current at HEAD by construction |

| Where the intent lives | Cost | Consequence |
|---|---|---|
| `CLAUDE.md` prose, as today | None | Rots silently when paths move |
| A new file format | A new grammar, a new root | Another place to look |
| **A `dit-map` fence in any document, entries pinned to paths** | One fence grammar | Reuses ADR 0020's fence mechanism and §7.4's staleness; readable on GitHub as a code block; a moved path makes the entry *broken*, named |

## Decision

**The code map has two halves, and DIT keeps both honest: the graph is derived
from source into the index, and the intent behind it is authored in the
repository and pinned to the paths it names.**

### 1. Code roots are registered

`.dit/config.yaml` gains `code:`, the one registry ADR 0024's consumer map and
this map share:

```yaml
code:
  - { id: webapp,  repo: webapp,  include: ["src/**"], exclude: ["src/generated/**"], generated: ["src/generated/**"] }
  - { id: service, repo: backend, include: ["src/**", "crates/**"] }
```

`repo:` names a `repos:` entry (Mode A) or is omitted for this workspace. Files
are read at HEAD through `dit-vcs`, never checked out. `generated:` marks paths
that are indexed but reported as generated — the one fact about generated code an
agent most needs and a graph cannot tell. (ADR 0024's `consumers:` becomes an
alias of `code:`; one registry, one set of include rules.)

### 2. The graph is derived at reindex, into the index

A new adapter crate, `dit-code`, parses each included file with the tree-sitter
grammar for its language and emits, per file:

- **symbols** it defines — name, kind (function, type, const, component, trait,
  impl, …), line, exported or not;
- **imports** — the target file resolved against the root (TS path aliases from
  `tsconfig.json` `paths`, relative paths, Rust `crate::` / `super::` / `mod`
  declarations and workspace crates), and the named symbols taken;
- **re-exports**, **calls** to imported or local symbols, **inherits** /
  **implements**.

Rows land in index tables keyed by `(root, path, blob_sha)`. A reindex re-parses
only files whose blob changed, so the graph is current at HEAD by construction —
there is no `update` to forget. Nothing is committed (I5). Resolution the
extractor cannot make (a dynamic import, a macro-generated item) is stored as
unresolved with the literal, never guessed.

**Languages, in order:** TypeScript/TSX/JavaScript and Rust (milestone 1 — the
two languages of the workspaces DIT serves today), Kotlin (milestone 3, for KMP
clients). A root in a language without a grammar is indexed for files only.

### 3. The graph is queried, never dumped

Read commands answer the questions agents ask graphify, from the index only (I2):

```
dit code uses <file|symbol>         what it imports and calls, by file and symbol
dit code users <file|symbol>        who imports it, who calls it (fan-in)
dit code path <a> <b>               the shortest import/call chain between two nodes
dit code explain <file|symbol>      a node, its neighbours, and any map entry naming it
dit code hubs [--root <id>]         the most depended-on files and symbols, per root
dit code where <text>               files and symbols matching a name
```

Output is bounded and ranked; a query that would print thousands of rows prints
the top of them and the count. The web client gets the same through read routes.

### 4. The intent is authored in a `dit-map` fence and pinned

```dit-map
map: webapp
entries:
  - task: "Customise how one entity is listed or edited"
    change: [ "webapp:src/resources/*/index.ts" ]
    example: "webapp:src/resources/product/index.ts"
    never: [ "webapp:src/generated/**" ]
    why: "The generic CRUD engine renders every entity; per-entity config lives in its resource folder."
  - task: "A People desk screen with verbs"
    example: "webapp:src/desks/people/PayrollRunsPage.tsx"
    change: [ "webapp:src/desks/people/", "webapp:src/routes.tsx", "webapp:src/shell/nav.ts" ]
```

Every path is `<root>:<glob>`. At reindex each entry is judged against the code
index: a `change` or `example` that matches no file at HEAD makes the entry
**broken**, named with the document and line; an `example` whose file has been
rewritten beyond a threshold since the entry was last confirmed is **stale**.
`dit code map confirm <map>` re-pins the entries a person has read and agrees
still hold — the same stance as `dit morse sync`: a pin moves because someone
checked, never on its own. `never:` globs are checked too: an entry that says
"never edit X" while X no longer exists is broken, not silently vacuous.

The fence names paths and prose only; no field names anything to run or fetch
(I7 holds; the guard gains the fence's keys by allowlist).

### 5. The agent guide reads both halves

`dit ai spec` gains a *Code map* section: the registered roots, which paths are
generated, the map entries that hold (broken ones are listed as broken, never
shown as guidance), and the `dit code` commands. `dit ai spec code` is the topic.
An agent entering a workspace learns where to work from entries that were checked
against the code this reindex, and how the code connects from a graph that is
current by construction.

## What this does not replace

Graphify's community detection (1,965 clusters) and its "god node" ranking are
not reproduced as such. `dit code hubs` gives the fan-in ranking that god nodes
approximate; clusters are replaced by the authored map, which says what a region
of code is *for* rather than inferring that some files are near each other. If a
workspace turns out to need inferred clusters, that is a later ADR.

Semantic search over code (embeddings) is out of scope.

## Invariants

- **I2** — every `dit code` read and the agent guide read the index only.
- **I3** — files are read through `dit-vcs` (`ls_tree`, `show_text` at HEAD).
- **I4** — tree-sitter is C-backed and not wasm-clean, so it lives in `dit-code`,
  an adapter under `dit-core`, never in `dit-model`/`dit-parse`/`dit-query`. The
  `dit-map` fence grammar is pure and lives in `dit-parse`.
- **I5** — the graph and every judgement about map entries are derived; only the
  fence (and the pin `confirm` writes) is authored.
- **I7** — `code:` and `dit-map` carry repository paths and prose; the guard gains
  their keys by allowlist, in the pull request that adds them.
- **I11** — unchanged; nothing here reaches the network.
- **Dependencies (§9)** — `tree-sitter` and one grammar crate per language are
  pre-1.0: pinned exactly, recorded in DESIGN §9, and checked against the
  workspace's `rust-version` before they are taken.

## Milestones

1. **Derived graph for TS and Rust.** `code:` registry; `dit-code` with the two
   grammars; index tables; incremental reindex by blob; `dit code uses | users |
   path | explain | hubs | where`. Parity check against graphify (Verification).
2. **The authored map.** `dit-map` fence, judging against the code index,
   `dit code map confirm`, `dit code check`, the agent guide's Code map section
   and `dit ai spec code`.
3. **Kotlin, and the seam link.** The Kotlin grammar; ADR 0024's consumer map read
   from this index — a literal API path in a caller becomes an edge to the spec
   operation it names, so an orphan call is a graph fact, not a second scan.

## Milestone 1 as built

Measured against `serpa-webapp-admin` at `dbb8d09`, graphify rebuilt at the same
tree (2026-09-27, release build, Apple silicon):

| Check | Target | Result |
|---|---|---|
| Import edges between files under `src/` | ≥ 95% of graphify's | **98.8%** direct (17,564 / 17,774); **99.9%** counting the 197 graphify draws *through* a barrel, which DIT stores as the barrel edge plus its re-export and follows in `dit code users` |
| Edges graphify has that DIT does not | classified | 13 — every one in files that exist only in the working tree (graphify reads the working tree; DIT reads HEAD by design) |
| Edges DIT has that graphify does not | — | 86 — dynamic `import()` with a literal target, mostly tests reloading modules; added after the first parity run showed them missing |
| Symbols per file, 200-file sample | within 5% | 191 of 200 files within 5%; DIT also indexes non-function constants (`export const schema = …` in every resource config), graphify indexes object-literal methods in test stubs |
| The five questions | same or better | all answered: users of `useResourceList` (29 files), what `PayrollRunsPage.tsx` imports with names and targets, `SerpaShell → AuthProvider → tokenStore`, hubs, where `RecordSurface` is defined |
| Cold refresh, 6,291 files | < 60 s | **11.5 s** (graphify `update`: 2 min 10 s) |
| Warm refresh | < 2 s | **0.22 s** |
| Dependencies | build on rust-version, pass `cargo deny` | pinned as above; `cargo deny check`: advisories, bans, licenses, sources ok |

Three changes the measurements forced, each with a test:

- Blobs are read in one `git cat-file --batch` (`Repo::read_blobs`), and parsing is
  spread over the machine's cores: the first cold run spent three minutes spawning
  one `git show` per file.
- An extractor version (`dit_code::EXTRACTOR_VERSION`) is stored in the index; a
  different one re-reads every file, because the cache is keyed by blob and a fixed
  extractor would otherwise never reach an unchanged file. `dit code refresh --full`
  forces the same.
- `dit code hubs` leaves generated files out unless `--generated` asks: in a
  schema-driven app the top of the fan-in list is always the generated base
  client, which says nothing about the code people write.

The map is refreshed by `dit code …` commands and `dit reindex`, never on the
reindex that runs when any `dit` command opens the workspace — a first parse of a
large root must not stall `dit ready`.

## Milestone 2 as built

- `dit-map` fences are read at reindex (and by the watcher) into `code_maps`; the first
  document to name a map owns it, a second is reported as skipped. A fence that does not
  parse keeps its name and the reason, and `dit code check` prints both.
- Judging runs at the end of every code refresh. Path existence is checked against the
  root's **whole repository tree** at HEAD, not only the indexed files, so an entry may
  point at a stylesheet or a config the grammars do not read. Staleness is judged from the
  **example alone**: it is the file an agent copies, so it is the one whose drift misleads;
  `change` and `never` globs only have to keep matching something.
- `dit code map confirm <map>` pins every root the map names to its HEAD in one commit,
  and refuses while any entry is broken.
- The agent guide gained *Reading the code* (the commands, the registered roots, what each
  verdict means for the reader) and `dit ai spec code` (the root registry, the fence and
  the confirm step).
- I7: `code`, `include`, `exclude` and `generated` joined the schema vocabulary; the fence
  refuses the forbidden keys at any depth.

## Consequences

**Easier.** The graph an agent reads is the code at HEAD, every time, with no
rebuild step and no 30 MB file in the repository or in merges. The places to
change, the patterns to copy and the directories never to touch are written once,
checked on every reindex, and reported broken the moment a refactor moves them —
so the part of `CLAUDE.md` that rots fastest stops rotting silently. One tool
answers "how does the code connect", "where do I work", "what is proven" and
"what may I pick up".

**Harder.** DIT takes a C-backed dependency and a grammar per language, with the
maintenance that implies; a language without a grammar gets files only. Import
resolution has edge cases per ecosystem (path aliases, barrels, Rust macros) and
will be wrong in places; unresolved edges are stored as such rather than guessed,
and the parity check says how often. Reindex does more work on a large root the
first time.

**No longer true.** A workspace that adopts this can drop graphify: its
`graphify-out/` directory, its merge driver and the `graphify update` step in
every `CLAUDE.md`. That removal is its own change, made after the parity check
below passes on the workspace in question — not assumed by this ADR.

## Verification

Before `accepted`, run against `serpa-webapp-admin` at one commit, with graphify's
graph built at the same commit:

- [ ] Import edges: DIT's resolved `imports` + `imports_from` edges cover at least
      95% of graphify's for the same file set; every miss is classified
      (alias, barrel, dynamic, generated) and listed.
- [ ] `contains`: symbol counts per file agree within 5% on a sample of 200 files.
- [ ] Five questions an agent asked graphify in the 2026-09 sessions answer the
      same (or better, with the reason) through `dit code`: who uses
      `useResourceList`, what `PayrollRunsPage.tsx` depends on, the path from
      `SerpaShell.tsx` to `tokenStore.ts`, the hubs of `src/crud`, where
      `RecordSurface` is defined.
- [ ] A full cold reindex of the root finishes in under 60 s on the reference
      machine; a reindex after touching one file finishes in under 2 s.
- [ ] Moving `src/resources/product/` breaks the map entry that names it, and
      `dit ai spec` stops presenting it as guidance.
- [ ] `tree-sitter` and the grammar crates build on the workspace `rust-version`,
      pass `cargo deny`, and are recorded in DESIGN §9.
