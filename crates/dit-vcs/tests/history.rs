//! Field-history walking against real git repositories.
//!
//! The walker's value depends on git behaviors that are easy to get wrong on
//! paper: what a root commit's diff looks like, what `--name-status` reports
//! for a moved folder, what a merge commit diffs to. Every test here builds
//! the situation in a real repo and asserts on the events observed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use dit_model::FieldEvent;
use dit_vcs::git::Repo;
use dit_vcs::walk_field_events;

fn hermetic_repo(path: &Path) -> Repo {
    let repo = Repo::init(path).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    repo
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// One issue file, frontmatter only — enough identity for the walker.
fn issue(id: &str, title: &str, status: &str, priority: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: {title}\nstatus: {status}\npriority: {priority}\n\
         updated: 2026-08-16T09:00:00Z\n---\n\nBody.\n"
    )
}

fn events_at<'a>(events: &'a [FieldEvent], sha: &str) -> Vec<&'a FieldEvent> {
    events.iter().filter(|e| e.commit_sha == sha).collect()
}

#[test]
fn creation_emits_one_event_per_field_with_no_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    write(
        &tmp.path().join(".dit/issues/2026/08/one/issue.md"),
        &issue("01K3M9ZXQ2R7VN8P4TDBCEFGHJ", "Login timeout", "todo", "p2"),
    );
    repo.add(".dit").unwrap();
    let root = repo.commit("create issue").unwrap().unwrap();

    let events = walk_field_events(&repo, None).unwrap();
    assert!(!events.is_empty());
    for e in &events {
        assert_eq!(
            e.parent_sha, "",
            "a root commit has no parent to diff against"
        );
        assert_eq!(e.commit_sha, root);
        assert_eq!(e.old_value, None, "every field starts from nothing");
        assert_eq!(e.issue_id, "01K3M9ZXQ2R7VN8P4TDBCEFGHJ");
    }
    let fields: Vec<&str> = events.iter().map(|e| e.field.as_str()).collect();
    assert!(fields.contains(&"status") && fields.contains(&"title"));
}

#[test]
fn a_later_edit_emits_exactly_the_changed_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    let path = tmp.path().join(".dit/issues/2026/08/one/issue.md");
    write(
        &path,
        &issue("01AAAAAAAAAAAAAAAAAAAAAAAA", "Login timeout", "todo", "p2"),
    );
    repo.add(".dit").unwrap();
    let first = repo.commit("create").unwrap().unwrap();

    // Only status changes; title, priority, updated are byte-identical.
    write(
        &path,
        &issue(
            "01AAAAAAAAAAAAAAAAAAAAAAAA",
            "Login timeout",
            "in_progress",
            "p2",
        ),
    );
    repo.add(".dit").unwrap();
    let second = repo.commit("start work").unwrap().unwrap();

    let events = walk_field_events(&repo, None).unwrap();
    let at_second = events_at(&events, &second);
    assert_eq!(at_second.len(), 1, "{events:#?}");
    assert_eq!(at_second[0].field, "status");
    assert_eq!(at_second[0].old_value.as_deref(), Some("todo"));
    assert_eq!(at_second[0].new_value.as_deref(), Some("in_progress"));
    assert_eq!(
        at_second[0].parent_sha, first,
        "diffed against the parent commit"
    );
    // Oldest-first: creation events come before the edit.
    assert!(events.first().map(|e| e.commit_sha.as_str()) == Some(first.as_str()));
}

