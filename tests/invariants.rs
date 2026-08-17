#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! One test per invariant from ARCHITECTURE.md §1.
//!
//! Everything here is deterministic and fast, because every invariant must be
//! gated by `just check` — not by nightly fuzzing, not by human review.

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

/// Every production `.rs` file under `crates/`, excluding test code in both
/// shapes it takes: inline `#[cfg(test)]` blocks and `tests/` directories.
/// Test files build fixtures — repos in tempdirs, driver scripts — which is
/// what real-fixture testing means here, and not what I1 governs.
fn source_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if p.is_dir() {
                if name != "tests" {
                    walk(&p, out);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(Path::new("crates"), &mut out);
    out
}

/// Strips `#[cfg(test)] mod tests { .. }` blocks so test code may use the
/// constructs production code may not.
fn production_source(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    match text.find("#[cfg(test)]") {
        Some(i) => text[..i].to_owned(),
        None => text,
    }
}

fn offenders(needles: &[&str], allow: &[&str]) -> Vec<String> {
    source_files()
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy().replace('\\', "/");
            !allow.iter().any(|a| s.contains(a))
        })
        .filter_map(|p| {
            let src = production_source(&p);
            needles
                .iter()
                .find(|n| src.contains(**n))
                .map(|n| format!("{}: contains `{n}`", p.display()))
        })
        .collect()
}

/// I1 — every write to disk goes through `Transaction` + `dit fmt`.
#[test]
fn i1_no_direct_filesystem_writes() {
    let bad = offenders(
        &["std::fs::write", "File::create", "fs::OpenOptions"],
        // The merge driver also writes: git hands it a temporary file (%A)
        // and demands the merge result — or, on any failure, conflict markers
        // — be written back into that same file before exiting. That file
        // belongs to git's machinery, not to the workspace, so it is written
        // directly. Everything under `.dit/` still flows through dit-store.
        &[
            "crates/dit-store/src/atomic",
            "crates/dit-vcs/src/merge_driver.rs",
            // The server token is a local credential living in the
            // disposable cache directory: it exists before any workspace is
            // opened, belongs to the machine, and is never workspace data.
            "crates/dit-server/src/config.rs",
        ],
    );
    assert!(
        bad.is_empty(),
        "I1 violated — write only through dit-store::atomic:\n  {}",
        bad.join("\n  ")
    );
}

/// I3 — only `dit-vcs` talks to git.
#[test]
fn i3_git_access_is_contained() {
    let bad = offenders(
        &["Command::new(\"git\")", "gix::", "git2::"],
        &["crates/dit-vcs/"],
    );
    assert!(
        bad.is_empty(),
        "I3 violated — git access belongs in dit-vcs:\n  {}",
        bad.join("\n  ")
    );
}

/// I9 — `field_events` is ordered topologically, never by wall clock.
#[test]
fn i9_field_events_never_ordered_by_ts() {
    let bad = offenders(&["ORDER BY ts", "order by ts", "MAX(ts)"], &[]);
    assert!(
        bad.is_empty(),
        "I9 violated — order field_events by `seq`, not `ts`:\n  {}",
        bad.join("\n  ")
    );
}

