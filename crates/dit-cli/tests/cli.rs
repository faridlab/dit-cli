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

/// The short ref out of `issue new` output, which prints `#N <short> <title>`
/// once the workspace numbers issues (ADR 0007) and `<short> <title>` when
/// it does not.
fn short_of(o: &Output) -> String {
    let text = stdout(o);
    let mut words = text.split_whitespace();
    let first = words.next().unwrap().to_owned();
    if first.starts_with('#') {
        words.next().unwrap().to_owned()
    } else {
        first
    }
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
    // Init says where the files went — the layout must never be a surprise.
    assert!(stdout(&init).contains("layout: root"), "{}", stdout(&init));

    let new = dit(
        tmp.path(),
        &[
            "--me", "farid", "issue", "new", "Login", "timeout", "--kind", "bug", "--status",
            "todo", "-P", "p1", "-l", "auth",
        ],
    );
    assert!(new.status.success(), "{}", stderr(&new));
    assert!(
        stdout(&new).starts_with("#1 "),
        "the first issue is #1: {}",
        stdout(&new)
    );
    let short = short_of(&new);
    assert_eq!(short.len(), 7, "the created issue's short ref is printed");

    let list = dit(tmp.path(), &["list", "status", "=", "todo"]);
    assert!(stdout(&list).contains("Login timeout"));
    assert!(stdout(&list).contains("#1"), "{}", stdout(&list));

    // #N is a first-class reference, not just display sugar.
    let by_number = dit(tmp.path(), &["issue", "show", "#1"]);
    assert!(by_number.status.success(), "{}", stderr(&by_number));
    assert!(stdout(&by_number).contains("status: todo"));
    assert!(stdout(&by_number).contains("priority: p1"));
    assert!(
        stdout(&by_number).contains(&format!("ref: {short}")),
        "show names the permanent ref behind the number: {}",
        stdout(&by_number)
    );

    // Every write was committed: the tree the user sees is clean.
    assert!(stdout(&dit(tmp.path(), &["status"])).contains("(clean)"));
}

#[test]
fn set_comment_and_history_are_all_visible_afterwards() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    let new = dit(tmp.path(), &["--me", "farid", "issue", "new", "Crash"]);
    let short = short_of(&new);

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
    let short = short_of(&new);

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

#[test]
fn ui_outside_a_workspace_fails_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = dit(tmp.path(), &["ui"]);
    assert!(!out.status.success(), "there is nothing to serve here");
}