#[test]
fn merge_commits_diff_against_every_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    let path = tmp.path().join(".dit/issues/2026/08/one/issue.md");
    write(
        &path,
        &issue("01BBBBBBBBBBBBBBBBBBBBBBBB", "Login timeout", "todo", "p2"),
    );
    repo.add(".dit").unwrap();
    repo.commit("create").unwrap().unwrap();

    // Two sides change different fields of the same file — separated by an
    // unchanged line so git's plain text merge resolves it (this test is
    // about the walker, not the merge driver).
    let base = issue("01BBBBBBBBBBBBBBBBBBBBBBBB", "Login timeout", "todo", "p2");
    repo.git(&["checkout", "-b", "side"]).unwrap();
    write(
        &path,
        &base.replace("2026-08-16T09:00:00Z", "2026-08-16T10:00:00Z"),
    );
    repo.add(".dit").unwrap();
    let side_tip = repo.commit("touch updated").unwrap().unwrap();

    repo.git(&["checkout", "main"]).unwrap();
    write(&path, &base.replace("status: todo", "status: in_progress"));
    repo.add(".dit").unwrap();
    let main_tip = repo.commit("start work").unwrap().unwrap();

    repo.git(&["merge", "side", "-m", "merge side"]).unwrap();
    let merge_sha = repo.head().unwrap();

    let events = walk_field_events(&repo, None).unwrap();
    let at_merge = events_at(&events, &merge_sha);
    // One event per parent diff: the timestamp change is visible against
    // main, the status change against side. Neither silently disappears.
    assert_eq!(at_merge.len(), 2, "{events:#?}");
    let mut by_field: Vec<(&str, &str)> = at_merge
        .iter()
        .map(|e| (e.field.as_str(), e.parent_sha.as_str()))
        .collect();
    by_field.sort_unstable();
    assert!(
        by_field.contains(&("updated", main_tip.as_str())),
        "{by_field:?}"
    );
    assert!(
        by_field.contains(&("status", side_tip.as_str())),
        "{by_field:?}"
    );
}

#[test]
fn moved_folders_keep_history_keyed_to_the_issue_id() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    let old = ".dit/issues/2026/08/one";
    let new = ".dit/issues/archive/one";
    write(
        &tmp.path().join(old).join("issue.md"),
        &issue("01CCCCCCCCCCCCCCCCCCCCCCCC", "Login timeout", "todo", "p2"),
    );
    repo.add(".dit").unwrap();
    repo.commit("create").unwrap().unwrap();

    fs::create_dir_all(tmp.path().join(".dit/issues/archive")).unwrap();
    repo.git(&["mv", old, new]).unwrap();
    repo.add(".dit").unwrap();
    let move_sha = repo.commit("archive it").unwrap().unwrap();

    let events = walk_field_events(&repo, None).unwrap();
    assert!(
        events_at(&events, &move_sha).is_empty(),
        "a pure move changes no field values: {events:#?}"
    );

    // And an edit after the move still belongs to the same id.
    write(
        &tmp.path().join(new).join("issue.md"),
        &issue("01CCCCCCCCCCCCCCCCCCCCCCCC", "Login timeout", "done", "p2"),
    );
    repo.add(".dit").unwrap();
    repo.commit("close it").unwrap().unwrap();
    let events = walk_field_events(&repo, None).unwrap();
    let status_events: Vec<&FieldEvent> = events.iter().filter(|e| e.field == "status").collect();
    assert_eq!(status_events.len(), 2, "{events:#?}");
    assert!(status_events
        .iter()
        .all(|e| e.issue_id == "01CCCCCCCCCCCCCCCCCCCCCCCC"));
}

#[test]
fn a_watermark_limits_the_walk_to_newer_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    let path = tmp.path().join(".dit/issues/2026/08/one/issue.md");

    write(
        &path,
        &issue("01DDDDDDDDDDDDDDDDDDDDDDDD", "Login timeout", "todo", "p2"),
    );
    repo.add(".dit").unwrap();
    repo.commit("create").unwrap().unwrap();
    write(
        &path,
        &issue(
            "01DDDDDDDDDDDDDDDDDDDDDDDD",
            "Login timeout",
            "in_progress",
            "p2",
        ),
    );
    repo.add(".dit").unwrap();
    let second = repo.commit("start").unwrap().unwrap();
    write(
        &path,
        &issue("01DDDDDDDDDDDDDDDDDDDDDDDD", "Login timeout", "done", "p2"),
    );
    repo.add(".dit").unwrap();
    let third = repo.commit("finish").unwrap().unwrap();

    let full = walk_field_events(&repo, None).unwrap();
    assert!(full.len() > 2, "creation + two edits: {full:#?}");

    let incremental = walk_field_events(&repo, Some(&second)).unwrap();
    let shas: Vec<&str> = incremental.iter().map(|e| e.commit_sha.as_str()).collect();
    assert!(
        !shas.contains(&second.as_str()),
        "the watermark commit is exclusive"
    );
    assert!(shas.iter().all(|s| *s == third), "{shas:?}");
    assert_eq!(
        incremental.len(),
        events_at(&full, &third).len(),
        "incremental and full walks agree on the newest commit"
    );
}
