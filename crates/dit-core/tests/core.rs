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
        blocked_by: vec![],
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
        blocked_by: vec![],
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

    // Issue templates are seeded (the description/criteria/UAT shape).
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

    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft_with("Blank", "")).unwrap();
    tx.commit("create").unwrap();

    let path = &dit.get(id.as_str()).unwrap().unwrap().path;
    let body = std::fs::read_to_string(tmp.path().join(path)).unwrap();
    assert!(
        body.contains("## Criteria") && body.contains("## User acceptance test"),
        "the default template's sections are seeded:\n{body}"
    );

    // A named template wins over the kind default.
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx
        .create_issue_from_template(draft_with("Crash", ""), "bug")
        .unwrap();
    tx.commit("create").unwrap();
    let path = &dit.get(id.as_str()).unwrap().unwrap().path;
    let body = std::fs::read_to_string(tmp.path().join(path)).unwrap();
    assert!(body.contains("## Steps to reproduce"), "{body}");

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
