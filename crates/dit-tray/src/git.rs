//! Is there a git for DIT to use (ADR 0029)? Asked without running `git`:
//! on a Mac without the Command Line Tools, `/usr/bin/git` is a stub whose
//! first run pops Apple's installer — and only `dit-vcs` runs git (I3).

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

/// The stub macOS puts in `/usr/bin` whether or not git is installed.
const STUB: &str = "/usr/bin/git";

/// True when a real git is reachable: one on `path` that is not the stub,
/// or the Command Line Tools installed (`tools_installed`), which make the
/// stub a real git.
pub fn git_present(path: Option<&OsStr>, tools_installed: impl FnOnce() -> bool) -> bool {
    let elsewhere = path
        .map(|p| {
            std::env::split_paths(p).any(|dir| {
                let git = dir.join("git");
                git != Path::new(STUB) && git.is_file()
            })
        })
        .unwrap_or(false);
    elsewhere || tools_installed()
}

/// `xcode-select -p` answers with a folder once the Command Line Tools (or
/// Xcode) are installed, and fails before.
pub fn command_line_tools_installed() -> bool {
    Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Apple's own installer for the Command Line Tools, git among them.
pub fn install_command_line_tools() {
    let _ = Command::new("/usr/bin/xcode-select")
        .arg("--install")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_git_outside_usr_bin_counts_without_asking_for_the_tools() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("git"), "").unwrap();
        let path = std::env::join_paths([tmp.path()]).unwrap();
        assert!(git_present(Some(&path), || panic!("not asked")));
    }

    #[test]
    fn the_usr_bin_stub_alone_needs_the_tools() {
        let path = std::env::join_paths(["/usr/bin"]).unwrap();
        assert!(!git_present(Some(&path), || false));
        assert!(git_present(Some(&path), || true));
        assert!(!git_present(None, || false));
    }
}
