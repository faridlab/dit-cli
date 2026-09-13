//! `release.md` ⇄ `Document` (DESIGN.md §15.2): typed extraction, canonical
//! creation, surgical patch application. Same discipline as issues: the
//! `Document` is the storage, `Release` a view of the known keys, and a
//! patch rewrites only what it names — unknown fields and comments survive
//! byte-for-byte (invariant 8).

use dit_model::{
    validate_date, validate_release_version, IdError, IssueId, Release, ReleasePatch, ReleaseStatus,
};

use crate::frontmatter::{serialize_scalar, serialize_seq, Document, FrontmatterError, Value};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ReleaseParseError {
    #[error(transparent)]
    Frontmatter(#[from] FrontmatterError),
    #[error("missing required field `{0}` — every release file must carry it")]
    MissingField(&'static str),
    #[error("field `{field}`: {message}")]
    BadField {
        field: &'static str,
        message: String,
    },
}

fn bad(field: &'static str, message: impl Into<String>) -> ReleaseParseError {
    ReleaseParseError::BadField {
        field,
        message: message.into(),
    }
}

/// Parse a release file into its typed view + the underlying document.
pub fn parse_release(input: &str) -> Result<(Release, Document), ReleaseParseError> {
    let doc = Document::parse(input)?;
    let release = release_from_document(&doc)?;
    Ok((release, doc))
}

/// Extract the typed view from an already-parsed document.
pub fn release_from_document(doc: &Document) -> Result<Release, ReleaseParseError> {
    let required_str = |key: &'static str| -> Result<String, ReleaseParseError> {
        scalar(doc, key)?
            .filter(|s| !s.is_empty())
            .ok_or(ReleaseParseError::MissingField(key))
    };

    let version = required_str("version")?;
    validate_release_version(&version).map_err(|e| bad("version", e.to_string()))?;
    let status_raw = required_str("status")?;
    let status = ReleaseStatus::parse(&status_raw).ok_or_else(|| {
        bad(
            "status",
            format!("unknown status `{status_raw}` (planned/in_dev/in_uat/released/rolled_back)"),
        )
    })?;
    let target_ref = scalar(doc, "target_ref")?.filter(|s| !s.is_empty());
    let repo = scalar(doc, "repo")?.filter(|s| !s.is_empty());
    let target = match scalar(doc, "target")?.filter(|s| !s.is_empty()) {
        Some(d) => {
            validate_date(&d).map_err(|e| bad("target", e.to_string()))?;
            Some(d)
        }
        None => None,
    };
    let includes = match doc.get("includes") {
        None | Some(Value::Scalar(None)) => Vec::new(),
        Some(Value::Seq(items)) => items
            .iter()
            .map(|i| IssueId::parse(i).map_err(|e: IdError| bad("includes", e.to_string())))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(bad("includes", "must be a list of issue ids")),
    };

    Ok(Release {
        version,
        status,
        target_ref,
        repo,
        target,
        includes,
    })
}

/// Read a key as a scalar: `Ok(None)` when absent or empty, an error when the
/// key holds a list or nested map where a scalar belongs.
fn scalar(doc: &Document, key: &'static str) -> Result<Option<String>, ReleaseParseError> {
    match doc.get(key) {
        None | Some(Value::Scalar(None)) => Ok(None),
        Some(Value::Scalar(Some(s))) => Ok(Some(s)),
        Some(_) => Err(bad(key, "must be a single value, not a list")),
    }
}

/// Serialize a brand-new release file with the canonical key order. Absent
/// optional fields write no key at all, so a fresh file carries only what
/// it knows.
pub fn serialize_new_release(release: &Release) -> Result<String, ReleaseParseError> {
    validate_release_version(&release.version).map_err(|e| bad("version", e.to_string()))?;
    let mut doc = Document::parse("---\nversion:\n---\n")?;
    doc.set_raw("version", &serialize_scalar(&release.version));
    doc.set_raw("status", release.status.as_str());
    if let Some(r) = &release.target_ref {
        doc.set_raw("target_ref", &serialize_scalar(r));
    }
    if let Some(r) = &release.repo {
        doc.set_raw("repo", &serialize_scalar(r));
    }
    if let Some(d) = &release.target {
        validate_date(d).map_err(|e| bad("target", e.to_string()))?;
        doc.set_raw("target", &serialize_scalar(d));
    }
    if !release.includes.is_empty() {
        let ids: Vec<String> = release
            .includes
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect();
        doc.set_raw("includes", &serialize_seq(&ids));
    }
    Ok(doc.to_string())
}

/// Apply a patch *surgically*: only the touched keys change; everything
/// else — unknown fields, comments, the body — survives byte-for-byte.
/// Returns the keys that were rewritten. There is no `updated` stamp on a
/// release file (§15.2 defines none), so an empty patch is a true no-op.
pub fn apply_release_patch(
    doc: &mut Document,
    patch: &ReleasePatch,
) -> Result<Vec<&'static str>, ReleaseParseError> {
    let mut touched = Vec::new();
    if let Some(s) = patch.status {
        doc.set_raw("status", s.as_str());
        touched.push("status");
    }
    if let Some(d) = &patch.target {
        validate_date(d).map_err(|err| bad("target", err.to_string()))?;
        doc.set_raw("target", &serialize_scalar(d));
        touched.push("target");
    }
    Ok(touched)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use dit_model::{IssueId, ReleaseStatus};

    const FILE: &str = "---\
\nversion: v0.2.0\
\nstatus: in_uat              # planned | in_dev | in_uat | released | rolled_back\
\ntarget_ref: release/0.2.0\
\nrepo: api\
\ntarget: 2026-10-01\
\nincludes:\
\n  - 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\
\n  - 01K3M9ZXQ2ZZZZZZZZZZZZZZZZ\
\napproved_by: qa-lead\
\n---\
\n\nShips the new login flow.\
\n";

    #[test]
    fn parses_every_known_field_and_keeps_the_body() {
        let (release, doc) = parse_release(FILE).unwrap();
        assert_eq!(release.version, "v0.2.0");
        assert_eq!(release.status, ReleaseStatus::InUat);
        assert_eq!(release.target_ref.as_deref(), Some("release/0.2.0"));
        assert_eq!(release.repo.as_deref(), Some("api"));
        assert_eq!(release.target.as_deref(), Some("2026-10-01"));
        assert_eq!(release.includes.len(), 2);
        assert_eq!(release.includes[1].as_str(), "01K3M9ZXQ2ZZZZZZZZZZZZZZZZ");
        assert!(doc.body().contains("login flow"));
    }

    #[test]
    fn optional_fields_may_be_absent() {
        let minimal = "---\nversion: v1\nstatus: planned\n---\n";
        let (release, _) = parse_release(minimal).unwrap();
        assert_eq!(release.target_ref, None);
        assert_eq!(release.repo, None);
        assert_eq!(release.target, None);
        assert!(release.includes.is_empty());
    }

    #[test]
    fn required_and_bad_fields_name_themselves() {
        assert_eq!(
            parse_release("---\nstatus: planned\n---\n").unwrap_err(),
            ReleaseParseError::MissingField("version")
        );
        assert_eq!(
            parse_release("---\nversion: v1\n---\n").unwrap_err(),
            ReleaseParseError::MissingField("status")
        );
        assert!(matches!(
            parse_release("---\nversion: v1\nstatus: shipped\n---\n").unwrap_err(),
            ReleaseParseError::BadField {
                field: "status",
                ..
            }
        ));
        // The target date uses the same validation as an issue's `due`.
        assert!(matches!(
            parse_release("---\nversion: v1\nstatus: planned\ntarget: next week\n---\n")
                .unwrap_err(),
            ReleaseParseError::BadField {
                field: "target",
                ..
            }
        ));
        assert!(matches!(
            parse_release("---\nversion: ../x\nstatus: planned\n---\n").unwrap_err(),
            ReleaseParseError::BadField {
                field: "version",
                ..
            }
        ));
        assert!(matches!(
            parse_release("---\nversion: v1\nstatus: planned\nincludes: [nope]\n---\n")
                .unwrap_err(),
            ReleaseParseError::BadField {
                field: "includes",
                ..
            }
        ));
    }

    #[test]
    fn a_new_release_is_canonical_and_reparseable() {
        let release = Release {
            version: "v0.3.0".into(),
            status: ReleaseStatus::Planned,
            target_ref: None,
            repo: Some("api".into()),
            target: Some("2026-11-15".into()),
            includes: vec![IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap()],
        };
        let file = serialize_new_release(&release).unwrap();
        assert!(
            file.starts_with(
                "---\nversion: v0.3.0\nstatus: planned\nrepo: api\ntarget: 2026-11-15\n"
            ),
            "unexpected serialization:\n{file}"
        );
        assert!(!file.contains("target_ref"), "absent fields write no key");
        let (back, _) = parse_release(&file).unwrap();
        assert_eq!(back, release);
    }

    #[test]
    fn patch_is_surgical_and_unknown_fields_survive() {
        let (before, mut doc) = parse_release(FILE).unwrap();
        let patch = ReleasePatch {
            status: Some(ReleaseStatus::Released),
            target: Some("2026-10-03".into()),
        };
        let touched = apply_release_patch(&mut doc, &patch).unwrap();
        assert_eq!(touched, vec!["status", "target"]);
        let out = doc.to_string();
        // Invariant 8: a field DIT does not know still survives the write,
        // and so does the trailing comment on the replaced line.
        assert!(out.contains("approved_by: qa-lead"), "{out}");
        assert!(out.contains("status: released"), "{out}");
        assert!(out.contains("# planned | in_dev"), "{out}");
        assert!(out.contains("target: 2026-10-03"), "{out}");
        assert!(out.contains("Ships the new login flow."), "{out}");
        let (after, _) = parse_release(&out).unwrap();
        assert_eq!(after.status, ReleaseStatus::Released);
        assert_eq!(after.includes, before.includes);
        assert_eq!(after.target_ref, before.target_ref);

        // An empty patch rewrites nothing at all.
        let (_, mut doc) = parse_release(FILE).unwrap();
        let untouched = doc.to_string();
        assert!(apply_release_patch(&mut doc, &ReleasePatch::default())
            .unwrap()
            .is_empty());
        assert_eq!(doc.to_string(), untouched);

        // A date that is not a date is refused rather than written.
        let bad = ReleasePatch {
            status: None,
            target: Some("soon".into()),
        };
        assert!(apply_release_patch(&mut doc, &bad).is_err());
    }
}
