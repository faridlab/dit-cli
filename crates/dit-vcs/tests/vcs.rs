//! Integration tests against real git repositories in tempdirs.
//!
//! Git's merge/rebase behavior is exactly the kind of thing that must be
//! run, not reasoned about: which side lands in %A during a rebase, whether
//! a stopped driver leaves a normal-looking file — every one of these was
//! verified by hand in a scratch repo before being written down here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use dit_vcs::git::{RebaseOutcome, Repo, VcsError};
use dit_vcs::sync::{self, SyncOptions};

/// A repo-local identity so commits never depend on the machine's global
/// git config (which may sign, rename, or refuse).
fn hermetic_repo(path: &Path) -> Repo {
    let repo = Repo::init(path).unwrap();
    repo.set_identity("DIT Test", "dit@test.local").unwrap();
    repo
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Install a merge driver implemented as a small shell script — the same
/// contract the real binary fulfills, observable from outside the process.
fn install_script_driver(repo: &Repo, dir: &Path, script: &str) {
    let script_path = dir.join("driver.sh");
    fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let _ = &script_path; // the driver contract is only tested on unix
    }
    repo.configure_merge_driver(&format!("{} %O %A %B %L %P", script_path.display()))
        .unwrap();
}

/// A one-file workspace layout that matches the real one: markdown under
/// `.dit/`, with the gitattributes that route it through the driver.
fn dit_file(root: &Path, name: &str, contents: &str) {
    write(&root.join(".dit/issues").join(name), contents);
}

#[test]
fn init_commit_head_and_cleanliness() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    dit_file(tmp.path(), "issue.md", "one\n");
    repo.add(".dit").unwrap();
    let sha = repo.commit("first").unwrap().unwrap_or_default();
    assert_eq!(sha.len(), 40, "a full commit sha: {sha}");
    assert!(repo.is_clean().unwrap());
    assert_eq!(repo.current_branch().unwrap(), "main");

    // Committing again with nothing staged is success-with-nothing.
    assert!(repo.commit("empty").unwrap().is_none());
}

#[test]
fn open_resolves_to_the_worktree_root_from_a_subdirectory() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    dit_file(tmp.path(), "a.md", "x\n");
    repo.add(".").unwrap();
    repo.commit("c").unwrap();
    let deep = Repo::open(&tmp.path().join(".dit/issues")).unwrap();
    assert_eq!(deep.root(), repo.root());
}

/// Helper: an upstream work repo, a bare remote, and a clone. The base
/// commit includes the gitattributes routing markdown through the merge
/// driver, so every clone inherits it.
fn workspace_with_remote(root: &Path) -> (Repo, std::path::PathBuf, Repo, std::path::PathBuf) {
    let up_dir = root.join("up");
    let upstream = hermetic_repo(&up_dir);
    write(&up_dir.join(".dit/.gitattributes"), "*.md merge=dit-md\n");
    dit_file(&up_dir, "issue.md", "base\n");
    upstream.add(".").unwrap();
    upstream.commit("base").unwrap();

    let bare = root.join("remote.git");
    Repo::clone_to_bare(&up_dir, &bare).unwrap();
    upstream.add_remote("origin", &bare).unwrap();

    let local_dir = root.join("local");
    let local = Repo::clone(&bare, &local_dir).unwrap();
    (upstream, bare, local, local_dir)
}

#[test]
fn a_remote_can_move_and_the_clone_can_see_it_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let (upstream, _bare, local, local_dir) = workspace_with_remote(tmp.path());

    dit_file(&local_dir, "issue.md", "local edit\n");
    local.add(".").unwrap();
    local.commit("local").unwrap();
    local.push("origin", "main").unwrap();
    assert_eq!(local.behind_count("origin", "main").unwrap(), 0);

    // The upstream moves. It edits a different file, so its own catch-up
    // rebase needs no merge machinery — pushing straight would be rejected,
    // because the remote already carries local's commit.
    dit_file(upstream.root(), "other.md", "upstream edit\n");
    upstream.add(".").unwrap();
    upstream.commit("upstream").unwrap();
    upstream.fetch("origin", "main").unwrap();
    upstream.git(&["rebase", "origin/main"]).unwrap();
    upstream.push("origin", "main").unwrap();

    // Local sees itself behind only after fetching.
    local.fetch("origin", "main").unwrap();
    assert_eq!(local.behind_count("origin", "main").unwrap(), 1);
}