/// I10 — comrak raw HTML rendering is never enabled.
#[test]
fn i10_comrak_unsafe_is_never_enabled() {
    let bad = offenders(
        &[
            "unsafe_ = true",
            "render.unsafe_ = true",
            "set_unsafe(true)",
        ],
        &[],
    );
    assert!(
        bad.is_empty(),
        "I10 violated — raw HTML rendering is an XSS vector:\n  {}",
        bad.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Behavioral guards — these run the real code, not text scans.
// ---------------------------------------------------------------------------

/// A workspace with one issue, plus the tempdir keeping it alive.
fn workspace_with_issue(title: &str) -> (dit_core::Dit, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = dit_core::Dit::init(tmp.path(), Path::new("/bin/true")).unwrap();
    let draft = dit_model::IssueDraft {
        title: title.to_owned(),
        kind: dit_model::IssueKind::Task,
        status: None,
        priority: Some(dit_model::Priority::P1),
        reporter: None,
        assignees: vec!["budi".to_owned()],
        labels: vec!["guard".to_owned()],
        epic: None,
        estimate: Some(3),
        sprint: None,
        due: None,
        blocked_by: Vec::new(),
        body: "body".to_owned(),
    };
    let mut tx = dit.transaction("guard").unwrap();
    let id = tx.create_issue(draft).unwrap();
    let short = id.short_ref().as_str().to_owned();
    tx.commit(&format!("create {short}")).unwrap();
    (dit, tmp)
}

/// Every Markdown file under `.dit/` — the files a git operation would
/// touch. Returns the paths, so callers can assert the sweep was real.
fn dit_markdown(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join(".dit"), &mut out);
    out
}

/// I2 — the read path answers from the index and never opens a workspace
/// file. The proof: wipe every issue file off disk while the workspace is
/// open, and reads return exactly what they returned before. If any read
/// touched the files, their absence would change the answer.
#[test]
fn i2_reads_survive_worktree_wipe() {
    let (dit, tmp) = workspace_with_issue("survives the wipe");
    let before = dit.query("", None).unwrap();
    assert_eq!(before.len(), 1);
    let short = before[0].issue.id.short_ref().as_str().to_owned();
    assert!(dit.get(&short).unwrap().is_some());

    let files = dit_markdown(tmp.path());
    assert!(
        !files.is_empty(),
        "the wipe must remove something to mean anything"
    );
    for f in &files {
        fs::remove_file(f).unwrap();
    }

    let after = dit.query("", None).unwrap();
    assert_eq!(
        after.len(),
        1,
        "reads must come from the index, not the files"
    );
    assert_eq!(after[0].issue.title, "survives the wipe");
    assert!(dit.get(&short).unwrap().is_some());
}

/// The complete vocabulary an issue file may carry. Anything else in the
/// frontmatter means a derived fact leaked into the source of truth.
const KNOWN_ISSUE_KEYS: &[&str] = &[
    "id",
    "title",
    "type",
    "status",
    "priority",
    "reporter",
    "assignees",
    "labels",
    "epic",
    "estimate",
    "sprint",
    "due",
    "blocked_by",
    "created",
    "updated",
];

/// I5 — derived data (commit links, time in status, staleness…) is computed,
/// never stored. The files a workspace actually wrote must contain only the
/// known vocabulary; the day someone writes `closes:` or `commit_sha:` into
/// an issue, this fails and forces the storage decision to be re-examined.
#[test]
fn i5_frontmatter_has_no_derived_fields() {
    let (_dit, tmp) = workspace_with_issue("no derived fields");

    let files = dit_markdown(tmp.path());
    assert!(!files.is_empty());
    for f in &files {
        let text = fs::read_to_string(f).unwrap();
        let (_, doc) = dit_parse::parse_issue(&text).unwrap();
        for key in doc.keys() {
            assert!(
                KNOWN_ISSUE_KEYS.contains(&key),
                "{}: frontmatter key `{key}` is outside the known vocabulary — \
                 derived data must never be stored",
                f.display()
            );
        }
    }
}

/// I6 — the merge driver may fail, but it may never leave git's `%A` file
/// as it found it: an unmerged-ours silently discards their side, which is
/// work disappearing. Every failure path — a panicking merge, an unreadable
/// side — must instead write conflict markers and exit 1.
#[test]
fn i6_merge_driver_failsafe() {
    fn exploding_merge(
        _base: &str,
        _ours: &str,
        _theirs: &str,
    ) -> Result<dit_vcs::MergeOutcome, dit_vcs::MergeError> {
        panic!("injected merge failure")
    }

    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("base");
    let ours = tmp.path().join("ours");
    let theirs = tmp.path().join("theirs");
    fs::write(&base, "shared line\n").unwrap();
    fs::write(&ours, "shared line\nours\n").unwrap();
    fs::write(&theirs, "shared line\ntheirs\n").unwrap();

    // A panicking merge function falls back to whole-file markers.
    let code = dit_vcs::merge_driver::drive_with(&base, &ours, &theirs, 7, exploding_merge);
    assert_eq!(
        code, 1,
        "a failed merge must exit 1 so git treats it as a conflict"
    );
    let merged = fs::read_to_string(&ours).unwrap();
    assert!(
        merged.contains("<<<<<<<"),
        "the %A file must carry conflict markers, was:\n{merged}"
    );
    assert!(merged.contains("ours"));
    assert!(merged.contains("theirs"));
    assert_ne!(
        merged, "shared line\nours\n",
        "the %A file must not survive untouched"
    );

    // An unreadable side takes the same path.
    fs::write(&ours, "shared line\nours\n").unwrap();
    fs::remove_file(&theirs).unwrap();
    let code = dit_vcs::merge_driver::drive_with(&base, &ours, &theirs, 7, exploding_merge);
    assert_eq!(code, 1);
    let merged = fs::read_to_string(&ours).unwrap();
    assert!(
        merged.contains("<<<<<<<"),
        "a missing side must also produce markers"
    );
}

/// The complete key vocabulary the schema files may use. `remote` names where
/// a repository lives for humans reading the config; nothing in DIT ever
/// fetches it automatically.
const KNOWN_SCHEMA_KEYS: &[&str] = &[
    "statuses",
    "id",
    "label",
    "category",
    "wip_limit",
    "terminal",
    "transitions",
    "from",
    "to",
    "requires",
    "derived",
    "on",
    "implies",
    "schema_version",
    "repos",
    "name",
    "remote",
    "branches",
];

/// Pull the key tokens out of the YAML the schema writers emit — keys sit at
/// the start of a line, after `- `, or inside a `{ }` flow map.
fn schema_keys(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim_start();
        if let Some(stripped) = rest.strip_prefix("- ") {
            rest = stripped;
        }
        let inner = rest.trim_start_matches('{').trim_end_matches('}');
        for part in inner.split(", ") {
            if let Some((key, _)) = part.split_once(':') {
                keys.push(key.trim().to_owned());
            }
        }
    }
    keys
}

