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
use dit_model::{FieldPatch, IssueDraft, IssueKind, Numbering, Priority};
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
        start: None,
        blocked_by: vec![],
        lane: None,
        number: None,
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
    let cid = tx.comment(&id, "budi", None, "Reproduced on 3G.").unwrap();
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
    tx.comment(&id, "budi", None, "Seen on Safari too.")
        .unwrap();
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

    Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();

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
    Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();
    let before = Repo::open(tmp.path()).unwrap().head().unwrap();
    let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();

    Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();

    let after = Repo::open(tmp.path()).unwrap().head().unwrap();
    assert_eq!(before, after, "re-init must not move history");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap(),
        gitignore,
        "the ignore entry is not duplicated"
    );
}

// -- ADR 0005–0008: visible layout, numbers, templates, generated index -----

use dit_core::DataLayout;
use dit_model::IssueId;

fn draft_with(title: &str, body: &str) -> IssueDraft {
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
        start: None,
        blocked_by: vec![],
        lane: None,
        number: None,
        body: body.into(),
    }
}

#[test]
fn init_scaffolds_the_visible_layout() {
    let tmp = tempfile::tempdir().unwrap();
    Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();

    // The five content roots are visible at the tree root (ADR 0005).
    for root in dit_model::CONTENT_ROOTS {
        assert!(
            tmp.path().join(root).is_dir(),
            "{root} missing from the root"
        );
    }
    // Machinery stays hidden — and the config states where data goes.
    let config = std::fs::read_to_string(tmp.path().join(".dit/config.yaml")).unwrap();
    assert!(config.contains("layout: root"), "{config}");
    assert!(config.contains("numbering: local"), "{config}");

    // Merge-driver routing sits at the tree root (ADR 0005).
    let attrs = std::fs::read_to_string(tmp.path().join(".gitattributes")).unwrap();
    assert!(attrs.contains("*.md merge=dit-md"), "{attrs}");
    assert!(attrs.contains("**/comments/*.md merge=dit-md"), "{attrs}");

    // Issue templates are seeded (the evidence-first shape).
    assert!(tmp.path().join(".dit/templates/default.md").is_file());
    assert!(tmp.path().join(".dit/templates/bug.md").is_file());

    // All of it is committed — a fresh clone sees the same scaffolding.
    let repo = Repo::open(tmp.path()).unwrap();
    assert!(repo.show_text("HEAD:.gitattributes").is_some());
    assert!(repo.show_text("HEAD:.dit/config.yaml").is_some());
    assert!(repo
        .show_text("HEAD:.gitignore")
        .unwrap()
        .contains(".dit-cache/"));
}

#[test]
fn init_can_lay_out_dotdir_for_guest_repos() {
    let tmp = tempfile::tempdir().unwrap();
    Dit::init_with_layout(
        tmp.path(),
        &std::env::current_exe().unwrap(),
        DataLayout::DotDir,
    )
    .unwrap();

    assert!(tmp.path().join(".dit/issues").is_dir());
    assert!(
        !tmp.path().join("issues").exists(),
        "the root stays the host's"
    );
    let config = std::fs::read_to_string(tmp.path().join(".dit/config.yaml")).unwrap();
    assert!(config.contains("layout: dotdir"), "{config}");
    // The attributes file hides under .dit/ with the rest (Mode C).
    assert!(tmp.path().join(".dit/.gitattributes").is_file());
    assert!(!tmp.path().join(".gitattributes").exists());
}

/// A directory that already owns `issues/` is not ours to write into: init
/// refuses rather than colonizes (ADR 0005).
#[test]
fn init_refuses_a_tree_that_already_owns_a_content_root() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("issues/mine")).unwrap();
    std::fs::write(tmp.path().join("issues/mine/plan.md"), "# My stuff\n").unwrap();
    let err = Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("issues"),
        "the refusal names the conflicting root: {err}"
    );
    assert!(
        !tmp.path().join(".gitattributes").exists(),
        "nothing was written before the refusal"
    );
}

/// Old workspaces (data under `.dit/issues/`, no `layout:` anywhere) must not
/// be silently split by a root-layout init — the migration command owns that
/// move.
#[test]
fn init_refuses_a_legacy_workspace_and_points_at_migration() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join(".dit/issues/2026/08")).unwrap();
    let err = Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("migrate"),
        "the refusal offers the way out: {err}"
    );
}

#[test]
fn creation_assigns_sequential_numbers_when_numbering_is_local() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    let mut tx = dit.transaction("farid").unwrap();
    let a = tx.create_issue(draft_with("First", "Body.")).unwrap();
    // Two issues in ONE transaction must still count up — the index does not
    // know about the first staged issue yet, so the facade carries the cursor.
    let b = tx.create_issue(draft_with("Second", "Body.")).unwrap();
    tx.commit("two issues").unwrap();

    assert_eq!(dit.get(a.as_str()).unwrap().unwrap().issue.number, Some(1));
    assert_eq!(dit.get(b.as_str()).unwrap().unwrap().issue.number, Some(2));

    // The next writer starts from the indexed max, not from 1 again.
    let mut tx = dit.transaction("farid").unwrap();
    let c = tx.create_issue(draft_with("Third", "Body.")).unwrap();
    tx.commit("third").unwrap();
    assert_eq!(dit.get(c.as_str()).unwrap().unwrap().issue.number, Some(3));

    // Numbers are queryable in DQL.
    let hits = dit.query("number = 2", None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].issue.id, b);

    // And `#2` resolves like a short ref does.
    assert_eq!(dit.get("#2").unwrap().unwrap().issue.id, b);
    assert!(dit.get("#99").unwrap().is_none());
}

#[test]
fn on_merge_numbering_leaves_numbers_unset_until_the_bot_assigns_them() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    std::fs::create_dir_all(tmp.path().join(".dit")).unwrap();
    std::fs::write(
        tmp.path().join(".dit/config.yaml"),
        "schema_version: 1\nlayout: dotdir\nnumbering: on-merge\n",
    )
    .unwrap();
    dit.reindex(ReindexMode::State).unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft_with("Unnumbered", "Body.")).unwrap();
    tx.commit("create").unwrap();

    assert_eq!(dit.get(id.as_str()).unwrap().unwrap().issue.number, None);
}

#[test]
fn set_numbering_flips_the_policy_and_takes_effect_immediately() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    dit.set_numbering(Numbering::OnMerge).unwrap();
    assert_eq!(dit.config().numbering, Numbering::OnMerge);
    // The change is committed, not left dirty in the tree.
    assert!(!dit.status().dirty);

    let mut tx = dit.transaction("farid").unwrap();
    let id = tx
        .create_issue(draft_with("Bot will number me", "Body."))
        .unwrap();
    tx.commit("create").unwrap();
    assert_eq!(dit.get(id.as_str()).unwrap().unwrap().issue.number, None);

    // And back: local numbering resumes from the indexed max.
    dit.set_numbering(Numbering::Local).unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx
        .create_issue(draft_with("Numbered again", "Body."))
        .unwrap();
    tx.commit("create").unwrap();
    assert_eq!(dit.get(id.as_str()).unwrap().unwrap().issue.number, Some(1));

    // The written config round-trips through a fresh open.
    let reopened = Dit::open(tmp.path()).unwrap();
    assert_eq!(reopened.config().numbering, Numbering::Local);
}

