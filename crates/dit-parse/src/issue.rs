//! `issue.md` ⇄ `Document`: typed extraction, canonical creation, surgical
//! patch application.
//!
//! The `Document` is always the storage; `Issue` is a *view* of the known
//! keys. Writes go through [`apply_patch`], which touches only the keys the
//! patch names. Writing back the whole issue would also rewrite fields
//! nobody changed — the classic source of spurious merge conflicts when two
//! people edit different fields of the same issue.

use dit_model::{
    parse_rfc3339, validate_date, FieldPatch, IdError, Issue, IssueDraft, IssueId, IssueKind,
    Priority,
};

use crate::fmt;
use crate::frontmatter::{serialize_scalar, serialize_seq, Document, FrontmatterError, Value};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IssueParseError {
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error("missing required field `{0}` — every issue file must carry it")]
    MissingField(&'static str),
    #[error("field `{field}`: {message}")]
    BadField {
        field: &'static str,
        message: String,
    },
}

fn bad(field: &'static str, message: impl Into<String>) -> IssueParseError {
    IssueParseError::BadField {
        field,
        message: message.into(),
    }
}

/// Parse an issue file into its typed view + the underlying document.
pub fn parse_issue(input: &str) -> Result<(Issue, Document), IssueParseError> {
    let doc = Document::parse(input)?;
    let issue = issue_from_document(&doc)?;
    Ok((issue, doc))
}

/// Extract the typed view from an already-parsed document.
pub fn issue_from_document(doc: &Document) -> Result<Issue, IssueParseError> {
    let required_str = |key: &'static str| -> Result<String, IssueParseError> {
        scalar(doc, key)?
            .filter(|s| !s.is_empty())
            .ok_or(IssueParseError::MissingField(key))
    };

    let id = IssueId::parse(&required_str("id")?).map_err(|e: IdError| bad("id", e.to_string()))?;
    let number = parse_positive(doc, "number")?;
    let title = required_str("title")?;
    let kind_raw = required_str("type")?;
    let kind = IssueKind::parse(&kind_raw).ok_or_else(|| {
        bad(
            "type",
            format!("unknown type `{kind_raw}` (task/bug/story/spike/chore)"),
        )
    })?;
    let status = required_str("status")?;

    let priority = match scalar(doc, "priority")?.filter(|s| !s.is_empty()) {
        Some(p) => Some(
            Priority::parse(&p)
                .ok_or_else(|| bad("priority", format!("unknown priority `{p}` (p0..p4)")))?,
        ),
        None => None,
    };
    let reporter = scalar(doc, "reporter")?.filter(|s| !s.is_empty());
    let assignees = doc.get_list("assignees").unwrap_or_default();
    let labels = doc.get_list("labels").unwrap_or_default();
    let epic = match scalar(doc, "epic")?.filter(|s| !s.is_empty()) {
        Some(e) => Some(IssueId::parse(&e).map_err(|e: IdError| bad("epic", e.to_string()))?),
        None => None,
    };
    let estimate = match scalar(doc, "estimate")?.filter(|s| !s.is_empty()) {
        Some(e) => Some(
            e.parse::<u32>()
                .map_err(|_| bad("estimate", format!("`{e}` is not a whole number")))?,
        ),
        None => None,
    };
    let sprint = scalar(doc, "sprint")?.filter(|s| !s.is_empty());
    let created = required_str("created")?;
    parse_rfc3339(&created).map_err(|e| bad("created", e.to_string()))?;
    let updated = required_str("updated")?;
    parse_rfc3339(&updated).map_err(|e| bad("updated", e.to_string()))?;
    let due = match scalar(doc, "due")?.filter(|s| !s.is_empty()) {
        Some(d) => {
            validate_date(&d).map_err(|e| bad("due", e.to_string()))?;
            Some(d)
        }
        None => None,
    };
    let blocked_by: Vec<IssueId> = doc
        .get_list("blocked_by")
        .unwrap_or_default()
        .iter()
        .map(|b| IssueId::parse(b).map_err(|e: IdError| bad("blocked_by", e.to_string())))
        .collect::<Result<_, _>>()?;
    let body = doc.body().to_owned();

    Ok(Issue {
        id,
        number,
        title,
        kind,
        status,
        priority,
        reporter,
        assignees,
        labels,
        epic,
        estimate,
        sprint,
        created,
        updated,
        due,
        blocked_by,
        body,
    })
}

