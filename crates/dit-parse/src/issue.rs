//! `issue.md` ⇄ `Document`: typed extraction, canonical creation, surgical
//! patch application.
//!
//! The `Document` is always the storage; `Issue` is a *view* of the known
//! keys. Writes go through [`apply_patch`], which touches only the keys the
//! patch names. Writing back the whole issue would also rewrite fields
//! nobody changed — the classic source of spurious merge conflicts when two
//! people edit different fields of the same issue.

use dit_model::{
    parse_rfc3339, validate_date, ClearableField, FieldPatch, IdError, Issue, IssueDraft, IssueId,
    IssueKind, Priority,
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
    let start = match scalar(doc, "start")?.filter(|s| !s.is_empty()) {
        Some(d) => {
            validate_date(&d).map_err(|e| bad("start", e.to_string()))?;
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
    let lane = scalar(doc, "lane")?.filter(|s| !s.is_empty());
    let flows = doc.get_list("flows").unwrap_or_default().to_vec();
    let claimed_by = scalar(doc, "claimed_by")?.filter(|s| !s.is_empty());
    let claimed_at = match scalar(doc, "claimed_at")?.filter(|s| !s.is_empty()) {
        Some(at) => {
            parse_rfc3339(&at).map_err(|e| bad("claimed_at", e.to_string()))?;
            Some(at)
        }
        None => None,
    };
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
        start,
        blocked_by,
        lane,
        flows,
        claimed_by,
        claimed_at,
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
    if let Some(d) = &draft.start {
        validate_date(d).map_err(|e| bad("start", e.to_string()))?;
        doc.set_raw("start", &serialize_scalar(d));
    }
    if !draft.blocked_by.is_empty() {
        let blocked: Vec<String> = draft
            .blocked_by
            .iter()
            .map(|b| b.as_str().to_owned())
            .collect();
        doc.set_raw("blocked_by", &serialize_seq(&blocked));
    }
    // Claims never ride creation (ADR 0015): no claimed_by/claimed_at here,
    // on purpose — `dit claim` is the only writer of those keys.
    if let Some(l) = &draft.lane {
        doc.set_raw("lane", &serialize_scalar(l));
    }
    // Flows may ride creation (ADR 0019): membership is ordinary authorship.
    if !draft.flows.is_empty() {
        doc.set_raw("flows", &serialize_seq(&draft.flows));
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
    // A field both set and cleared is a contradiction — refused by name
    // rather than resolved by whichever branch happens to run last.
    for field in &patch.clear {
        let also_set = match field {
            ClearableField::Priority => patch.priority.is_some(),
            ClearableField::Epic => patch.epic.is_some(),
            ClearableField::Estimate => patch.estimate.is_some(),
            ClearableField::Sprint => patch.sprint.is_some(),
            ClearableField::Due => patch.due.is_some(),
            ClearableField::Start => patch.start.is_some(),
            ClearableField::Lane => patch.lane.is_some(),
            ClearableField::ClaimedBy => patch.claimed_by.is_some(),
            ClearableField::ClaimedAt => patch.claimed_at.is_some(),
        };
        if also_set {
            return Err(bad(
                field.key(),
                "cannot be set and cleared in the same patch",
            ));
        }
    }
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
    if let Some(d) = &patch.start {
        validate_date(d).map_err(|err| bad("start", err.to_string()))?;
        doc.set_raw("start", &serialize_scalar(d));
        touched.push("start");
    }
    if let Some(b) = &patch.blocked_by {
        let blocked: Vec<String> = b.iter().map(|x| x.as_str().to_owned()).collect();
        doc.set_raw("blocked_by", &serialize_seq(&blocked));
        touched.push("blocked_by");
    }
    if let Some(l) = &patch.lane {
        doc.set_raw("lane", &serialize_scalar(l));
        touched.push("lane");
    }
    if let Some(f) = &patch.flows {
        // The list fields clear by being set to `[]`; `flows` follows
        // `labels`, so there is no clearable variant (ADR 0019).
        doc.set_raw("flows", &serialize_seq(f));
        touched.push("flows");
    }
    if let Some(c) = &patch.claimed_by {
        doc.set_raw("claimed_by", &serialize_scalar(c));
        touched.push("claimed_by");
    }
    if let Some(c) = &patch.claimed_at {
        parse_rfc3339(c).map_err(|e| bad("claimed_at", e.to_string()))?;
        doc.set_raw("claimed_at", &serialize_scalar(c));
        touched.push("claimed_at");
    }
    for field in &patch.clear {
        // Removing the line, not writing an empty value: an absent key and
        // an empty key both read as "none", but only one of them is what a
        // hand-written file looks like.
        doc.remove(field.key());
        if !touched.contains(&field.key()) {
            touched.push(field.key());
        }
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
    use dit_model::ClearableField;

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

    fn draft(title: &str) -> IssueDraft {
        IssueDraft {
            number: None,
            title: title.into(),
            kind: IssueKind::Bug,
            status: Some("todo".into()),
            priority: None,
            reporter: None,
            assignees: vec![],
            labels: vec![],
            epic: None,
            estimate: None,
            sprint: None,
            due: None,
            start: None,
            blocked_by: vec![],
            lane: None,
            flows: Vec::new(),
            body: String::new(),
        }
    }

    #[test]
    fn parses_lane_and_claim_fields() {
        let file = "---\
\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\
\ntitle: Login timeout\
\ntype: bug\
\nstatus: todo\
\ncreated: 2026-08-16T09:12:00Z\
\nupdated: 2026-08-16T11:40:00Z\
\nlane: frontend\
\nclaimed_by: fe-1\
\nclaimed_at: 2026-08-16T11:38:00Z\
\n---\
\n\nBody.\
\n";
        let (issue, _) = parse_issue(file).unwrap();
        assert_eq!(issue.lane.as_deref(), Some("frontend"));
        assert_eq!(issue.claimed_by.as_deref(), Some("fe-1"));
        assert_eq!(issue.claimed_at.as_deref(), Some("2026-08-16T11:38:00Z"));
    }

    #[test]
    fn a_bad_claimed_at_names_the_field() {
        let file = "---\
\nid: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\
\ntitle: Login timeout\
\ntype: bug\
\nstatus: todo\
\ncreated: 2026-08-16T09:12:00Z\
\nupdated: 2026-08-16T11:40:00Z\
\nclaimed_at: yesterday\
\n---\
\n\nBody.\
\n";
        assert!(matches!(
            parse_issue(file).unwrap_err(),
            IssueParseError::BadField {
                field: "claimed_at",
                ..
            }
        ));
    }

    #[test]
    fn lane_and_claim_patches_are_surgical() {
        let mut doc = Document::parse(FILE).unwrap();
        let patch = FieldPatch {
            lane: Some("backend".into()),
            claimed_by: Some("be-1".into()),
            claimed_at: Some("2026-08-17T09:00:00Z".into()),
            ..FieldPatch::default()
        };
        let touched = apply_patch(&mut doc, &patch, "2026-08-17T09:00:00Z").unwrap();
        assert!(touched.contains(&"lane"));
        assert!(touched.contains(&"claimed_by"));
        assert!(touched.contains(&"claimed_at"));
        let text = doc.to_string();
        assert!(text.contains("lane: backend\n"));
        assert!(text.contains("claimed_by: be-1\n"));
        assert!(text.contains("claimed_at: 2026-08-17T09:00:00Z\n"));
        // The unknown field survives a claim patch untouched.
        assert!(text.contains("future_field: keep me"));

        // Release removes the lines entirely — an absent key, not an empty one.
        let release = FieldPatch {
            clear: vec![ClearableField::ClaimedBy, ClearableField::ClaimedAt],
            ..FieldPatch::default()
        };
        apply_patch(&mut doc, &release, "2026-08-17T09:30:00Z").unwrap();
        let text = doc.to_string();
        assert!(!text.contains("claimed_by"));
        assert!(!text.contains("claimed_at"));
        assert!(text.contains("lane: backend\n"));
    }

    #[test]
    fn a_patch_cannot_set_and_clear_a_claim_in_one_go() {
        let mut doc = Document::parse(FILE).unwrap();
        let contradictory = FieldPatch {
            claimed_by: Some("be-1".into()),
            clear: vec![ClearableField::ClaimedBy],
            ..FieldPatch::default()
        };
        assert!(matches!(
            apply_patch(&mut doc, &contradictory, "2026-08-17T09:00:00Z").unwrap_err(),
            IssueParseError::BadField {
                field: "claimed_by",
                ..
            }
        ));
    }

    #[test]
    fn new_issue_writes_lane_but_never_claims() {
        let id = IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap();
        let draft = IssueDraft {
            lane: Some("frontend".into()),
            ..draft("Fix login timeout")
        };
        let file = serialize_new_issue(&id, &draft, "2026-08-16T09:12:00Z").unwrap();
        assert!(file.contains("lane: frontend\n"));
        assert!(!file.contains("claimed_by"));
        assert!(!file.contains("claimed_at"));
    }

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
    fn a_start_date_round_trips_and_a_bad_one_names_its_field() {
        // `start` is optional and additive: a file without it parses, and a
        // file with it keeps the value through a write.
        let (without, _) = parse_issue(FILE).unwrap();
        assert_eq!(without.start, None);

        let with = FILE.replace("sprint: 2026-W33", "sprint: 2026-W33\nstart: 2026-08-20");
        let (issue, _) = parse_issue(&with).unwrap();
        assert_eq!(issue.start.as_deref(), Some("2026-08-20"));

        // A date that is not a date is rejected by name, like every other
        // field — never silently dropped.
        let bad = FILE.replace("sprint: 2026-W33", "sprint: 2026-W33\nstart: next tuesday");
        assert!(matches!(
            parse_issue(&bad).unwrap_err(),
            IssueParseError::BadField { field: "start", .. }
        ));
    }

    #[test]
    fn patching_start_touches_only_start_and_updated() {
        let (before, mut doc) = parse_issue(FILE).unwrap();
        let patch = FieldPatch {
            start: Some("2026-09-05".into()),
            ..FieldPatch::default()
        };
        let touched = apply_patch(&mut doc, &patch, "2026-08-17T10:00:00Z").unwrap();
        // A patch writes nothing it was not asked to.
        assert_eq!(touched, vec!["start", "updated"]);

        let (after, _) = parse_issue(&doc.to_string()).unwrap();
        assert_eq!(after.start.as_deref(), Some("2026-09-05"));
        assert_eq!(after.due, before.due);
        assert_eq!(after.title, before.title);
        assert_eq!(after.body, before.body);
        // Invariant 8: a field DIT does not know still survives the write.
        assert!(doc.to_string().contains("future_field: keep me"));

        // A date that is not a date is refused rather than written.
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let bad = FieldPatch {
            start: Some("soon".into()),
            ..FieldPatch::default()
        };
        assert!(apply_patch(&mut doc, &bad, "2026-08-17T10:00:00Z").is_err());
    }

    #[test]
    fn clearing_removes_the_key_and_bumps_updated() {
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let patch = FieldPatch {
            clear: vec![ClearableField::Priority, ClearableField::Sprint],
            ..FieldPatch::default()
        };
        let touched = apply_patch(&mut doc, &patch, "2026-08-17T10:00:00Z").unwrap();
        assert_eq!(touched, vec!["priority", "sprint", "updated"]);
        let out = doc.to_string();
        assert!(!out.contains("priority:"), "{out}");
        assert!(!out.contains("sprint:"), "{out}");
        assert!(out.contains("updated: 2026-08-17T10:00:00Z"), "{out}");
        assert!(out.contains("future_field: keep me"), "invariant 8");
        let issue = issue_from_document(&doc).unwrap();
        assert_eq!(issue.priority, None);
        assert_eq!(issue.sprint, None);
        assert_eq!(issue.estimate, Some(3), "untouched");

        // Clearing a key that is not there is a no-op that still counts as
        // touched — the caller asked for "no due date" and got it.
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let before = doc.to_string();
        let patch = FieldPatch {
            clear: vec![ClearableField::Due],
            ..FieldPatch::default()
        };
        let touched = apply_patch(&mut doc, &patch, "2026-08-17T10:00:00Z").unwrap();
        assert_eq!(touched, vec!["due", "updated"]);
        assert_ne!(doc.to_string(), before, "updated moved");

        // Setting and clearing the same field in one patch is a contradiction,
        // refused by name rather than resolved by ordering.
        let (_, mut doc) = parse_issue(FILE).unwrap();
        let contradictory = FieldPatch {
            due: Some("2026-09-01".into()),
            clear: vec![ClearableField::Due],
            ..FieldPatch::default()
        };
        assert!(matches!(
            apply_patch(&mut doc, &contradictory, "2026-08-17T10:00:00Z").unwrap_err(),
            IssueParseError::BadField { field: "due", .. }
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
            start: None,
            blocked_by: vec![],
            lane: None,
            flows: Vec::new(),
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
            start: None,
            blocked_by: vec![],
            lane: None,
            flows: Vec::new(),
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
