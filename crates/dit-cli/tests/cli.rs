//! End-to-end CLI tests: every one runs the real `dit` binary against a
//! real temporary workspace, the same path a user's shell takes. The
//! alternative — calling library functions — would test the facade again,
//! which `dit-core`'s own tests already do; what only this file can catch
//! is the argument parsing, exit codes and stdout that make a CLI usable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Command, Output};

/// Run the real binary in `cwd`, with DIT_ME stripped so tests never inherit
/// the calling shell's alias.
fn dit(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dit"))
        .args(args)
        .current_dir(cwd)
        .env_remove("DIT_ME")
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn init_new_list_and_show_form_the_basic_loop() {
    let tmp = tempfile::tempdir().unwrap();
    let init = dit(tmp.path(), &["init"]);
    assert!(init.status.success(), "{}", stderr(&init));
    assert!(
        tmp.path().join("README.md").exists(),
        "init writes a README"
    );

    let new = dit(
        tmp.path(),
        &[
            "--me", "farid", "issue", "new", "Login", "timeout", "--kind", "bug", "--status",
            "todo", "-P", "p1", "-l", "auth",
        ],
    );
    assert!(new.status.success(), "{}", stderr(&new));
    let short = stdout(&new).split_whitespace().next().unwrap().to_owned();
    assert_eq!(short.len(), 7, "the created issue's short ref is printed");

    let list = dit(tmp.path(), &["list", "status", "=", "todo"]);
    assert!(stdout(&list).contains("Login timeout"));
    assert!(stdout(&list).contains(&short));

    let show = dit(tmp.path(), &["issue", "show", &short]);
    assert!(stdout(&show).contains("status: todo"));
    assert!(stdout(&show).contains("priority: p1"));

    // Every write was committed: the tree the user sees is clean.
    assert!(stdout(&dit(tmp.path(), &["status"])).contains("(clean)"));
}

#[test]
fn set_comment_and_history_are_all_visible_afterwards() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    let new = dit(tmp.path(), &["--me", "farid", "issue", "new", "Crash"]);
    let short = stdout(&new).split_whitespace().next().unwrap().to_owned();

    let set = dit(
        tmp.path(),
        &[
            "--me",
            "budi",
            "issue",
            "set",
            &short,
            "status=in_progress",
            "labels=auth,login",
        ],
    );
    assert!(set.status.success(), "{}", stderr(&set));
    assert!(dit(
        tmp.path(),
        &[
            "--me",
            "budi",
            "issue",
            "comment",
            &short,
            "Reproduced on 3G."
        ]
    )
    .status
    .success());

    let show = stdout(&dit(tmp.path(), &["issue", "show", &short]));
    assert!(show.contains("status: in_progress"), "{show}");
    assert!(show.contains("auth, login"), "{show}");
    assert!(show.contains("Reproduced on 3G."), "{show}");
    assert!(
        show.contains("status: todo -> in_progress  (budi)"),
        "history names the actor: {show}"
    );
}

#[test]
fn doctor_passes_on_a_fresh_init() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    let doctor = dit(tmp.path(), &["doctor"]);
    assert!(
        doctor.status.success(),
        "{}{}",
        stdout(&doctor),
        stderr(&doctor)
    );
    assert!(
        stdout(&doctor).contains("[ ok  ] merge-driver"),
        "{}",
        stdout(&doctor)
    );
    assert!(
        stdout(&doctor).contains("[ ok  ] cache-ignored"),
        "{}",
        stdout(&doctor)
    );
}

#[test]
fn an_unknown_reference_is_an_error_not_a_crash() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    let show = dit(tmp.path(), &["issue", "show", "nope"]);
    assert_eq!(show.status.code(), Some(2));
    assert!(stderr(&show).contains("no issue matches"));
}

#[test]
fn a_bad_field_name_names_the_offender() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    let set = dit(tmp.path(), &["issue", "set", "01K3MA1", "colr=red"]);
    assert_eq!(set.status.code(), Some(2));
    assert!(
        stderr(&set).contains("unknown field `colr`"),
        "{}",
        stderr(&set)
    );
}

#[test]
fn the_merge_driver_leaves_conflict_markers_not_a_silent_winner() {
    let tmp = tempfile::tempdir().unwrap();
    // Both sides rewrote the same body line differently — the one conflict
    // no policy can pick a winner for, so the driver must fall back to
    // markers. (A same-field frontmatter clash alone is auto-resolved by
    // commit order, which is the point of the driver.)
    let base = tmp.path().join("base.md");
    let ours = tmp.path().join("ours.md");
    let theirs = tmp.path().join("theirs.md");
    std::fs::write(&base, "---\nstatus: todo\n---\nfirst\nsecond\nthird\n").unwrap();
    std::fs::write(&ours, "---\nstatus: todo\n---\nfirst\nours-line\nthird\n").unwrap();
    std::fs::write(
        &theirs,
        "---\nstatus: todo\n---\nfirst\ntheirs-line\nthird\n",
    )
    .unwrap();

    let run = dit(
        tmp.path(),
        &[
            "merge-driver",
            base.to_str().unwrap(),
            ours.to_str().unwrap(),
            theirs.to_str().unwrap(),
            "7",
            ".dit/issues/2026/08/x/issue.md",
        ],
    );
    assert_eq!(run.status.code(), Some(1), "a conflict exits 1");
    let merged = std::fs::read_to_string(&ours).unwrap();
    assert!(merged.contains("<<<<<<<"), "markers are written: {merged}");
    assert!(merged.contains("ours-line"), "ours is inside the markers");
    assert!(
        merged.contains("theirs-line"),
        "theirs is inside the markers"
    );
}

#[test]
fn reindex_rebuilds_after_the_cache_is_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    let new = dit(tmp.path(), &["--me", "farid", "issue", "new", "Gone"]);
    let short = stdout(&new).split_whitespace().next().unwrap().to_owned();

    std::fs::remove_file(tmp.path().join(".dit-cache/index.sqlite")).unwrap();
    let empty = dit(tmp.path(), &["issue", "show", &short]);
    assert_eq!(
        empty.status.code(),
        Some(2),
        "a missing index answers nothing"
    );

    let reindex = dit(tmp.path(), &["reindex"]);
    assert!(reindex.status.success(), "{}", stderr(&reindex));
    assert!(
        stdout(&reindex).contains("1 issues"),
        "{}",
        stdout(&reindex)
    );
    assert!(dit(tmp.path(), &["issue", "show", &short]).status.success());
}
