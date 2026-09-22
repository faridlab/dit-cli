---
id: 0022
title: "Morse: API scenarios live in the repo, stay declarative, and are never fired automatically"
status: accepted
date: 2026-09-23
supersedes: null
---

## Context

A Postman collection is the artifact DIT exists to rescue. It describes the
API of a codebase, it is edited by the same people who change that API, and it
lives in a hosted workspace that the repo knows nothing about. It travels as an
exported JSON blob. It is not reviewed in the pull request that breaks it, and
nothing anywhere notices when the endpoint it calls stops existing. The part
teams depend on most is the part that rots fastest: the **scenario** — register,
then log in with what register returned, then fetch the profile with the token
login returned.

Two halves of the answer are already built. §7.4 records each source of a
document as a `{ path, commit }` pair and computes freshness at query time, so
a document can be told it has gone stale against the code it describes. ADR
0020 put authored structure that no derivation can produce into a fence in a
document body, written in the YAML subset `dit-parse` already speaks, parsed at
reindex into the index so the read path never touches disk.

The collision is just as concrete. ADR 0020 drew the line explicitly: a fence
"may not name anything to run or fetch — I7 forbids a `run:`, `command:`,
`url:` or `on_enter:` step, and a flow whose steps *do* things is remote code
execution by pull request." A saved request is, unavoidably, a `url:`. I7 as
written in §17.3 forbids it outright.

There are two further edges, and both are sharper than the first:

- **Chaining is where Postman puts JavaScript.** `pm.test`, pre-request
  scripts, `pm.environment.set`. Copying that shape means a pulled file
  contains code that runs on a maintainer's machine — the exact thing §17.3 was
  written to prevent, and worse than a `url:` because a URL at least does
  nothing until something fetches it.
- **Nothing in DIT currently reaches the network on the say-so of repo
  content.** The one outbound call in the codebase today is the self-updater,
  against a hardcoded constant (see Verification). Morse would be the first
  egress whose destination is read out of a file that arrived in a pull
  request.

## Options considered

| Where scenarios live | Cost | Consequence |
|---|---|---|
| A sixth content root, `morse/` | Reserved-name check, layout migration, §4.1 and §18 both move | A new top-level folder for something not every workspace uses |
| Under `.dit/morse/` | None | §4.1 is explicit that `.dit/` is machinery and never browsing content; scenarios are content people read and edit |
| **A `dit-morse` fence in a document** | None | Reuses ADR 0020's mechanism whole: no new grammar, no new fuzz target, no schema movement, readable as a plain code block on GitHub, and the document may live anywhere |

| How values chain between steps | Cost | Consequence |
|---|---|---|
| JavaScript in a sandbox | A JS runtime, a sandbox threat model of its own | Repo content becomes executable; a sandbox escape is RCE by pull request. Reopens §17.3 rather than living inside it |
| Declarative by default, WASM modules for the rest | A second runtime, a new supply chain | Keeps files inert, but buys expressiveness nobody has asked for yet with a permanent maintenance surface |
| **Declarative only** | Every unsupported transform becomes a feature request | Files stay data. A capture is a selector, an assertion is a comparison, and there is no expression to escape from. What a real auth chain needs — read a field, put it in the next header — is covered |

| The endpoint catalogue | Cost | Consequence |
|---|---|---|
| Import OpenAPI into one file per endpoint, committed | A large generated tree | Two sources of truth for the same fact; every spec change is a noisy diff nobody reviews; and it is derived data written into the repo, which I5 forbids |
| **Derive endpoints from the spec at a recorded commit** | The spec must be in the repo, or vendored into it | One source of truth. The spec is already the API's contract. Endpoints are computed at reindex, and a scenario naming one that no longer exists is *stale*, which is a thing §7.4 already knows how to say |

