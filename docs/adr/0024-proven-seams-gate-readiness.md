---
id: 0024
title: "Proven seams: a scenario's proof is per environment, readiness reads it, and callers are derived"
status: accepted
date: 2026-09-27
supersedes: null
---

## Context

ADR 0015 made readiness derived: an issue is ready when every blocker has
reached the workflow's `gate` status. ADR 0022 and 0023 gave the workspace
scenarios that say an API chain works, and a pin that `dit morse sync` moves
only after a green run — "proven to work at this commit". The two halves
never meet. Readiness asks whether the *issue* upstream is done; nothing asks
whether the *thing it promised* answers.

Two lanes working one product in parallel (the frontend and backend sessions
of serpa, coordinated through a `hr-completion` flow) produced a week of
evidence about where that gap costs. Every item below was a backend issue at
`done` or `review`, a frontend issue that therefore turned ready, and a
screen that was built against a seam that did not answer:

| What the frontend met | How long it took to find | What would have caught it |
|---|---|---|
| Payslip PDF: `404 slip_not_available` for a slip of a posted run | Found only when the button was tried live | The scenario was proven on the default tenant; the frontend's tenant was never proven |
| Four new modules: `relation "announcements.announcements" does not exist`, list routes answering 405 | Found after four screens were built | Same: migrations had run on one tenant, not the other |
| Payroll cancel: `404 not found: payroll run` for a draft the list returned | Found at the live check | Proof per tenant |
| `status[nin]=rejected,cancelled` silently ignored — the parser knows `notin` | Found only because a count looked wrong | An expectation that the filter held, and a caller whose path used an operator the spec does not list |
| `POST /auth/refresh` with body `{}` → 400 on the cookie lane | Found by a hand-written probe | A scenario for the cookie lane |
| A generated client reading `/lifecycle/discipline_records` where no list route is mounted | Found at the live check | Knowing that the caller exists and that no scenario covers the operation it calls |
| The payslip PDF route itself is in no registered spec — `payroll.openapi.yaml` lists the slip CRUD operations only | Found while writing this ADR | An orphan-call report: a consumer calls a path that no spec describes |

None of these are structural. A call graph of either repository was correct
the whole time; the frontend's code called exactly the paths it meant to. The
failures live **between** repositories and **per environment**, which is
where static code-intelligence tools (graphify and its kind) cannot see and
where DIT already stands: it is the one place both lanes write, it holds
the spec registry, and it already owns the notion of a proven pin.

Four gaps, each small against what exists:

1. **A pin names a commit but not an environment.** "Proven at a3f9c2d" was
   true on one tenant and false on the one the consumer uses.
2. **Readiness cannot read proof.** `gate: terminal` is satisfied by a status
   field a person sets.
3. **The consuming side is invisible.** The registry knows producers (specs)
   and scenarios; it does not know which code calls which operation, so an
   operation with callers and no proof, or a call to a path no spec
   describes, is never named.
4. **"Ignored without error" cannot be expected.** `expect` compares one
   value; a filter that was dropped returns rows that each violate it, and no
   comparator says "every row".

## Options considered

| Where per-environment proof lives | Cost | Consequence |
|---|---|---|
| One pin per scenario, as today, plus a note | None | The frontend's failure mode stays invisible — the note is prose |
| A scenario copied per environment | Duplicate fences | Two sources of truth for one chain, drifting |
| **`proven:` in the fence, keyed by environment name, written only by `sync`** | One map in the fence grammar | One chain, several proofs; each still means "a person ran this green here, at this commit" |

| How readiness uses proof | Cost | Consequence |
|---|---|---|
| A new status ("verified") people move issues to | None | Record-keeping again — the thing §15 rejects |
| Fire the scenario from `dit ready` | Egress on a read path | Violates I11 outright |
| **An issue names the scenarios it needs; readiness reads their pins** | One frontmatter list, one derived check | `dit ready` stays a read. "Ready" means the promised seam was proven, for the environment that issue works against, at or after the commit the blocker closed at |

