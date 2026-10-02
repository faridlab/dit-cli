---
id: 0027
title: "Morse authoring: environments edited from the page but never trusted, new requests by method and path, imports, and every Postman body shape"
status: accepted
date: 2026-10-03
supersedes: null
---

## Context

ADR 0023 gave Morse a Postman-shaped workbench, but authoring stopped at the
edge of the OpenAPI catalogue and of the terminal. Everything below came from
using it as a Postman replacement:

- **A request no spec describes cannot be made from the page.** DESIGN §20.2
  already allows `requests:` — method and path — inside a fence; the screen
  only opens tabs for catalogue operations, and Send refuses inline requests
  (an implementation choice in `routes.rs`, not a rule).
- **Environments are read-only in the browser.** ADR 0023: the screen
  "does not read a value, and it cannot write the local file". Every new
  environment, every changed password, means opening a terminal or an editor.
  For the people the desktop app is for, that is a dead end.
- **Bodies are JSON only.** Real APIs take `application/x-www-form-urlencoded`
  (OAuth token endpoints), raw XML, plain text, and `multipart/form-data` with
  files. Postman offers all of them; Morse can send none.
- **There is no way in.** Teams arriving from Postman have collections and
  environments, and every developer has a `curl` line in a README. Each has
  to be retyped as a fence.
- Steps cannot be deleted, renamed, duplicated or reordered, and a scenario
  cannot be renamed or deleted, from the screen — only by editing the fence.

The rules these collide with are the ones that keep a pull request from
aiming DIT at someone: I7 (no committed field names an address that is
fetched), I11 (only `dit-morse` sends, and no read path reaches it), §20.5
(nothing fires on its own; the allowlist is the trust boundary), §20.6
(secrets never enter the repo).

## Options considered

| Question | Options | Chosen |
|---|---|---|
| Environments in the browser | read-only (0023) · values editable, allowlist not · everything | **values editable, allowlist not** |
| A request outside the spec | spec only · method + path in the page, host never | **method + path** |
| Imports | none · cURL · Postman collection · Postman environment | **all three** |
| Bodies | JSON · + form + raw · + multipart with files | **JSON, form, raw with a type, multipart with files** |

Letting the page edit the allowlist was rejected: the allowlist is the only
thing between an injected script and "send this to any host", and §20.5 rests
on the browser never being able to grant trust. It stays in `dit morse allow`
and, later, a confirmation the desktop tray shows outside the page.

## Decision

**1. Environments are edited from the page, and trusted from nowhere but
this machine's terminal.** The screen may create, rename and delete an
environment and set its `server:` and variable values in
`.dit/morse.local.yaml` — the git-ignored file §20.6 already puts them in.
Values are **write-only**: the page can set or clear one and learns only
whether it is set; nothing returns a value. The `allow_hosts:` list is not
writable from the page. So a page may point an environment at a new server,
and nothing will be sent there until a person on this machine allows that
host. The file is written by `dit-core` through `dit-store::atomic` — it is
local configuration, never committed, so it has no `Transaction` and no
commit, exactly like `dit morse allow` today.

**2. A request is a method and a path.** "New request" picks a spec (whose
`servers:` or environment gives the host), a method from the closed list, and
a path. It is saved as an entry in a scenario's `requests:` (§20.2) and sent
from its tab like an operation. The host is never typed in the page and never
stored: the page's draft has no field that could hold one, the same as ADR
0023's operation drafts. A Send of an inline request is a one-step plan
through `dit-morse` with every gate of a Send.

**3. Imports convert once and keep nothing they cannot hold.** Each import
produces fences (or, for environments, local values) and a report of what was
dropped:

- **cURL.** The method, path, query, headers and body are kept. The host must
  match a registered spec's server or an environment's server — the path is
  then matched to a catalogue operation (method + path template) or becomes
  an inline request. An `Authorization` or cookie value is never kept: it
  becomes `{{token}}` (or `{{cookie}}`), named in `requires:`.
- **Postman collection (v2.1).** One document, one scenario per folder,
  one step per request, in order. `{{baseUrl}}`-style host variables become
  the spec or environment host; other `{{var}}`s keep their names and are
  listed in `requires:`. **Scripts are dropped** — pre-request and test
  scripts are exactly the escape hatch ADR 0022 refuses — and every dropped
  script is listed by request name, so nobody discovers it later. Postman
  tests that are a plain status check are kept as `expect.status`.
- **Postman environment.** Becomes an environment in the local file. A
  variable that holds a URL and is used as the host becomes `server:`; the
  rest become variables. Nothing is committed.

**4. A step carries one of four body shapes.** Exactly one may appear:

```yaml
body: { email: "{{email}}" }                 # JSON — unchanged
form: { grant_type: password, user: "{{u}}" } # x-www-form-urlencoded
raw:                                          # any text, with its type
  type: application/xml
  text: "<login><u>{{u}}</u></login>"
multipart:                                    # multipart/form-data
  - { name: title, value: "{{title}}" }
  - { name: avatar, file: fixtures/avatar.png, type: image/png }
```

`raw` may name a `file:` instead of `text:` (Postman's "binary"). The type is
a media type with an optional charset — no line breaks, so it cannot smuggle
a header. Values substitute `{{variables}}` as everywhere else.

**A file part is read from the repository at HEAD, never from the working
tree.** The path is relative, may not climb (`..`), may not be absolute and
may not enter `.dit/` or `.git/`. Reading the committed blob rather than the
file means a git-ignored file — `.dit/morse.local.yaml` with its secrets, a
`.env` — cannot be named by a fence in a pull request and sent to an allowed
host: it is not in HEAD. A file part is at most 10 MB. The bytes are read by
`dit-core` (through `dit-vcs`) and handed to `dit-morse` in the plan, so the
sender still reads nothing on its own.

**5. Scenarios and steps are managed from the screen.** Rename or delete a
scenario; rename, duplicate, delete and reorder steps; change a step's target
operation; edit `env:` and `requires:`. All of it is the fence writer
(ADR 0023) rewriting the fence through `Transaction` + `write_doc`, one
commit per change, the prose around the fence untouched. A fence with a `#`
comment stays read-only from the screen, as before.

## Consequences

- ADR 0023's "the screen … cannot write the local file" is replaced by
  decision 1; its "method and path come from the spec, never from the page"
  is narrowed to "the host never comes from the page" (decision 2). The
  allowlist rule is unchanged.
- A compromised page can now set an environment's server and variable values.
  It still cannot cause a request to a host this machine has not allowed, and
  it still cannot read a value. It could overwrite a value with a wrong one —
  a nuisance a person notices, not an exfiltration.
- Fences gain three keys (`form`, `raw`, `multipart`); a fence written before
  this ADR parses unchanged. An older DIT reading a newer fence reports the
  unknown key rather than dropping the body (§18).
- Imports are a migration path, not a sync: re-importing a collection writes
  new fences. Nothing tracks the Postman original.
- `dit morse` gains CLI twins: `env set|unset|rm`, `import curl|postman|postman-env`,
  so an agent can do what the page does.

## Verification

Fence grammar round-trips are pinned in `dit-parse` (each body shape parses
and is written back to the same fence; two shapes in one step, a type with a
line break, and a climbing file path are refused). `dit-core` pins that a
file part is read from HEAD (an ignored file and an uncommitted change are
both refused), that an environment value is never returned, and that the
allowlist is untouched by every page-facing operation. The server's security
suite pins that no morse route returns a variable value.
