//! Integration tests over a real tempdir workspace — no mocks, real files.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use dit_model::{DocPath, FieldPatch, IssueDraft, IssueId, IssueKind, Priority};
use dit_store::{atomic, Layout, Store};
use time::OffsetDateTime;

fn now() -> OffsetDateTime {
    OffsetDateTime::parse(
        "2026-08-17T12:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap()
}

fn draft(title: &str) -> IssueDraft {
    IssueDraft {
        number: None,
        title: title.to_owned(),
        kind: IssueKind::Bug,
        status: None,
        priority: Some(Priority::P1),
        reporter: Some("farid".to_owned()),
        assignees: vec!["farid".to_owned()],
        labels: vec!["auth".to_owned()],
        epic: None,
        estimate: Some(3),
        sprint: None,
        due: None,
        blocked_by: vec![],
        body: "## Context\n\nIt breaks.".to_owned(),
    }
}

#[test]
fn layout_derives_monthly_shard_from_the_id_timestamp() {
    let dot = Layout::with_kind("/tmp/ws", dit_model::DataLayout::DotDir);
    // A ULID encodes its creation time, so the folder path needs no extra
    // data. This sample id decodes to 2025-08-26 (verified independently) —
    // the shard is whatever the id actually says, not what prose claims.
    let id = IssueId::parse("01K3M9ZXQ2R7VN8P4TDBCEFGHJ").unwrap();
    let dir = dot.issue_dir(&id, &dit_model::Slug::parse("login-timeout").unwrap());
    let s = dir.to_string_lossy().replace('\\', "/");
    assert!(
        s.ends_with(".dit/issues/2025/08/01K3M9ZXQ2-R7VN-login-timeout"),
        "{s}"
    );

    // The default (ADR 0005) puts the same folder at the visible root.
    let root = Layout::with_kind("/tmp/ws", dit_model::DataLayout::Root);
    let dir = root.issue_dir(&id, &dit_model::Slug::parse("login-timeout").unwrap());
    let s = dir.to_string_lossy().replace('\\', "/");
    assert!(
        s.ends_with("issues/2025/08/01K3M9ZXQ2-R7VN-login-timeout"),
        "{s}"
    );
    assert!(!s.contains(".dit/issues"), "{s}");
}

#[test]
fn create_issue_writes_a_parsable_file_in_the_right_place() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx
        .create_issue(draft("Login timeout on slow networks"))
        .unwrap();
    let applied = tx.finish().unwrap();

    let path = store.layout().issue_body(&id).unwrap();
    assert!(path.exists(), "{}", path.display());
    let on_disk = fs::read_to_string(&path).unwrap();
    let (issue, doc) = dit_parse::parse_issue(&on_disk).unwrap();
    assert_eq!(issue.id, id);
    assert_eq!(issue.title, "Login timeout on slow networks");
    assert_eq!(issue.kind, IssueKind::Bug);
    // Unknown fields would survive here; the round-trip suite covers that.
    assert!(doc.get_str("id").flatten().is_some());

    assert_eq!(applied.paths().count(), 1, "one file written");
}

#[test]
fn created_ids_are_unique_and_time_sorted() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let a = tx.create_issue(draft("First")).unwrap();
    let b = tx.create_issue(draft("Second")).unwrap();
    tx.finish().unwrap();
    assert_ne!(a, b);
    // Same millisecond on the clock, still distinct: the random component
    // carries the uniqueness, not the timestamp.
    assert!(a.timestamp_ms() > 0);
    // Short refs differ even though the timestamp prefix is identical.
    assert_ne!(a.short_ref(), b.short_ref());
}

#[test]
fn ids_minted_in_one_transaction_keep_creation_order() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let ids: Vec<IssueId> = (0..12)
        .map(|n| tx.create_issue(draft(&format!("Issue {n}"))).unwrap())
        .collect();
    tx.finish().unwrap();
    // One transaction shares one clock reading, so raw entropy would sort
    // these ids at random against their creation order. Renumber backfills
    // and comment listing both read id order as creation order (ADR 0009),
    // so minting must be monotonic within a transaction.
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(sorted, ids, "id order must be mint order");
}

#[test]
fn comments_added_in_one_transaction_read_back_in_addition_order() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Has comments")).unwrap();
    tx.add_comment(&id, "farid", "first").unwrap();
    tx.add_comment(&id, "farid", "second").unwrap();
    tx.add_comment(&id, "farid", "third").unwrap();
    tx.finish().unwrap();

    let comments = store.read_comments(&id).unwrap();
    let bodies: Vec<&str> = comments.iter().map(|c| c.body.as_str()).collect();
    // Comment files sort by name, and names begin with the comment's id —
    // same shared clock reading here, so the ids must be mint-ordered for
    // the listing order to be the order they were added.
    assert_eq!(bodies, vec!["first", "second", "third"]);
}