/// Read a key as a scalar: `Ok(None)` when absent or empty, an error when the
/// key holds a list or nested map where a scalar belongs.
fn scalar(doc: &Document, key: &'static str) -> Result<Option<String>, IssueParseError> {
    match doc.get(key) {
        None | Some(Value::Scalar(None)) => Ok(None),
        Some(Value::Scalar(Some(s))) => Ok(Some(s)),
        Some(_) => Err(bad(key, "must be a single value, not a list")),
    }
}

/// Read a positive whole number (`number:` is 1-based — ADR 0007).
fn parse_positive(doc: &Document, key: &'static str) -> Result<Option<u32>, IssueParseError> {
    match scalar(doc, key)?.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(raw) => match raw.parse::<u32>() {
            Ok(n) if n > 0 => Ok(Some(n)),
            _ => Err(bad(key, format!("`{raw}` is not a positive whole number"))),
        },
    }
}

/// Serialize a brand-new issue file with the canonical key order;
/// `created`/`updated` are the same instant. Only the store calls this (it
/// mints the id and the timestamp); nothing else may create files.
pub fn serialize_new_issue(
    id: &IssueId,
    draft: &IssueDraft,
    now_rfc3339: &str,
) -> Result<String, IssueParseError> {
    // `now` must be a valid RFC3339 timestamp — a bad clock is a write-path
    // bug, and refusing here keeps malformed timestamps out of git.
    parse_rfc3339(now_rfc3339).map_err(|e| bad("created", e.to_string()))?;
    let mut doc = Document::parse("---\nid:\n---\n")?;
    doc.set_raw("id", &serialize_scalar(id.as_str()));
    if let Some(n) = draft.number {
        doc.set_raw("number", &n.to_string());
    }
    doc.set_raw("title", &serialize_scalar(&draft.title));
    doc.set_raw("type", draft.kind.as_str());
    doc.set_raw(
        "status",
        &serialize_scalar(draft.status.as_deref().unwrap_or("todo")),
    );
    if let Some(p) = draft.priority {
        doc.set_raw("priority", p.as_str());
    }
    if let Some(r) = &draft.reporter {
        doc.set_raw("reporter", &serialize_scalar(r));
    }
    if !draft.assignees.is_empty() {
        doc.set_raw("assignees", &serialize_seq(&draft.assignees));
    }
    if !draft.labels.is_empty() {
        doc.set_raw("labels", &serialize_seq(&draft.labels));
    }
    if let Some(e) = draft.epic {
        doc.set_raw("epic", &serialize_scalar(e.as_str()));
    }
    if let Some(est) = draft.estimate {
        doc.set_raw("estimate", &est.to_string());
    }
    if let Some(s) = &draft.sprint {
        doc.set_raw("sprint", &serialize_scalar(s));
    }
    if let Some(d) = &draft.due {
        validate_date(d).map_err(|e| bad("due", e.to_string()))?;
        doc.set_raw("due", &serialize_scalar(d));
    }
    if !draft.blocked_by.is_empty() {
        let blocked: Vec<String> = draft
            .blocked_by
            .iter()
            .map(|b| b.as_str().to_owned())
            .collect();
        doc.set_raw("blocked_by", &serialize_seq(&blocked));
    }
    doc.set_raw("created", &serialize_scalar(now_rfc3339));
    doc.set_raw("updated", &serialize_scalar(now_rfc3339));
    let body = fmt::format_body(&draft.body).map_err(|e| bad("body", e.to_string()))?;
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

/// Apply a field patch to a document *surgically*: only the touched keys and
/// `updated` change; everything else — including unknown fields and comments
/// — survives byte-for-byte. Returns the keys that were rewritten.
pub fn apply_patch(
    doc: &mut Document,
    patch: &FieldPatch,
    updated_rfc3339: &str,
) -> Result<Vec<&'static str>, IssueParseError> {
    parse_rfc3339(updated_rfc3339).map_err(|e| bad("updated", e.to_string()))?;
    let mut touched = Vec::new();
    if let Some(n) = patch.number {
        if n == 0 {
            return Err(bad("number", "numbers are 1-based — 0 is a bug, not an id"));
        }
        doc.set_raw("number", &n.to_string());
        touched.push("number");
    }
    if let Some(t) = &patch.title {
        if t.is_empty() {
            return Err(bad("title", "cannot be empty"));
        }
        doc.set_raw("title", &serialize_scalar(t));
        touched.push("title");
    }
    if let Some(k) = patch.kind {
        doc.set_raw("type", k.as_str());
        touched.push("type");
    }
    if let Some(s) = &patch.status {
        if s.is_empty() {
            return Err(bad("status", "cannot be empty"));
        }
        doc.set_raw("status", &serialize_scalar(s));
        touched.push("status");
    }
    if let Some(p) = patch.priority {
        doc.set_raw("priority", p.as_str());
        touched.push("priority");
    }
    if let Some(r) = &patch.reporter {
        doc.set_raw("reporter", &serialize_scalar(r));
        touched.push("reporter");
    }
    if let Some(a) = &patch.assignees {
        doc.set_raw("assignees", &serialize_seq(a));
        touched.push("assignees");
    }
    if let Some(l) = &patch.labels {
        doc.set_raw("labels", &serialize_seq(l));
        touched.push("labels");
    }
    if let Some(e) = patch.epic {
        doc.set_raw("epic", &serialize_scalar(e.as_str()));
        touched.push("epic");
    }
    if let Some(est) = patch.estimate {
        doc.set_raw("estimate", &est.to_string());
        touched.push("estimate");
    }
    if let Some(s) = &patch.sprint {
        doc.set_raw("sprint", &serialize_scalar(s));
        touched.push("sprint");
    }
    if let Some(d) = &patch.due {
        validate_date(d).map_err(|err| bad("due", err.to_string()))?;
        doc.set_raw("due", &serialize_scalar(d));
        touched.push("due");
    }
    if let Some(b) = &patch.blocked_by {
        let blocked: Vec<String> = b.iter().map(|x| x.as_str().to_owned()).collect();
        doc.set_raw("blocked_by", &serialize_seq(&blocked));
        touched.push("blocked_by");
    }
    if !touched.is_empty() {
        doc.set_raw("updated", &serialize_scalar(updated_rfc3339));
        touched.push("updated");
    }
    Ok(touched)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const FILE: &str = "---\
\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\
\ntitle: Login timeout\
\ntype: bug\
\nstatus: in_progress\
\npriority: p1\
\nreporter: farid\
\nassignees: [farid]\
\nlabels: [auth, frontend]\
\nestimate: 3\
\nsprint: 2026-W33\
\ncreated: 2026-08-16T09:12:00Z\
\nupdated: 2026-08-16T11:40:00Z\
\nfuture_field: keep me\
\n---\
\n\n## Context\
\n\
\nUsers on 3G get logged out.\
\n";

    #[test]
    fn parses_every_known_field() {
        let (issue, _) = parse_issue(FILE).unwrap();
        assert_eq!(issue.id.as_str(), "01K3M9ZXQ2R7VN8P4TDBCEFGHJ");
        assert_eq!(issue.kind, IssueKind::Bug);
        assert_eq!(issue.status, "in_progress");
        assert_eq!(issue.priority, Some(Priority::P1));
        assert_eq!(issue.assignees, vec!["farid"]);
        assert_eq!(issue.labels, vec!["auth", "frontend"]);
        assert_eq!(issue.estimate, Some(3));
        assert_eq!(issue.sprint.as_deref(), Some("2026-W33"));
        assert!(issue.body.contains("3G"));
    }

    #[test]
    fn required_fields_are_enforced() {
        let missing_status = FILE.replace("status: in_progress\n", "");
        assert_eq!(
            parse_issue(&missing_status).unwrap_err(),
            IssueParseError::MissingField("status")
        );
    }

    #[test]
    fn bad_values_name_the_field() {
        let bad_id = FILE.replace("id: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ", "id: not-a-ulid");
        assert!(matches!(
            parse_issue(&bad_id).unwrap_err(),
            IssueParseError::BadField { field: "id", .. }
        ));
        let bad_time = FILE.replace("created: 2026-08-16T09:12:00Z", "created: yesterday");
        assert!(matches!(
            parse_issue(&bad_time).unwrap_err(),
            IssueParseError::BadField {
                field: "created",
                ..
            }
        ));
    }

    #[test]
    fn new_issue_is_canonical_and_reparseable() {
        let id = IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap();
        let draft = IssueDraft {
            number: None,
            title: "Fix login timeout".into(),
            kind: IssueKind::Bug,
            status: Some("todo".into()),
            priority: Some(Priority::P1),
            reporter: Some("farid".into()),
            assignees: vec![],
            labels: vec!["auth".into()],
            epic: None,
            estimate: Some(3),
            sprint: None,
            due: Some("2026-09-01".into()),
            blocked_by: vec![],
            body: "Body here".into(),
        };
        let file = serialize_new_issue(&id, &draft, "2026-08-16T09:12:00Z").unwrap();
        assert!(
            file.starts_with("---\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\ntitle: Fix login timeout\ntype: bug\nstatus: todo\n"),
            "unexpected serialization:\n{file}"
        );
        assert!(file.contains("created: 2026-08-16T09:12:00Z\nupdated: 2026-08-16T09:12:00Z\n"));
        let (issue, _) = parse_issue(&file).unwrap();
        assert_eq!(issue.title, "Fix login timeout");
        assert_eq!(issue.due.as_deref(), Some("2026-09-01"));
        assert!(issue.body.contains("Body here"));
    }

    #[test]
    fn patch_is_surgical_and_reparseable() {
        let (issue_before, mut doc) = parse_issue(FILE).unwrap();
        let patch = FieldPatch {
            status: Some("review".into()),
            labels: Some(vec!["auth".into()]),
            ..FieldPatch::default()
        };
        let touched = apply_patch(&mut doc, &patch, "2026-08-16T12:00:00Z").unwrap();
        assert_eq!(touched, vec!["status", "labels", "updated"]);
        let out = doc.to_string();
        assert!(out.contains("status: review"));
        assert!(out.contains("labels: [auth]"));
        assert!(out.contains("updated: 2026-08-16T12:00:00Z"));
        // Unknown field and untouched fields byte-identical.
        assert!(out.contains("future_field: keep me"));
        assert!(out.contains("title: Login timeout"));
        assert!(out.contains("estimate: 3"));
        assert!(!out.contains("12:00:00Z\n12:00"));
        // Reparse agrees with the patch.
        let issue_after = issue_from_document(&doc).unwrap();
        assert_eq!(issue_after.status, "review");
        assert!(issue_after.updated.starts_with("2026-08-16T12:00"));
        assert_eq!(issue_before.id, issue_after.id);
    }

    #[test]
    fn empty_patch_touches_nothing() {
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let before = doc.to_string();
        let touched =
            apply_patch(&mut doc, &FieldPatch::default(), "2026-08-16T12:00:00Z").unwrap();
        assert!(touched.is_empty());
        assert_eq!(
            doc.to_string(),
            before,
            "empty patch must not bump `updated`"
        );
    }

    #[test]
    fn clearing_a_list_writes_an_empty_flow_seq() {
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let patch = FieldPatch {
            labels: Some(vec![]),
            ..FieldPatch::default()
        };
        apply_patch(&mut doc, &patch, "2026-08-16T12:00:00Z").unwrap();
        assert!(doc.to_string().contains("labels: []"));
        assert_eq!(doc.get_list("labels").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn number_parses_and_must_be_positive() {
        let numbered = FILE.replace(
            "id: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ",
            "number: 12\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ",
        );
        let (issue, _) = parse_issue(&numbered).unwrap();
        assert_eq!(issue.number, Some(12));

        let zero = FILE.replace(
            "id: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ",
            "number: 0\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ",
        );
        assert!(
            matches!(
                parse_issue(&zero).unwrap_err(),
                IssueParseError::BadField {
                    field: "number",
                    ..
                }
            ),
            "numbers are 1-based — 0 is a bug, not an id"
        );

        let word = FILE.replace(
            "id: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ",
            "number: twelve\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ",
        );
        assert!(matches!(
            parse_issue(&word).unwrap_err(),
            IssueParseError::BadField {
                field: "number",
                ..
            }
        ));
    }

    #[test]
    fn new_issue_with_a_number_serializes_it_after_id() {
        let id = IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap();
        let draft = IssueDraft {
            number: Some(12),
            title: "Fix login timeout".into(),
            kind: IssueKind::Bug,
            status: None,
            priority: None,
            reporter: None,
            assignees: vec![],
            labels: vec![],
            epic: None,
            estimate: None,
            sprint: None,
            due: None,
            blocked_by: vec![],
            body: String::new(),
        };
        let file = serialize_new_issue(&id, &draft, "2026-08-16T09:12:00Z").unwrap();
        assert!(
            file.starts_with(
                "---\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\nnumber: 12\ntitle: Fix login timeout\n"
            ),
            "unexpected serialization:\n{file}"
        );
        // Unnumbered drafts carry no `number:` line at all.
        let unnumbered = IssueDraft {
            number: None,
            ..draft
        };
        let file = serialize_new_issue(&id, &unnumbered, "2026-08-16T09:12:00Z").unwrap();
        assert!(!file.contains("number:"));
    }

    #[test]
    fn patching_a_number_is_surgical() {
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let patch = FieldPatch {
            number: Some(13),
            ..FieldPatch::default()
        };
        let touched = apply_patch(&mut doc, &patch, "2026-08-16T12:00:00Z").unwrap();
        assert_eq!(touched, vec!["number", "updated"]);
        let issue = issue_from_document(&doc).unwrap();
        assert_eq!(issue.number, Some(13));
    }
}