#[test]
fn sync_rebases_local_commits_onto_a_moved_remote_and_pushes() {
    let tmp = tempfile::tempdir().unwrap();
    let (upstream, _bare, local, local_dir) = workspace_with_remote(tmp.path());

    // The remote gains one commit first; local then has its own on top.
    dit_file(upstream.root(), "issue.md", "upstream\n");
    upstream.add(".").unwrap();
    upstream.commit("remote edit").unwrap();
    upstream.push("origin", "main").unwrap();

    // A different file, so the rebase is clean — but the driver must still
    // be configured, because sync never rebases on trust alone.
    install_script_driver(
        &local,
        tmp.path(),
        "#!/bin/sh\nprintf 'resolved-by-driver\\n' > \"$2\"\nexit 0\n",
    );
    dit_file(&local_dir, "other.md", "local\n");
    local.add(".").unwrap();
    local.commit("local edit").unwrap();

    let mut opts = SyncOptions {
        retry_delay: std::time::Duration::ZERO,
        ..SyncOptions::default()
    };
    let report = sync::sync(&local, &mut opts).unwrap();
    assert_eq!(report.pulled, 1, "{report:?}");
    assert_eq!(report.pushed, 1, "{report:?}");
    assert!(report.needs_human.is_empty());
    // The local commit now sits on top of the remote one.
    let count = local.rev_list_count("HEAD").unwrap();
    assert!(count >= 3, "base + remote + local, got {count}");
}

#[test]
fn sync_refuses_to_rebase_without_a_configured_driver() {
    let tmp = tempfile::tempdir().unwrap();
    let (upstream, _bare, local, local_dir) = workspace_with_remote(tmp.path());

    // The remote moves ahead with a DIT file; local also commits, so a
    // rebase is unavoidable — but no merge driver is installed.
    dit_file(upstream.root(), "issue.md", "upstream\n");
    upstream.add(".").unwrap();
    upstream.commit("remote edit").unwrap();
    upstream.push("origin", "main").unwrap();

    dit_file(&local_dir, "issue.md", "local\n");
    local.add(".").unwrap();
    local.commit("local edit").unwrap();

    let mut opts = SyncOptions {
        retry_delay: std::time::Duration::ZERO,
        ..SyncOptions::default()
    };
    let err = sync::sync(&local, &mut opts).unwrap_err();
    assert!(matches!(err, VcsError::MergeDriverNotConfigured), "{err}");
}

#[test]
fn sync_reports_conflicts_as_state_never_as_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (upstream, _bare, local, local_dir) = workspace_with_remote(tmp.path());

    // A driver that behaves like the fail-safe: writes markers, exits 1.
    // (The real driver's behavior is covered by its own unit tests; here we
    // prove the sync loop handles a stopped rebase.)
    let driver = tmp.path().join("fail-driver.sh");
    fs::write(
        &driver,
        "#!/bin/sh\nprintf '<<<<<<< ours\\n%s=======\\n%s>>>>>>> theirs\\n' \"$(cat \"$2\")\" \"$(cat \"$3\")\" > \"$2\"\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o755)).unwrap();
    }
    local
        .configure_merge_driver(&format!("{} %O %A %B %L %P", driver.display()))
        .unwrap();

    dit_file(upstream.root(), "issue.md", "upstream version\n");
    upstream.add(".").unwrap();
    upstream.commit("remote edit").unwrap();
    upstream.push("origin", "main").unwrap();

    dit_file(&local_dir, "issue.md", "local version\n");
    local.add(".").unwrap();
    local.commit("local edit").unwrap();

    let mut opts = SyncOptions {
        retry_delay: std::time::Duration::ZERO,
        ..SyncOptions::default()
    };
    let report = sync::sync(&local, &mut opts).unwrap();
    assert!(!report.needs_human.is_empty(), "{report:?}");
    assert!(
        local.rebase_in_progress(),
        "the rebase must stay stopped for a human"
    );
    let conflicted = fs::read_to_string(local_dir.join(".dit/issues/issue.md")).unwrap();
    assert!(
        conflicted.contains("<<<<<<<"),
        "markers must be on disk: {conflicted}"
    );
}

#[test]
fn sync_survives_a_push_rejected_mid_flight() {
    let tmp = tempfile::tempdir().unwrap();
    let (upstream, bare, local, local_dir) = workspace_with_remote(tmp.path());

    install_script_driver(
        &local,
        tmp.path(),
        "#!/bin/sh\n# resolve everything by preferring theirs\nexit 0\n",
    );

    dit_file(&local_dir, "issue.md", "local\n");
    local.add(".").unwrap();
    local.commit("local edit").unwrap();

    // Between the local fetch and the local push, the remote moves: the
    // hook makes an upstream commit land right in that window.
    let up_dir = upstream.root().to_path_buf();
    let up2 = upstream.clone();
    let mut opts = SyncOptions {
        retry_delay: std::time::Duration::ZERO,
        after_fetch: Some(Box::new(move || {
            dit_file(&up_dir, "other.md", "raced in\n");
            up2.add(".").unwrap();
            up2.commit("raced").unwrap();
            up2.push("origin", "main").unwrap();
        })),
        ..SyncOptions::default()
    };

    let report = sync::sync(&local, &mut opts).unwrap();
    assert_eq!(report.pushed, 1, "{report:?}");
    // The racing commit is in the local history after the re-rebase.
    let count = local.rev_list_count("HEAD").unwrap();
    assert!(count >= 3, "base + raced + local = at least 3, got {count}");
    // And after the successful push the local view of the remote is even.
    local.fetch("origin", "main").unwrap();
    assert_eq!(local.behind_count("origin", "main").unwrap(), 0);
    let _ = &bare;
}

