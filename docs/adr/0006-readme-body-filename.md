---
id: 0006
title: README.md is the body file of an issue or doc page
status: accepted
date: 2026-08-18
supersedes: null
---

## Context

The body file was `issue.md` (issues) and `page.md` (doc pages). On a forge,
browsing into an issue folder showed only a file listing — `issue.md` renders
only after an explicit click. Combined with ADR 0005's content-at-root layout,
the natural read path on a forge becomes: repo home → `issues/` → month →
issue folder. The last step should show the issue itself, not a filename.

Forge behavior being relied on (GitHub, and GitLab equivalents):

- A directory's `README.md` renders automatically below the file listing.
- A rendered markdown file shows its YAML frontmatter as a table — which for an
  issue means title, status, assignees, labels appear as a card above the body,
  with zero hosting effort.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Keep `issue.md` / `page.md` | One extra click per issue on every forge; no frontmatter card below listings | Filenames self-describe when grepping blind |
| `index.md` | Same auto-render intent | Not rendered below listings on GitHub; `README.md` is the convention every forge commits to |
| **`README.md`** | Discovery rule changes in `dit-parse`/`dit-index`; migration rename | **Chosen** |

## Decision

The body of an aggregate (issue, epic, doc page, flow) is **`README.md`** inside
its folder. Comments keep their `<ulid>-<author>.md` names (§4.4).

One reserved collision: `issues/README.md` (and the root `README.md`) belong to
the generated index (ADR 0008) — the indexer must never treat a generated index
as an issue or doc page.

## Consequences

- Folder shape is the type signal now: `…/<id>-<slug>/README.md` is an issue
  body, `…/<slug>/README.md` under `docs/`/`notes/` is a page. The path, not the
  filename, carries the meaning — which is already how sharding works.
- `grep -r status: issues` still works identically; targeted greps change from
  `**/issue.md` to `**/README.md`.
- Cheap now (pre-1.0, one pilot), a breaking format change after — same window
  argument as ADR 0005.

## Verification

The forge-rendering claims are about hosted web UIs, not git — they cannot be
verified from a terminal. They are to be confirmed on the `serpa-dit` pilot
push after migration: browse into an issue folder on GitHub and observe the
frontmatter table + body rendering below the listing. If any claim fails there,
this ADR is revisited before the format freezes.
