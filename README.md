# DIT — Done in Git

> Project management that lives in your repo. Git isn't just the sync layer —
> it's the database, the audit log, and the permission system.

**Status: v0.1 slice running.** The CLI, the merge driver, and the browser UI
work end to end on real workspaces — dogfooding starts now. What is deliberately
not built yet: git hooks, commit-trailer integration, `dit validate`, the AI
layer, the block editor. See [`DESIGN.md`](DESIGN.md) §10 for the roadmap.

## What this is

A local-first project management tool that stores issues, epics, documents, and
changelogs as Markdown files inside a git repo — with a fast Rust index, a
Notion-grade block editor, a browser UI, GitLens-style history, and an AI layer
for changelogs and business-flow documentation.

Roughly: **Jira + Confluence + GitLens, merged, running locally, with git as the database.**

## Why

Git already provides, for free, what Jira built from scratch and sells:

| Need | What Jira built | What git already has |
|---|---|---|
| Change history | An activity log table | `git log`, `git blame` |
| Who changed what | An audit table | Author + committer + signature |
| Multi-device sync | A server + API | `fetch` / `push` |
| Offline work | Nothing | Native |
| Reviewing changes | Approval workflows | Pull requests |
| Permissions | Custom RBAC | Git host permissions + CODEOWNERS |
| Backup | A paid service | Every clone is a full backup |

## Getting started

```bash
rustup target add wasm32-unknown-unknown
cargo install just

just check      # runs every gate: fmt, clippy, tests, architecture, invariants, wasm
```

## Using it

```bash
cargo run -p dit-cli -- init              # make the current repo a workspace
cargo run -p dit-cli -- issue new "Fix the login flow" -P 1 -a budi -l area:auth
cargo run -p dit-cli -- list "status != done AND assignee = @me"
cargo run -p dit-cli -- ui                # board, detail, DQL search — in the browser
```

`dit ui` serves on 127.0.0.1, authenticates with a per-session token it
prints (and puts in the URL fragment once), and opens the browser. The same
workspace can be worked from the terminal and the browser at once — both go
through one `dit-core`.

Open in VS Code and accept the recommended extensions.

## Documents

| File | What it is |
|---|---|
| [`DESIGN.md`](DESIGN.md) | What is being built and why. 19 sections, with a table of contents and glossary. |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | How code is written and changed. Ten invariants, dependency rules, TDD policy. |
| [`CLAUDE.md`](CLAUDE.md) | Operating instructions for AI coding agents. |

**Read `ARCHITECTURE.md` §1 before your first PR.** Ten invariants are
non-negotiable, and each one is enforced by a test in `tests/invariants.rs`.

## Layout

```
crates/
  dit-model  dit-parse  dit-query      pure core — no I/O, compiles to wasm32
  dit-store  dit-index  dit-vcs  dit-ai  adapters — touch the outside world
  dit-core                              facade — the only public API
  dit-cli    dit-server  dit-wasm       delivery
tests/
  architecture.rs   dependency direction
  invariants.rs     invariants I1–I10
apps/web/                               React + TypeScript UI
```

## License

Apache-2.0
