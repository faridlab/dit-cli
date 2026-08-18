# DIT — Done in Git

> Project management that lives in your repo. Git isn't just the sync layer —
> it's the database, the audit log, and the permission system.

DIT stores issues, epics, documents, and changelogs as Markdown files inside a
git repo. A fast local index answers queries; a frontmatter-aware merge driver
merges two people's edits to the same issue field-by-field; one `dit` binary
serves the browser UI itself. No server to run, no account to create — every
clone is the whole workspace.

Roughly: **Jira + Confluence + GitLens, merged, running locally, with git as
the database.**

## Status

**v0.1 slice running.** The CLI, the merge driver, and the browser UI
work end to end on real workspaces — dogfooding starts now. What is deliberately
not built yet: git hooks, commit-trailer integration, `dit validate`, the AI
layer, the block editor. See [`DESIGN.md`](DESIGN.md) §10 for the roadmap.

## Install

Requires git. The installer downloads a release binary for your platform and
falls back to building from source when there isn't one:

```bash
curl -fsSL https://raw.githubusercontent.com/faridlab/dit-cli/main/scripts/install.sh | bash
```

From source:

```bash
git clone https://github.com/faridlab/dit-cli
cd dit-cli
npm ci --prefix apps/web && npm run build --prefix apps/web   # embeds the UI
cargo install --path crates/dit-cli --features embed-ui --locked
```

Without the npm step the binary still works from the terminal — `dit ui` just
won't serve pages until it is built with `--features embed-ui`.

## 30-second quickstart

```bash
mkdir my-tracker && cd my-tracker   # any git repo becomes the workspace
dit init                             # git init, merge driver, README
dit issue new "Fix the login flow" -P p1 -a budi -l area:auth
dit list "status != done AND assignee = @me"
dit ui                               # board, detail, DQL search — in the browser
```

Everything you just made is Markdown — `issues/`, `docs/`, `notes/`, `changelogs/` at the
root, machinery in `.dit/` — one commit per change. Push it, clone it elsewhere, and the
workspace travels with the repo.

## Merging is the product

Two people edit the same issue on different machines. Both `dit sync`. The
merge driver reads the frontmatter on all three sides and merges field-by-field:
status moved by one, assignee by the other — both land. Only a genuine edit to
the same field leaves conflict markers, as a state git already knows how to
finish, never a corrupt file.

## Commands at a glance

| Category | Command | What it does |
| --- | --- | --- |
| **Workspace** | `dit init` | Make the current directory a workspace: git init, merge driver, README. |
|  | `dit doctor` | Check everything that silently breaks a workspace when wrong. |
|  | `dit status` | Branch, head and working-tree state. |
|  | `dit sync` | Fetch, rebase onto the remote, push. Exits 1 when files need a human. |
|  | `dit reindex` | Rebuild the local index from git. |
|  | `dit install-driver` | Register this binary as the repository's merge driver. |
| **Issues** | `dit issue new <title>` | Create — kind, status, priority, assignees, labels, estimate, body. |
|  | `dit issue show <ref>` | One issue: fields, body, comments, field history. |
|  | `dit issue set <ref> field=value…` | Change fields, e.g. `status=done labels=a,b`. |
|  | `dit issue comment <ref> <text>` | Add a comment. |
|  | `dit list [DQL]` | List issues matching a query; no query = all. |
|  | `dit board` | The board: one column per workflow status. |
| **UI** | `dit ui` | Serve this workspace to the browser and open it. |

`dit ui` serves on 127.0.0.1, authenticates with a per-session token it
prints (and puts in the URL fragment once), and opens the browser. The same
workspace can be worked from the terminal and the browser at once — both go
through one `dit-core`.

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

## Documents

| File | What it is |
|---|---|
| [`DESIGN.md`](DESIGN.md) | What is being built and why. 19 sections, with a table of contents and glossary. |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | How code is written and changed. Ten invariants, dependency rules, TDD policy. |
| [`CLAUDE.md`](CLAUDE.md) | Operating instructions for AI coding agents. |

**Read `ARCHITECTURE.md` §1 before your first PR.** Ten invariants are
non-negotiable, and each one is enforced by a test in `tests/invariants.rs`.
For development: `rustup target add wasm32-unknown-unknown`, `cargo install
just`, then `just check` runs every gate — fmt, clippy, tests, architecture,
invariants, wasm. Open in VS Code and accept the recommended extensions.

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
scripts/
  install.sh                           the curl | bash installer
```

## License

Apache-2.0