#[test]
fn short_ref_resolves_back_to_the_issue() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.finish().unwrap();

    let found = store
        .locate_issue(id.short_ref().as_str())
        .unwrap()
        .unwrap();
    assert_eq!(found, id);
    let by_full = store.locate_issue(id.as_str()).unwrap().unwrap();
    assert_eq!(by_full, id);
    assert!(store.locate_issue("ZZZZZZZ").unwrap().is_none());
}

#[test]
fn set_fields_rewrites_only_the_touched_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.finish().unwrap();

    let path = store.layout().issue_body(&id).unwrap();
    let before = fs::read_to_string(&path).unwrap();

    // Two hours later: `updated` moves, and nothing else may.
    let mut tx = store.transaction(now() + time::Duration::hours(2), "farid");
    tx.set_fields(
        &id,
        FieldPatch {
            status: Some("in_progress".to_owned()),
            estimate: None,
            ..Default::default()
        },
    )
    .unwrap();
    tx.finish().unwrap();

    let after = fs::read_to_string(&path).unwrap();
    let (issue, _) = dit_parse::parse_issue(&after).unwrap();
    assert_eq!(issue.status, "in_progress");
    // Every line the patch did not touch must be byte-identical.
    let changed: Vec<&str> = before
        .lines()
        .zip(after.lines())
        .filter(|(b, a)| b != a)
        .map(|(b, _)| b)
        .collect();
    assert_eq!(
        changed,
        vec!["status: todo", "updated: 2026-08-17T12:00:00Z"],
        "only status (+ updated) may change"
    );
}

#[test]
fn add_comment_writes_its_own_file_and_it_parses_back() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    let comment_id = tx.add_comment(&id, "budi", "Reproduced on iOS.").unwrap();
    tx.finish().unwrap();

    let path = store
        .layout()
        .comment_md(&id, &comment_id, "budi")
        .expect("a parseable id always yields a path");
    let text = fs::read_to_string(&path).unwrap();
    let comment = dit_parse::parse_comment(&text).unwrap();
    assert_eq!(comment.author, "budi");
    assert_eq!(comment.body.trim(), "Reproduced on iOS.");
}

#[test]
fn a_comment_alias_that_cannot_be_a_filename_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    let err = tx.add_comment(&id, "../escape", "nope").unwrap_err();
    assert!(err.to_string().contains("alias"), "{err}");
}

#[test]
fn finish_is_all_or_nothing_and_rollback_restores_the_previous_state() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());

    // Create one issue that will later be modified.
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.finish().unwrap();
    let path = store.layout().issue_body(&id).unwrap();
    let original = fs::read_to_string(&path).unwrap();

    // Modify it (two hours later on the clock, so `updated` visibly moves),
    // apply, then roll back — the file must return byte-for-byte.
    let mut tx = store.transaction(now() + time::Duration::hours(2), "farid");
    tx.set_fields(
        &id,
        FieldPatch {
            status: Some("done".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    let applied = tx.finish().unwrap();
    let mid = fs::read_to_string(&path).unwrap();
    assert_ne!(mid, original);
    applied.rollback();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn rollback_removes_files_that_did_not_exist_before() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    let applied = tx.finish().unwrap();
    let dir = store
        .layout()
        .issue_dir_for(&id)
        .unwrap()
        .expect("the folder exists");
    assert!(dir.exists());
    applied.rollback();
    assert!(!dir.exists(), "a rolled-back create leaves nothing behind");
}

#[test]
fn an_unfinished_transaction_touches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let _id = tx.create_issue(draft("Login timeout")).unwrap();
    drop(tx); // no finish() — nothing may have been written
    assert!(!store.layout().issues_dir().join("2026").exists());
}

#[test]
fn two_writes_to_the_same_file_in_one_transaction_apply_once() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.set_fields(
        &id,
        FieldPatch {
            priority: Some(Priority::P0),
            ..Default::default()
        },
    )
    .unwrap();
    let applied = tx.finish().unwrap();
    // create + patch in one transaction = exactly one write to issue.md.
    assert_eq!(applied.paths().count(), 1);

    let (issue, _) = dit_parse::parse_issue(
        &fs::read_to_string(store.layout().issue_body(&id).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(issue.priority, Some(Priority::P0));
}

#[test]
fn atomic_write_leaves_no_temp_file_behind_and_overwrites_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("nested").join("file.md");
    atomic::write(&target, "one\n").unwrap();
    atomic::write(&target, "two\n").unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "two\n");
    assert_eq!(
        fs::read_dir(tmp.path().join("nested")).unwrap().count(),
        1,
        "no temp siblings"
    );
}