| Several specs in one workspace | Cost | Consequence |
|---|---|---|
| Derive the set from the `spec:` of every fence | None | A spec that has just been registered and has no scenario yet is invisible — the exact moment someone wants to browse its endpoints |
| Discover `openapi.{yaml,json}` by walking the tree | None | The identifier is implicit and moves when a folder is renamed, breaking every fence that named it with nothing stating so |
| **A `specs:` list in `.dit/config.yaml`** | Two keys added to the schema vocabulary | Mirrors `repos:`, which already carries named sources past I7. The id namespaces `operationId`, which is only unique inside one document |

| What `dit morse sync` does before moving the pin | Cost | Consequence |
|---|---|---|
| Re-resolve offline and bump | None; runs in CI | The pin means "still structurally connected". An endpoint that kept its shape and changed its behaviour passes — the common failure |
| Offline by default, `--run` to verify | None | Two meanings for one pin, with nothing in the file saying which happened. A reader cannot tell a structural claim from a proven one |
| **Fire the scenario and bump only on green** | Needs a live environment and an allowed host; cannot run in CI | The pin becomes "this was proven to work at this commit" — §15's stance, verification rather than record-keeping. CI running `sync` would be the auto-firing this ADR forbids, so the limitation is the design |

## Decision

**Morse stores what a human wrote and derives everything else, and DIT never
sends a request the user did not just ask for.**

**A scenario is a `dit-morse` fence.** It declares the spec it is written
against, its steps, what each step expects, and what it carries forward:

```dit-morse
scenario: register
spec: { path: "docs/api/openapi.yaml", commit: a3f9c2d }
env: local
steps:
  - id: create
    operation: createUser              # an operationId from the spec
    body: { email: "{{email}}", password: "{{password}}" }
    expect: { status: 201 }
    capture: { user_id: $.data.id }
  - id: login
    operation: loginUser
    body: { email: "{{email}}", password: "{{password}}" }
    expect:
      status: 200
      jsonpath:
        $.token: { exists: true }
    capture: { token: $.token }
  - id: me
    operation: getCurrentUser
    headers: { Authorization: "Bearer {{token}}" }
    expect:
      status: 200
      jsonpath:
        $.id: "{{user_id}}"
```

**Specs are registered, and the registry is what tells them apart.** A
`specs:` list in `.dit/config.yaml` maps an `id` to a **path in the
repository**:

```yaml
repos:
  - { name: backend, remote: "git@github.com:acme/backend.git" }
specs:
  - { id: auth,    repo: backend, path: "services/auth/openapi.yaml" }
  - { id: legacy,                 path: "vendor/legacy-v2.json" }
```

`repo:` is optional and names an entry in `repos:`; omitted, it means this
workspace. It is what makes Morse work in **Mode A**, DIT's default, where the
spec is not in this repository at all — the spec is read through a git ref with
`show_text` exactly as §5 reads code, never merged and never checked out. The
pin is therefore a commit **in the repo that holds the spec**, because the
question a pin answers is whether the API has moved, and the API moves in the
code repo's history. Vendoring a copy into the workspace instead was rejected
twice over: the copy is derived data in the source of truth (I5), and the pin
would record the copy's history rather than the code's, so staleness would
measure the wrong thing.

This is forced, not a convenience: `operationId` is unique only within one
OpenAPI document, so two services both describing `createUser` is normal, and
an unqualified `operation: createUser` cannot resolve. A step therefore says
`operation: auth/loginUser`, and one scenario may cross services. `path` is
never a URL — a spec pulled from the network is a file DIT fetches of its own
accord, and vendoring it into the tree puts the change in a diff a person
reads. The registry is also what lets the Morse screen list a service with no
scenario yet.

**Endpoints are derived, not stored.** A step names an `operationId`; the
method, path, and schema come from the spec at reindex. The `commit` recorded
beside the spec path is what §7.4 already uses: when the spec has moved since,
the scenario is **stale**, and when an `operation` no longer resolves, the
scenario is **broken** — both computed at query time, never written into the
file (I5). An endpoint that exists in no spec is written inline in the fence
under `requests:`; that is authored content and belongs in the file.