#[test]
fn renumber_backfills_unnumbered_issues_append_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    // #1 and #2 exist before the legacy pair — their numbers must not move.
    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft_with("First", "Body.")).unwrap();
    tx.create_issue(draft_with("Second", "Body.")).unwrap();
    tx.commit("two numbered").unwrap();

    // Two "legacy" issues: created under on-merge, so they have no number.
    dit.set_numbering(Numbering::OnMerge).unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    let older = tx
        .create_issue(draft_with("Older legacy", "Body."))
        .unwrap();
    let newer = tx
        .create_issue(draft_with("Newer legacy", "Body."))
        .unwrap();
    tx.commit("two legacy").unwrap();
    dit.set_numbering(Numbering::Local).unwrap();

    let count = dit.renumber().unwrap();
    assert_eq!(count, 2, "exactly the unnumbered issues gain a number");
    assert_eq!(
        dit.get(older.as_str()).unwrap().unwrap().issue.number,
        Some(3)
    );
    assert_eq!(
        dit.get(newer.as_str()).unwrap().unwrap().issue.number,
        Some(4)
    );
    assert!(
        older.as_str() < newer.as_str(),
        "fixture sanity: ULID order must be creation order here"
    );

    // The assignment is a visible, ordinary field edit (ADR 0009).
    let events = dit.history(&older, Some("number")).unwrap();
    assert_eq!(
        events.len(),
        1,
        "backfill logs a number field event: {events:?}"
    );

    // Append-only in one reviewable commit, and the tree is clean after.
    assert!(!dit.status().dirty);
    let head = std::process::Command::new("git")
        .args(["log", "--oneline", "-1", "--format=%s"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        "dit renumber: 2 issue(s)",
        "the backfill is a single commit"
    );

    // Idempotent: nothing left to do means no commit, not an error.
    assert_eq!(dit.renumber().unwrap(), 0);
    assert!(!dit.status().dirty);
}

#[test]
fn renumber_refuses_where_merge_serialization_owns_numbers() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.set_numbering(Numbering::OnMerge).unwrap();

    let err = dit.renumber().unwrap_err();
    assert!(
        err.to_string().contains("on-merge"),
        "the refusal names the policy that owns assignment: {err}"
    );
    assert!(!dit.status().dirty, "a refusal changes nothing");
}

#[test]
fn renumber_requires_a_clean_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    std::fs::write(tmp.path().join("scratch.txt"), "uncommitted").unwrap();

    let err = dit.renumber().unwrap_err();
    assert!(
        err.to_string().contains("not clean"),
        "the refusal says what to do: {err}"
    );
}

#[test]
fn an_empty_body_is_seeded_from_the_issue_template() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();

    // A kind with no template of its own (task) falls back to default.md.
    let mut draft = draft_with("Blank", "");
    draft.kind = IssueKind::Task;
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft).unwrap();
    tx.commit("create").unwrap();
    let path = &dit.get(id.as_str()).unwrap().unwrap().path;
    let body = std::fs::read_to_string(tmp.path().join(path)).unwrap();
    assert!(
        body.contains("## Summary")
            && body.contains("## Acceptance criteria")
            && body.contains("## Do not"),
        "the default template's sections are seeded:\n{body}"
    );

    // The issue's own kind seeds the body when no template is named.
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft_with("Crash", "")).unwrap();
    tx.commit("create").unwrap();
    let path = &dit.get(id.as_str()).unwrap().unwrap().path;
    let body = std::fs::read_to_string(tmp.path().join(path)).unwrap();
    assert!(
        body.contains("## Steps to reproduce") && body.contains("## Guard"),
        "the bug template's sections are seeded:\n{body}"
    );

    // A named template wins over the kind default.
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx
        .create_issue_from_template(draft_with("Crash", ""), "story")
        .unwrap();
    tx.commit("create").unwrap();
    let path = &dit.get(id.as_str()).unwrap().unwrap().path;
    let body = std::fs::read_to_string(tmp.path().join(path)).unwrap();
    assert!(body.contains("## Model"), "{body}");

    // A name that is not a template is an error, not a silent default.
    let mut tx = dit.transaction("farid").unwrap();
    let err = tx
        .create_issue_from_template(draft_with("X", ""), "nope")
        .unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
}

#[test]
fn the_generated_index_lists_issues_by_number_and_is_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft_with("First", "Body.")).unwrap();
    tx.create_issue(draft_with("Second", "Body.")).unwrap();
    tx.commit("two issues").unwrap();

    assert!(dit.build_docs_index().unwrap(), "the first build writes");
    let index_path = tmp.path().join("issues/README.md");
    let first = std::fs::read_to_string(&index_path).unwrap();
    assert!(
        first.starts_with("<!-- generated by dit"),
        "the marker is the first thing in the file:\n{first}"
    );
    assert!(first.contains("#1"), "{first}");
    assert!(
        first.contains("[First]("),
        "entries link into the folder: {first}"
    );
    assert!(
        first.contains("](2026/"),
        "links are relative to the issues root: {first}"
    );

    // Regeneration from an unchanged repo is byte-identical — otherwise every
    // CI run would commit churn (ADR 0008, determinism).
    assert!(
        !dit.build_docs_index().unwrap(),
        "an up-to-date index is not rewritten"
    );
    assert_eq!(std::fs::read_to_string(&index_path).unwrap(), first);

    // The generated file is an output, never an input (ADR 0008): it is not
    // an issue, not even a skipped one.
    let report = dit.reindex(ReindexMode::All).unwrap();
    assert_eq!(report.issues, 2, "{report:?}");
    assert_eq!(report.skipped, 0, "{report:?}");
}

#[test]
fn the_generated_index_shows_a_bare_short_ref_when_there_is_no_number() {
    // On-merge numbering (and every pre-ADR-0007 issue) has `number: None`.
    // A `#` belongs to numbers alone — `#06M5683` reads as a number handle
    // and is exactly what a legacy workspace would render.
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.set_numbering(Numbering::OnMerge).unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft_with("Unnumbered", "Body.")).unwrap();
    tx.commit("one issue").unwrap();
    let short = id.short_ref().as_str().to_owned();

    dit.build_docs_index().unwrap();
    let index = std::fs::read_to_string(tmp.path().join("issues/README.md")).unwrap();
    assert!(
        index.contains(&format!("- **{short}** [Unnumbered](")),
        "an unnumbered issue shows its bare short ref:\n{index}"
    );
    assert!(
        !index.contains(&format!("#{short}")),
        "the hash is reserved for numbers:\n{index}"
    );
}

#[test]
fn doctor_flags_duplicate_numbers_as_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let a = tx.create_issue(draft_with("First", "Body.")).unwrap();
    let b = tx.create_issue(draft_with("Second", "Body.")).unwrap();
    tx.commit("two issues").unwrap();

    let before = dit.doctor();
    assert!(
        before
            .iter()
            .any(|d| d.code == "numbers" && d.level == DiagnosticLevel::Ok),
        "{before:?}"
    );

    // The repair hatch: a hand renumber that collides is visible in doctor.
    let mut tx = dit.transaction("farid").unwrap();
    tx.set_fields(
        &b,
        FieldPatch {
            number: Some(1),
            ..FieldPatch::default()
        },
    )
    .unwrap();
    tx.commit("renumber").unwrap();
    let _ = a;

    let after = dit.doctor();
    assert!(
        after
            .iter()
            .any(|d| d.code == "numbers" && d.level == DiagnosticLevel::Error),
        "{after:?}"
    );
}