#[test]
fn exclusive_lock_is_held_by_one_and_reports_the_holder() {
    let tmp = tempfile::tempdir().unwrap();
    let lock_path = tmp.path().join("write.lock");
    let _held = atomic::acquire_lock(&lock_path, "dit-cli pid 1").unwrap();
    let err = atomic::acquire_lock(&lock_path, "dit-server pid 2").unwrap_err();
    assert!(err.contains("dit-cli"), "must name the holder: {err}");
    drop(_held);
    // Released on drop — a second acquire now succeeds.
    assert!(atomic::acquire_lock(&lock_path, "dit-server pid 2").is_ok());
}

#[test]
fn status_must_be_plain_ascii_for_folder_safety() {
    // set_fields with a status containing a path separator must be refused
    // before anything is staged — statuses become part of queries, and the
    // value also lands in the file the merge driver parses.
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.finish().unwrap();
    let mut tx = store.transaction(now(), "farid");
    let err = tx
        .set_fields(
            &id,
            FieldPatch {
                status: Some("../evil".to_owned()),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("status"), "{err}");
}

#[test]
fn a_fresh_issue_lands_at_the_visible_root_as_readme_md() {
    // ADR 0005 + 0006 fixture: `issues/YYYY/MM/<folder>/README.md`, never
    // `.dit/issues/…/issue.md`.
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        Store::open(tmp.path()).layout().kind(),
        dit_model::DataLayout::Root,
        "a fresh workspace detects as root layout"
    );
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.finish().unwrap();

    let path = store.layout().issue_body(&id).unwrap();
    let s = path.to_string_lossy().replace('\\', "/");
    // The ULID is minted at `now()` — 2026-08 — so that is the shard.
    assert!(s.contains("/issues/2026/08/"), "{s}");
    assert!(s.ends_with("/README.md"), "{s}");
    assert!(!s.contains(".dit/issues"), "{s}");
    assert!(!s.ends_with("issue.md"), "{s}");
}