#[test]
fn rebase_invokes_the_configured_merge_driver() {
    let tmp = tempfile::tempdir().unwrap();
    // The repo lives in a subdirectory so the driver script and its log sit
    // OUTSIDE the worktree: a driver tracked by the repo would be deleted by
    // the very checkouts this test performs.
    let dir = tmp.path().join("repo");
    let repo = hermetic_repo(&dir);
    write(&dir.join(".dit/.gitattributes"), "*.md merge=dit-md\n");
    dit_file(&dir, "issue.md", "base\n");
    repo.add(".").unwrap();
    repo.commit("base").unwrap();

    // A driver that records its invocation and "resolves" deterministically.
    let log = tmp.path().join("driver.log");
    let log_str = log.display().to_string();
    let driver = tmp.path().join("record-driver.sh");
    fs::write(
        &driver,
        format!(
            "#!/bin/sh\necho called >> {log_str}\nprintf 'resolved-by-driver\\n' > \"$2\"\nexit 0\n"
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&driver, fs::Permissions::from_mode(0o755)).unwrap();
    }
    repo.configure_merge_driver(&format!("{} %O %A %B %L %P", driver.display()))
        .unwrap();

    // Branch side edits the file...
    repo.git(&["checkout", "-b", "side"]).unwrap();
    dit_file(&dir, "issue.md", "side\n");
    repo.add(".").unwrap();
    repo.commit("side").unwrap();

    // ...and so does main, differently.
    repo.git(&["checkout", "main"]).unwrap();
    dit_file(&dir, "issue.md", "main\n");
    repo.add(".").unwrap();
    repo.commit("main").unwrap();

    repo.git(&["checkout", "side"]).unwrap();
    let outcome = repo.rebase("main").unwrap();
    assert_eq!(outcome, RebaseOutcome::Clean);
    assert!(fs::read_to_string(&log).unwrap().contains("called"));
    let merged = fs::read_to_string(dir.join(".dit/issues/issue.md")).unwrap();
    assert_eq!(merged, "resolved-by-driver\n");
}

#[test]
fn merge_driver_config_requires_an_absolute_path() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    let err = repo
        .configure_merge_driver("dit merge-driver %O %A %B")
        .unwrap_err();
    assert!(err.to_string().contains("absolute"), "{err}");
    assert!(!repo.merge_driver_ready());
}

#[test]
fn commit_trailers_can_be_walked_from_the_log() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    dit_file(tmp.path(), "issue.md", "x\n");
    repo.add(".").unwrap();
    repo.commit("fix: a thing\n\nCloses: #Q2R7VN8").unwrap();
    let lines = repo
        .log_lines("HEAD", "%H %(trailers:key=Closes,valueonly)")
        .unwrap();
    assert!(lines.iter().any(|l| l.contains("Q2R7VN8")), "{lines:?}");
}

#[test]
fn dirty_worktree_refuses_to_rebase() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    dit_file(tmp.path(), "issue.md", "base\n");
    repo.add(".").unwrap();
    repo.commit("base").unwrap();
    dit_file(tmp.path(), "issue.md", "uncommitted\n");
    let err = repo.rebase("main").unwrap_err();
    assert!(matches!(err, VcsError::DirtyWorktree(_)), "{err}");
}

#[test]
fn commit_with_nothing_staged_but_dirty_tree_is_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = hermetic_repo(tmp.path());
    dit_file(tmp.path(), "issue.md", "base\n");
    repo.add(".").unwrap();
    repo.commit("base").unwrap();
    // Nothing staged, but the tree is dirty: git refuses with a different
    // phrase than the clean-tree case, and that is still "nothing to commit".
    dit_file(tmp.path(), "issue.md", "unstaged edit\n");
    let out = repo.commit("nothing staged").unwrap();
    assert_eq!(out, None, "a dirty-but-unstaged tree must report Ok(None)");
    let before = repo.head().unwrap();
    assert_eq!(repo.head().unwrap(), before);
}
