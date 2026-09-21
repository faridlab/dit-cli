//! Comment files: frontmatter (id/author/created) plus a markdown body.

use dit_model::Comment;

use crate::frontmatter::{Document, Value};
use crate::issue::IssueParseError;
use crate::{fmt, serialize_scalar};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CommentError {
    #[error(transparent)]
    Parse(#[from] IssueParseError),
    #[error(transparent)]
    Frontmatter(#[from] crate::FrontmatterError),
    #[error("comment files are written by the store, never edited surgically — this file is malformed: {0}")]
    Malformed(&'static str),
}

/// Serialize a new comment file. Comments are immutable once written, so there
/// is no patch path — only creation. `reply_to` is the parent comment for a
/// threaded reply, or None for a top-level comment (§4.4).
pub fn serialize_comment(
    id: &dit_model::IssueId,
    author: &str,
    created_rfc3339: &str,
    reply_to: Option<&dit_model::IssueId>,
    body: &str,
) -> Result<String, CommentError> {
    dit_model::parse_rfc3339(created_rfc3339).map_err(|e| IssueParseError::BadField {
        field: "created",
        message: e.to_string(),
    })?;
    // Any alias survives serialization (unsafe characters get escaped); the
    // store rejects aliases that would produce an unworkable filename.
    let mut doc = Document::parse("---\nid:\n---\n")?;
    doc.set_raw("id", &serialize_scalar(id.as_str()));
    doc.set_raw("author", &serialize_scalar(author));
    doc.set_raw("created", &serialize_scalar(created_rfc3339));
    // Bare `null` when top-level — the form every existing comment file
    // already carries — and the parent ULID when a reply.
    match reply_to {
        None => doc.set_raw("reply_to", "null"),
        Some(parent) => doc.set_raw("reply_to", &serialize_scalar(parent.as_str())),
    }
    let body = fmt::format_body(body).map_err(|e| IssueParseError::BadField {
        field: "body",
        message: e.to_string(),
    })?;
    let body = if body.is_empty() {
        String::new()
    } else if body.starts_with('\n') {
        body
    } else {
        format!("\n{body}")
    };
    doc.set_body(body);
    Ok(doc.to_string())
}

/// Parse a comment file back into its typed form. `reply_to` is `null` (or
/// absent) for a top-level comment, or a parent comment ULID for a reply;
/// a value the grammar cannot hold is rejected loudly, never dropped.
pub fn parse_comment(input: &str) -> Result<Comment, CommentError> {
    let doc = Document::parse(input)?;
    // get_str is Option<Option<String>>: the outer None means the key holds a
    // list, the inner None means an empty value. Both are malformed here.
    let id = doc
        .get_str("id")
        .flatten()
        .ok_or(CommentError::Malformed("missing `id`"))?;
    let id = dit_model::IssueId::parse(&id)
        .map_err(|_| CommentError::Malformed("`id` is not a ULID"))?;
    let author = doc
        .get_str("author")
        .flatten()
        .ok_or(CommentError::Malformed("missing `author`"))?;
    let created = doc
        .get_str("created")
        .flatten()
        .ok_or(CommentError::Malformed("missing `created`"))?;
    dit_model::parse_rfc3339(&created)
        .map_err(|_| CommentError::Malformed("`created` is not RFC3339"))?;
    let reply_to = match doc.get("reply_to") {
        None => None,
        Some(Value::Scalar(None)) => None,
        Some(Value::Scalar(Some(s))) if s == "null" || s.is_empty() => None,
        Some(Value::Scalar(Some(s))) => Some(
            dit_model::IssueId::parse(s.as_str())
                .map_err(|_| CommentError::Malformed("`reply_to` is not a ULID or null"))?,
        ),
        // A list where a parent id belongs is future data we cannot read.
        Some(_) => {
            return Err(CommentError::Malformed(
                "`reply_to` holds a value this version cannot read",
            ))
        }
    };
    Ok(Comment {
        id,
        author,
        created,
        reply_to,
        // The file separates frontmatter from body with a blank line and
        // ends with a newline; the typed value is the content itself,
        // without those file-format artifacts.
        body: {
            let body = doc.body();
            let body = body.strip_prefix('\n').unwrap_or(body);
            body.strip_suffix('\n').unwrap_or(body).to_owned()
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn serialize_then_parse_round_trips() {
        let id = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        let text = serialize_comment(
            &id,
            "farid",
            "2026-08-16T10:03:00Z",
            None,
            "Reproduced on iOS 18.",
        )
        .unwrap();
        assert!(text.starts_with("---\nid: 01K3MA1F7XQW8N2V5RTGBCDEFH\nauthor: farid\n"));
        assert!(text.contains("reply_to: null\n"));
        let back = parse_comment(&text).unwrap();
        assert_eq!(back.id, id);
        assert_eq!(back.author, "farid");
        assert_eq!(back.created, "2026-08-16T10:03:00Z");
        assert_eq!(back.reply_to, None);
        assert_eq!(back.body, "Reproduced on iOS 18.");
    }

    #[test]
    fn a_reply_round_trips_its_parent() {
        let parent = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        let reply = dit_model::IssueId::parse("01K3MA9ZC2HJ5M8PQRTVWXYZK1").unwrap();
        let text = serialize_comment(
            &reply,
            "be-1",
            "2026-08-16T10:20:00Z",
            Some(&parent),
            "The payload hits the 500 path.",
        )
        .unwrap();
        assert!(text.contains("reply_to: 01K3MA1F7XQW8N2V5RTGBCDEFH\n"));
        let back = parse_comment(&text).unwrap();
        assert_eq!(back.id, reply);
        assert_eq!(back.reply_to, Some(parent));
    }

    #[test]
    fn a_reply_to_that_is_not_a_ulid_is_rejected_loudly() {
        let id = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        let text = "---\
\nid: 01K3MA9ZC2HJ5M8PQRTVWXYZK1\
\nauthor: be-1\
\ncreated: 2026-08-16T10:20:00Z\
\nreply_to: parent-comment\
\n---\
\n\nBody.\
\n";
        let _ = id;
        assert!(parse_comment(text).is_err());
    }

    #[test]
    fn an_absent_reply_to_reads_as_top_level() {
        // The oldest comment files predate the key entirely.
        let text = "---\
\nid: 01K3MA1F7XQW8N2V5RTGBCDEFH\
\nauthor: farid\
\ncreated: 2026-08-16T10:03:00Z\
\n---\
\n\nBody.\
\n";
        let back = parse_comment(text).unwrap();
        assert_eq!(back.reply_to, None);
    }

    #[test]
    fn rejects_a_bad_timestamp() {
        let id = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        assert!(serialize_comment(&id, "farid", "not-a-time", None, "x").is_err());
    }

    #[test]
    fn a_multiline_author_is_escaped_not_rejected() {
        // Serialization is always safe — newline becomes an escaped `\n`
        // inside a quoted scalar. Filename safety is the store's problem.
        let id = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        let text = serialize_comment(&id, "a\nb", "2026-08-16T10:03:00Z", None, "x").unwrap();
        let line = text.lines().nth(2).unwrap();
        assert_eq!(line, r#"author: "a\nb""#);
    }
}
