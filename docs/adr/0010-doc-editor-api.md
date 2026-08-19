---
id: 0010
title: "The doc editor reads pages from the file tree; issue reads stay index-only"
status: accepted
date: 2026-08-19
---

## Context

DESIGN.md §13 specifies the document layer — pages are plain Markdown under
the four doc roots (`docs/`, `notes/`, `epics/`, `changelogs/`), history is
`git log`, nothing about a page is stored anywhere but the file. §16.3
already reserves the write shape: `Transaction::write_doc`.

The web UI now needs the editor (list, read, create, edit, delete), which
raises one architectural question: **I2 says reads never touch files on
disk.** The index has no doc tables — §13 routes doc labels, wiki-links and
mentions through "Index", but none of that is built. A doc read today has
no index to go through.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Build the doc index first, serve list/read from it | Large: schema + indexer + reindex tiers before any UI lands | Correct end-state, but couples the editor to work §13 does not need yet; pages have no queryable fields until labels exist |
| Serve docs from the file tree, writes through `Transaction` | Small; the file tree is already the source of truth | A read path that opens files — needs the I2 boundary written down (this ADR) |
| No editor until the doc index exists | None | The layer stays unreachable except by hand-editing files |

## Decision

**The doc read path is file-backed; the write path is exactly the issue
write path.** Concretely:

- `Dit::list_docs()` walks the four doc roots through the layout (so Mode C
  `.dit/`-prefixed workspaces work unchanged) and returns entries with
  filesystem metadata (mtime, size). Files whose names fall outside
  `DocPath` rules are skipped, not fatal — one hand-named `BADNAME.MD`
  cannot break the listing.
- `Dit::read_doc(path)` reads the file. `mtime` is display metadata read
  from the filesystem, never written anywhere; the page's real history is
  git (§13).
- Writes go through `Transaction::write_doc` / `delete_doc` → staged in the
  store transaction, formatted by `dit fmt` (invariant 1), landed as one
  git commit with the index absorb unchanged. Deletions ride the same
  `Changeset` rollback semantics as writes: a failed transaction restores
  deleted bytes exactly.
- The path is the attack surface, so it is a **value object in the pure
  core**: `dit_model::DocPath` validates once — doc-root allowlist (`issues`
  excluded: issue bodies carry schema-owned frontmatter), slug-safe
  segments, `.md` only, no traversal shapes — and the store joins it onto
  the layout's content dir without re-checking. `Server` / CLI / wasm all
  call the same parse.

**I2's scope is now explicit:** "reads never touch files" governs the issue
read surface (`get`, `query`, `board`, `history`, `comments` — the
`test_reads_survive_worktree_wipe` proof). Doc list/read are a file browser
over the source of truth itself. When the doc index lands (labels,
wiki-links, backlinks per §13), `list_docs`/`read_doc` move under I2 with
it and this ADR is superseded.

## Consequences

- Staleness (§7.4 `dit docs check`) is unaffected — it was always git-based.
- `delete_doc` of a missing page is `DitError::NotFound` (404 at delivery),
  and malformed paths are `DitError::DocPath` (400) — the editor can show
  both inline.
- Renaming a page is out of scope for v1: it is delete-plus-create in two
  commits, and §13's wiki-link resolution would need `git mv` semantics
  first (the reason `Repo::mv` exists for layout migration).
- Empty directories left by deletions are not pruned: git does not track
  them, and the walk skips missing roots anyway.

## Verification

Fixtures on both sides of each boundary: `DocPath` traversal/shape suite in
dit-model; `write_doc_creates_a_formatted_page_under_the_right_root`,
`remove_doc_deletes_the_file_and_rollback_restores_it`,
`write_and_remove_in_one_transaction_last_write_wins`,
`write_doc_in_a_dotdir_workspace_lands_under_dot_dit` in dit-store;
`a_saved_doc_lands_as_one_commit_and_reads_back`,
`a_deleted_doc_disappears_in_one_commit`,
`doc_paths_are_sandboxed_at_the_facade`,
`list_docs_skips_names_the_editor_cannot_address` in dit-core. The server
suite pins the HTTP shapes (list/read/put/delete plus traversal-as-400).
