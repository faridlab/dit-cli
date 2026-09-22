# Coordination plane for parallel actors

DIT can now be driven by several parallel actors (human or AI session, one
per work stream) without out-of-band coordination:

- **Lanes** — issues carry a `lane`; lanes are free-form values in the data
  (the `workflow.yaml` registry orders, labels and staffs them, but never
  gates what a lane may be); `dit workflow init` scaffolds the registry and
  a protocol section for the workspace's CLAUDE.md, idempotently.
- **Dependencies** — `blocked_by` is settable from the CLI (`blocked_by=#12,#13`);
  readiness is derived (`dit ready`, with `--until review` as the per-call
  gate override). A cancelled blocker is reported as broken, never satisfied.
- **Claims** — `dit claim` writes exclusive intent as one commit; stale
  claims (TTL in workflow.yaml, default 15 minutes) are takable; renews are
  frugal; the CLI refuses protocol violations with the way out named.
- **Safer references** — `#N`, short refs and `epic=` resolve with ambiguity
  rejection: duplicates name their candidates instead of silently picking the
  first hit. Status writes validate against the workflow.
- **Threaded comments** — `reply_to` is writable; conversations between
  actors happen on the issue under discussion, indented in the activity
  stream with a per-comment Reply action.
- **Evidence reports** — `dit workflow init` seeds
  `.dit/templates/integration-report.md` (expectation vs actual, the request
  and the response verbatim, diagnosis, unblock); `dit issue comment
  --template integration-report` drafts it in $EDITOR and refuses to post it
  untouched.
- **The lane inbox** — `dit inbox [--lane X]` lists the threads on a lane's
  issues whose latest author is not the lane's voice (its registered owners,
  falling back to assignees and the current claimant) — the questions still
  waiting for an answer.
- **Flows** — an orchestration is a flow: `flows: [name, ...]` on an issue,
  and one issue may sit in several flows at once (`dit issue set REF
  flows=a,b`, `--flow` on `issue new`, DQL `flow = x`). `dit flow list`
  counts them; `dit flow show <name>` renders one as a stage-by-stage text
  tree (`all` unions every flow).
- **The Flow screen** — the orchestration as a diagram in archify's visual
  language (github.com/tt-a1i/archify): a dotted ground grid, dashed lane
  bands captioned `01 / Name`, phase headers over the stage columns, nodes
  as semantic fill/stroke pairs with a status sigil and centered mono
  labels, and orthogonal `blocked_by` arrows — the critical path
  emphasized, a satisfied dependency dashed, a broken one dashed rose.
  Stages are computed by longest-path layering, and the rows inside a
  stage follow the mean row of each node's blockers, so the arrows do not
  cross wherever the order of the work disagrees with the order of its
  priorities. The critical path is the chain with the most work still left
  in it, not merely the longest one: a finished chain says nothing about
  when the flow lands. The screen is read-only and refreshes on any
  process's write via the file watcher feeding the existing event channel.
- **Reading the flow** — the diagram is something to interrogate, not only
  to look at. It pans, zooms and fits, with a minimap once it outgrows the
  window; `f` finds a node by title or number; clicking a node lights
  everything it waits on and everything waiting on it and dims the rest,
  with the full detail in the sidebar — blockers, dependents, claim, reach,
  and the blockers that live outside this flow, named rather than counted,
  because no arrow can draw them; picking two nodes traces the chain of
  dependencies between them; `[` and `]` walk the critical path; and the
  legend is a filter, so isolating "ready" answers what can be started now.
- **Naming the columns** — a flow's diagram is still derived, but the part no
  derivation can produce is now authored: a `dit-flow` fence in any document
  declares the order and labels of its phases, the groups inside a lane, and
  the captions on its arrows. An issue joins a phase with a reserved label
  (`labels: [phase/build]`), which merges as a set, so two branches that
  disagree produce two visible labels instead of one edit silently winning.
  Members nobody has placed draw in a trailing **Unphased** column, a flow
  with no fence draws exactly as it did before, and a fence that does not
  parse leaves the diagram standing under a banner naming the document and
  line. No key in that fence may name something to run or fetch.
- **A second kind of arrow** — `fed_by` says "that feeds this": a result, an
  outcome, a return path. It draws, it can carry a caption, and it touches
  nothing derived — not readiness, not stages, not the critical path. It
  exists so `blocked_by` never has to be borrowed for a picture, because an
  arrow drawn with `blocked_by` that does not really gate makes `dit ready`
  lie to every actor polling it. A blocker sitting in a later phase than the
  issue it blocks is reported on the diagram, never refused.
- **Reading further** — the diagram keeps its detail in proportion to the
  zoom, puts the current reading in the address bar so it can be pasted into
  a thread, previews a node's immediate neighbours on hover, explains itself
  under `?`, fills the screen under `F`, and can be coloured by state, lane,
  type or priority from data that already exists. It also tells the flow as a
  story — the phases when there are phases, the critical path when there are
  not — and marks the nodes that have real commits behind them, read from
  history rather than authored.
- **`dit ai`** — one command installs `docs/dit-for-agents.md` and points
  `AGENTS.md`, and every agent file the repo already has, at it, while
  `dit ai add claude` (or `cursor`, or `copilot`) points one named tool and
  creates its file, because naming a tool is the whole request; `dit ai spec`
  prints the same specification from the binary, so it can never disagree with
  the binary. It covers what an AI session cannot guess: the data model, the
  fence grammar, and the rules that carry consequences — never edit an issue
  file directly, never store a derived fact, a merge conflict is a state and
  not a failure. `dit doctor` reports a guide written by an older version.
  This replaces the protocol block `dit workflow init` used to write into
  `CLAUDE.md`, whose text shipped with stray indentation and a mangled dash.

ADRs 0015-0021 record the decisions.
