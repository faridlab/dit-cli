//! Everything in this workspace that talks to git.
//!
//! Reading git is one thing; changing history — push, merge, rebase — is
//! another, and the pure-Rust git libraries genuinely cannot do the second
//! half. So this crate owns both halves for the whole workspace, shelling
//! out to the real `git` binary where history is at stake, and nothing
//! outside it may spawn a git command at all. That keeps the question "can
//! this code corrupt the repo?" answerable by reading one crate.
//!
//! The frontmatter-aware merge driver lives here too: git hands it three
//! temp files and expects a merged result back, and its most important
//! property is what it does when things go wrong — leave loud conflict
//! markers, never a silent copy of one side.

pub mod git;
pub mod history;
pub mod merge_driver;
pub mod sync;

pub use git::{RebaseOutcome, Repo, VcsError};
pub use history::{walk_field_events, EMPTY_TREE};
pub use merge_driver::{
    drive, merge_documents, FieldConflict, FieldPolicy, MergeContext, MergeError, MergeOutcome,
    Side,
};
pub use sync::{sync, ConflictInfo, FieldResolution, SyncOptions, SyncReport};
