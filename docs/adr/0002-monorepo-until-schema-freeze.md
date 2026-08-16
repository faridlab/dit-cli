---
id: 0002
title: One repo until the schema freeze, not separate dit-cli and dit-webapp repos
status: accepted
date: 2026-08-16
supersedes: null
---

## Context

Splitting into `dit-cli` (Rust core) and `dit-webapp` (React UI), with the built
UI vendored back into the core repo for stable releases, was proposed.

The underlying want is legitimate: **frontend contributors should not need a Rust
toolchain, and Rust contributors should not need Node.**

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Two repos, vendored `dist/` | Cross-repo contract versioning; +34 MB per 200 releases (measured); broken `git bisect` | Independent toolchains |
| Two repos, CLI **downloads** the UI release at first run | A second trust decision on privileged code, made after install; needs pinning + signature verification, which re-couples versions anyway | Saves ~0.7 MB of a 4.2 MB binary |
| One repo, `embed-ui` feature flag | One `[features]` block | Independent toolchains, single contract, atomic changes |

## Decision

One repo. `dit-server` gets an `embed-ui` feature:

- `cargo run -p dit-server` — no Node required. Vite serves the UI on :5173 and proxies `/api`.
- `cargo build --release --features embed-ui` — embeds `apps/web/dist` via `rust-embed`.
- `apps/web/dist/` is gitignored; CI builds it at release time.

## Consequences

The decisive argument is not repo size — it is the API contract. DESIGN.md §6.5
specifies types generated from OpenAPI with a CI gate that fails when the
generated client drifts. In one repo that is a single job. Across two repos it
becomes: publish the spec, version it, pin it in the webapp, and answer "which
spec version is this build against?" — reintroducing exactly the drift the gate
was built to eliminate. It is the same shape of problem §12.2 solved by putting
markdown serialization in exactly one place.

Second: during v0.1–v0.4 the API changes constantly. Adding one field would mean
Rust change → OpenAPI regen → publish → webapp PR → build → vendor back. Six
steps across two repos for one logical change.

Third: `git bisect` on a vendored `dist/` lands on "chore: update webapp dist"
commits that explain nothing.

### Why not download the UI at first run

An earlier draft of this ADR rejected this on three grounds. **One of them was
wrong and has been withdrawn.**

**Withdrawn: "it breaks offline-first."** Principle 6 is about *usage* — trains,
planes, dead WiFi — not installation. Nobody installs software offline;
`cargo install` and `brew install` both need a network. Fetching once at install
time and running offline forever after does not contradict the principle. That
objection conflated install-time with use-time and does not survive contact with
how people actually get software.

**Still standing: it is a second trust decision, made later, on privileged code.**

The obvious counter is fair — *the binary is downloaded too, so why is the UI
different?* The difference is not whether a download happens; it is how many
artifacts must be trusted, and when.

Embedded: one artifact, one signature, one hash. The UI version equals the binary
version by construction. Compromising it means compromising the release you
already chose to trust.

Fetched separately: two artifacts and two trust decisions, the second happening
*after* install — possibly on a different network, possibly years later. Doing it
safely means implementing pinning and signature verification inside DIT: new
security-critical code we would have to write and keep correct, guarding a UI
that runs against an API with full filesystem access (§17.2).

And that fix eats the benefit. Pin to an exact hash and the CLI once again knows
exactly which UI build it works with — versions are re-coupled, which is the
independence that motivated the idea. Leave it unpinned ("latest compatible") and
that looseness *is* the attack surface.

**Decisive: the benefit is negligible at this size.** Measured, the UI is ~0.7 MB
inside a 4.2 MB binary. Downloading it separately saves about 17% of a 4 MB
download — while adding a signing pipeline, a verification path, and field version
skew where two users on the same CLI produce unreproducible bug reports.

This is not a principle violation. It is a bad trade at this size.

### The rule this produces

The useful question is not "code or asset" — it is **size x privilege**:

| | Small | Large |
|---|---|---|
| **Privileged** (executes against the local API) | **Embed** — the UI | Embed and hash-pin; or reconsider why it is that large |
| **Unprivileged** (inert data) | Embed | **Download on demand** — the embedding model |

The UI is small and privileged, so it ships embedded. The multilingual embedding
model (§7.1) is ~470 MB and inert, so it is fetched on demand and shared across
workspaces — never embedded.

What would change the UI decision: if it grew to tens of megabytes — bundled
fonts, icon sets, offline documentation — the calculus flips and it becomes worth
paying for pinning and verification.

**`dit fetch-assets` covers the real need.** Pre-warming every optional asset
before going offline deserves an explicit command, not a silent runtime fetch.
Explicit, resumable, and inspectable beats implicit.

## Revisit when

The schema freezes at v1.0 (DESIGN.md §10), or the UI acquires an independent
release cadence — for example the static Web Viewer of §6.4, which genuinely is a
separate product.

To keep that split cheap, the OpenAPI spec is treated as a real published
artifact from day one even inside this repo.

## Verification

Repo growth from vendoring a ~370 KB bundle over 10 releases, measured after
`git gc --aggressive`:

```
localized source change (delta-compresses well) : ~0 KB per release
minifier identifier-renaming cascade (realistic): ~172 KB per release
                                                  -> ~34 MB per 200 releases
```

Real but moderate — this was not the deciding factor, and the initial assumption
that it would dominate was wrong.

```
$ cargo check -p dit-server                    # no Node toolchain needed
$ cargo check -p dit-server --features embed-ui
```
