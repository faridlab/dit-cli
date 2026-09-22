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
mod flowshape;
mod ids;
mod issue;
mod layout;
mod morse;
mod openapi;
mod readiness;
mod release;
mod status;
mod time;
mod workflow;

pub use comment::{Author, Comment};
pub use config::{Config, Numbering, RepoLink, SpecEntry, SCHEMA_MAX};
pub use doc::{DocEntry, DocPath, DocPathError, DOC_ROOTS};
pub use events::{ChangeSummary, DayCount, EventSource, FieldEvent, StoredFieldEvent};
pub use flowshape::{
    phase_of_label, phases_of, FlowArrowLabel, FlowGroup, FlowPhase, FlowShape, PHASE_LABEL_PREFIX,
};
pub use ids::{IdError, IssueId, Seq, ShortRef, Slug};
pub use issue::{ClearableField, FieldPatch, Issue, IssueDraft, IssueKind, Priority};
pub use layout::{
    generated_index_paths, is_comment, is_generated_index, is_issue_body, is_release_file,
    looks_like_issue_body, release_version_from_path, DataLayout, CONTENT_ROOTS,
    GENERATED_INDEX_MARKER, ISSUE_BODY_FILE, LEGACY_ISSUE_BODY_FILE, RELEASES_DIR, RELEASE_FILE,
};
pub use morse::{
    variables_in, Capture, Expect, ExpectRule, InlineRequest, JsonCheck, MorseScenario, MorseStep,
    MorseValue, OperationRef, Selector, SpecPin, StepTarget, SuspectedSecret, UnboundVariable,
    VAR_CLOSE, VAR_OPEN,
};
pub use openapi::{OpenApiSpec, SpecOperation, SpecServer};
pub use readiness::{claim_liveness, readiness, ClaimLiveness, Readiness};
pub use release::{
    validate_release_version, Release, ReleasePatch, ReleaseStatus, ReleaseVersionError,
};
pub use status::{Status, StatusCategory};
pub use time::{format_rfc3339, parse_rfc3339, validate_date, TimeError};
pub use workflow::{
    resolve_status, Coordination, DerivedRule, DerivedSignal, DerivedStatusSignal, Gate, Lane,
    ReadinessConfig, Transition, Workflow, WorkflowError, WorkflowStatus,
};
