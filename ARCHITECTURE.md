# DIT — Architecture Rules

This document governs **how code is written and changed**. For *what* is being built and *why*, read `DESIGN.md`.

The rules here come with one condition: **every rule must be machine-checkable, or it isn't a rule — it's a hope.** A rule that only lives in a document will be broken within three months by someone who never read it. The "Check" column throughout this document shows how each rule is enforced.

---

## Table of Contents

1. [Ten non-negotiable invariants](#1-ten-non-negotiable-invariants)
2. [Dependency direction](#2-dependency-direction)
3. [DDD — what we take and what we reject](#3-ddd--what-we-take-and-what-we-reject)
4. [SOLID in Rust — the honest version](#4-solid-in-rust--the-honest-version)
5. [Code rules](#5-code-rules)
6. [TDD — where it's mandatory, where it isn't](#6-tdd--where-its-mandatory-where-it-isnt)
7. [Change workflow](#7-change-workflow)
8. [Definition of Done](#8-definition-of-done)
9. [Anti-patterns that are explicitly forbidden](#9-anti-patterns-that-are-explicitly-forbidden)

---

## 1. Ten non-negotiable invariants

These matter more than any methodology in this document. Methodology makes code pleasant to maintain; violating these invariants **breaks the product** — some of them silently, and you only find out once someone else's data is involved.

| # | Invariant | Why | Check |
|---|---|---|---|
| **I1** | Every write to disk goes through `Transaction` **and** `dit fmt`. There is no other way. | A second serializer = a fake commit every time you open an issue in the UI. `DESIGN.md` §12.1 | `test_no_direct_fs_write`: greps `std::fs::write`/`File::create` outside `dit-store::atomic` |
| **I2** | Reads never touch files on disk — only the index. | Two read paths will diverge. §16.2 | `test_reads_survive_worktree_wipe`: fill the index, delete the entire working tree, verify every read API still returns the same results |
| **I3** | No code outside `dit-vcs` calls the `git` binary or `gix`. | Anti-corruption layer. Without it, git behavior is scattered everywhere and can't be mocked or fixtured. | `test_git_access_is_contained` |
| **I4** | `dit-model`, `dit-parse`, `dit-query` compile to `wasm32-unknown-unknown` with **zero I/O dependencies**. | §6.4. This is what makes WASM work rather than an aspiration. | CI job `cargo check --target wasm32-unknown-unknown -p dit-model -p dit-parse -p dit-query` |
| **I5** | Derived data is never written to the source of truth. | Principle 3. Stored derived data = conflicts + lies. | `test_frontmatter_has_no_derived_fields` (list of forbidden fields) |
| **I6** | The merge driver **never** leaves `%A` as-is when it fails. Even a panic must write diff3 conflict markers. | Risk #0 — the only path that can silently erase someone's work. | `test_merge_driver_failsafe` — **mandatory in `just check`**, not nightly. Table of injected failures: missing binary, panic, corrupt `fields.yaml`, unparseable YAML, OOM, timeout. Assert: `%A` always contains markers. Fuzz `merge_driver_never_silently_resolves` as a second layer. |
| **I7** | Not one field in a DIT file names an executable, a shell command, a binary path, or a URL that gets fetched automatically. | RCE via pull request. §17.3 | `test_no_executable_fields_in_schema` |
| **I8** | Unknown frontmatter fields are **preserved as-is** across a round-trip. | Cross-version compatibility. An old client must not delete a new client's data. §18.2 | Property test `unknown_fields_survive_roundtrip` |
| **I9** | `field_events` is ordered by `seq` (topological), never by `ts` (wall clock). | `ts` produces contradictory duplicate rows — verified. §14.1 | `test_no_order_by_ts` (grep SQL) + backward-clock fixture |
| **I10** | comrak's `render.unsafe_` is never enabled; strict CSP in `dit-server`. | XSS → CSRF against a local API that has full filesystem access. §17.2 | `test_comrak_unsafe_disabled` + `test_csp_header_present` |

**If a change needs to violate one of these invariants, it isn't a code change — it's a design change.** Write an ADR, change `DESIGN.md` first.

---

## 2. Dependency direction

This is the substance of Clean Architecture that we take. What we reject is its name, its four layers, and `dyn Trait` at every boundary.

```
        ┌──────────────────────────────────────────────┐
        │  PURE CORE — no I/O, compiles to wasm32      │
        │                                              │
        │   dit-model ◄── dit-parse ◄── dit-query      │
        └──────────────────────────────────────────────┘
                          ▲
        ┌─────────────────┴────────────────────────────┐
        │  ADAPTERS — touch the outside world          │
        │                                              │
        │   dit-store   dit-index   dit-vcs   dit-ai   │
        └──────────────────────────────────────────────┘
                          ▲
        ┌─────────────────┴────────────────────────────┐
        │  FACADE                                      │
        │              dit-core                        │
        └──────────────────────────────────────────────┘
                          ▲
        ┌─────────────────┴────────────────────────────┐
        │  DELIVERY                                    │
        │   dit-cli    dit-server    dit-wasm          │
        └──────────────────────────────────────────────┘
```

**Rules:**

1. Dependencies only point **upward** in this diagram. `dit-model` must not know anything about SQLite, git, HTTP, or AI.
2. Delivery **must not** bypass `dit-core` to call an adapter directly. `dit-cli` must not `use dit_index::...`.
3. Two adapters never depend on each other. `dit-index` does not call `dit-vcs`; `dit-core` is what coordinates the two.
4. No circular dependencies (guaranteed by Cargo, but checked anyway so the error message is clearer).

**Check** — `tests/architecture.rs` at the workspace root, run on every CI:

```rust
// Reads `cargo metadata`, enforces the list of allowed edges.
const ALLOWED: &[(&str, &[&str])] = &[
    ("dit-model",  &[]),
    ("dit-parse",  &["dit-model"]),
    ("dit-query",  &["dit-model"]),
    ("dit-store",  &["dit-model", "dit-parse"]),
    ("dit-index",  &["dit-model", "dit-parse", "dit-query"]),
    ("dit-vcs",    &["dit-model"]),
    ("dit-ai",     &["dit-model"]),
    ("dit-core",   &["dit-model", "dit-parse", "dit-query",
                     "dit-store", "dit-index", "dit-vcs", "dit-ai"]),
    ("dit-cli",    &["dit-core", "dit-server"]),
    //                      ^^^^^^^^^^ `dit ui` runs the server's router
    // in-process: delivery calling delivery, never an adapter. Without it,
    // "one binary, no separate installation" (DESIGN.md §6.5) dies.
    ("dit-server", &["dit-core"]),
    ("dit-wasm",   &["dit-model", "dit-parse", "dit-query"]),
];

#[test]
fn dependencies_point_inward() { /* fails with the name of the offending edge */ }
```

Adding a new edge means editing this list — and that forces a discussion in the PR, which is exactly the point.

---

## 3. DDD — what we take and what we reject

We take **the way of thinking**, not the catalog of patterns. In Rust, some DDD patterns are already satisfied by the type system, and writing them out again as a class hierarchy only adds indirection without paying for itself.

### 3.1 What we take

| DDD concept | Form in DIT | Example |
|---|---|---|
| **Value Object** | `newtype` + a validating constructor. **Parse, don't validate** — once the type exists, it is guaranteed valid. | `IssueId(Ulid)`, `ShortRef`, `Slug`, `RepoId`, `Seq(i64)` |
| **Entity** | A struct with explicit identity | `Issue { id: IssueId, .. }` |
| **Aggregate** | **The issue folder** — `issue.md` + `comments/` + `attachments/`. The boundary is the folder. | `.dit/issues/2026/08/<id>-<rnd>-<slug>/` |
| **Aggregate root** | `issue.md`. Comments have no life outside their issue. | — |
| **Ubiquitous language** | The `DESIGN.md` Appendix D glossary. Code **must** use those terms exactly. | `field_events`, not `history_log`; `seq`, not `order`; `short ref`, not `slug_id` |
| **Domain service** | Free functions in `dit-model`. Not a struct. | `resolve_status(file, derived) -> Status` |
| **Anti-corruption layer** | `dit-vcs` is the only thing that speaks git (I3) | — |
| **Bounded context** | Crate boundaries (§2) | — |

**Transaction rule = aggregate rule:** one `Transaction` may touch several aggregates, but it produces **one commit**. What we do **not** promise is cross-aggregate invariants — git is not a transactional database, and pretending otherwise will give birth to sagas and compensating actions that will never be correct. If a business rule needs two aggregates to be consistent, it is enforced by `dit validate` in CI, not at runtime.

### 3.2 The inversion that needs explaining: Domain Event

In ordinary DDD, the domain **emits** events and those events are then stored. In DIT it is the other way around: **events are derived from git after the fact** (`DESIGN.md` §14.1).

This is not a compromise, it is a consequence of Principle 3. The effect: a change made through Vim, through the GitHub web UI, or through some entirely different tool **still gets recorded**. An event system emitted by the application can never do that.

The practical consequence for whoever writes the code: **never add an event bus.** If you need to react to changes, subscribe to `Dit::subscribe()`, whose source is the file watcher and the result of reindexing — not a direct call from the write side.

### 3.3 What we reject — and why

| Rejected | Why |
|---|---|
| **A generic `trait Repository<T>`** | This is the most important rejection. DIT's core asymmetry is **read through the index, write through git** — two completely different mechanisms. A `Repository` with `save()`/`find_by_id()` disguises precisely that difference, and the next person will write `repo.save(issue)` and then be confused about why the index didn't change. `Dit` (§16) deliberately has two separate surfaces. |
| **A base class / `AggregateRoot`, `Entity` trait** | Rust has no inheritance. A marker trait with no behavior is ceremony. |
| **A trait for every adapter "for testability"** | See §4.5. A trait with one implementor is cost without benefit. |
| **Event sourcing as the write mechanism** | Git **already is** event sourcing. Adding a second layer on top of it is duplication. |
| **CQRS as a framework** | The read/write split already exists naturally (index vs git). It doesn't need the name or the infrastructure. |
| **A DTO layer separate from the domain model** | `serde` + newtypes are enough. An extra DTO means every field gets written three times. |
| **Specification pattern** | DQL is our query language. We don't need a second one. |

---

## 4. SOLID in Rust — the honest version

SOLID was formulated for object-oriented languages with inheritance. Some of it translates to Rust directly, some of it is misleading if applied literally.

### 4.1 SRP — Single Responsibility

Translated as: **one crate, one reason to change.** There are only four legitimate reasons to change — the data format, git behavior, the index schema, and UI requirements. If one change forces edits to three crates, the boundary is wrong.

*Check:* PR review. If a PR touches > 3 crates, its description must explain why.

### 4.2 OCP — Open/Closed

Applies in exactly **one** place in DIT: `LlmProvider` (§7.1), because there really are many real implementors there and new ones will keep appearing.

Everywhere else, "open for extension" in Rust is achieved with an `enum` + an **exhaustive** `match`, and that is better: adding a variant makes compilation fail at every place that needs updating. A trait does the opposite — it hides those places.

The rule: **`enum` as the default, `trait` when the implementors come from outside the crate.**

### 4.3 LSP — Liskov

Almost irrelevant without inheritance. What remains real: `LlmProvider` implementors **must** honor the same contract — `capabilities()` has to be honest, and a provider that doesn't support embedding must say so rather than return a zero vector.

*Check:* a single contract test suite run against **every** implementor.

### 4.4 ISP — Interface Segregation

Applied directly: `LlmProvider` is split.

```rust
pub trait Completion { async fn complete(&self, req: CompletionRequest) -> Result<CompletionStream>; }
pub trait Embedding  { async fn embed(&self, texts: &[String]) -> Result<Vec<Vector>>; }
```

The reason is concrete, not theoretical: `fastembed` only does embedding, and some endpoints only do completion. One fat trait forces both of them to write `unimplemented!()` — and `unimplemented!()` is a panic waiting its turn.

### 4.5 DIP — Dependency Inversion, and this is where people go wrong most often in Rust

The literal version ("always depend on abstractions") is **wrong** for Rust. Monomorphization means concrete types carry no runtime cost, while `dyn Trait` adds indirection, and — more expensively — it adds a layer someone has to read.

The DIT rule:

> **A new trait is justified only if there are ≥ 2 real implementors, or it is a genuine process boundary.**
>
> "We might need it later" and "so it can be mocked" are **not** justifications.

What passes this rule across all of DIT: `Completion`, `Embedding`, and `VcsBackend` (and even there, `DESIGN.md` §6.2 recommends **not** making it generic yet — ~5 free functions are enough).

For testing, the replacement is not mocks but **real fixtures**: an actual git repo in a tempdir, an in-memory SQLite, an LLM provider that reads responses from a file. Faster to write, far more honest, and they catch bugs that mocks hide — three rounds of `DESIGN.md` review found 71 bugs, and almost all of them came from running the real thing.

---

## 5. Code rules

All of them are enforced, not suggested.

### 5.1 Workspace-level lints

Root `Cargo.toml`:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
unwrap_used   = "deny"     # except in #[cfg(test)]
expect_used   = "deny"     # except in #[cfg(test)] and fn main
panic         = "deny"     # in library crates
todo          = "deny"
dbg_macro     = "deny"
print_stdout  = "deny"     # libraries use tracing; only dit-cli may print
```

`panic`/`unwrap` are set to `deny` not out of tidiness — `dit-parse` and the merge driver consume untrusted input (`DESIGN.md` §17), and a panic in the merge driver is a path to Risk #0.

### 5.2 Errors

- **Library crates** → `thiserror`, error types that mean something in the domain.
- **Binaries** (`dit-cli`) → `anyhow`, at the outermost layer only.
- **A conflict is not an error.** It is a field in `SyncReport`, not an `Err` variant (§16.5). This one is enforced in review, and it matters.
- Every user-visible `Err` must state **what can be done about it**, not just what failed.

### 5.3 Naming

Code uses the glossary terms **exactly**. Synonyms are forbidden because they silently fracture the ubiquitous language:

| Use | Not |
|---|---|
| `issue` | ticket, task, card, item |
| `short_ref` | slug_id, key, code |
| `seq` | order, position, index |
| `field_events` | history, audit_log, activity |
| `source of truth` / `index` | database, storage, repo (ambiguous with git repo) |
| `workspace` | project, instance |

*Check:* `test_forbidden_synonyms` — greps for forbidden identifiers in `crates/`.

### 5.4 Length & shape

There is no rigid line limit. What is enforced:

- `rustfmt` defaults, no custom configuration. Arguing about formatting is a waste.
- `clippy::cognitive_complexity` = warn. The warning gets read, not automatically fixed.
- Public functions in `dit-core` must have a doc comment that names the **invariant** they protect, rather than restating their parameter names.

### 5.5 Dependencies

- Adding a dependency requires a reason in the PR description.
- Pre-1.0 crates on the critical path must be pinned exactly and recorded in `DESIGN.md` §9. Currently: `gix`, `sqlite-vec`, `comrak` (because of the experimental options).
- `cargo-deny` for licenses and advisories.
- On the JS side: a CI gate rejects GPL/proprietary-licensed packages — `@blocknote/xl-*` in particular.

---

## 6. TDD — where it's mandatory, where it isn't

### 6.1 The split

| Crate | TDD | Reason |
|---|---|---|
| `dit-model`, `dit-parse`, `dit-query` | **Test-first mandatory** | The contract is pure and clear; the test is cheaper than the implementation |
| `dit-store`, `dit-vcs`, `dit-index`, `dit-core` | **Test-first mandatory** | This is where the most expensive bugs are, and the fixtures already exist |
| `dit-ai` | Test-first for the pipeline & output verification; real providers use recorded responses | Network calls are not tested in CI |
| `dit-cli` | Test after, end-to-end integration | — |
| `dit-server` | Test after; **except** the security paths (tokens, the `Host` header, CSP), which are **test-first mandatory** | §17.2 |
| `apps/web` | Not mandatory. Test behavior, not rendering. | Test-first fights UI iteration and produces brittle tests |

### 6.2 The cycle

```
1. Write a FAILING test, and make sure it fails for the right reason.
2. Write the simplest code that makes it pass.
3. Refactor with the test staying green.
4. If this is a bug fix — its fixture goes in at step 1. Always.
```

Step 1 has a part people often skip: **make sure it fails for the right reason.** A test that fails because of a typo in a function name proves nothing.

### 6.3 The absolute rule

> **Every bug fix ships together with a fixture that reproduces it.**

No exceptions. `DESIGN.md` §19.1 already has ten initial fixtures — all of them come from real bugs found by actually running the command. That list grows; it never shrinks.

### 6.4 What gets tested as a property, not as an example

Some DIT invariants are stated more accurately as properties (`proptest`):

```
fmt(fmt(x))             == fmt(x)
fmt(trim(fmt(x)))       == trim(fmt(x))
parse(serialize(i))     == i
unknown_fields(parse(serialize(x))) == unknown_fields(x)      // I8
merge(base, a, b) converges and loses no fields
merge(base, a, b) == merge(base, b, a)   for SET-typed fields
```

### 6.5 What we do NOT do

- **Don't test private functions.** If it feels necessary, the module boundary is wrong.
- **Don't use a mock when a real fixture is possible.** A git repo in a tempdir is faster to write than a git mock, and it catches what the mock hides.
- **Don't chase coverage numbers.** Coverage is reported, never a gate. Coverage as a gate produces tests that test getters.
- **Don't snapshot-test things that legitimately change.** Golden files are only for output that really does have to be stable byte-for-byte — namely `dit fmt`.

---

## 7. Change workflow

The same applies to humans and to AI.

```
1. READ
   └─ The relevant part of DESIGN.md. If the change touches
      merge, history, or AI — read §9 (risks) too.

2. CHECK THE INVARIANTS
   └─ Does this change touch any of I1–I10?
      If yes → stop. Write an ADR, change DESIGN.md first.

3. TEST FIRST
   └─ Per §6.1. If this is a bug fix → its fixture, now.

4. IMPLEMENT
   └─ As small as possible to get the test green.

5. VERIFY
   └─ `just check`   (fmt, clippy, test, architecture, wasm, deny)

6. COMMIT
   └─ Conventional commit + DIT trailer:

      fix(parse): preserve unknown frontmatter fields

      Old clients deleted fields they did not recognize when
      rewriting, violating I8.

      Closes: #Q2R7VN8
```

### When an ADR is mandatory

- Violating or changing one of invariants I1–I10
- Adding a dependency on the critical path
- Adding a cross-crate dependency edge
- Changing the format in the source of truth (triggers §18 versioning)
- Choosing between approaches that are equally reasonable, where the next person will ask "why like this?"

ADRs live in `.dit/docs/adr/` — dogfooding, using DIT to build DIT.

---

## 8. Definition of Done

A change is done when **all** of these hold:

- [ ] `just check` is green
- [ ] Tests written first (for the crates that require it)
- [ ] The bug fix has a fixture that reproduces it
- [ ] No invariant I1–I10 violated without an ADR
- [ ] New terms added to the `DESIGN.md` glossary
- [ ] User-visible changes added to `.dit/changelogs/unreleased/`
- [ ] If it touches the schema: `schema_version` considered (§18.1)
- [ ] If it touches an untrusted input path: fuzz targets updated
- [ ] The PR description cites the relevant part of `DESIGN.md`

---

## 9. Anti-patterns that are explicitly forbidden

This list exists because every one of them looked reasonable at the moment it was written.

| Anti-pattern | Why it's forbidden |
|---|---|
| `trait FooRepository` | §3.3. Hides the read-index/write-git asymmetry |
| A trait with one implementor | §4.5 |
| Writing files outside `Transaction` | I1 — fake commits, body conflicts |
| `Command::new("git")` outside `dit-vcs` | I3 |
| Storing derived data in the frontmatter | I5, Principle 3 |
| `unwrap()` in library code | §5.1 — a path to Risk #0 |
| `ORDER BY ts` on `field_events` | I9 — contradictory duplicate results, verified |
| An internal event bus | §3.2 — git is already the event log |
| Config that can name a command | I7 — RCE via PR |
| Enabling `render.unsafe_` "so the HTML works" | I10 |
| Adding an editor block with no markdown equivalent | Principle 1 — files must stay useful outside DIT |
| Coverage as a CI gate | §6.5 |
| Fixing a bug without a fixture | §6.3 |

---

## Appendix — `just check`

One command that runs every gate. Contributors and AI only have to remember this one.

The binding rule: **every invariant I1–I10 has a check that runs in `just check`.** If an invariant is only guarded by a nightly fuzz run or by human review, it isn't really guarded — the violation will land in `main` first and only be noticed later.

```make
check: fmt clippy test arch wasm deny web-license invariants

invariants:   cargo test --test invariants          # I1–I10, deterministic & fast

fmt:          cargo fmt --all -- --check
clippy:       cargo clippy --workspace --all-targets -- -D warnings
test:         cargo test --workspace
arch:         cargo test --test architecture -- --include-ignored
wasm:         cargo check --target wasm32-unknown-unknown \
                  -p dit-model -p dit-parse -p dit-query
deny:         cargo deny check
web-license:  node scripts/check-js-licenses.mjs   # reject GPL/proprietary

# Not part of `check` — too slow. Nightly in CI.
# This is the SECOND LAYER, not the primary gate for any invariant.
fuzz:         cargo +nightly fuzz run frontmatter    -- -max_total_time=300
              cargo +nightly fuzz run merge_driver   -- -max_total_time=300
bench:        cargo bench --bench corpus_50k
```

### Invariant → test map

One file, `tests/invariants.rs`, so that no invariant loses its guard unnoticed during a refactor:

| Invariant | Test | Form |
|---|---|---|
| I1 | `test_no_direct_fs_write` | Source grep |
| I2 | `test_reads_survive_worktree_wipe` | Fixture: delete the working tree, reads still work |
| I3 | `test_git_access_is_contained` | Source grep |
| I4 | (separate CI job) | `cargo check --target wasm32` |
| I5 | `test_frontmatter_has_no_derived_fields` | List of forbidden fields |
| I6 | `test_merge_driver_failsafe` | Table of injected failures |
| I7 | `test_no_executable_fields_in_schema` | Schema grep |
| I8 | `unknown_fields_survive_roundtrip` | `proptest` |
| I9 | `test_no_order_by_ts` + `test_clock_skew_fixture` | Grep SQL + fixture |
| I10 | `test_comrak_unsafe_disabled`, `test_csp_header_present` | Config assertion |