/// `dit ui` must actually serve: bind, answer HTTP on the loopback, and
/// reject an unauthenticated request — the same gate the browser hits.
/// The opener is skipped automatically because the test's stdout is a pipe.
#[test]
fn ui_serves_the_workspace_over_http() {
    use std::io::{Read, Write};

    let tmp = tempfile::tempdir().unwrap();
    assert!(dit(tmp.path(), &["init"]).status.success());

    // Reserve a free port, then hand it to the server.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_dit"))
        .args(["ui", "--port", &port.to_string()])
        .current_dir(tmp.path())
        .env_remove("DIT_ME")
        .spawn()
        .unwrap();

    let request = "GET /api/status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let mut answered = false;
    for _ in 0..100 {
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            if stream.write_all(request.as_bytes()).is_ok() {
                let mut buf = String::new();
                if stream.read_to_string(&mut buf).is_ok() && buf.starts_with("HTTP/1.1 401") {
                    answered = true;
                    break;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(answered, "`dit ui` never answered on port {port}");
}

#[test]
fn init_can_tuck_everything_under_dot_for_guest_repos() {
    let tmp = tempfile::tempdir().unwrap();
    let init = dit(tmp.path(), &["init", "--layout", "dotdir"]);
    assert!(init.status.success(), "{}", stderr(&init));
    assert!(
        stdout(&init).contains("layout: dotdir"),
        "{}",
        stdout(&init)
    );
    assert!(
        tmp.path().join(".dit/issues").is_dir(),
        "content lives under .dit/"
    );
    assert!(
        !tmp.path().join("issues").exists(),
        "no issues/ colonizes the guest tree root"
    );

    let new = dit(tmp.path(), &["issue", "new", "Hidden"]);
    assert!(new.status.success(), "{}", stderr(&new));
    let show = dit(tmp.path(), &["issue", "show", "#1"]);
    assert!(show.status.success(), "{}", stderr(&show));
}

#[test]
fn a_template_seeds_the_body_and_the_names_are_listable() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);

    let list = dit(tmp.path(), &["templates", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let names = stdout(&list);
    for expected in ["default", "bug", "story", "spike"] {
        assert!(names.contains(expected), "templates list: {names}");
    }

    let new = dit(
        tmp.path(),
        &["issue", "new", "Crash", "on", "save", "--template", "bug"],
    );
    assert!(new.status.success(), "{}", stderr(&new));
    let show = dit(tmp.path(), &["issue", "show", "#1"]);
    assert!(
        stdout(&show).contains("Steps to reproduce"),
        "the bug template's sections seed the body: {}",
        stdout(&show)
    );

    let missing = dit(tmp.path(), &["issue", "new", "X", "--template", "nope"]);
    assert_eq!(
        missing.status.code(),
        Some(2),
        "a missing template is a you-asked-for-something-absent exit"
    );
}

#[test]
fn docs_build_writes_the_index_readme_exactly_like_the_adr_says() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    dit(tmp.path(), &["--me", "farid", "issue", "new", "First"]);
    dit(tmp.path(), &["--me", "farid", "issue", "new", "Second"]);

    let build = dit(tmp.path(), &["docs", "build", "--index"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let readme = std::fs::read_to_string(tmp.path().join("issues/README.md")).unwrap();
    assert!(
        readme.starts_with("<!-- generated by dit"),
        "the marker leads: {readme}"
    );
    assert!(readme.contains("#1"), "{readme}");
    assert!(readme.contains("[First]("), "{readme}");

    // A second build is a no-op, not a churn commit.
    let again = dit(tmp.path(), &["docs", "build", "--index"]);
    assert!(
        stdout(&again).contains("already current"),
        "{}",
        stdout(&again)
    );
    assert!(stdout(&dit(tmp.path(), &["status"])).contains("(clean)"));

    let no_flag = dit(tmp.path(), &["docs", "build"]);
    assert_eq!(no_flag.status.code(), Some(2));
}

#[test]
fn migrate_layout_moves_a_dotdir_workspace_and_keeps_everything_readable() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init", "--layout", "dotdir"]);
    let new = dit(tmp.path(), &["--me", "farid", "issue", "new", "Carried"]);
    let short = short_of(&new);
    assert!(dit(
        tmp.path(),
        &["--me", "budi", "issue", "comment", "#1", "Still here."],
    )
    .status
    .success());

    let migrate = dit(tmp.path(), &["migrate-layout", "root"]);
    assert!(migrate.status.success(), "{}", stderr(&migrate));
    assert!(
        stdout(&migrate).contains("dotdir -> root"),
        "{}",
        stdout(&migrate)
    );
    assert!(
        tmp.path().join("issues").is_dir(),
        "content moved to the tree root"
    );

    // The issue, its comment and its history survive the move.
    let show = dit(tmp.path(), &["issue", "show", &short]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("Still here."), "{}", stdout(&show));
    assert!(
        stdout(&show).contains("ref:"),
        "the numbered handle still resolves to the same issue"
    );
    assert!(stdout(&dit(tmp.path(), &["status"])).contains("(clean)"));

    // And the workspace keeps working in its new shape: the next issue is
    // created under the visible root, and the old home is gone.
    assert!(dit(tmp.path(), &["issue", "new", "After"]).status.success());
    let after = dit(tmp.path(), &["issue", "show", "#2"]);
    assert!(after.status.success(), "{}", stderr(&after));
    assert!(
        !tmp.path().join(".dit/issues").exists(),
        "the dotdir content root is gone, not left behind as a fork"
    );
}

#[test]
fn renumber_backfills_legacy_issues_without_moving_existing_numbers() {
    let tmp = tempfile::tempdir().unwrap();
    dit(tmp.path(), &["init"]);
    assert!(dit(tmp.path(), &["--me", "farid", "issue", "new", "Kept"])
        .status
        .success());

    // Two "legacy" issues: the workspace spent time on `on-merge`, where
    // issues wait for the bot. Flip the policy by editing config, the way a
    // user without the settings panel would.
    let config = tmp.path().join(".dit/config.yaml");
    let git = |msg: &str| {
        let _ = Command::new("git")
            .args(["add", ".dit"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(tmp.path())
            .output()
            .unwrap()
    };
    std::fs::write(
        &config,
        "schema_version: 1\nlayout: root\nnumbering: on-merge\n",
    )
    .unwrap();
    assert!(git("switch to on-merge").status.success());
    assert!(dit(tmp.path(), &["issue", "new", "Legacy older"])
        .status
        .success());
    assert!(dit(tmp.path(), &["issue", "new", "Legacy newer"])
        .status
        .success());
    std::fs::write(
        &config,
        "schema_version: 1\nlayout: root\nnumbering: local\n",
    )
    .unwrap();
    assert!(git("switch back to local").status.success());

    // Doctor points at the way out first.
    assert!(
        stdout(&dit(tmp.path(), &["doctor"])).contains("dit renumber"),
        "the warn names the command"
    );

    let r = dit(tmp.path(), &["renumber"]);
    assert!(r.status.success(), "{}", stderr(&r));
    assert!(
        stdout(&r).contains("assigned 2 number(s)"),
        "{}",
        stdout(&r)
    );

    // #1 keeps pointing where it always did; the legacy pair lands after it
    // in creation order.
    assert!(stdout(&dit(tmp.path(), &["issue", "show", "#1"])).contains("Kept"));
    assert!(stdout(&dit(tmp.path(), &["issue", "show", "#2"])).contains("Legacy older"),);
    assert!(stdout(&dit(tmp.path(), &["issue", "show", "#3"])).contains("Legacy newer"),);

    // Idempotent, and the whole backfill is one clean commit.
    assert!(
        stdout(&dit(tmp.path(), &["renumber"])).contains("nothing to do"),
        "a second run has nothing to do"
    );
    assert!(stdout(&dit(tmp.path(), &["status"])).contains("(clean)"));
}