#[test]
fn doctor_warns_about_unnumbered_issues_only_where_backfill_applies() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    // A legacy unnumbered issue under `local` numbering: doctor points at
    // the way out (ADR 0009).
    dit.set_numbering(Numbering::OnMerge).unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft_with("Legacy", "Body.")).unwrap();
    tx.commit("create").unwrap();
    dit.set_numbering(Numbering::Local).unwrap();

    let warned = dit.doctor();
    assert!(
        warned.iter().any(|d| d.code == "numbers"
            && d.level == DiagnosticLevel::Warn
            && d.message.contains("dit renumber")),
        "{warned:?}"
    );

    // Backfilling clears it.
    dit.renumber().unwrap();
    let healed = dit.doctor();
    assert!(
        healed
            .iter()
            .any(|d| d.code == "numbers" && d.level == DiagnosticLevel::Ok),
        "{healed:?}"
    );

    // And under `on-merge` a fresh unnumbered issue is by design: no warn.
    dit.set_numbering(Numbering::OnMerge).unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    tx.create_issue(draft_with("Bot numbers me at merge", "Body."))
        .unwrap();
    tx.commit("create").unwrap();
    let by_design = dit.doctor();
    assert!(
        by_design
            .iter()
            .any(|d| d.code == "numbers" && d.level == DiagnosticLevel::Ok),
        "{by_design:?}"
    );
}

/// The legacy fixture: a pre-ADR-0005 workspace — data under `.dit/issues/`,
/// bodies still named `issue.md`, no `layout:` in config. Field names are the
/// on-disk wire format (`type:`, not `kind:`).
fn legacy_issue() -> &'static str {
    "---\n\
     id: 01M08FFCAG185N2V5RTGBCDEFH\n\
     title: Fix login timeout\n\
     type: bug\n\
     status: todo\n\
     priority: p1\n\
     reporter: farid\n\
     assignees: [farid]\n\
     labels: [auth]\n\
     created: 2026-08-01T09:00:00Z\n\
     updated: 2026-08-01T09:00:00Z\n\
     ---\n\n\
     Users get logged out.\n"
}

#[test]
fn migrate_layout_moves_a_legacy_dotdir_workspace_to_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repo::init(tmp.path()).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    std::fs::write(tmp.path().join(".gitignore"), ".dit-cache/\n").unwrap();
    let dir = tmp
        .path()
        .join(".dit/issues/2026/08/01M08FFCAG-185N-fix-login");
    std::fs::create_dir_all(dir.join("comments")).unwrap();
    std::fs::write(dir.join("issue.md"), legacy_issue()).unwrap();
    std::fs::write(
        dir.join("comments/01M08FFCAX-185N-budi.md"),
        "---\nid: 01M08FFCAX185N2W5RTGBCDEFH\nauthor: budi\ncreated: 2026-08-01T10:00:00Z\n---\n\nSeen on Safari too.\n",
    )
    .unwrap();
    repo.add(".gitignore").unwrap();
    repo.add(".dit").unwrap();
    repo.commit("legacy workspace").unwrap();

    let mut dit = Dit::open(tmp.path()).unwrap();
    assert_eq!(dit.layout(), DataLayout::DotDir, "detected as legacy");

    let report = dit.migrate_layout(DataLayout::Root).unwrap();
    assert_eq!(report.roots_moved, 1, "{report:?}");
    assert_eq!(report.bodies_renamed, 1, "{report:?}");

    // Content now lives at the visible root, with README.md bodies.
    let new_body = tmp
        .path()
        .join("issues/2026/08/01M08FFCAG-185N-fix-login/README.md");
    assert!(new_body.is_file(), "the body was renamed in place");
    assert!(
        !tmp.path().join(".dit/issues").exists(),
        "the old tree is gone"
    );
    let config = std::fs::read_to_string(tmp.path().join(".dit/config.yaml")).unwrap();
    assert!(config.contains("layout: root"), "{config}");
    assert!(
        tmp.path().join(".gitattributes").is_file(),
        "attributes moved too"
    );

    // Everything survived: issue, comment, history — reindexed from the move.
    let id = IssueId::parse("01M08FFCAG185N2V5RTGBCDEFH").unwrap();
    assert!(dit.get(id.as_str()).unwrap().is_some());
    assert_eq!(dit.comments(&id).unwrap().len(), 1);
    assert!(!dit.history(&id, Some("status")).unwrap().is_empty());
    assert!(!dit.status().dirty, "the migration is one clean commit");

    // And new writes land in the new place, numbered after a rebuilt index.
    let mut tx = dit.transaction("farid").unwrap();
    let c = tx
        .create_issue(draft_with("Post-migration", "Body."))
        .unwrap();
    tx.commit("after migration").unwrap();
    assert!(dit
        .get(c.as_str())
        .unwrap()
        .unwrap()
        .path
        .starts_with("issues/"));
}

#[test]
fn migrate_layout_refuses_when_the_layout_is_already_current() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    // The `workspace()` fixture carries no legacy `.dit/issues/`, so it
    // detects as the root layout already.
    let err = dit.migrate_layout(DataLayout::Root).unwrap_err();
    assert!(err.to_string().contains("already"), "{err}");
}

// -- doc pages (§13; file-backed reads per ADR 0010) -------------------------

#[test]
fn a_saved_doc_lands_as_one_commit_and_reads_back() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let repo = Repo::open(tmp.path()).unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc(
        "docs/adr-0010-doc-editor.md",
        "# Doc editor\n\nPage   text.\n",
    )
    .unwrap();
    let head = tx
        .commit("dit docs save: docs/adr-0010-doc-editor.md")
        .unwrap()
        .expect("one commit for one save");

    // The file exists, reads back canonically formatted, and the commit
    // message names it.
    let text = dit.read_doc("docs/adr-0010-doc-editor.md").unwrap();
    assert!(text.starts_with("# Doc editor"), "{text}");
    assert_eq!(text, dit_parse::fmt::fmt(&text).unwrap());
    let subject = repo
        .log_lines(format!("{head}~1..{head}").as_str(), "%s")
        .unwrap();
    assert_eq!(
        subject,
        vec!["dit docs save: docs/adr-0010-doc-editor.md".to_owned()]
    );

    // The listing sees it, with display metadata from the filesystem.
    let docs = dit.list_docs();
    let paths: Vec<&str> = docs.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec!["docs/adr-0010-doc-editor.md"]);
    assert!(docs[0].bytes > 0, "size comes from the file");
}

#[test]
fn a_deleted_doc_disappears_in_one_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc("notes/scratch.md", "# Scratch\n").unwrap();
    tx.commit("dit docs save: notes/scratch.md").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    tx.delete_doc("notes/scratch.md").unwrap();
    tx.commit("dit docs delete: notes/scratch.md").unwrap();

    assert!(dit.list_docs().is_empty());
    let err = dit.read_doc("notes/scratch.md").unwrap_err();
    assert!(matches!(err, DitError::NotFound(_)), "{err}");
    assert!(!tmp.path().join("notes/scratch.md").exists());
}

