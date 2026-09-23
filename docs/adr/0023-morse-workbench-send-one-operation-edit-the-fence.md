---
id: 0023
title: "Morse workbench: one operation may be sent, the screen edits the fence, and path parameters are named"
status: accepted
date: 2026-09-23
supersedes: null
---

## Context

ADR 0022 built Morse as a read model with one control that reaches the
network: Run, which fires a whole scenario. The screen that came out of it is
a grid of spec cards and a list of scenarios. Pointed at a real workspace —
serpa's 45 OpenAPI documents, 6,647 operations — it is a catalogue, not a
tool. Everyone who opens it arrives from Postman, and Postman's working loop
is: pick an endpoint, fill it in, press Send, read what came back, save it
into a collection. Morse had none of that loop. The only way to try an
endpoint was to hand-write a fence in a document, reindex, and press Run.

Four of Postman's habits collide with decisions ADR 0022 recorded, so the
redesign could not simply copy the layout:

- **Send fires one request that no fence describes.** §20.5's table lists
  exactly three things that fire: `dit morse run`, the Run control, and
  `dit morse sync`. A single operation sent from a tab is a fourth.
- **Postman shows the response body.** §20.7 keeps bodies and captured values
  out of the browser entirely, because a response is the likeliest place in
  the product for a real token to appear, and §17.2 makes XSS the primary
  threat against a local server.
- **Postman edits a request in place and saves it.** In Morse the request
  lives in a fence inside a document body, which is prose a person owns.
- **Postman's Scripts tab** is where its tests and chaining live. ADR 0022
  forbids that outright.

Building the prototype against serpa turned up a fifth thing, and it is a
defect rather than a design question: **a path parameter in the spec was
never filled.** `dit-core` hands the spec's path to `dit-morse` exactly as
the document writes it, and `dit-morse` substitutes only `{{name}}`. A step
calling `GET /parties/{id}` sent the literal `{id}`. The fence grammar had no
way to say what `id` should be. Nothing tested it. 1,955 of serpa's 6,647
operations — 29% — have a path parameter.

## Options considered

| Sending one operation | Cost | Consequence |
|---|---|---|
| No Send; an operation tab is read-only with "new scenario from this" | None | Nothing about egress moves, but the first thing anyone tries on the screen is not there |
| Send saves a fence first, then runs it | A commit per experiment | "What fires is what is in the repo" stays literally true, and history fills with half-finished attempts nobody meant to keep |
| **Send fires the draft through `dit-morse`, never committed** | One more row in §20.5's table | The loop people expect. Every gate a run passes still applies, and the browser gains no reach it did not already have (Decision) |

| What the response panel shows | Cost | Consequence |
|---|---|---|
| The body, transiently, as plain text | Reverses §20.7 and the DTO boundary | Closest to Postman; one injected script reads every token the API returns |
| **Status, time, size, each check, the names of captures** | The body is one terminal command away | §20.7 holds unchanged |

| Editing from the screen | Cost | Consequence |
|---|---|---|
| Read-only tabs; edit the fence in Docs | None | Two places to work on one request, and the one with a form cannot save |
| **A form over the fence, written back through a transaction** | A fence writer that round-trips | The fence stays the source of truth (I1); the form is a view of it |

| Path parameters | Cost | Consequence |
|---|---|---|
| Fill `{id}` from a variable of the same name | None | A binding nobody wrote down: renaming a capture silently changes which record a request touches |
| Require `{{id}}` in the spec path | Impossible | The spec is not a DIT file and is not ours to edit |
| **A `params:` map on the step** | One key in the grammar | Explicit, reviewed in the diff, and checkable before anything is sent |

## Decision

**The Morse screen is a request workbench over the same data model.** An
explorer lists specs by OpenAPI tag, scenarios by document, this machine's
environments, and recent runs. Each operation, scenario step, scenario,
spec and environment opens in a tab. A request tab has Docs, Params, Headers,
Body, Expect and Capture; Expect and Capture take the place of Postman's
Scripts, and there is no Scripts tab to take the place of.

**One operation may be sent, from its tab or from `dit morse send`.** The
server takes the draft — an operation reference, and `params`, `query`,
`headers`, `body`, `expect`, `capture`, all in the fence's own vocabulary —
and runs it as a one-step plan through `dit-morse`. Every gate a scenario run
passes, a Send passes:

