//! Repo layout, atomic writes, and the write transaction. This is the only
//! crate in the workspace that opens files for writing — every other crate
//! reads.

pub mod atomic;
pub mod layout;
mod store;

pub use layout::Layout;
pub use store::{Changeset, IssueFile, Store, StoreError, Transaction};