#[test]
fn a_moved_doc_carries_its_content_and_history_in_one_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc("docs/flows/auth.md", "# Auth\n\nbody text\n")
        .unwrap();
    tx.commit("dit docs save: docs/flows/auth.md").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    tx.move_doc("docs/flows/auth.md", "notes/auth.md").unwrap();
    tx.commit("dit docs move: docs/flows/auth.md -> notes/auth.md")
        .unwrap();

    // The new location serves the exact bytes; the old one is gone.
    assert_eq!(
        dit.read_doc("notes/auth.md").unwrap(),
        "# Auth\n\nbody text\n"
    );
    assert!(matches!(
        dit.read_doc("docs/flows/auth.md").unwrap_err(),
        DitError::NotFound(_)
    ));
    let listed = dit.list_docs();
    let paths: Vec<&str> = listed.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec!["notes/auth.md"]);
    assert!(!dit.status().dirty);

    // Git recorded it as a rename, so the page's history follows it — the
    // same guarantee the layout migration gives (ADR 0005).
    let rename = std::process::Command::new("git")
        .args(["log", "-1", "--name-status", "--format="])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&rename.stdout).trim(),
        "R100\tdocs/flows/auth.md\tnotes/auth.md",
        "the move must be one rename commit, not delete-plus-create"
    );
}

#[test]
fn moving_a_doc_onto_an_existing_page_is_refused_without_writing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc("docs/a.md", "# A\n").unwrap();
    tx.write_doc("notes/b.md", "# B\n").unwrap();
    tx.commit("dit docs save: two pages").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    let err = tx.move_doc("docs/a.md", "notes/b.md").unwrap_err();
    assert!(matches!(err, DitError::Refuse(_)), "{err}");
    tx.abort();

    // A refused move writes nothing: both pages intact, tree clean.
    assert_eq!(dit.read_doc("docs/a.md").unwrap(), "# A\n");
    assert_eq!(dit.read_doc("notes/b.md").unwrap(), "# B\n");
    assert!(!dit.status().dirty);
}

#[test]
fn moving_a_missing_doc_is_not_found_and_a_self_move_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc("docs/stay.md", "# Stay\n").unwrap();
    tx.commit("dit docs save: docs/stay.md").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    assert!(matches!(
        tx.move_doc("docs/ghost.md", "notes/ghost.md").unwrap_err(),
        DitError::NotFound(_)
    ));
    tx.abort();

    // Renaming a page onto itself stages nothing — no commit, no error.
    let mut tx = dit.transaction("farid").unwrap();
    tx.move_doc("docs/stay.md", "docs/stay.md").unwrap();
    let commit = tx.commit("dit docs move: self").unwrap();
    assert!(
        commit.is_none(),
        "a self-move must not be a commit: {commit:?}"
    );
}

#[test]
fn doc_paths_are_sandboxed_at_the_facade() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    for escape in [
        "../outside.md",
        "issues/2026/08/x-readme/readme.md",
        ".dit/config.yaml",
        "docs/../notes/x.md",
    ] {
        let mut tx = dit.transaction("farid").unwrap();
        let err = tx.write_doc(escape, "# nope\n").unwrap_err();
        assert!(!err.to_string().is_empty(), "{escape} refused: {err}");
        tx.abort();
    }
    // Nothing escaped: no doc roots, nothing outside the workspace.
    assert!(dit.list_docs().is_empty());
    assert!(!tmp.path().parent().unwrap().join("outside.md").exists());
}

#[test]
fn list_docs_skips_names_the_editor_cannot_address() {
    // Hand-made files with names outside the DocPath rules (uppercase,
    // dotfiles) must not break the listing — they are simply not editable
    // through the doc editor until renamed.
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());
    std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
    std::fs::write(tmp.path().join("docs/good.md"), "# good\n").unwrap();
    std::fs::write(tmp.path().join("docs/BAD-NAME.MD"), "# bad\n").unwrap();
    std::fs::write(tmp.path().join("docs/.secret.md"), "# hidden\n").unwrap();
    std::fs::write(tmp.path().join("docs/notes.txt"), "not markdown\n").unwrap();

    let docs = dit.list_docs();
    let paths: Vec<&str> = docs.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, vec!["docs/good.md"]);
}

#[test]
fn the_activity_feed_and_time_travel_read_the_whole_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    let mut tx = dit.transaction("farid").unwrap();
    let first = tx.create_issue(draft("Login timeout")).unwrap();
    tx.commit("create the first issue").unwrap();
    // The board as it stood right after the first issue was created.
    let after_first = dit.activity(None, 10).unwrap()[0].seq;

    let mut tx = dit.transaction("budi").unwrap();
    let second = tx
        .create_issue(draft("Merge driver drops changes"))
        .unwrap();
    tx.commit("create the second issue").unwrap();

    let mut tx = dit.transaction("budi").unwrap();
    tx.set_fields(
        &first,
        FieldPatch {
            status: Some("done".into()),
            ..FieldPatch::default()
        },
    )
    .unwrap();
    tx.commit("finish the first issue").unwrap();

    // The feed spans issues, newest first, and pages by cursor.
    let page = dit.activity(None, 2).unwrap();
    assert_eq!(page.len(), 2);
    assert!(page[0].seq > page[1].seq, "newest first");
    let next = dit.activity(Some(page[1].seq), 50).unwrap();
    assert!(
        next.iter().all(|e| e.seq < page[1].seq),
        "the cursor never repeats a row"
    );
    let ids: std::collections::HashSet<_> = dit
        .activity(None, 100)
        .unwrap()
        .into_iter()
        .map(|e| e.issue_id)
        .collect();
    assert!(ids.contains(first.as_str()) && ids.contains(second.as_str()));

    // Now: two issues, one of them finished.
    let now = dit.activity_summary(None, 365).unwrap();
    assert_eq!(now.now.done, 1);
    assert_eq!(now.now.todo, 1);
    assert_eq!(now.seq, now.max_seq, "no cutoff means the end of history");

    // Then: only the first issue existed, and it was not done yet.
    let then = dit.activity_summary(Some(after_first), 365).unwrap();
    assert_eq!(then.at_cutoff.todo, 1);
    assert_eq!(then.at_cutoff.done, 0);
    assert_eq!(then.now.done, 1, "now is always now, whatever the cutoff");
    assert_eq!(then.since.created, 1, "the second issue was born after");
    assert_eq!(then.since.finished, 1);
    assert!(then.since.touched >= 2);

    // The histogram covers the days work actually happened on.
    assert!(!now.days.is_empty());
    assert_eq!(
        now.days.iter().map(|d| d.count).sum::<usize>(),
        dit.activity(None, 500).unwrap().len()
    );

    // A cutoff past the end is clamped rather than inventing a future.
    let clamped = dit.activity_summary(Some(9_999), 365).unwrap();
    assert_eq!(clamped.seq, clamped.max_seq);
}

#[test]
fn a_start_date_is_stored_queried_and_read_back() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    let mut tx = dit.transaction("farid").unwrap();
    let id = tx
        .create_issue(draft("Index rebuild after force push"))
        .unwrap();
    tx.commit("create").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    tx.set_fields(
        &id,
        FieldPatch {
            start: Some("2026-09-20".into()),
            due: Some("2026-09-30".into()),
            ..FieldPatch::default()
        },
    )
    .unwrap();
    tx.commit("schedule it").unwrap();

    // Read path: the index carries it, not just the file.
    let stored = dit.get(id.as_str()).unwrap().unwrap();
    assert_eq!(stored.issue.start.as_deref(), Some("2026-09-20"));
    assert_eq!(stored.issue.due.as_deref(), Some("2026-09-30"));

    // Query path: the field is a real column the index can filter on. DQL
    // date comparisons take relative dates (`start <= +7d`), which the query
    // crate pins against an injected clock; here the point is only that the
    // field reached the index at all.
    assert_eq!(dit.query("start <= +3650d", None).unwrap().len(), 1);

    // History path: scheduling is a change like any other, so the Gantt's
    // drag is auditable rather than silent.
    let events = dit.history(&id, Some("start")).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].new_value.as_deref(), Some("2026-09-20"));
    assert_eq!(events[0].author, "farid");

    // The tree is clean: one commit wrote both fields.
    assert!(!dit.status().dirty);
}

