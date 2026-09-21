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
    /// The parent comment this replies to (§4.4), or None for a top-level
    /// comment. Threading lives here, never in the filename.
    #[serde(default)]
    pub reply_to: Option<crate::ids::IssueId>,
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
            reply_to: None,
            body: "Already reproduced on an iPhone 12.".into(),
        };
        let json = serde_json::to_value(&c).unwrap();
        let back: Comment = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn a_reply_roundtrips_with_its_parent() {
        let parent = crate::ids::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        let reply = Comment {
            id: crate::ids::IssueId::parse("01K3MA9ZC2HJ5M8PQRTVWXYZK1").unwrap(),
            author: "be-1".into(),
            created: "2026-08-16T10:20:00Z".into(),
            reply_to: Some(parent),
            body: "The payload hits the 500 path; see the guard test.".into(),
        };
        let json = serde_json::to_value(&reply).unwrap();
        assert_eq!(
            json.get("reply_to").and_then(|v| v.as_str()),
            Some("01K3MA1F7XQW8N2V5RTGBCDEFH")
        );
        let back: Comment = serde_json::from_value(json).unwrap();
        assert_eq!(back, reply);
    }
}
