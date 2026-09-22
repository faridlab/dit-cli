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
  Stages and the critical path are computed (longest-path layering); the
  screen is read-only and refreshes on any process's write via the file
  watcher feeding the existing event channel.

ADRs 0015-0019 record the decisions.