#[test]
fn recent_comments_span_issues_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());

    let mut tx = dit.transaction("farid").unwrap();
    let a = tx.create_issue(draft("Login timeout")).unwrap();
    let b = tx
        .create_issue(draft("Merge driver drops changes"))
        .unwrap();
    tx.commit("create 2 issues").unwrap();

    // Nothing yet: an empty feed, not an error.
    assert!(dit.recent_comments(10).unwrap().is_empty());

    let mut tx = dit.transaction("farid").unwrap();
    tx.comment(&a, "farid", None, "first on a").unwrap();
    tx.commit("comment").unwrap();
    let mut tx = dit.transaction("budi").unwrap();
    tx.comment(&b, "budi", None, "then on b").unwrap();
    tx.commit("comment").unwrap();

    let feed = dit.recent_comments(10).unwrap();
    assert_eq!(feed.len(), 2, "{feed:?}");
    // Newest first, each row naming the issue it belongs to.
    assert_eq!(feed[0].issue_id, b);
    assert_eq!(feed[0].title, "Merge driver drops changes");
    assert_eq!(feed[0].comment.author, "budi");
    assert_eq!(feed[1].issue_id, a);
    assert_eq!(feed[1].comment.body.trim(), "first on a");
    assert_eq!(dit.recent_comments(1).unwrap().len(), 1);
}

const RELEASE_FILE: &str = "---\nversion: v0.2.0\nstatus: in_uat\ntarget_ref: release/0.2.0\nrepo: api\ntarget: 2026-10-01\nincludes:\n  - 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\napproved_by: qa-lead\n---\n\nShips the login flow.\n";

/// Commit a release plan the way `dit release plan` (v0.9) eventually will —
/// by hand, through git, because the read model ships before the writer.
fn commit_release(root: &Path, version: &str, text: &str) {
    let dir = root.join(".dit/releases").join(version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("release.md"), text).unwrap();
    let repo = Repo::open(root).unwrap();
    repo.add(".dit/releases").unwrap();
    repo.commit(&format!("plan {version}")).unwrap();
}

#[test]
fn a_workspace_without_releases_answers_with_an_empty_list() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    assert!(dit.releases().unwrap().is_empty());
    assert!(dit.release("v0.2.0").unwrap().is_none());
    // A full rebuild over a tree with no `.dit/releases/` is fine too.
    dit.reindex(ReindexMode::All).unwrap();
    assert!(dit.releases().unwrap().is_empty());
    // And patching a plan that does not exist is NotFound, not a crash.
    let mut tx = dit.transaction("farid").unwrap();
    let err = tx
        .set_release(
            "v0.2.0",
            dit_model::ReleasePatch {
                status: Some(dit_model::ReleaseStatus::Released),
                target: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, DitError::NotFound(_)), "{err}");
}

#[test]
fn releases_are_indexed_from_git_and_patched_in_one_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    commit_release(tmp.path(), "v0.2.0", RELEASE_FILE);
    commit_release(
        tmp.path(),
        "v0.1.0",
        "---\nversion: v0.1.0\nstatus: released\ntarget: 2026-09-01\n---\n",
    );
    commit_release(
        tmp.path(),
        "v1.0.0",
        "---\nversion: v1.0.0\nstatus: planned\n---\n",
    );

    // Hand-made commits are not absorbed by a transaction; a rebuild is.
    dit.reindex(ReindexMode::All).unwrap();
    let versions: Vec<String> = dit
        .releases()
        .unwrap()
        .into_iter()
        .map(|r| r.release.version)
        .collect();
    assert_eq!(
        versions,
        ["v0.1.0", "v0.2.0", "v1.0.0"],
        "dated first, then by version"
    );
    let plan = dit.release("v0.2.0").unwrap().expect("indexed");
    assert_eq!(plan.release.status, dit_model::ReleaseStatus::InUat);
    assert_eq!(plan.release.target.as_deref(), Some("2026-10-01"));
    assert_eq!(plan.release.includes.len(), 1);
    assert_eq!(plan.path, ".dit/releases/v0.2.0/release.md");

    // One patch, one commit, attributed to the person who acted.
    let head_before = Repo::open(tmp.path()).unwrap().head().unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    tx.set_release(
        "v0.2.0",
        dit_model::ReleasePatch {
            status: Some(dit_model::ReleaseStatus::Released),
            target: Some("2026-10-03".into()),
        },
    )
    .unwrap();
    let sha = tx.commit("release v0.2.0: released").unwrap().unwrap();
    assert_ne!(sha, head_before);
    let repo = Repo::open(tmp.path()).unwrap();
    let message = repo.git(&["log", "-1", "--format=%B", &sha]).unwrap();
    assert!(message.contains("Dit-Author: farid"), "{message}");
    assert!(
        repo.is_clean().unwrap(),
        "the write is committed, not left dirty"
    );

    // The index absorbed the commit; the file kept what it did not touch.
    let plan = dit.release("v0.2.0").unwrap().expect("still indexed");
    assert_eq!(plan.release.status, dit_model::ReleaseStatus::Released);
    assert_eq!(plan.release.target.as_deref(), Some("2026-10-03"));
    assert_eq!(plan.release.target_ref.as_deref(), Some("release/0.2.0"));
    let on_disk =
        std::fs::read_to_string(tmp.path().join(".dit/releases/v0.2.0/release.md")).unwrap();
    assert!(on_disk.contains("approved_by: qa-lead"), "{on_disk}");
    assert!(on_disk.contains("Ships the login flow."), "{on_disk}");

    // Release files never masquerade as issues or events.
    assert!(dit.query("", None).unwrap().is_empty());
    assert!(dit.activity(None, 100).unwrap().is_empty());

    // The date is validated at the write boundary — nothing lands.
    let mut tx = dit.transaction("farid").unwrap();
    assert!(tx
        .set_release(
            "v0.2.0",
            dit_model::ReleasePatch {
                status: None,
                target: Some("soon".into()),
            },
        )
        .is_err());
}

#[test]
fn the_alias_is_persisted_per_clone_and_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());
    assert_eq!(dit.me(), None, "nothing configured yet");

    dit.set_me("farid").unwrap();
    assert_eq!(dit.me().as_deref(), Some("farid"));
    // Survives a reopen: it is the clone's setting, not the process's.
    assert_eq!(
        Dit::open(tmp.path()).unwrap().me().as_deref(),
        Some("farid")
    );
    // The surrounding whitespace is not part of the alias.
    dit.set_me("  budi ").unwrap();
    assert_eq!(dit.me().as_deref(), Some("budi"));

    // An alias that could not name a comment file is refused, so a later
    // comment never fails on it.
    for bad in ["", "   ", "Farid", "far id", "../x"] {
        let err = dit.set_me(bad).unwrap_err();
        assert!(matches!(err, DitError::Refuse(_)), "{bad:?}: {err}");
    }
    assert_eq!(
        dit.me().as_deref(),
        Some("budi"),
        "a refusal changes nothing"
    );
    // The setting is not workspace data: the tree stays clean.
    assert!(Repo::open(tmp.path()).unwrap().is_clean().unwrap());
}

