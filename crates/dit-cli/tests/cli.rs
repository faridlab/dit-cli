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
    command(cwd).args(args).output().unwrap()
}

/// The binary in `cwd`, isolated from the person running the tests: no
/// alias, no workspace named by the shell, and a config directory of its
/// own — `dit init` registers what it creates, and that list must never be
/// the real `~/.config/dit`. One config per `cwd`, so the commands of one
/// test share a list and different tests do not.
fn command(cwd: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dit"));
    cmd.current_dir(cwd)
        .env_remove("DIT_ME")
        .env_remove("DIT_WORKSPACE")
        .env("XDG_CONFIG_HOME", config_home(cwd));
    cmd
}

fn config_home(cwd: &Path) -> std::path::PathBuf {
    static BASE: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let base = BASE.get_or_init(|| tempfile::tempdir().unwrap());
    let key: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    base.path().join(key)
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
    // Opening self-heals a missing cache before the first read (the upgrade
    // path), so the show already answers; the explicit reindex below stays
    // covered for its own sake.
    assert!(dit(tmp.path(), &["issue", "show", &short]).status.success());

    let reindex = dit(tmp.path(), &["reindex"]);
    assert!(reindex.status.success(), "{}", stderr(&reindex));
    assert!(
        stdout(&reindex).contains("1 issues"),
        "{}",
        stdout(&reindex)
    );
    assert!(dit(tmp.path(), &["issue", "show", &short]).status.success());
}

/// Start `dit ui` in `cwd` on a free port and wait until it answers an
/// unauthenticated request with 401 — the same gate the browser hits. The
/// opener is skipped automatically because the test's stdout is a pipe.
fn start_ui(cwd: &Path) -> (std::process::Child, u16) {
    start_ui_with(cwd, &[])
}

fn start_ui_with(cwd: &Path, extra: &[&str]) -> (std::process::Child, u16) {
    // Reserve a free port, then hand it to the server.
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let mut child = command(cwd)
        .args(["ui", "--port", &port.to_string()])
        .args(extra)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if http_get(port, "/api/workspaces", None).starts_with("HTTP/1.1 401") {
            return (child, port);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("`dit ui` never answered on port {port}");
}

fn http_get(port: u16, path: &str, token: Option<&str>) -> String {
    use std::io::{Read, Write};
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n");
    let mut buf = String::new();
    if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
        if stream.write_all(request.as_bytes()).is_ok() {
            let _ = stream.read_to_string(&mut buf);
        }
    }
    buf
}

/// Outside any workspace, `dit ui` still serves (ADR 0028): the list is
/// empty and the page offers to create the first workspace.
#[test]
fn ui_outside_a_workspace_serves_an_empty_list() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut child, port) = start_ui(tmp.path());
    let token = std::fs::read_to_string(config_home(tmp.path()).join("dit/server-token")).unwrap();
    let list = http_get(port, "/api/workspaces", Some(token.trim()));
    let _ = child.kill();
    let _ = child.wait();
    assert!(list.starts_with("HTTP/1.1 200"), "{list}");
    assert!(list.contains("\"workspaces\":[]"), "{list}");
}

/// `dit ui --all` serves every workspace wherever it runs (ADR 0029): the
/// menu bar app starts it from a home folder that may well be a git
/// repository, which plain `dit ui` would open as that repository's code map.
#[test]
fn ui_all_serves_every_workspace_even_inside_a_plain_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("dotfiles");
    std::fs::create_dir_all(&code).unwrap();
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&code)
        .status()
        .unwrap()
        .success());
    let at = tmp.path().join("DIT");
    let made = dit(
        &code,
        &["workspace", "new", "acme", "--at", at.to_str().unwrap()],
    );
    assert!(made.status.success(), "{}", stderr(&made));
    let token_file = config_home(&code).join("dit/server-token");

    // Plain `dit ui` here is the repository's code map: no workspace list.
    let (mut child, port) = start_ui_code_map(&code);
    let token = std::fs::read_to_string(code.join(".dit/code/server-token")).unwrap_or_default();
    let list = http_get(port, "/api/workspaces", Some(token.trim()));
    let _ = child.kill();
    let _ = child.wait();
    assert!(list.starts_with("HTTP/1.1 404"), "{list}");

    let (mut child, port) = start_ui_with(&code, &["--all"]);
    let token = std::fs::read_to_string(&token_file).unwrap();
    let list = http_get(port, "/api/workspaces", Some(token.trim()));
    let status = http_get(port, "/w/acme/api/status", Some(token.trim()));
    let _ = child.kill();
    let _ = child.wait();
    assert!(list.contains("\"name\":\"acme\""), "{list}");
    assert!(status.starts_with("HTTP/1.1 200"), "{status}");
}

