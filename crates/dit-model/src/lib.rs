//! Pure domain types — no I/O. This crate must compile to wasm32 and behave
//! identically everywhere, so it never touches the filesystem, the clock, or
//! randomness. Values are built through validating constructors (parse, don't
//! validate): once a value exists, it is guaranteed well-formed.
//!
//! ## Why there is no `ulid` dependency
//!
//! `ulid`'s serde support requires its `std` feature; `std` pulls in `rand`
//! via `getrandom`, and `getrandom` does not compile for
//! `wasm32-unknown-unknown` without an explicit backend. Owning ~60 lines of
//! Crockford base32 is cheaper than the workaround. Consequently there is no
//! `IssueId::new()` here: minting an ID needs entropy, and entropy is I/O —
//! generation lives in `dit-store`. The same reasoning keeps `now()` out of
//! this crate: adapters inject the clock.

mod comment;
mod config;
mod doc;
mod events;
mod ids;
mod issue;
mod layout;
mod status;
mod time;
mod workflow;

pub use comment::{Author, Comment};
pub use config::{Config, Numbering, RepoLink, SCHEMA_MAX};
pub use doc::{DocEntry, DocPath, DocPathError, DOC_ROOTS};
pub use events::{EventSource, FieldEvent, StoredFieldEvent};
pub use ids::{IdError, IssueId, Seq, ShortRef, Slug};
pub use issue::{FieldPatch, Issue, IssueDraft, IssueKind, Priority};
pub use layout::{
    generated_index_paths, is_comment, is_generated_index, is_issue_body, looks_like_issue_body,
    DataLayout, CONTENT_ROOTS, GENERATED_INDEX_MARKER, ISSUE_BODY_FILE, LEGACY_ISSUE_BODY_FILE,
};
pub use status::{Status, StatusCategory};
pub use time::{format_rfc3339, parse_rfc3339, validate_date, TimeError};
pub use workflow::{
    resolve_status, DerivedRule, DerivedSignal, DerivedStatusSignal, Transition, Workflow,
    WorkflowError, WorkflowStatus,
};