#[test]
fn clearing_a_field_removes_it_from_the_file_and_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Has a due date")).unwrap();
    tx.commit("create").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    tx.set_fields(
        &id,
        FieldPatch {
            due: Some("2026-09-01".into()),
            ..FieldPatch::default()
        },
    )
    .unwrap();
    tx.commit("set due").unwrap();
    let path = tmp.path().join(dit.get(id.as_str()).unwrap().unwrap().path);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("due: 2026-09-01"));

    let mut tx = dit.transaction("farid").unwrap();
    tx.set_fields(
        &id,
        FieldPatch {
            clear: vec![
                dit_model::ClearableField::Due,
                dit_model::ClearableField::Priority,
            ],
            ..FieldPatch::default()
        },
    )
    .unwrap();
    tx.commit("clear due").unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("due:"), "{text}");
    assert!(!text.contains("priority:"), "{text}");
    let stored = dit.get(id.as_str()).unwrap().unwrap().issue;
    assert_eq!(stored.due, None);
    assert_eq!(stored.priority, None);
    // History records the removal as a change to nothing.
    let events = dit.history(&id, Some("due")).unwrap();
    let last = events.last().unwrap();
    assert_eq!(last.old_value.as_deref(), Some("2026-09-01"));
    assert_eq!(last.new_value, None);
}

#[test]
fn deleting_an_issue_removes_its_files_but_keeps_its_history() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let doomed = tx.create_issue(draft("Doomed")).unwrap();
    let survivor = tx.create_issue(draft("Survivor")).unwrap();
    tx.comment(&doomed, "farid", None, "last words").unwrap();
    tx.commit("create").unwrap();
    let folder = tmp
        .path()
        .join(dit.get(doomed.as_str()).unwrap().unwrap().path)
        .parent()
        .unwrap()
        .to_path_buf();
    assert!(folder.join("comments").is_dir());
    let events_before = dit.history(&doomed, None).unwrap().len();
    assert!(events_before > 0);

    let mut tx = dit.transaction("farid").unwrap();
    tx.delete_issue(&doomed).unwrap();
    let sha = tx.commit("delete doomed").unwrap().expect("one commit");
    let repo = Repo::open(tmp.path()).unwrap();
    assert!(repo.is_clean().unwrap(), "the deletion is committed");
    assert!(repo
        .git(&["log", "-1", "--format=%B", &sha])
        .unwrap()
        .contains("Dit-Author: farid"));

    // Files and folder gone; the other issue untouched.
    assert!(!folder.exists(), "{}", folder.display());
    assert!(dit.get(survivor.as_str()).unwrap().is_some());
    // The index dropped the row and its comments…
    assert!(dit.get(doomed.as_str()).unwrap().is_none());
    assert!(dit.comments(&doomed).unwrap().is_empty());
    assert_eq!(dit.query("", None).unwrap().len(), 1);
    assert!(dit.recent_comments(10).unwrap().is_empty());
    // …but history outlives its subject, and the deletion is itself an event.
    let events = dit.history(&doomed, Some("status")).unwrap();
    assert!(events.len() > 1, "{events:?}");
    assert_eq!(events.last().unwrap().new_value, None, "removed → nothing");

    // Deleting what is not there is NotFound; a full rebuild agrees.
    let mut tx = dit.transaction("farid").unwrap();
    assert!(matches!(
        tx.delete_issue(&doomed),
        Err(DitError::NotFound(_))
    ));
    drop(tx);
    dit.reindex(ReindexMode::All).unwrap();
    assert!(dit.get(doomed.as_str()).unwrap().is_none());
    assert_eq!(dit.query("", None).unwrap().len(), 1);
}

// -- ADR 0015/0018: the coordination plane -----------------------------------

use dit_core::{spawn_watcher, ClaimOptions, LaneSpec, Readiness};

fn issue_with(dit: &mut Dit, title: &str, patch: dit_core::FieldPatch) -> dit_core::IssueId {
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft(title)).unwrap();
    tx.set_fields(&id, patch).unwrap();
    tx.commit("create").unwrap().unwrap();
    id
}

