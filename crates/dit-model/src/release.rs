//! Release plans (DESIGN.md §15.2) — the shape of
//! `.dit/releases/<version>/release.md`.
//!
//! This is the read model and two small edits (status, target date). What is
//! deliberately NOT here: which issues are *actually* in an environment,
//! whether a claimed issue's commit is an ancestor of the deployed ref, the
//! deployments list. All of that is computed from git by the verification
//! engine (§15.1, v0.9) — storing it in the file would be a claim someone
//! typed, which is exactly the thing DIT exists to replace.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::IssueId;

/// `status:` in a release's frontmatter. The set is fixed by the file format
/// (§15.2); a new lifecycle stage is a versioned change, not an everyday edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    Planned,
    InDev,
    InUat,
    Released,
    RolledBack,
}

impl ReleaseStatus {
    /// Every status, in lifecycle order — what a picker offers.
    pub const ALL: [ReleaseStatus; 5] = [
        ReleaseStatus::Planned,
        ReleaseStatus::InDev,
        ReleaseStatus::InUat,
        ReleaseStatus::Released,
        ReleaseStatus::RolledBack,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseStatus::Planned => "planned",
            ReleaseStatus::InDev => "in_dev",
            ReleaseStatus::InUat => "in_uat",
            ReleaseStatus::Released => "released",
            ReleaseStatus::RolledBack => "rolled_back",
        }
    }

    /// Parse the frontmatter wire form. Lockstep with the serde rename
    /// (snake_case) — a mismatch means files and the API disagree.
    pub fn parse(s: &str) -> Option<ReleaseStatus> {
        match s {
            "planned" => Some(ReleaseStatus::Planned),
            "in_dev" => Some(ReleaseStatus::InDev),
            "in_uat" => Some(ReleaseStatus::InUat),
            "released" => Some(ReleaseStatus::Released),
            "rolled_back" => Some(ReleaseStatus::RolledBack),
            _ => None,
        }
    }
}

impl fmt::Display for ReleaseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a string cannot be a release version.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReleaseVersionError {
    #[error("a release version is required")]
    Empty,
    #[error("a release version may be at most 64 characters, got {0}")]
    TooLong(usize),
    #[error(
        "`{0}` is not a release version — use letters, digits, dots, dashes, underscores or `+`, \
         and do not start with a dot or dash"
    )]
    BadShape(String),
}

const MAX_VERSION_LEN: usize = 64;

/// A release version doubles as a directory name under `.dit/releases/`, so
/// it is validated as one: a closed character set, no leading dot or dash
/// (hidden folders, option-lookalikes), no path separators, no `..`. The
/// same rule holds for every caller — server, CLI, indexer — because it
/// lives here, in the pure core.
pub fn validate_release_version(s: &str) -> Result<(), ReleaseVersionError> {
    if s.is_empty() {
        return Err(ReleaseVersionError::Empty);
    }
    if s.len() > MAX_VERSION_LEN {
        return Err(ReleaseVersionError::TooLong(s.len()));
    }
    let shape_ok = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        && !s.starts_with('.')
        && !s.starts_with('-')
        && !s.contains("..");
    if shape_ok {
        Ok(())
    } else {
        Err(ReleaseVersionError::BadShape(s.to_owned()))
    }
}

/// The aggregate as it lives in `release.md`. Only what a human edits — the
/// verified state of a release is a git question (§15.1), never a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub status: ReleaseStatus,
    /// The code ref this release ships from (`release/0.2.0`). Read, never
    /// resolved here — only `dit-vcs` talks to git.
    pub target_ref: Option<String>,
    /// Which repo, in a polyrepo (§5.0).
    pub repo: Option<String>,
    /// The planned date, `YYYY-MM-DD` — where the roadmap draws the
    /// milestone. A plan, not a record: the deployment files (v0.9) carry
    /// what actually happened.
    pub target: Option<String>,
    /// The issues this release claims. Verification (v0.9) checks the claim
    /// against the deployed ref; nothing here asserts it is true.
    pub includes: Vec<IssueId>,
}

/// An additive patch: `None` means "don't touch". Only the two fields the
/// roadmap edits are patchable — `includes` is filled by `dit release plan`
/// (v0.9), and `version` is the file's identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleasePatch {
    pub status: Option<ReleaseStatus>,
    pub target: Option<String>,
}

impl ReleasePatch {
    pub fn is_empty(&self) -> bool {
        *self == ReleasePatch::default()
    }

    /// The frontmatter keys this patch will rewrite.
    pub fn touched_keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        if self.status.is_some() {
            keys.push("status");
        }
        if self.target.is_some() {
            keys.push("target");
        }
        keys
    }
}

impl Release {
    /// Apply a patch in place. Date validation is the parser's job, at the
    /// write boundary — this method only moves values.
    pub fn apply(&mut self, patch: &ReleasePatch) {
        if let Some(s) = patch.status {
            self.status = s;
        }
        if let Some(t) = &patch.target {
            self.target = Some(t.clone());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_status_round_trips_through_its_wire_form() {
        for status in ReleaseStatus::ALL {
            assert_eq!(ReleaseStatus::parse(status.as_str()), Some(status));
            assert_eq!(status.to_string(), status.as_str());
        }
        assert_eq!(ReleaseStatus::parse("shipped"), None);
        // The serde form is the file form — one vocabulary, not two.
        let json = serde_json::to_value(ReleaseStatus::RolledBack).unwrap();
        assert_eq!(json, serde_json::json!("rolled_back"));
    }

    #[test]
    fn versions_are_directory_safe() {
        for ok in ["v0.2.0", "2026.09", "1.0.0-rc.1", "build_7+meta"] {
            assert_eq!(validate_release_version(ok), Ok(()), "{ok}");
        }
        assert_eq!(
            validate_release_version(""),
            Err(ReleaseVersionError::Empty)
        );
        for bad in ["../etc", "a/b", ".hidden", "-flag", "v 1", "v1\\x", "v0..1"] {
            assert!(
                matches!(
                    validate_release_version(bad),
                    Err(ReleaseVersionError::BadShape(_))
                ),
                "{bad} must be rejected"
            );
        }
        assert!(matches!(
            validate_release_version(&"9".repeat(65)),
            Err(ReleaseVersionError::TooLong(65))
        ));
    }

    #[test]
    fn patch_is_additive_and_names_its_keys() {
        let mut release = Release {
            version: "v0.2.0".into(),
            status: ReleaseStatus::Planned,
            target_ref: Some("release/0.2.0".into()),
            repo: None,
            target: None,
            includes: vec![IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap()],
        };
        assert!(ReleasePatch::default().is_empty());
        let patch = ReleasePatch {
            status: Some(ReleaseStatus::InUat),
            target: Some("2026-10-01".into()),
        };
        assert_eq!(patch.touched_keys(), vec!["status", "target"]);
        release.apply(&patch);
        assert_eq!(release.status, ReleaseStatus::InUat);
        assert_eq!(release.target.as_deref(), Some("2026-10-01"));
        // Untouched fields survive verbatim.
        assert_eq!(release.target_ref.as_deref(), Some("release/0.2.0"));
        assert_eq!(release.includes.len(), 1);
    }
}