| How callers are known | Cost | Consequence |
|---|---|---|
| Authors list callers by hand | Rots on the first refactor | A second catalogue nobody updates |
| A language server or a call graph per consumer | Heavy, per-language | Solves the wrong problem — the call inside the consumer is not what breaks |
| **Derive calls from registered consumer repos at reindex**: literal API path strings matched against spec paths | A string matcher; no execution | Derived (I5), read through `dit-vcs` like specs (§20.2), cheap, and good enough for the literal paths real clients write |

## Decision

**A seam is proven, not declared.** Three additions to Morse and one to
readiness, none of which sends a request that ADR 0022's list does not
already allow.

### 1. Proof is per environment

The fence keeps `spec:` and gains a map that only `dit morse sync` writes:

```dit-morse
scenario: payslip-pdf
spec: { id: payroll, commit: 1708c99 }
requires: [slip_id]
requests:                      # the PDF route is in no registered spec (see Context)
  - id: pdf
    method: GET
    path: /api/v1/payroll/salary-slips/{{slip_id}}/pdf
steps:
  - id: pdf
    request: pdf
    expect: { status: 200, header: { content-type: application/pdf } }
proven:
  local:        { commit: 1708c99, on: 2026-09-26 }
  local-hrperf: { commit: 1708c99, on: 2026-09-27 }
```

`sync <scenario> --env <name>` fires, and on green writes that one key. A
scenario is **proven for an environment** when that key exists and its commit
is not stale against the spec's repository (§20.4's computation, per key).
Nothing else writes `proven:`; reindex, the watcher and CI never do. The
environment *name* is committed; its base URL and values stay in the local,
gitignored file ADR 0022 defined.

### 2. Issues name the seams they need, and readiness reads them

```yaml
# frontend issue
needs_scenarios: [payslip-pdf]
env: local-hrperf          # the environment this lane works against
# backend issue
proves: [payslip-pdf]
```

Both are lists of scenario names — never URLs, never commands (I7 holds
unchanged; the guard gains the two keys by allowlist, deliberately).

Readiness (ADR 0015) gains one derived condition, **off by default** and
switched on per workflow:

```yaml
coordination:
  readiness:
    pick_from: todo
    gate: terminal
    proof: required        # absent = today's behaviour
```

With `proof: required`, an issue is ready when its blockers have reached the
gate **and** every scenario in `needs_scenarios` is proven for the issue's
`env`, at a commit that is not older than the commit its `proves:` blocker
closed at. `dit ready` explains a refusal in words: *"blocked: payslip-pdf is
proven on `local`, not on `local-hrperf`."* Moving a `proves:` issue to a
terminal status while its scenarios are unproven is allowed and flagged —
proof is a report, not a lock, for the same reason ADR 0022 keeps `check` a
report.

### 3. Callers are derived

A `consumers:` list in `.dit/config.yaml` mirrors `specs:`:

```yaml
consumers:
  - { id: webapp-admin, repo: webapp, include: ["src/**/*.{ts,tsx}"], exclude: ["src/generated/**"] }
```

At reindex, string literals shaped like API paths (`api/v1/...`, template
literals with `${...}` segments) are read through `dit-vcs` and matched
against every registered spec's paths, with `{param}` and `${...}` as
wildcards. The result is index-only (I5) and yields three derived facts:

- **covered** — an operation with callers and a scenario proven for the
  consumer's environment;
- **unproven seam** — an operation with callers and no proven scenario;
- **orphan call** — a literal path no registered spec describes at HEAD. This
  is the class that became a 404 or 405 in the table above.

`dit morse check` reports them beside stale and broken. The matcher is a
heuristic over literals and says so: a path assembled at runtime from
variables is not seen, and that limit is printed with the report rather than
hidden.

### 4. "Every row" is expectable

One comparator family joins `expect.jsonpath`, declarative like the rest:

```yaml
expect:
  jsonpath:
    $.data[*].status: { none_of: [rejected, cancelled] }
    $.data[*].date:   { each: { gte: "{{from}}" } }
```

It is what turns "the filter was silently dropped" into a red step. No
expression language is added; `each`, `none_of` and `any_of` are built-ins in
the sense ADR 0022 requires.

### 5. For an agent entering the workspace

The canonical agent document (ADR 0021) gains a derived section listing, per
consumer, the seams it calls and their proof per environment. An agent picking
up a frontend issue reads which contracts are proven where, instead of
guessing paths and finding the gaps by trying them — the path the table above
records.

