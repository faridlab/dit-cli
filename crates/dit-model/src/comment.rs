//! Comments: one file per comment. Concurrent appends are structurally
//! impossible to conflict, because two writers never touch the same file.

use serde::{Deserialize, Serialize};

/// A comment author is a `people/<alias>.yaml` alias, not free text — the
/// same alias that maps to a git author in team mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub alias: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    /// A ULID, minted by the store. Comments and issues share the ID grammar,
    /// so this reuses `IssueId`'s validating serde.
    pub id: crate::ids::IssueId,
    pub author: String,
    /// RFC3339.
    pub created: String,
    /// The comment body — markdown, never frontmatter-parsed.
    pub body: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn comment_roundtrips_through_serde() {
        let c = Comment {
            id: crate::ids::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap(),
            author: "farid".into(),
            created: "2026-08-16T10:03:00Z".into(),
            body: "Already reproduced on an iPhone 12.".into(),
        };
        let json = serde_json::to_value(&c).unwrap();
        let back: Comment = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }
}
