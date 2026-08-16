//! Pure domain types. No I/O, compiles to `wasm32-unknown-unknown` (invariant I4).
//!
//! Value objects follow **parse, don't validate**: once the type exists, it is
//! guaranteed valid. See ARCHITECTURE.md §3.1.
//!
//! ## Why `ulid` is `default-features = false`
//!
//! The default `std` feature pulls in `rand` -> `getrandom`, which does not
//! compile for `wasm32-unknown-unknown` without an explicit backend. That would
//! break invariant I4 on day one.
//!
//! Dropping it also removes `Ulid::new()`, and that turns out to be the correct
//! boundary rather than a workaround: **minting an ID needs entropy, and entropy
//! is I/O.** ID generation therefore belongs in `dit-store`, not here. This crate
//! only ever parses and inspects IDs.

mod ids;
mod status;

pub use ids::{IdError, IssueId, Seq, ShortRef, Slug};
pub use status::{Status, StatusCategory};