#[test]
fn resolve_rejects_duplicate_numbers_naming_both_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let a = issue_with(
        &mut dit,
        "First holder of 285",
        dit_core::FieldPatch {
            number: Some(285),
            ..Default::default()
        },
    );
    let b = issue_with(
        &mut dit,
        "Second holder of 285",
        dit_core::FieldPatch {
            number: Some(285),
            ..Default::default()
        },
    );
    let err = dit.resolve("#285").unwrap_err();
    match err {
        DitError::Ambiguous { listing, .. } => {
            assert!(listing.contains(a.as_str()), "{listing}");
            assert!(listing.contains(b.as_str()), "{listing}");
            assert!(listing.contains("Second holder"), "{listing}");
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    // The short ref stays unambiguous and resolves exactly.
    assert_eq!(dit.resolve(a.short_ref().as_str()).unwrap(), a);
    assert_eq!(dit.resolve(b.as_str()).unwrap(), b);
    assert!(matches!(dit.resolve("#99"), Err(DitError::NotFound(_))));
}

#[test]
fn claim_guards_readiness_exclusivity_and_renewal() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let blocker = issue_with(&mut dit, "Backend endpoint", Default::default());
    let waiter = issue_with(
        &mut dit,
        "Frontend integration",
        dit_core::FieldPatch {
            blocked_by: Some(vec![blocker]),
            ..Default::default()
        },
    );

    // Blocked: the waiter cannot claim while the blocker is not through the gate.
    let err = dit
        .claim(&waiter, "fe-1", ClaimOptions::default())
        .unwrap_err();
    assert!(
        err.to_string().contains("blockers not through the gate"),
        "{err}"
    );
    assert!(
        err.to_string().contains(blocker.short_ref().as_str()),
        "{err}"
    );

    // Through the gate: claim lands as one commit and is readable back.
    issue_with(
        &mut dit,
        "Backend endpoint",
        dit_core::FieldPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    );
    // The patch above targeted the same issue? No: it created a new one.
    // Mark the actual blocker done.
    let mut tx = dit.transaction("be-1").unwrap();
    tx.set_fields(
        &blocker,
        dit_core::FieldPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("blocker done").unwrap();

    dit.claim(&waiter, "fe-1", ClaimOptions::default()).unwrap();
    let got = dit.get(waiter.as_str()).unwrap().unwrap();
    assert_eq!(got.issue.claimed_by.as_deref(), Some("fe-1"));
    assert!(got.issue.claimed_at.is_some());

    // A foreign live claim is refused with the way out named.
    let err = dit
        .claim(&waiter, "be-1", ClaimOptions::default())
        .unwrap_err();
    assert!(err.to_string().contains("--takeover"), "{err}");

    // Own live claim + plain claim again: nothing to write.
    let again = dit.claim(&waiter, "fe-1", ClaimOptions::default()).unwrap();
    assert!(!again.wrote, "{again:?}");

    // Renewal refreshes silently.
    let renew = dit
        .claim(
            &waiter,
            "fe-1",
            ClaimOptions {
                renew: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(renew.wrote);

    // Release clears the pair; then another actor can claim.
    dit.claim(
        &waiter,
        "fe-1",
        ClaimOptions {
            release: true,
            ..Default::default()
        },
    )
    .unwrap();
    let got = dit.get(waiter.as_str()).unwrap().unwrap();
    assert_eq!(got.issue.claimed_by, None);
    dit.claim(&waiter, "be-1", ClaimOptions::default()).unwrap();
}

#[test]
fn a_stale_claim_is_takable_and_renew_on_foreign_stale_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let id = issue_with(&mut dit, "Abandoned claim", Default::default());
    // Hand-age the claim far past any TTL.
    let mut tx = dit.transaction("fe-1").unwrap();
    tx.set_fields(
        &id,
        dit_core::FieldPatch {
            claimed_by: Some("fe-1".into()),
            claimed_at: Some("2020-01-01T00:00:00Z".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("old claim").unwrap();

    // Plain claim by another actor takes the stale claim over.
    dit.claim(&id, "be-1", ClaimOptions::default()).unwrap();
    let got = dit.get(id.as_str()).unwrap().unwrap();
    assert_eq!(got.issue.claimed_by.as_deref(), Some("be-1"));

    // Re-age and try --renew from the foreign actor: refused, naming the move.
    let mut tx = dit.transaction("be-1").unwrap();
    tx.set_fields(
        &id,
        dit_core::FieldPatch {
            claimed_at: Some("2020-01-01T00:00:00Z".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("re-age").unwrap();
    let err = dit
        .claim(
            &id,
            "fe-1",
            ClaimOptions {
                renew: true,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(err.to_string().contains("stale"), "{err}");
}

#[test]
fn ready_admits_only_what_the_gate_admits() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let blocker = issue_with(
        &mut dit,
        "Backend in review",
        dit_core::FieldPatch {
            status: Some("review".into()),
            ..Default::default()
        },
    );
    let waiter = issue_with(
        &mut dit,
        "Frontend waiting",
        dit_core::FieldPatch {
            blocked_by: Some(vec![blocker]),
            lane: Some("frontend".into()),
            ..Default::default()
        },
    );

    // Default gate (terminal): the review blocker keeps the waiter out.
    let ready = dit.ready(None, None).unwrap();
    assert!(ready.iter().all(|r| r.issue.issue.id != waiter));

    // --until review admits it for this call only.
    let ready = dit.ready(None, Some("review")).unwrap();
    assert!(ready.iter().any(|r| r.issue.issue.id == waiter));

    // Lane filter narrows to the lane's issues.
    let ready = dit.ready(Some("backend"), Some("review")).unwrap();
    assert!(ready.is_empty(), "the blocker is not in the backend lane");

    // An unknown status in --until is a refusal, not an empty answer.
    assert!(dit.ready(None, Some("shipping")).is_err());
}

#[test]
fn status_writes_validate_membership_unless_forced() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let id = issue_with(&mut dit, "Charset was not enough", Default::default());

    let mut tx = dit.transaction("farid").unwrap();
    let err = tx
        .set_fields(
            &id,
            dit_core::FieldPatch {
                status: Some("doing".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
    drop(tx);
    assert!(err.to_string().contains("`doing` is not one of"), "{err}");
    assert!(err.to_string().contains("backlog"), "{err}");

    let mut tx = dit.transaction("farid").unwrap();
    tx.set_fields_opts(
        &id,
        dit_core::FieldPatch {
            status: Some("doing".into()),
            ..Default::default()
        },
        true,
    )
    .unwrap();
    tx.commit("forced write").unwrap();
    assert_eq!(dit.get(id.as_str()).unwrap().unwrap().issue.status, "doing");
}

#[test]
fn refresh_state_absorbs_an_external_commit_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let id = issue_with(&mut dit, "Externally edited", Default::default());

    // An external process commits a change to the issue file behind the
    // facade's back: write, add, commit — no absorb happens.
    let path = tmp.path().join(dit.get(id.as_str()).unwrap().unwrap().path);
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replace("status: todo", "status: in_progress");
    std::fs::write(&path, edited).unwrap();
    let repo = dit_vcs::Repo::open(tmp.path()).unwrap();
    let rel = path
        .strip_prefix(tmp.path())
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    repo.add(&rel).unwrap();
    repo.commit("external edit").unwrap();

    // The facade's index is stale; refresh_state brings it in, exactly once.
    assert!(dit.refresh_state().unwrap(), "HEAD moved: rebuild expected");
    assert_eq!(
        dit.get(id.as_str()).unwrap().unwrap().issue.status,
        "in_progress"
    );
    assert!(!dit.refresh_state().unwrap(), "second run: nothing to do");
}

#[test]
fn workflow_init_scaffolds_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let lanes = vec![
        LaneSpec {
            id: "backend".into(),
            label: "Backend".into(),
            owners: vec!["be-1".into()],
        },
        LaneSpec {
            id: "frontend".into(),
            label: "Frontend".into(),
            owners: vec!["fe-1".into()],
        },
    ];
    let first = dit.init_workflow(&lanes).unwrap();
    assert!(first.lanes_written && first.protocol_written);

    let yaml = std::fs::read_to_string(".dit/schema/workflow.yaml")
        .or_else(|_| std::fs::read_to_string(tmp.path().join(".dit/schema/workflow.yaml")))
        .unwrap();
    assert!(yaml.contains("lanes:"), "{yaml}");
    assert!(yaml.contains("id: backend"), "{yaml}");
    assert!(yaml.contains("coordination:"), "{yaml}");

    let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(
        claude.contains("<!-- dit:workflow-protocol -->"),
        "{claude}"
    );
    assert!(claude.contains("dit ready --lane"), "{claude}");

    // Second run: nothing written, hand edits survive.
    std::fs::write(
        tmp.path().join("CLAUDE.md"),
        format!("{claude}\nA hand rule stays.\n"),
    )
    .unwrap();
    let second = dit.init_workflow(&lanes).unwrap();
    assert_eq!(
        second,
        dit_core::WorkflowInitReport::default(),
        "idempotent: nothing to do"
    );
    let claude_after = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(
        claude_after.contains("A hand rule stays."),
        "{claude_after}"
    );

    // The registered lanes are live in the facade after reload.
    let board = dit.workflow_board().unwrap();
    let ids: Vec<Option<String>> = board.lanes.iter().map(|l| l.id.clone()).collect();
    assert!(ids.contains(&Some("backend".into())), "{ids:?}");
    assert!(ids.contains(&Some("frontend".into())), "{ids:?}");
    assert!(
        ids.contains(&None),
        "the Unlaned row always exists: {ids:?}"
    );
}

#[test]
fn the_workflow_board_derives_blocker_and_claim_states() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[
        LaneSpec {
            id: "backend".into(),
            label: "Backend".into(),
            owners: vec!["be-1".into()],
        },
        LaneSpec {
            id: "frontend".into(),
            label: "Frontend".into(),
            owners: vec!["fe-1".into()],
        },
    ])
    .unwrap();

    let blocker = issue_with(
        &mut dit,
        "Endpoint returns 500",
        dit_core::FieldPatch {
            lane: Some("backend".into()),
            ..Default::default()
        },
    );
    let cancelled = issue_with(
        &mut dit,
        "Doomed dependency",
        dit_core::FieldPatch {
            lane: Some("backend".into()),
            status: Some("cancelled".into()),
            ..Default::default()
        },
    );
    let waiter = issue_with(
        &mut dit,
        "Integrate the endpoint",
        dit_core::FieldPatch {
            lane: Some("frontend".into()),
            blocked_by: Some(vec![blocker, cancelled]),
            ..Default::default()
        },
    );

    dit.claim(
        &waiter,
        "fe-1",
        ClaimOptions {
            force: true,
            ..Default::default()
        },
    )
    .unwrap();

    let board = dit.workflow_board().unwrap();
    let frontend = board
        .lanes
        .iter()
        .find(|l| l.id.as_deref() == Some("frontend"))
        .unwrap();
    let card = frontend
        .cards
        .iter()
        .find(|c| c.id == waiter)
        .expect("the waiter sits in its lane");
    match &card.readiness {
        Readiness::Blocked {
            unsatisfied,
            broken,
        } => {
            assert_eq!(unsatisfied, &vec![blocker]);
            assert_eq!(broken, &vec![cancelled]);
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    let dispositions: Vec<_> = card.blockers.iter().map(|b| b.state).collect();
    assert!(dispositions.contains(&dit_core::BlockerDisposition::Unsatisfied));
    assert!(dispositions.contains(&dit_core::BlockerDisposition::Broken));
    let claim = card.claim.as_ref().expect("the claim is on the card");
    assert_eq!(claim.claimed_by, "fe-1");
    assert!(!claim.stale);

    // The backend row carries its own issues; the Unlaned row stays empty here.
    let backend = board
        .lanes
        .iter()
        .find(|l| l.id.as_deref() == Some("backend"))
        .unwrap();
    assert!(backend.cards.iter().any(|c| c.id == blocker));
    let unlaned = board.lanes.iter().find(|l| l.id.is_none()).unwrap();
    assert!(unlaned.cards.is_empty());
}

#[test]
fn the_watcher_signals_external_commits_and_ignores_own_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let id = issue_with(&mut dit, "Watched", Default::default());
    let dit = std::sync::Arc::new(std::sync::Mutex::new(dit));
    let rx = spawn_watcher(std::sync::Arc::clone(&dit));

    // An external commit: the watcher must refresh and signal once.
    let path = {
        let dit = dit.lock().unwrap();
        tmp.path().join(dit.get(id.as_str()).unwrap().unwrap().path)
    };
    let edited = std::fs::read_to_string(&path)
        .unwrap()
        .replace("status: todo", "status: review");
    std::fs::write(&path, edited).unwrap();
    let repo = dit_vcs::Repo::open(tmp.path()).unwrap();
    let rel = path
        .strip_prefix(tmp.path())
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    repo.add(&rel).unwrap();
    repo.commit("external change").unwrap();

    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("an external commit signals exactly once");
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(750))
            .is_err(),
        "one commit, one signal"
    );
    {
        let mut dit = dit.lock().unwrap();
        assert_eq!(
            dit.get(id.as_str()).unwrap().unwrap().issue.status,
            "review",
            "the watcher refreshed the index"
        );

        // An own-process write: the watermark already moved in absorb_commit,
        // so the watcher has nothing to say.
        let mut tx = dit.transaction("farid").unwrap();
        tx.set_fields(
            &id,
            dit_core::FieldPatch {
                status: Some("done".into()),
                ..Default::default()
            },
        )
        .unwrap();
        tx.commit("own write").unwrap();
    }
    // Own writes are announced by the write path immediately; the watcher
    // adds at most one further coalesced frame for the same commit (a
    // refetch hint — it can never loop, frames cause no writes).
    let mut extra = 0;
    while rx
        .recv_timeout(std::time::Duration::from_millis(1200))
        .is_ok()
    {
        extra += 1;
        assert!(extra <= 1, "one commit, at most one watcher frame on top");
    }
}

#[test]
fn workflow_init_seeds_the_evidence_report_template_and_hand_edits_survive() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let lanes = vec![LaneSpec {
        id: "backend".into(),
        label: "Backend".into(),
        owners: vec!["be-1".into()],
    }];
    let first = dit.init_workflow(&lanes).unwrap();
    assert!(first.report_template_written);
    let path = tmp.path().join(".dit/templates/integration-report.md");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("## Expectation"), "{text}");
    assert!(text.contains("## Request"), "{text}");

    // A hand edit survives the second run; the report stays unwritten.
    std::fs::write(&path, format!("{text}<!-- tuned -->\n")).unwrap();
    let second = dit.init_workflow(&lanes).unwrap();
    assert!(!second.report_template_written);
    assert!(std::fs::read_to_string(&path).unwrap().contains("tuned"));
}

#[test]
fn the_inbox_lists_threads_the_lane_has_not_answered() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[
        LaneSpec {
            id: "backend".into(),
            label: "Backend".into(),
            owners: vec!["be-1".into()],
        },
        LaneSpec {
            id: "frontend".into(),
            label: "Frontend".into(),
            owners: vec![],
        },
    ])
    .unwrap();

    let api = issue_with(
        &mut dit,
        "Endpoint returns 500",
        dit_core::FieldPatch {
            lane: Some("backend".into()),
            ..Default::default()
        },
    );
    let ui = issue_with(
        &mut dit,
        "Page renders the panel",
        dit_core::FieldPatch {
            lane: Some("frontend".into()),
            ..Default::default()
        },
    );

    // A frontend question on the backend issue: unanswered for backend.
    let mut tx = dit.transaction("fe-1").unwrap();
    let root = tx
        .comment(&api, "fe-1", None, "expected JSON, got HTML — see report")
        .unwrap();
    tx.commit("question").unwrap();

    let inbox = dit.inbox(Some("backend")).unwrap();
    assert_eq!(inbox.len(), 1, "{inbox:?}");
    assert_eq!(inbox[0].issue.issue.id, api);
    assert_eq!(inbox[0].root.body, "expected JSON, got HTML — see report");
    assert_eq!(inbox[0].last_author, "fe-1");
    assert_eq!(inbox[0].replies, 0);

    // The lane filter: the same thread is not the frontend's inbox (the
    // issue is not theirs), and an own-thread never waits on its own lane.
    assert!(dit.inbox(Some("frontend")).unwrap().is_empty());

    // The owner answers: the thread leaves the inbox.
    let mut tx = dit.transaction("be-1").unwrap();
    tx.comment(
        &api,
        "be-1",
        Some(&root),
        "fixed in the cast hint; probe green",
    )
    .unwrap();
    tx.commit("answer").unwrap();
    assert!(dit.inbox(Some("backend")).unwrap().is_empty());

    // A follow-up question reopens it, and the count of replies reflects the
    // thread's depth.
    let mut tx = dit.transaction("fe-1").unwrap();
    tx.comment(
        &api,
        "fe-1",
        Some(&root),
        "confirmed on the second route — one more?",
    )
    .unwrap();
    tx.commit("follow-up").unwrap();
    let inbox = dit.inbox(Some("backend")).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(
        inbox[0].replies, 2,
        "root + two replies, replies counted past the root"
    );
    assert_eq!(inbox[0].last_author, "fe-1");

    // The owners-empty lane falls back to the issue's assignees as its
    // voice: a stranger's thread waits, an assignee's own note does not.
    let mut tx = dit.transaction("be-1").unwrap();
    tx.set_fields(
        &ui,
        dit_core::FieldPatch {
            assignees: Some(vec!["fe-1".to_owned()]),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("assign").unwrap();
    let mut tx = dit.transaction("be-1").unwrap();
    tx.comment(
        &ui,
        "be-1",
        None,
        "the panel flashes on load — backend eyes?",
    )
    .unwrap();
    tx.commit("question on ui").unwrap();
    assert_eq!(dit.inbox(Some("frontend")).unwrap().len(), 1);
}
