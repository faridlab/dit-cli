//! The operating system's own folder dialog (ADR 0030), opened by the local
//! server on behalf of the page: the page learns the one path a person
//! picked and nothing else about the disk. The programs are fixed — never
//! named by anything in a repository (I7).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

/// Opens a folder dialog with a prompt and answers the chosen folder, `None`
/// when the person cancelled, or why it could not open.
pub type FolderChooser = Arc<dyn Fn(&str) -> Result<Option<PathBuf>, String> + Send + Sync>;

/// The dialog this machine has, if any: Finder's on macOS, zenity's on
/// Linux when installed.
pub fn system_folder_chooser() -> Option<FolderChooser> {
    if cfg!(target_os = "macos") {
        return Some(Arc::new(choose_with_osascript));
    }
    if cfg!(target_os = "linux") && on_path("zenity") {
        return Some(Arc::new(choose_with_zenity));
    }
    None
}

fn choose_with_osascript(prompt: &str) -> Result<Option<PathBuf>, String> {
    // The prompt is ours, but quote it as AppleScript text all the same.
    let quoted = prompt.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("POSIX path of (choose folder with prompt \"{quoted}\")");
    // `activate` first: the dialog belongs to a background process, and
    // would otherwise open behind the browser that asked for it.
    let out = Command::new("/usr/bin/osascript")
        .args(["-e", "activate", "-e", &script])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot open the folder dialog: {e}"))?;
    if out.status.success() {
        return Ok(chosen(&String::from_utf8_lossy(&out.stdout)));
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // -128 is AppleScript's "User canceled".
    if stderr.contains("-128") {
        return Ok(None);
    }
    Err(format!("the folder dialog failed: {}", stderr.trim()))
}

fn choose_with_zenity(prompt: &str) -> Result<Option<PathBuf>, String> {
    let out = Command::new("zenity")
        .args(["--file-selection", "--directory", "--title", prompt])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot open the folder dialog: {e}"))?;
    match out.status.code() {
        Some(0) => Ok(chosen(&String::from_utf8_lossy(&out.stdout))),
        // zenity answers 1 when the dialog is closed or cancelled.
        Some(1) => Ok(None),
        _ => Err(format!(
            "the folder dialog failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// The dialog's answer as a path: one line, without the trailing slash
/// AppleScript adds (except for `/` itself).
fn chosen(stdout: &str) -> Option<PathBuf> {
    let line = stdout.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let trimmed = line.trim_end_matches('/');
    Some(PathBuf::from(if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    }))
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_loses_applescripts_trailing_slash() {
        assert_eq!(
            chosen("/Users/me/Projects/\n"),
            Some(PathBuf::from("/Users/me/Projects"))
        );
        assert_eq!(chosen("/\n"), Some(PathBuf::from("/")));
        assert_eq!(chosen("\n"), None);
    }
}
