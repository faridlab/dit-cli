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
/// is no patch path — only creation.
pub fn serialize_comment(
    id: &dit_model::IssueId,
    author: &str,
    created_rfc3339: &str,
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
    doc.set_raw("reply_to", "null");
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

/// Parse a comment file back into its typed form. `reply_to: null` is stored
/// but not modeled yet, so it is only validated, not returned.
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
    if let Some(v) = doc.get("reply_to") {
        // The only written form is `null`; anything else is future data we
        // must not silently drop.
        let is_null = match v {
            Value::Scalar(None) => true,
            Value::Scalar(Some(s)) => s == "null" || s.is_empty(),
            _ => false,
        };
        if !is_null {
            return Err(CommentError::Malformed(
                "`reply_to` holds a value this version cannot read",
            ));
        }
    }
    Ok(Comment {
        id,
        author,
        created,
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
            "Reproduced on iOS 18.",
        )
        .unwrap();
        assert!(text.starts_with("---\nid: 01K3MA1F7XQW8N2V5RTGBCDEFH\nauthor: farid\n"));
        let back = parse_comment(&text).unwrap();
        assert_eq!(back.id, id);
        assert_eq!(back.author, "farid");
        assert_eq!(back.created, "2026-08-16T10:03:00Z");
        assert_eq!(back.body, "Reproduced on iOS 18.");
    }

    #[test]
    fn rejects_a_bad_timestamp() {
        let id = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        assert!(serialize_comment(&id, "farid", "not-a-time", "x").is_err());
    }

    #[test]
    fn a_multiline_author_is_escaped_not_rejected() {
        // Serialization is always safe — newline becomes an escaped `\n`
        // inside a quoted scalar. Filename safety is the store's problem.
        let id = dit_model::IssueId::parse("01K3MA1F7XQW8N2V5RTGBCDEFH").unwrap();
        let text = serialize_comment(&id, "a\nb", "2026-08-16T10:03:00Z", "x").unwrap();
        let line = text.lines().nth(2).unwrap();
        assert_eq!(line, r#"author: "a\nb""#);
    }
}
