# Coordination plane for parallel actors

DIT can now be driven by several parallel actors (human or AI session, one
per work stream) without out-of-band coordination:

- **Lanes** — issues carry a `lane` from a registry in
  `.dit/schema/workflow.yaml`; `dit workflow init` scaffolds the registry and
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
  actors happen on the issue under discussion.
- **Live UI** — a read-only Workflow screen (swimlanes per lane, blocked/ready
  and claim badges) that refreshes on any process's write via the new file
  watcher feeding the existing event channel.

ADRs 0015-0018 record the decisions.