- the operation must resolve in the catalogue at HEAD, so method and path
  come from the spec, never from the page;
- the base URL comes from the spec's `servers:` or this machine's
  environment, never from the draft — the draft has no field that could name
  one;
- the host must be in this machine's allowlist, and the page still cannot add
  one;
- redirects are not followed, substituted values are percent-encoded, and a
  value nothing binds refuses the request before it is built.

Nothing is written: a draft is not committed, and its run lands in the index
like any other and is gone at the next reindex (§20.7).

This is not new reach for an attacker, and the reason is worth keeping: an
injected script on the page could already `PUT` a document containing a fence
and then press Run. Send removes a commit from that path, not a gate. What an
injection still cannot do is choose the host, which is the thing §20.5's
allowlist exists to protect.

`dit morse run <scenario>` and the Run control are unchanged. Sending one
step of a scenario on its own is a Send of that step's contents: values an
earlier step would have captured are not there, and the screen says so rather
than sending an empty string.

**The response panel keeps §20.7 exactly.** It shows the status, the time,
the size of the body, each check with whether it held, and the *names* of what
was captured. The body and the captured values stay on the server side of the
boundary, and the panel offers the terminal command that prints them.

**A step names its path parameters in `params:`.** `{name}` in the spec's
path — OpenAPI's own syntax — is filled from the step's `params:` map, whose
values may reference `{{variables}}` like any other field. Each value is
percent-encoded as one segment, so a captured `../admin` cannot become a
different endpoint. A path parameter with no entry refuses the step before
anything is sent, naming the parameter. An inline request's path may use the
same `{name}` form.

**The screen edits the fence, and the fence stays the source of truth.** An
edit made in a step's tab is parsed into the same `MorseScenario` the indexer
builds, serialised back by a fence writer in `dit-parse`, and written through
`Transaction` + `write_doc` after a pause — the same one-commit-per-pause
rhythm as a document. Only the fence's own lines change; the prose around it
is left byte-for-byte. "Save to scenario" from an operation tab appends a
step to an existing scenario, or appends a new fence to a document, the same
way. The writer's output must parse back to what was written, which is what
a test pins.

A fence containing a `#` comment is not rewritten from the screen: a
re-serialised fence would drop it, and a comment is a person's words. The
tab says so and points at the document.

**This machine's environments are listed by name, never by value.** The
screen reads each environment's name, its server, which variables are set
and which are not, and the allowed hosts. It does not read a value, and it
cannot write the local file.

**CLI twins.** `dit morse send <spec>/<operationId>` takes `--env`, `--param`,
`--query`, `--header` and `--body`, and prints the response to the terminal
that asked, the way `dit morse run` prints a run.

## Consequences

**Easier.** The loop people arrive expecting exists: find an endpoint in 6,647,
fill it in, send it, keep it. What is kept is a fence, reviewed in a pull
request, pinned to the spec's commit — the Postman habit ends in the place
ADR 0022 wanted it to end.

**Harder.** A fourth thing fires a request, and §20.5's table, the I11 checks
and every sentence that lists "the only things that fire" move together. The
fence writer is a second place the grammar lives, and it has to keep up with
the parser; the round-trip test is what holds them together. Comments in a
fence make it read-only from the screen, which will surprise someone.

**No longer true.** "Opening a tab never fires" is still true; "only a scenario
fires" is not. A path parameter is no longer silently sent as the literal
`{id}`.

## Verification

**The path-parameter defect, by reading and then by test.** In
`dit-core/src/morse.rs` the planned step's path is `found.path.clone()` — the
spec's path verbatim. `dit-morse/src/template.rs` replaces `{{` … `}}` and
nothing else. No test in `dit-morse`, `dit-core` or `dit-parse` contains a
spec path with a `{name}` segment. The fix lands with a failing test that
sends `/parties/{id}` to a local server and asserts the request line the
server received.

**The size of it.** Counting operations whose path contains `{` across
serpa-service's 45 documents at `e358a9f`: 1,955 of 6,647.

**Send adds no reach an injection lacked.** `PUT /api/docs/{path}` accepts a
body containing a `dit-morse` fence from the same token the page holds, and
`POST /api/morse/run/{scenario}` fires it after the reindex the write
triggers. Neither endpoint lets the caller name a host; neither does Send.