/// I7 — no field in a DIT file may name an executable, a shell command, or a
/// URL that is fetched automatically: any of those turns a pulled workspace
/// into remote code execution. The guard checks the exact bytes the writers
/// emit, so adding such a key fails here before it ever reaches a user.
#[test]
fn i7_no_executable_fields_in_schema() {
    let workflow = dit_parse::write_workflow(&dit_model::Workflow::default_workflow());
    let config = dit_parse::write_config(&dit_model::Config::default());

    let keys: Vec<String> = schema_keys(&workflow)
        .into_iter()
        .chain(schema_keys(&config))
        .collect();
    assert!(!keys.is_empty(), "the writers must emit something to check");

    for key in &keys {
        assert!(
            KNOWN_SCHEMA_KEYS.contains(&key.as_str()),
            "schema key `{key}` is outside the known vocabulary — a DIT file naming an \
             executable or a fetched URL is remote code execution via pull"
        );
    }
    for banned in [
        "command",
        "cmd",
        "exec",
        "executable",
        "script",
        "shell",
        "url",
        "hook",
        "run",
    ] {
        assert!(
            !keys.iter().any(|k| k == banned),
            "schema key `{banned}` is forbidden"
        );
    }
}

/// I8 — unknown frontmatter fields survive every write DIT makes. A user who
/// keeps `mood:` on their issue must not lose it because DIT re-serialized
/// the file from its typed view.
#[test]
fn i8_unknown_fields_survive_roundtrip() {
    let text = "---\n\
                id: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n\
                title: Roundtrip\n\
                type: task\n\
                status: todo\n\
                created: 2026-08-17T00:00:00Z\n\
                updated: 2026-08-17T00:00:00Z\n\
                mood: curious\n\
                reviewed_by: someone-else\n\
                ---\n\n\
                Body stays.\n";
    let (_, mut doc) = dit_parse::parse_issue(text).unwrap();

    let patch = dit_model::FieldPatch {
        status: Some("in_progress".to_owned()),
        ..Default::default()
    };
    dit_parse::apply_patch(&mut doc, &patch, "2026-08-17T01:00:00Z").unwrap();
    let out = doc.serialize();

    assert!(
        out.contains("status: in_progress"),
        "the patch must land:\n{out}"
    );
    assert!(
        out.contains("mood: curious"),
        "unknown fields must survive:\n{out}"
    );
    assert!(
        out.contains("reviewed_by: someone-else"),
        "unknown fields must survive:\n{out}"
    );
    assert!(out.contains("Body stays."), "the body must survive:\n{out}");
}

/// I10 (server side) — every response carries a strict content security
/// policy, so a markdown-rendering slip cannot become script execution in
/// the browser.
#[tokio::test]
async fn i10_csp_header_present() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = dit_core::Dit::init(tmp.path(), Path::new("/bin/true")).unwrap();
    let state = dit_server::AppState::new(dit, "tester", "test-token");
    let app = dit_server::app(state);

    let request = Request::builder()
        .method("GET")
        .uri("/api/status")
        .header("host", "localhost:7700")
        .header("authorization", "Bearer test-token")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(request).await.unwrap();

    let csp = res
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        csp.contains("default-src 'none'"),
        "every response must carry a strict CSP, got: {csp}"
    );
}