## Invariants

- **I2** — readiness, `check`, and the consumer map read only the index.
- **I5** — proof is written by a person running `sync`; callers, coverage and
  orphans are computed and never stored.
- **I7** — `needs_scenarios`, `proves`, `env`, `proven`, `consumers`,
  `include`, `exclude` name scenarios, environments and repository paths.
  None is fetched or executed. The I7 guard gains them by explicit allowlist,
  in the pull request that adds them.
- **I11** — unchanged. The only new write path is `sync` writing one more key
  after a run it already performs.

## Milestones

1. **Proof per environment and readiness** — `proven:` map, `sync --env`,
   `needs_scenarios` / `proves` / `env`, `proof: required`. No new egress, no
   new adapter. This alone would have kept every row of the table above from
   turning a frontend issue ready.
2. **Consumer map** — `consumers:` registry, the literal matcher, covered /
   unproven / orphan in `check` and on the Morse screen.
3. **Row comparators and the agent section** — `each`, `none_of`, `any_of`;
   the ADR 0021 section.

## Milestone 1 as built

What landed, and where it departs from the decision above:

- `proven:` in the fence, parsed and written by the fence writer; `dit morse sync`
  records `proven.<env>` surgically beside the existing `spec:` repin, only on green,
  with the environment resolved as `--env`, else the fence's `env:`, else `default`.
- Proofs are judged at reindex into `morse_proofs` (index version 8) with the same
  staleness computation as the pin; `dit morse check` prints each one.
- `needs_scenarios`, `proves` and `env` on issues, end to end: file, index side tables
  and column, `dit issue set`, the server DTO, and the I5 vocabulary.
- `coordination.readiness.proof: required` and `Readiness::Unproven`; `dit ready`
  names what is held back and the command that would prove it.
- **Deferred:** the condition that a proof be no older than the commit its `proves:`
  blocker closed at. Milestone 1 asks only that the proof exists for the issue's `env`
  and is fresh against the spec; the commit ordering needs the blocker's closing commit,
  which is derived history this milestone does not read.
- **Deferred:** `needs_scenarios`, `proves` and `env` at creation. They are set with
  `dit issue set`; `IssueDraft` is unchanged, so no creation path moved.

Found while building it, and fixed with a fixture (`apostrophe_in_a_title`): an
apostrophe inside a plain scalar was read as an opening quote across the frontmatter,
fence and config parsers, so an issue titled `Work plan's lane` was committed and then
skipped by the indexer; the reindex report now names each skipped file with its reason.

## Consequences

**Easier.** "Done upstream" and "usable downstream" stop being the same
claim. A lane that works against a second environment says so once, in `env`,
and readiness holds it to proof there. A generated client or a hand-written
fetch that points at a path no spec describes is named at reindex, before
anyone builds a screen on it.

**Harder.** Proof has to be produced per environment by a person, so a
workspace with many environments and few `sync` runs will see more issues
held back — which is the point, but it will be felt. The literal matcher will
miss dynamically assembled paths and occasionally match a string that is not
a call; both are reported as its limits, not fixed by guessing harder.

**Not decided here.** Whether a failed `run` may offer to file an issue in the
producer's lane (with the step, status and error code, never the body) is
left for the milestone 2 review; it writes through a transaction and sends
nothing, but it touches §20.7's line on what a run leaves behind.

## Verification

To be produced by running, as ADR 0022 requires, before this moves to
`accepted`:

- [ ] Against serpa-dit: write `payslip-pdf`, `announcements-list` and
      `auth-refresh-cookie`; `sync` each on `local` and `local-hrperf`; show
      the two pins disagree where the table above says they should.
- [ ] Turn on `proof: required` in a copy of serpa-dit's workflow and show
      that the frontend issue for the payslip download is not ready while
      `payslip-pdf` is unproven on `local-hrperf`.
- [ ] Run the literal matcher over serpa-webapp-admin against serpa's 45
      registered specs; report counts of covered, unproven and orphan, and
      check that `lifecycle/discipline_records` appears as an operation with a
      caller and no mounted route proof.
- [ ] `i7_no_executable_fields_in_schema` fails until the new keys are
      allowlisted, and passes after.
