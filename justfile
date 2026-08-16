# One command runs every gate. Contributors and AI only need to remember `just check`.
set shell := ["bash", "-uc"]

default: check

# Everything CI enforces.
check: fmt clippy test arch invariants wasm

fmt:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# ARCHITECTURE.md §2 — dependency direction.
arch:
    cargo test --test architecture

# ARCHITECTURE.md §1 — invariants I1–I10.
invariants:
    cargo test --test invariants

# Invariant I4 — the pure core must stay pure.
wasm:
    cargo check --target wasm32-unknown-unknown -p dit-model -p dit-parse -p dit-query

deny:
    cargo deny check

web-license:
    node scripts/check-js-licenses.mjs

# The UI ships inside the binary, so its size is part of the install (ADR 0003).
web-size:
    npm run build --prefix apps/web
    node scripts/check-bundle-size.mjs

fix:
    cargo fmt --all
    cargo clippy --workspace --all-targets --fix --allow-dirty

# NOT part of `check` — too slow. Nightly in CI. This is a SECOND layer,
# never the primary gate for any invariant.
fuzz:
    cargo +nightly fuzz run frontmatter   -- -max_total_time=300
    cargo +nightly fuzz run merge_driver  -- -max_total_time=300

bench:
    cargo bench --bench corpus_50k
