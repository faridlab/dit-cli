---
id: 0014
title: "The release read model ships ahead of the verification engine"
status: accepted
date: 2026-09-13
supersedes: null
---

## Context

DESIGN.md §15 places releases in v0.9 and says why: the valuable half of the
feature — `dit release verify`, `dit release diff`, `dit release plan` — rests
on the commit-trailer linkage (v0.2), `field_events` (v0.4.5), the changelog
generator (v0.5) and tested polyrepo support (v0.7). Building the verification
engine before those exist means building on foundations that are still
shifting.

The roadmap view in the web UI needs something smaller and much earlier: the
list of releases with a date to draw each milestone at, the lifecycle status
to colour the lane, and the issues each release claims. `release.md` (§15.2)
already carries all of that except the date. Nothing in it needs git to prove
anything; it is a file a human wrote, read back.

The constraint in play is §15.4's warning: keep the scope narrow to what git
can prove, and do not rebuild "Jira, but worse". A read model that only
*shows* claims, and never asserts them verified, stays on the right side of
that line — as long as the UI does not present it as verification.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Wait for v0.9 and ship releases whole | None now | The roadmap has no milestones for four milestones' worth of time; teams keep them in a spreadsheet |
| Build the verification engine now | Trailer linkage and polyrepo are not there to build on | The engine would be rewritten when its inputs land — and a half-right verifier that says "verified" is worse than none |
| Store the roadmap's milestones outside `release.md` (config, a new file) | A second place releases live | Two truths for one release; the v0.9 writer would have to reconcile them |
| **Ship the read model of `release.md` now, plus one field and two edits** | A parser, an index table, two routes | The roadmap draws real files; v0.9 adds the engine on top of the same files without a migration |

## Decision

**The read model ships now; the verification engine still waits for v0.9.**

- `release.md` gains one optional field, `target: YYYY-MM-DD` — the planned
  date the roadmap draws the milestone at. It is validated like an issue's
  `due` and is a plan, not a record: the append-only deployment files (§15.2,
  v0.9) carry what actually happened.
- `dit-model` gets `Release`, `ReleaseStatus` (the five §15.2 values) and
  `ReleasePatch { status, target }`; `dit-parse` parses and surgically patches
  the file, preserving unknown keys and comments (invariant 8); `dit-store`
  knows the path (`.dit/releases/<version>/release.md`, under `.dit/` in
  every layout) and patches only through `Transaction`; `dit-index` holds
  `releases` and `release_includes`, filled by the same pipeline that indexes
  issues; `dit-core` exposes `releases()`, `release(version)` and
  `Transaction::set_release`.
- The server serves `GET /api/releases` and `PATCH /api/releases/{version}`
  with a body of `{ status?, target? }`. One patch is one commit with the
  `Dit-Author` trailer, exactly like an issue patch.
- There is deliberately **no create route and no `includes` edit**. Creating
  a plan and filling its scope is `dit release plan --from <DQL>` (§15.3),
  which belongs with the engine. Until then a plan is a file someone commits
  — the fixture in the tests does exactly that.
- The API never says "verified". `includes` is what the release *claims*;
  nothing in this read model checks the claim against a deployed ref.

## Consequences

- The roadmap can draw milestones today from files that will not change
  shape when v0.9 lands; the engine reads the same `release.md`.
- A version doubles as a directory name, so it is validated as one
  (`validate_release_version`): a closed character set, no leading dot or
  dash, no `..`. The rule lives in `dit-model` so the server, the CLI and the
  indexer cannot disagree.
- The index schema grew (`releases`, `release_includes`, and the
  `issue_blocked_by` side table that landed with it), so `INDEX_VERSION` is
  4 — an older cache is dropped and rebuilt, never migrated (§6).
- `release.md` files under `.dit/` are inside the history walker's pathspecs
  but are filtered out by shape, so they produce no `field_events`. When
  v0.9 wants "when did this enter UAT", it adds release events deliberately
  rather than inheriting accidental ones.
- The temptation this creates: a UI that shows a release as "released" looks
  like a statement of fact. The status is what someone typed; the roadmap
  must label it as such until `dit release verify` exists.

## Verification

`git ls-tree` over a prefix that does not exist is empty and succeeds, so a
workspace without `.dit/releases/` indexes cleanly:

```
$ git init -q --initial-branch=main . && git commit -q --allow-empty -m x
$ git ls-tree -r HEAD -- .dit/releases; echo "exit=$?"
exit=0
```

The history walker's diff pathspecs include `.dit/` in both layouts
(`DataLayout::diff_pathspecs`), and `looks_like_issue_body` rejects
`.dit/releases/v0.2.0/release.md`, so a plan file yields no field events —
pinned by `releases_are_indexed_from_git_and_patched_in_one_commit` in
`crates/dit-core/tests/core.rs`, which also asserts the `Dit-Author` trailer
on the patch commit and that unknown frontmatter survives the patch.