/// The menu bar app runs `dit ui --all --stop-with-parent`: if the app is
/// killed, the server must not live on, orphaned, holding the port.
#[test]
#[cfg(unix)]
fn a_server_started_with_stop_with_parent_stops_when_its_parent_dies() {
    let tmp = tempfile::tempdir().unwrap();
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    // A parent that starts the server and lives until the test closes its
    // stdin — then dies, at a moment the test chooses rather than a guess.
    let mut parent = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "\"$0\" ui --all --stop-with-parent --port {port} >/dev/null 2>&1 & read _"
        ))
        .arg(env!("CARGO_BIN_EXE_dit"))
        .current_dir(tmp.path())
        .env("XDG_CONFIG_HOME", config_home(tmp.path()))
        .env_remove("DIT_WORKSPACE")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut up = false;
    for _ in 0..200 {
        if http_get(port, "/api/workspaces", None).starts_with("HTTP/1.1 401") {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(up, "the server started");
    drop(parent.stdin.take());
    parent.wait().unwrap();
    // Within a few seconds of the parent's death, the port is free.
    let mut gone = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            gone = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(gone, "the server outlived its parent on port {port}");
}

/// Start plain `dit ui` in a repository that is not a workspace, and wait
/// for its code map to answer.
fn start_ui_code_map(cwd: &Path) -> (std::process::Child, u16) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let mut child = command(cwd)
        .args(["ui", "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if http_get(port, "/api/status", None).starts_with("HTTP/1.1 401") {
            return (child, port);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("`dit ui` never answered on port {port}");
}

/// `dit ui` inside a workspace registers it and serves it at `/w/<name>/`,
/// behind the token.
#[test]
fn ui_serves_the_workspace_over_http() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(dit(tmp.path(), &["init"]).status.success());
    let (mut child, port) = start_ui(tmp.path());
    let token = std::fs::read_to_string(config_home(tmp.path()).join("dit/server-token")).unwrap();
    let current = stdout(&dit(tmp.path(), &["workspace", "current"]));
    let name = current
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned();
    let unauthenticated = http_get(port, &format!("/w/{name}/api/status"), None);
    let status = http_get(port, &format!("/w/{name}/api/status"), Some(token.trim()));
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        unauthenticated.starts_with("HTTP/1.1 401"),
        "{unauthenticated}"
    );
    assert!(status.starts_with("HTTP/1.1 200"), "{name}: {status}");
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

/// `dit ai init` outside a workspace is refused, names the command that makes
/// one, and leaves nothing behind — not a guide, not a `.dit-cache/`.
#[test]
fn ai_init_outside_a_workspace_is_refused_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(tmp.path())
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    std::fs::write(tmp.path().join("CLAUDE.md"), "# rules\n").unwrap();

    let out = dit(tmp.path(), &["ai", "init"]);
    assert!(!out.status.success());
    let said = format!("{}{}", stdout(&out), stderr(&out));
    assert!(said.contains("dit init"), "{said}");
    assert!(said.contains("--ai"), "{said}");
    assert!(!tmp.path().join("docs/dit-for-agents.md").exists());
    assert!(
        !tmp.path().join(".dit-cache").exists(),
        "no cache left behind"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap(),
        "# rules\n"
    );
}

/// `dit init --ai` makes the workspace and installs the agent guide in one
/// step: the workspace first, so the guide describes something that exists.
#[test]
fn init_with_ai_creates_the_workspace_and_installs_the_guide() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("CLAUDE.md"), "# rules\n").unwrap();
    let out = dit(tmp.path(), &["init", "--ai"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(tmp.path().join(".dit/config.yaml").exists());
    assert!(tmp.path().join("docs/dit-for-agents.md").exists());
    let claude = std::fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
    assert!(claude.starts_with("# rules\n"), "{claude}");
    assert!(
        claude.contains("Never edit an issue file by hand"),
        "{claude}"
    );
}

#[test]
fn ai_spec_prints_a_topic_and_names_the_topics_it_does_not_know() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(dit(tmp.path(), &["init"]).status.success());
    let morse = dit(tmp.path(), &["ai", "spec", "morse"]);
    assert!(morse.status.success(), "{}", stderr(&morse));
    assert!(stdout(&morse).contains("dit-morse"), "{}", stdout(&morse));

    let bad = dit(tmp.path(), &["ai", "spec", "nonsense"]);
    assert!(!bad.status.success());
    let said = format!("{}{}", stdout(&bad), stderr(&bad));
    assert!(
        said.contains("issues") && said.contains("flow") && said.contains("morse"),
        "{said}"
    );
}

/// Document templates from the terminal (ADR 0031): list the kinds, make a
/// page placed by its stage, and never overwrite one.
#[test]
fn docs_new_makes_a_page_from_a_template_at_its_stage_folder() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(dit(tmp.path(), &["init"]).status.success());
    let list = dit(tmp.path(), &["docs", "templates"]);
    assert!(list.status.success(), "{}", stderr(&list));
    for id in [
        "brd",
        "prd",
        "fsd",
        "business-flow",
        "data-model",
        "release-notes",
    ] {
        assert!(
            stdout(&list).contains(id),
            "{id} missing:\n{}",
            stdout(&list)
        );
    }

    let made = dit(tmp.path(), &["docs", "new", "prd", "Checkout", "v2"]);
    assert!(made.status.success(), "{}", stderr(&made));
    assert_eq!(stdout(&made).trim(), "docs/business/checkout-v2.md");
    let page = std::fs::read_to_string(tmp.path().join("docs/business/checkout-v2.md")).unwrap();
    assert!(page.contains("# Checkout v2"), "{page}");

    let again = dit(tmp.path(), &["docs", "new", "prd", "Checkout", "v2"]);
    assert!(!again.status.success());
    assert!(
        stderr(&again).contains("already exists"),
        "{}",
        stderr(&again)
    );
    let unknown = dit(tmp.path(), &["docs", "new", "memo", "Anything"]);
    assert_eq!(unknown.status.code(), Some(2), "{}", stderr(&unknown));
}