**Chaining is declarative and there is no escape hatch.** `capture` maps a name
to a JSONPath selector, a header name, or a status. `expect` compares against a
literal or another variable. `{{name}}` substitutes into path parameters, query
values, headers, and body values — never into the host, the scheme, or the
port, so a captured response value cannot redirect the next request somewhere
else. There is no expression language, no `pre_request`, no `script`, no
`transform`. Anything the selectors cannot express becomes a new built-in
selector in a later version, discussed in a pull request against DIT — not a
line of code in a user's repo.

**Updating a spec deletes nothing and merges nothing.** The question an API
client forces — *does re-importing lose my edits?* — does not arise, because
there is no import and no imported copy. New spec bytes arrive as an ordinary
file change that git merges and a reviewer reads; the catalogue is recomputed
from HEAD; hand-written `requests:` never came from a spec and are untouched;
and an operation that vanished or was renamed makes its scenario **broken**,
named, never silently dropped.

**The pin is the only thing a person owns, and it never moves on its own.**
Reindex, the watcher and opening a document all leave `commit:` alone — a pin
that advanced by itself could never report staleness, which is the whole point
of recording it (§7.4's reasoning, unchanged). `dit morse sync <scenario>`
moves it: it re-resolves every operation against HEAD, **fires the scenario**,
and bumps the pin only if the chain comes back green. The pin then asserts
"proven to work at this commit", not "still structurally connected". The
consequence is accepted deliberately: `sync` needs a live environment and an
allowed host and therefore **cannot run in CI**, because CI moving pins on its
own is precisely the auto-firing below. CI runs `dit morse check`, which reads
and reports and never sends. Two people bumping the same pin is an ordinary
body conflict, resolved through §5.3's diff3, and a fence left with markers
degrades to ADR 0020's parse-failure banner rather than breaking the screen.

**Nothing fires a request except a person asking for one, in that moment.**
This is the whole of the I7 reconciliation, so it is stated as a list rather
than a principle:

- `dit reindex`, the file watcher, `dit doctor`, `dit validate`, `dit ready`,
  CI, the merge driver, and the indexer **never** issue a request. They parse
  the fence and store its shape. Parsing is not fetching.
- Opening the document in the UI, hovering a step, scrolling past the fence,
  and rendering the NodeView **never** issue a request. There is no preview
  that is secretly a call.
- The only two things that do are `dit morse run <scenario>` and the Run
  control on the Morse screen, both of which name what they are about to hit
  before they hit it.
- **A host must be allowed locally before it can be reached.** The allowlist
  lives in the gitignored local config, never in a committed file, on the same
  reasoning that keeps the merge driver out of `.dit/config.yaml` in §17.3. A
  scenario that arrives in a pull request pointing at an unfamiliar host does
  not run; it prints the host and asks. `http` and `https` only, and a redirect
  to a host outside the allowlist is not followed.
- **Secrets never enter the repo.** `env:` names an environment; its values
  live in gitignored local storage or the OS keychain. The fence may reference
  `{{token}}`; it may not contain one. `dit validate` flags a fence whose
  literal value looks like a credential.

**A run is derived.** Response bodies, timings, and pass/fail land in the
index and are gone at the next reindex. Nothing about a run is committed: a
response body is the most likely place in the whole product for a real
credential or real personal data to appear, and git history does not forget.
A scenario's last result is a fact about a machine at a moment, not about the
project.

**I7 is amended, and the amendment is narrow.** §17.3 currently reads "no field
may name … a URL that will be fetched automatically." The operative word was
always *automatically*, and it now has to carry weight it was not carrying
before, so it is restated: **no field in a DIT file may name anything DIT
executes or fetches of its own accord.** A URL that a person selects and fires,
against a host that person has separately allowed on that machine, is on the
same side of the line as `remote:` in `config.yaml` and `dit ai spec` — DIT
tells a human what it is about to do, and the human decides.

**The guard has to move with it**, because today it would not notice. The I7
test inspects only the two schema writers; a `url:` inside a fence is invisible
to it (Verification). It grows a third input, the Morse writer, where `url:`
and `operation:` are the only additions to the vocabulary and `script`,
`command`, `run`, `exec`, `hook`, and `shell` stay banned there too. The
`specs:` registry goes the other way and is caught by the guard as it stands:
`specs` and `path` are in neither list, so the allowlist assertion fires the
moment `write_config` emits them (Verification). That is the mechanism working
— adding a key to the config schema is meant to require a sentence in a pull
request, and `path` earns its place the way `remote` did, by being resolved
only against the repository and never fetched. A second
test asserts that no path from `reindex`, the watcher, `doctor`, `validate`, or
the server's read handlers can reach the HTTP adapter.

**Egress is confined to one crate**, the way I3 confines git. `dit-morse` is a
new adapter under `dit-core`; it is the only crate that may make a request
whose destination came from repo content, and the request types and the fence
grammar live in `dit-model`/`dit-parse`, I/O-free and wasm-clean (I4).

**Morse is the third item in the navigation**, after Home and Docs and before
Board. It does not appear until there is a screen behind it.

**The base URL comes from the spec's `servers:`, never from a DIT file.**
OpenAPI already states where the API lives, in a file maintained by the people
who maintain the API and which is not a DIT file. `env:` selects a server by
its description; a local override lives in `.dit/morse.local.yaml`, gitignored
by `dit init` on every branch it writes and reported as an error by
`dit doctor` if it is ever tracked. **Secret values never enter the repo.**
What a scenario commits is the *names* it needs — `requires: [email, password]`
— so a clone knows what to fill in without being handed anyone's key, and
`dit morse check` names a missing variable before anything runs. A committed
`environments.yaml` of base URLs was rejected: it is the shape I7 refuses, and
it would leave the local allowlist as the only thing between a pulled branch
and an outbound request. As it stands, no URL Morse can reach is ever
introduced by a committed DIT file.

**Egress containment becomes invariant I11**, not an extension of I7. The two
are different failures: I7 is about what a file may say and is caught by
reading the file; I11 is about who may call out and cannot be. It reads —
*only `dit-morse` makes an outbound request whose destination came from repo
content, and no read path can reach it* — and carries two checks,
`test_egress_is_contained` mirroring I3's crate containment, and
`test_no_read_path_reaches_egress` for reindex, the watcher, `doctor`,
`validate` and the server's read handlers. It is written down before the crate
exists because a containment test is nearly free before there is anything to
contain and expensive afterwards; ARCHITECTURE.md records that it is unguarded
until Morse 2 rather than leaving the gap to be discovered.

**Morse ships in two milestones, split on the network.** Morse 1 is read-only
and sends nothing: the registry, the fence parsed in `dit-parse`, the catalogue
and scenario shapes in the index, `dit morse check`, and a read-only screen.
None of I7's or I11's surface is touched, and the differentiator — staleness
against the spec's commit — is what lands first. Morse 2 adds the `dit-morse`
crate with I11's tests, the allowlist and environment storage, `run`, and
`sync`. The ordering is deliberate: the half that carries the security surface
is built against a data model that is already settled and already has users.

## Consequences

**Easier.** The scenario that proves registration works is reviewed in the pull
request that changes registration, by the same reviewer, in the same diff. When
the spec moves, every scenario written against it is told so — which is the
thing no API client does today, because no API client knows what commit it is
looking at. And a new contributor gets the collection by cloning, with no
invitation to a workspace and no exported JSON in a chat thread.

**Harder.** `dit morse sync` cannot run unattended, so pins advance only as
fast as someone runs scenarios against a real environment — a workspace that
never syncs accumulates stale scenarios, and "stale" stops carrying
information once everything is stale. `dit morse check` in CI is what keeps
that visible, and it is a report, not a gate anyone can satisfy automatically.
Declarative-only will feel narrow to anyone arriving from Postman,
and the first genuinely awkward case — an HMAC signature, a nonce, a multi-step
OAuth dance — will arrive as a request for scripting. The answer has to be a
built-in, every time, or the invariant is gone. Morse also makes DIT a program
that sends requests to hosts named in files, which is a sentence that has to
stay true in its narrowest reading forever; the list above is load-bearing and
each line of it needs its own test.

**No longer true.** ADR 0020's flat statement that a fence may never name a
`url:` holds for `dit-flow` and stops being a property of fences in general.
And §17.3's phrasing changes, which is a change to a document that other
sections cite — §17.3, ARCHITECTURE.md I7, and the test that enforces it move
together or not at all.

## Verification

Both facts below were produced by running, not by reading.

**The I7 guard would not have caught this.** It passes today:

```
$ cargo test --test invariants i7_
running 1 test
test i7_no_executable_fields_in_schema ... ok
```

And what it inspects is exactly two strings — `dit_parse::write_workflow` of
the default workflow and `dit_parse::write_config` of the default config
(`tests/invariants.rs:391-404`). `schema_keys` then pulls key tokens out of
that YAML and checks them against an allowlist plus a banned list containing
`url`, `script`, `run`, and `command`. A `url:` written inside a document body
— which is where a `dit-morse` fence lives — is not in either string and is
never seen. The invariant is real; its enforcement currently covers two files.
Extending the guard is therefore part of the decision, not a follow-up.

**The `specs:` registry trips the guard, which is the point.** Reading the two
lists out of `tests/invariants.rs` and checking the keys the registry needs:

```
specs      allowlisted=False banned=False
path       allowlisted=False banned=False
id         allowlisted=True  banned=False
url        allowlisted=False banned=True
```

`specs` and `path` appear in neither list, so `i7_no_executable_fields_in_schema`
fails on the allowlist assertion — not on the banned list — as soon as
`write_config` emits them. The registry cannot be added quietly. And `url` is
banned outright in config, which is the reason the registry stores a repository
path: a spec address that DIT could fetch has no place in a committed file.

**`operationId` is unique only within one document.** This is what forces the
namespace rather than making it a convenience — the OpenAPI specification
requires an `operationId` to be unique among the operations described by *that*
API, so two registered services both defining `createUser` is legal and
ordinary. An unqualified `operation: createUser` in a workspace with more than
one spec has no single answer, so `<spec id>/<operationId>` is the minimum that
resolves.

**Morse would be the first egress driven by repo content.** Searching the
workspace for an HTTP client finds `ureq` declared by exactly one crate,
`dit-cli`, and used in exactly one file:

```
$ grep -rn "ureq" crates/*/src/**/*.rs
crates/dit-cli/src/upgrade.rs:221:    let agent = ureq::Agent::config_builder()
```

Its destination is a compile-time constant — `const REPO_API: &str =
"https://api.github.com/repos/faridlab/dit-cli"`
(`crates/dit-cli/src/upgrade.rs:28`) — so no byte from any repository has ever
chosen where DIT connects. `dit-ai` declares no HTTP client at all. This is why
the allowlist is local and gitignored rather than a field in `.dit/config.yaml`:
there is no existing precedent to lean on, and the one adjacent mechanism that
does exist, the merge driver in §17.3, is kept out of the committed config for
precisely this reason.

**Not verified.** Whether JSONPath capture plus literal comparison is
sufficient for a real production auth chain has not been tested against a real
API. The register → login → me shape above is the common case and is clearly
covered; signed requests and OAuth redirects are not, and the first
implementation milestone should be pointed at a real service before the fence
grammar is frozen.
