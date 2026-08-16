//! Local HTTP + WebSocket server. This is the primary UI surface (DESIGN.md §6.5).
//!
//! Security is not optional here: bind to 127.0.0.1 by default, auth token in a
//! header (never a cookie), validate the `Host` header, strict CSP (invariant I10).
//!
//! ## Why the UI lives in this repo but not in this build
//!
//! `cargo run -p dit-server` does **not** require a Node toolchain. The UI is
//! served by Vite on :5173 during development, proxying `/api` here.
//!
//! `cargo build --features embed-ui` embeds `apps/web/dist` into the binary via
//! `rust-embed`, producing the single-file release artifact.
//!
//! So a frontend contributor never builds Rust, and a Rust contributor never
//! builds the frontend — without splitting the repo and without a hand-synced
//! API contract. `apps/web/dist` is gitignored; CI builds it at release time.

fn main() {
    if cfg!(feature = "embed-ui") {
        eprintln!("dit-server 0.0.1 — scaffold (UI embedded)");
    } else {
        eprintln!("dit-server 0.0.1 — scaffold (dev mode: run `npm run dev` in apps/web)");
    }
}
