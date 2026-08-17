//! Facade tests against real git repositories.
//!
//! These exercise the whole pipeline the way the CLI and server will: a
//! transaction writes files, git commits them, the index absorbs the result,
//! and reads answer from the index alone. Nothing here mocks git, SQLite or
//! the filesystem — every failure mode that matters only exists when the
//! real pieces run.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use dit_core::{DiagnosticLevel, Dit, DitError, ReindexMode};
use dit_model::{FieldPatch, IssueDraft, IssueKind, Priority};
use dit_vcs::Repo;

/// A repo-local identity so commits never depend on the machine's global
/// git config (which may sign, rename, or refuse), plus a .gitignore that
/// keeps the index database out of git.
fn workspace(path: &Path) -> Dit {
    let repo = Repo::init(path).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    std::fs::write(path.join(".gitignore"), ".dit-cache/\n").unwrap();
    repo.add(".gitignore").unwrap();
    repo.commit("bootstrap").unwrap();
    Dit::open(path).unwrap()
}

fn draft(title: &str) -> IssueDraft {
    IssueDraft {
        title: title.into(),
        kind: IssueKind::Bug,
        status: Some("todo".into()),
        priority: Some(Priority::P1),
        reporter: Some("farid".into()),
        assignees: vec!["farid".into()],
        labels: vec!["auth".into()],
        epic: None,
        estimate: Some(3),
        sprint: None,
        due: None,
        blocked_by: vec![],
        body: "Users get logged out.".into(),
    }
}

#[test]
fn open_rejects_a_plain_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let err = Dit::open(tmp.path()).unwrap_err();
    assert!(
        err.to_string().contains("not inside a git repository"),
        "{err}"
    );
}

#[test]
fn a_committed_create_is_visible_to_every_read() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    let short = id.short_ref().as_str().to_owned();
    let sha = tx.commit("create 1 issue").unwrap().unwrap();
    assert_eq!(sha.len(), 40);

    // By full id and by short ref, from the index.
    assert!(dit.get(id.as_str()).unwrap().is_some());
    let by_short = dit.get(&short).unwrap().expect("short ref resolves");
    assert_eq!(by_short.issue.id, id);
    // By DQL.
    let hits = dit.query("status = todo AND label = auth", None).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    // History includes the creation events.
    let events = dit.history(&id, Some("status")).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].new_value.as_deref(), Some("todo"));
    // git saw exactly one commit, and the tree is clean.
    assert!(!dit.status().dirty);
}

#[test]
fn a_status_change_updates_the_index_and_appends_history() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.commit("create").unwrap();

    let mut tx = dit.transaction("budi").unwrap();
    tx.set_fields(
        &id,
        FieldPatch {
            status: Some("in_progress".into()),
            ..FieldPatch::default()
        },
    )
    .unwrap();
    tx.commit("start work").unwrap();

    let issue = dit.get(id.as_str()).unwrap().unwrap();
    assert_eq!(issue.issue.status, "in_progress");
    let events = dit.history(&id, Some("status")).unwrap();
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[1].old_value.as_deref(), Some("todo"));
    assert_eq!(events[1].new_value.as_deref(), Some("in_progress"));
    assert_eq!(
        events[1].author, "budi",
        "attribution follows the transaction author"
    );
}

#[test]
fn comments_are_written_committed_and_readable_from_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    let cid = tx.comment(&id, "budi", "Reproduced on 3G.").unwrap();
    tx.commit("create with comment").unwrap();

    let comments = dit.comments(&id).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, cid);
    assert_eq!(comments[0].author, "budi");
    assert_eq!(comments[0].body, "Reproduced on 3G.");
}

#[test]
fn abort_discards_everything_and_makes_no_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let head_before = dit.status().head;

    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft("Login timeout")).unwrap();
    tx.abort();

    assert_eq!(dit.status().head, head_before, "no commit happened");
    assert!(
        dit.query("", None).unwrap().is_empty(),
        "nothing was indexed"
    );
    assert!(!dit.status().dirty, "no stray files on disk");
    // The lock is gone, so the next writer can proceed.
    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft("Fresh start")).unwrap();
    tx.commit("works after abort").unwrap().unwrap();
}

#[test]
fn a_second_writer_is_told_who_holds_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut other = Dit::open(tmp.path()).unwrap();

    let _tx = dit.transaction("farid").unwrap();
    let err = other.transaction("budi").unwrap_err();
    match err {
        DitError::Busy { held_by } => assert_eq!(held_by, "farid"),
        other => panic!("expected Busy, got {other:?}"),
    }
}

#[test]
fn a_failed_git_commit_rolls_the_files_back() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft("Login timeout")).unwrap();
    // A stale git index lock makes `git add` fail after the files are already
    // on disk — a real post-write failure, not a simulated one.
    std::fs::write(tmp.path().join(".git/index.lock"), b"stale").unwrap();
    let err = tx.commit("create").unwrap_err();
    assert!(err.to_string().contains("git"), "{err}");
    std::fs::remove_file(tmp.path().join(".git/index.lock")).unwrap();
    assert!(!dit.status().dirty, "the written file was rolled back");
    assert!(dit.query("", None).unwrap().is_empty());
}

