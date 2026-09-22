# Morse — API scenarios in the repository

A workspace can now keep its API scenarios where the code is, and be told
when they stop matching the API. This is the read-only half: nothing here
sends a request.

- **Specs are registered, not imported.** `specs:` in `.dit/config.yaml`
  maps an id to an OpenAPI document — `{ id: auth, path: api/openapi.yaml }`,
  or `{ id: auth, repo: backend, path: ... }` for a document in a linked
  code repo. The endpoint catalogue is derived from that document at every
  reindex and never copied into a DIT file, so updating the document is an
  ordinary file change with nothing to merge or delete. The path is a path
  inside a repository; a URL is refused, because a file DIT could fetch on
  its own accord is remote code execution by pull request.
- **Scenarios are a `dit-morse` fence** in any document, naming the spec
  they are pinned to, the steps they walk, what each step expects, and what
  it carries forward. Chaining is by selector — a JSONPath, a response
  header, the status — with no expression language and no script hook. The
  keys that would become one are refused by name.
- **`dit morse check`** reports every scenario as fresh, stale, broken or
  unreadable, and exits non-zero on the last two. Stale carries the number
  of commits the spec has moved since the pin; it does not fail the check,
  because the world moving is a fact rather than a fault. Broken names the
  step: an operation the document no longer describes, an unregistered
  spec, or a value read before the step that captures it.
- **`dit morse specs` / `dit morse operations <spec>`** list the catalogue.
- **Endpoints no document describes** are written inline in the fence under
  `requests:`, and a step calls one with `request:` instead of `operation:`.
  They carry a path, never a URL — a path on the server the scenario's spec
  names — so no committed file introduces an address.
- **`dit doctor` gained two Morse checks.** `morse-env`: the environment file
  is gitignored from `dit init` onwards, and being tracked is an error rather
  than a warning. `morse-secrets`: a fence carrying a literal that looks like
  a real credential is an error, named down to the document, line, step and
  field — while `Bearer {{token}}` passes silently.
- **A Morse screen** sits third in the rail, after Home and Docs, with the
  same information and no Run control — running a scenario comes later, and
  a button that did nothing would be worse than none.

Supporting changes:

- The YAML reader understands block scalars (`|`, `>`, with chomping), which
  real OpenAPI documents use for descriptions and which previously failed
  the whole file as inconsistent indentation. A JSON reader was added
  alongside it, so `openapi.json` is read as readily as `openapi.yaml`.
- A new invariant, **I11**: only `dit-morse` may make an outbound request
  whose destination came from repo content, and no read path can reach it.
  Its containment test ships now, before the crate it guards exists.
