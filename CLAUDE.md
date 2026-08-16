# CLAUDE.md

Instructions for Claude Code in the DIT repo. Read this in full before changing any code.

**DIT (Done in Git)** — a local-first project management tool. The source of truth is Markdown files inside a git repo; SQLite is only a disposable index. Rust + React, running as a local server accessed through the browser.

Reference documents:
- `DESIGN.md` — what is being built and why (19 sections, with a table of contents & glossary)
- `ARCHITECTURE.md` — the complete rules and the reasoning behind them

---

## Verify before finishing

```bash
just check
```

One command, every gate: fmt, clippy, test, architecture check, wasm32 target, `cargo deny`, JS licenses.

**Never report work as done without a green `just check`.**

---

## Ten invariants — violating these breaks the product

Before writing code, check whether the change touches any of these:

1. **Write only through `Transaction` + `dit fmt`.** No `std::fs::write` outside `dit-store::atomic`.
2. **Read only through the index.** The read path never touches files on disk.
3. **Only `dit-vcs` talks to git.** No `Command::new("git")` or `gix` in any other crate.
4. **`dit-model` / `dit-parse` / `dit-query` must compile to `wasm32` and be I/O-free.**
5. **Derived data never enters the source of truth.** commit↔issue links, time-in-status, `repo:`, document staleness — all computed, never stored.
6. **The merge driver never leaves `%A` as-is on failure.** Even a panic must write diff3 conflict markers. This is the only path that can silently delete someone's work.
7. **No field in a DIT file names an executable, a shell command, or a URL that is fetched automatically.** That is RCE via pull request.
8. **Unknown frontmatter fields are preserved as-is** across round-trips.
9. **`field_events` is ordered by `seq`, never `ts`.** `ORDER BY ts` produces contradictory duplicate rows.
10. **comrak's `render.unsafe_` is never enabled.** Strict CSP in `dit-server`.

**If a task requires violating any of them: stop and tell the user.** That is a design change, not a code change — it needs an ADR and a `DESIGN.md` update first. Do not do it on your own.

---

## Workflow

```
1. Read the relevant DESIGN.md section
2. Check invariants 1–10
3. Write a FAILING test — make sure it fails for the right reason
4. Implement the minimum needed to turn it green
5. just check
6. Commit: conventional commit + trailer "Closes: #<ref>"
```

### TDD: where it is mandatory

| Crate | Rule |
|---|---|
| `dit-model`, `dit-parse`, `dit-query`, `dit-store`, `dit-vcs`, `dit-index`, `dit-core` | **Test-first mandatory** |
| `dit-server` security paths (token, `Host` header, CSP) | **Test-first mandatory** |
| `dit-ai`, `dit-cli`, the rest of `dit-server` | Test after implementation |
| `apps/web` | Not mandatory; test behavior, not rendering |

### Absolute rule

> **Every bug fix ships with a fixture that reproduces it.** No exceptions.

---

## Dependency direction

```
dit-model ← dit-parse ← dit-query            (pure, wasm, no I/O)
      ↑
dit-store, dit-index, dit-vcs, dit-ai        (adapters)
      ↑
dit-core                                     (facade — the only public API)
      ↑
dit-cli, dit-server, dit-wasm                (delivery)
```

Dependencies only point upward. Delivery must never call an adapter directly — always through `dit-core`. Adding an edge means editing the list in `tests/architecture.rs`, and that is meant to be discussed.

---

## Do not do this

| Don't | Instead |
|---|---|
| Create a `trait FooRepository` | `Dit` has separate read and write surfaces — that is deliberate |
| Create a trait with one implementor | Use a concrete type. A trait needs ≥ 2 real implementors |
| Use mocks | Real fixtures: a git repo in a tempdir, in-memory SQLite, recorded LLM responses |
| `unwrap()` / `expect()` / `panic!` in library crates | Return `Result`. Allowed only in `#[cfg(test)]` and `fn main` |
| `println!` in a library | `tracing`. Only `dit-cli` prints |
| Add an event bus | Git is already the event log. Use `Dit::subscribe()` |
| Treat a merge conflict as `Err` | A conflict is a **state**, a field on `SyncReport` — not a failure |
| Chase coverage numbers | Coverage is reported, never a gate |
| Add an editor block without a markdown equivalent | The schema must be a strict subset of CommonMark + `dit-*` fenced blocks |
| Add a dependency silently | State the reason. Pre-1.0 crates must be pinned and recorded in `DESIGN.md` §9 |

---

## Naming — use glossary terms exactly

| Use | Don't |
|---|---|
| `issue` | ticket, task, card, item |
| `short_ref` | slug_id, key, code |
| `seq` | order, position, index |
| `field_events` | history, audit_log, activity |
| `workspace` | project, instance |

Full glossary: `DESIGN.md` Appendix D.

---

## Where to find what

| Question | Section |
|---|---|
| Why Markdown and not SQLite as the source of truth? | `DESIGN.md` §3 |
| Folder structure, frontmatter schema, ID rules | §4 |
| Deployment modes, branch strategy, conflict handling | §5 |
| Crate map, git library, indexing pipeline, DQL | §6 |
| AI providers, changelog, flow documents | §7 |
| Risk list | §9 |
| Roadmap & milestones | §10 |
| Editor, `dit fmt`, custom blocks | §12 |
| `field_events`, blame, time travel | §14 |
| Release plan | §15 |
| **`dit-core` API shape** | §16 |
| **Hostile input, prompt injection, XSS** | §17 |
| Schema versioning & migration | §18 |
| Testing strategy & fixture list | §19 |

---

## When in doubt

- **In doubt about design** → ask the user. Don't guess and then build on the guess.
- **In doubt whether some git/SQLite behavior is correct** → **run it first in a throwaway repo.** Three review rounds of `DESIGN.md` found 71 bugs, nearly all of them by running the commands, not by reading them. Including things that felt obviously right, like "a ULID prefix is like a git short SHA" (turned out: zero random bits) and "merging main into the data branch is safe" (turned out: it drags in the entire codebase).
- **In doubt whether a rule applies** → `ARCHITECTURE.md` has the reasoning behind every rule.

Run it first, then write it down.