#[test]
fn reindex_rebuilds_a_deleted_index_from_git_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.comment(&id, "budi", "Seen on Safari too.").unwrap();
    tx.commit("create").unwrap();

    // Simulate a lost cache: a fresh Dit on a wiped database file.
    drop(dit);
    let cache = tmp.path().join(".dit-cache/index.sqlite");
    std::fs::remove_file(&cache).unwrap();
    let mut dit = Dit::open(tmp.path()).unwrap();
    assert!(
        dit.get(id.as_str()).unwrap().is_none(),
        "empty index answers nothing"
    );

    let report = dit.reindex(ReindexMode::All).unwrap();
    assert_eq!(report.issues, 1, "{report:?}");
    assert_eq!(report.comments, 1, "{report:?}");
    assert!(report.events > 0, "{report:?}");
    assert!(report.skipped == 0, "{report:?}");

    assert!(dit.get(id.as_str()).unwrap().is_some());
    assert_eq!(dit.comments(&id).unwrap().len(), 1);
    assert_eq!(dit.history(&id, Some("status")).unwrap().len(), 1);
}

#[test]
fn the_board_groups_issues_by_workflow_column() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let a = tx.create_issue(draft("First")).unwrap();
    let mut b_draft = draft("Second");
    b_draft.status = Some("in_progress".into());
    let b = tx.create_issue(b_draft).unwrap();
    tx.commit("two issues").unwrap();

    let board = dit.board().unwrap();
    let todo = board.columns.iter().find(|c| c.status == "todo").unwrap();
    let doing = board
        .columns
        .iter()
        .find(|c| c.status == "in_progress")
        .unwrap();
    assert_eq!(todo.issues.len(), 1);
    assert_eq!(todo.issues[0].issue.id, a);
    assert_eq!(doing.issues.len(), 1);
    assert_eq!(doing.issues[0].issue.id, b);
    // Column order follows the workflow declaration, not the data — and the
    // default workflow starts with backlog.
    assert_eq!(board.columns.first().unwrap().status, "backlog");
}

#[test]
fn doctor_names_what_is_wrong_and_what_would_break() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());

    let fresh = dit.doctor();
    assert!(fresh
        .iter()
        .any(|d| d.code == "merge-driver" && d.level == DiagnosticLevel::Warn));

    let tx = Dit::open(tmp.path()).unwrap();
    // The merge driver contract needs an executable; any real file stands in.
    let driver = tmp.path().join("driver.sh");
    std::fs::write(&driver, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let repo = Repo::open(tmp.path()).unwrap();
    repo.configure_merge_driver(&format!("{} %O %A %B %L %P", driver.display()))
        .unwrap();
    drop(tx);

    let after = Dit::open(tmp.path()).unwrap().doctor();
    assert!(after
        .iter()
        .any(|d| d.code == "merge-driver" && d.level == DiagnosticLevel::Ok));
}

#[test]
fn an_unparsable_workflow_falls_back_but_is_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());
    assert!(dit
        .doctor()
        .iter()
        .any(|d| d.code == "schema" && d.level == DiagnosticLevel::Ok));

    std::fs::create_dir_all(tmp.path().join(".dit/schema")).unwrap();
    std::fs::write(
        tmp.path().join(".dit/schema/workflow.yaml"),
        "statuses: [ broken\n",
    )
    .unwrap();
    let dit = Dit::open(tmp.path()).unwrap();
    // Still opens, still has a usable default workflow.
    assert!(!dit.workflow().statuses.is_empty());
    assert!(dit
        .doctor()
        .iter()
        .any(|d| d.code == "schema" && d.level == DiagnosticLevel::Error));
}

/// `dit init` in a directory that already holds a project must join it, not
/// colonize it: the project's .gitignore keeps its lines (gaining only the
/// cache entry) and its README is left alone. Found by dogfooding init in
/// DIT's own repo — the first run rewrote both files and committed it.
#[test]
fn init_in_an_existing_project_preserves_gitignore_and_readme() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join(".gitignore"),
        "/target\nnode_modules/\n.env\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("README.md"), "# My Project\n\nReal docs.\n").unwrap();

    Dit::init(tmp.path(), Path::new("/bin/true")).unwrap();

    let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.contains("/target"),
        "existing lines survive:\n{gitignore}"
    );
    assert!(gitignore.contains("node_modules/"));
    assert!(
        gitignore.contains(".dit-cache/"),
        "the cache entry is added:\n{gitignore}"
    );

    let readme = std::fs::read_to_string(tmp.path().join("README.md")).unwrap();
    assert_eq!(
        readme, "# My Project\n\nReal docs.\n",
        "the README is untouched"
    );

    // The bootstrap commit carries the appended entry, not a clobbered file.
    let repo = Repo::open(tmp.path()).unwrap();
    let committed = repo.show_text("HEAD:.gitignore").unwrap_or_default();
    assert!(committed.contains("/target"), "history keeps the old lines");
}

/// A second init over an already-initialized workspace is a no-op on disk:
/// the ignore entry is present and no empty commit is created.
#[test]
fn init_twice_appends_once_and_skips_the_empty_commit() {
    let tmp = tempfile::tempdir().unwrap();
    Dit::init(tmp.path(), Path::new("/bin/true")).unwrap();
    let before = Repo::open(tmp.path()).unwrap().head().unwrap();
    let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();

    Dit::init(tmp.path(), Path::new("/bin/true")).unwrap();

    let after = Repo::open(tmp.path()).unwrap().head().unwrap();
    assert_eq!(before, after, "re-init must not move history");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap(),
        gitignore,
        "the ignore entry is not duplicated"
    );
}