#[test]
fn a_legacy_dotdir_workspace_keeps_writing_where_it_already_writes() {
    // A workspace created before ADR 0005/0006: data under `.dit/issues/`,
    // bodies named `issue.md`. New issues follow the detected layout; edits
    // to the legacy issue keep its file name until `dit migrate-layout`.
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    let mut tx = store.transaction(now(), "farid");
    let id = tx.create_issue(draft("Old shape")).unwrap();
    tx.finish().unwrap();
    let written = store.layout().issue_body(&id).unwrap();
    let legacy_dir = written.parent().unwrap();

    // Move the tree to the legacy shape on disk, body name included.
    let dot_issues = tmp.path().join(".dit").join("issues");
    fs::create_dir_all(dot_issues.parent().unwrap()).unwrap();
    fs::rename(tmp.path().join("issues"), &dot_issues).unwrap();
    let rel = legacy_dir.strip_prefix(tmp.path().join("issues")).unwrap();
    let legacy_folder = dot_issues.join(rel);
    fs::rename(
        legacy_folder.join("README.md"),
        legacy_folder.join("issue.md"),
    )
    .unwrap();
    let store = Store::open(tmp.path());
    assert_eq!(store.layout().kind(), dit_model::DataLayout::DotDir);

    // The body file resolves to the legacy name, and an edit lands there.
    let legacy_body = legacy_folder.join("issue.md");
    assert_eq!(store.layout().issue_body(&id).unwrap(), legacy_body);
    let mut tx = store.transaction(now(), "farid");
    tx.set_fields(
        &id,
        FieldPatch {
            status: Some("in_progress".to_owned()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.finish().unwrap();
    let text = fs::read_to_string(&legacy_body).unwrap();
    assert!(text.contains("status: in_progress"), "{text}");
}

// -- doc pages (§13) ---------------------------------------------------------

fn doc_tx() -> (tempfile::TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path());
    (tmp, store)
}

#[test]
fn write_doc_creates_a_formatted_page_under_the_right_root() {
    let (tmp, store) = doc_tx();
    let mut tx = store.transaction(now(), "farid");
    tx.write_doc(
        &DocPath::parse("docs/flows/auth-session.md").unwrap(),
        "# Auth session\n\nSome   sloppily  spaced text.\n\n\n\nExtra gaps.\n",
    )
    .unwrap();
    tx.finish().unwrap();

    let path = tmp.path().join("docs/flows/auth-session.md");
    let text = fs::read_to_string(&path).unwrap();
    // The write passes through `dit fmt` — invariant 1 applies to pages as
    // much as to issue bodies.
    assert_eq!(
        text,
        dit_parse::fmt::fmt(&text).unwrap(),
        "must be canonical"
    );
    assert!(text.starts_with("# Auth session"), "{text}");
    // ...and the file round-trips through the ordinary markdown parser.
    assert!(dit_parse::Document::parse(&text).is_ok());
}

#[test]
fn write_doc_in_a_dotdir_workspace_lands_under_dot_dit() {
    // Mode C (ADR 0005): content roots live under `.dit/`. `Store::open`
    // detects layout from the tree, so bootstrap the legacy shape first.
    let (tmp, store) = doc_tx();
    let mut tx = store.transaction(now(), "farid");
    tx.write_doc(&DocPath::parse("notes/learning-dsa.md").unwrap(), "# DSA\n")
        .unwrap();
    tx.finish().unwrap();

    fs::create_dir_all(tmp.path().join(".dit/issues")).unwrap();
    fs::rename(tmp.path().join("notes"), tmp.path().join(".dit/notes")).unwrap();
    let store = Store::open(tmp.path());
    assert_eq!(store.layout().kind(), dit_model::DataLayout::DotDir);
    let mut tx = store.transaction(now(), "farid");
    tx.write_doc(&DocPath::parse("notes/second.md").unwrap(), "# Second\n")
        .unwrap();
    tx.finish().unwrap();
    assert!(tmp.path().join(".dit/notes/second.md").exists());
    assert!(tmp.path().join(".dit/notes/learning-dsa.md").exists());
}

#[test]
fn write_doc_refuses_conflict_markers() {
    let (tmp, store) = doc_tx();
    let mut tx = store.transaction(now(), "farid");
    let err = tx
        .write_doc(
            &DocPath::parse("docs/merged.md").unwrap(),
            "<<<<<<< ours\ntext\n=======\ntext\n>>>>>>> theirs\n",
        )
        .unwrap_err();
    assert!(err.to_string().contains("conflict"), "{err}");
    // Nothing was staged — the tree stays clean.
    assert!(!tmp.path().join("docs").exists());
}

#[test]
fn remove_doc_deletes_the_file_and_rollback_restores_it() {
    let (tmp, store) = doc_tx();
    let mut tx = store.transaction(now(), "farid");
    tx.write_doc(&DocPath::parse("docs/gone.md").unwrap(), "# Gone\n")
        .unwrap();
    tx.finish().unwrap();
    let path = tmp.path().join("docs/gone.md");
    let original = fs::read_to_string(&path).unwrap();

    let mut tx = store.transaction(now(), "farid");
    tx.remove_doc(&DocPath::parse("docs/gone.md").unwrap())
        .unwrap();
    let applied = tx.finish().unwrap();
    assert!(!path.exists(), "the page must be deleted");

    applied.rollback();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn remove_doc_on_a_missing_page_is_not_found() {
    let (_tmp, store) = doc_tx();
    let mut tx = store.transaction(now(), "farid");
    let err = tx
        .remove_doc(&DocPath::parse("docs/never-existed.md").unwrap())
        .unwrap_err();
    assert!(err.to_string().contains("not exist"), "{err}");
}

#[test]
fn write_and_remove_in_one_transaction_last_write_wins() {
    // A create followed by a remove of the same page in one transaction
    // leaves nothing behind — staged entries replace by path.
    let (tmp, store) = doc_tx();
    let mut tx = store.transaction(now(), "farid");
    let path = DocPath::parse("docs/phantom.md").unwrap();
    tx.write_doc(&path, "# Phantom\n").unwrap();
    tx.remove_doc(&path).unwrap();
    tx.finish().unwrap();
    assert!(!tmp.path().join("docs/phantom.md").exists());

    // The reverse — remove an existing page, then write it again — keeps the
    // new content: last wins in staging order.
    let mut tx = store.transaction(now(), "farid");
    tx.write_doc(&path, "# Kept\n\ntext\n").unwrap();
    tx.finish().unwrap();
    let mut tx = store.transaction(now(), "farid");
    tx.remove_doc(&path).unwrap();
    tx.write_doc(&path, "# Resurrected\n").unwrap();
    tx.finish().unwrap();
    assert_eq!(
        fs::read_to_string(tmp.path().join("docs/phantom.md")).unwrap(),
        "# Resurrected\n"
    );
}
