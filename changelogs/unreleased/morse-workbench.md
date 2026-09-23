# Morse becomes a workbench

The Morse screen was a grid of spec cards. It is now laid out the way people
arriving from Postman expect — an explorer, tabs, a request over its
response — while everything that made Morse different stays where it was
(ADR 0023, DESIGN.md §20.9).

- **An explorer of specs, scenarios, environments and runs.** Specs are
  grouped by their OpenAPI tags, so a workspace with thousands of operations
  can be browsed; a filter searches every operation by path, operationId or
  summary. Scenarios are grouped by the document holding their fence.
- **Tabs.** A single click opens a preview tab, a double click keeps it, and
  an edited draft is marked and asks before it is thrown away.
- **Send one operation.** An operation opens as a draft pre-filled from the
  spec — path parameters, query parameters, the required body fields, the
  documented success status — and Send fires it through the same gates as a
  run: the method and path come from the spec, the server from the spec or
  this machine's environment, and the host must be allowed here. The page
  still cannot trust a host. `dit morse send <spec>/<operationId>` is the
  terminal twin, with `--env`, `--param`, `--query`, `--header` and `--body`.
- **Expect and Capture instead of scripts.** Status, JSONPath checks and
  captures from the body, a header or the status. There is no Scripts tab.
- **The response panel shows what crossed the boundary** — status, time,
  size, each check, the names of what was captured — and never the body or a
  captured value. It hands over the terminal command that prints them;
  `dit morse run` and `send` now print the response body there.
- **Save to scenario.** A draft becomes a step in an existing scenario, or
  the first step of a new one appended to a document and pinned where the
  spec stands now. One commit, through the same transaction a document save
  uses. Names the step reads that nothing provides are added to `requires:`.
- **Steps are edited in place.** A step's tab writes back to its fence after
  a pause, one commit per pause, and only the fence's lines change — the
  prose around it is left byte for byte. A fence carrying a `#` comment is
  read-only here, because rewriting it would drop the comment.
- **A credential written out is refused before it is committed**, in the
  form and on the server, with the same rule `dit doctor` applies.
- **Environments are listed by name.** The picker, the environment page and
  the allowlist show names, servers and which variables are set — never a
  value — and none of them can change the local file.

Fixed:

- **A path parameter was sent literally.** A step calling `GET /parties/{id}`
  sent `{id}` to the server, because nothing filled OpenAPI's single-brace
  segments. A step now names them in `params:`, each value percent-encoded
  as one segment; an unfilled one is refused before anything is sent, and
  `dit morse check` reports the scenario broken for it. This affected 29% of
  the operations in the workspace that found it.

Supporting changes:

- The catalogue now carries each operation's tag, parameters, body fields and
  response codes, and each spec's `servers:`, resolved through local `$ref`s.
  A `$ref` to another document is not followed — that would be a fetch.
  The index version moved, so the first start after upgrading rebuilds it.
- I7's guard now inspects the fence writer's output, and I11 gained its
  second check: no server handler that answers through the read path can
  reach anything that sends.
