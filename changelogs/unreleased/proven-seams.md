# Proven seams: proof per environment, and readiness that reads it

A Morse pin said a scenario was proven at a commit. It did not say where — and
a chain green on one tenant can answer 404 on another, while "the upstream
issue is done" kept turning downstream work ready against a seam that did not
answer (ADR 0024, DESIGN.md §20.10).

- **Proof per environment.** `dit morse sync <scenario> --env <name>` now also
  records `proven.<name>: { commit, on }` in the fence, only on a green run.
  Reindex judges each proof against the spec like the pin — holds, stale or
  broken — and `dit morse check` prints them per environment.
- **Issues name their seams.** `needs_scenarios`, `proves` and `env` join the
  frontmatter, set with `dit issue set`, carried through the index, the API and
  the web client.
- **Readiness can ask for proof.** `proof: required` under
  `coordination.readiness` holds an issue whose blockers are done as
  *unproven* until every scenario it needs holds for its `env`. `dit ready`
  lists what is held and the command that would prove it. Off by default.
