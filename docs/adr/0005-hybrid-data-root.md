---
id: 0005
title: Content at the tree root, machinery in .dit/ (Modes A & B)
status: accepted
date: 2026-08-18
supersedes: null
---

## Context

§5.0 already admits the cost: "Everything in the DIT repo lives under the hidden
`.dit/` directory. `ls` on a fresh clone shows what looks like an empty
directory … it surprises anyone opening the repo for the first time." The
mitigation was a static root `README.md`.

The pilot repo (`serpa-dit`, tracking the Serpa project) shows the mitigation is
not enough. Browsing it on a forge shows an apparently empty repository; the
actual data is hidden, and reaching it means walking `.dit/issues/2026/08/` past
ULID folder names. Principle 1 says the plain text is supposed to be the thing
humans read — the layout was fighting the principle.

This matters beyond aesthetics because §5.0 positions Mode A for non-software
users (research, marketing, personal). For them, a hidden dot-directory is not
a convention they know; a repo whose root shows `issues/`, `notes/`, `docs/` is.

The original reason for `.dit/` everywhere was path identity across all three
modes (§5.0, §5.1), so `dit-store` needs one path abstraction with no
branching. That is a real engineering benefit — but it was bought with the
usability cost above, and only **Mode C** has a *structural* need for the
prefix: it is the one mode where DIT is a guest inside a code repo that may
already have its own `docs/` directory.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Keep `.dit/` everywhere (status quo) | Usability cost admitted in §5.0; pilot confirms it | One layout in code, tests, docs |
| Everything at the tree root in Mode A | Root mixes content with machinery (`schema/`, `people/`, `config.yaml` read like project files) | Friendliest `ls`, but machinery pollutes the content view |
| **Content at root, machinery in `.dit/`** | ADR + `DESIGN.md` §4.1/§5.0/§5.1 rewrite; `dit-store` gains one branch point; pilot migration | **Chosen** |

## Decision

In **Modes A and B** — the modes where DIT owns the tree — human content lives
at the root of that tree, machinery stays under `.dit/`:

- **Content roots (visible):** `issues/`, `epics/`, `docs/`, `notes/`,
  `changelogs/`.
- **Machinery (hidden):** `.dit/config.yaml`, `.dit/schema/`, `.dit/people/`,
  `.dit/views/`, `.dit/state/`, `.dit/releases/`, `.dit/archive/`,
  `.dit/.migrations/`.

`notes/` is a first-class docs space (§13): dated pages (`2026-08-17.md`) and
topic pages (`learning-dsa.md`). The forge-browsing story is completed by
`README.md` bodies (ADR 0006) and the generated index (ADR 0008).

**Mode C keeps everything under `.dit/`** — the guest mode. Mode B's orphan
branch follows Mode A's layout at the branch root; §5.1's "paths exactly
identical in both modes" becomes "identical across A & B; C prefixes every root
with `.dit/`". The fork is principled: *DIT-owned tree* vs *guest in someone
else's tree*.

The merge-driver attributes file follows the same rule: **`.gitattributes` at
the tree root in Modes A & B** (DIT owns the root there), `.dit/.gitattributes`
in Mode C. `dit doctor` checks it at the mode-correct path.

The five content-root names are reserved. `dit init` refuses to lay out into a
tree where one of them already exists and is not DIT-shaped.

**The layout is one boolean, not a free-form path.** `config.yaml` records
`layout: root | dotdir` — `root` is the default for Modes A & B, `dotdir` for
Mode C (and available in A for users who prefer the hidden style). `dit init`
shows the exact tree it is about to write and asks for confirmation when the
target is non-empty; `dit ui` → Settings exposes the same field, and changing
it offers a guided migration (`git mv` + reindex + attributes-file move).
Arbitrary custom roots are deliberately refused: every consumer of the layout
(indexer, merge driver, wiki-links, docs) branches on one bit, or it branches
on a thousand configs.

## Consequences

- `dit-store` resolves paths through a data-root abstraction with exactly one
  branch point (DIT-owned tree vs guest). Every other crate sees resolved paths.
- Per-branch `.gitignore` in Mode B is unchanged (`.dit-cache/` +
  `.dit-worktree/` on the code branch, `.dit-cache/` on the data branch).
- Wiki-links are unaffected: §13's unique-suffix resolution never depended on
  the prefix.
- Existing Mode A/B repos migrate with `git mv` + reindex; `dit` offers this
  guided migration both at init-time mistakes and from `dit ui` Settings (the
  `layout:` flip). Cheap now — pre-1.0, one pilot repo exists.
- Mode C users see no change at all.

## Verification

The move of `.gitattributes` out from under `.dit/` re-opens §5.3's
anchored-pattern pitfall, so it was re-run in a throwaway repo (git 2.42.0,
macOS) rather than assumed:

```
$ printf '**/comments/*.md merge=dit-md\n' > .gitattributes
$ git check-attr merge -- \
    issues/2026/08/01M08FFCAG-185N-fix-login/comments/01M08FFCAX-farid.md \
    issues/2026/08/01M08FFCAG-185N-fix-login/README.md
issues/2026/08/01M08FFCAG-185N-fix-login/comments/01M08FFCAX-farid.md: merge: dit-md
issues/2026/08/01M08FFCAG-185N-fix-login/README.md: merge: unspecified
```

A root-level attributes file with `**/comments/*.md` still reaches the deep
comment files, and does not touch the issue body. The single-star form
(`comments/*`) remains wrong for the same reason §5.3 documents.
