//! Atomic file operations — the single place in the workspace that opens
//! files for writing.
//!
//! A crash or power loss mid-write must never leave a half-written Markdown
//! file behind: the source of truth is git, and a corrupt file becomes a
//! conflict the merge driver cannot reason about. So every write goes to a
//! temporary sibling first, is flushed to disk, and is then moved onto the
//! target with an atomic rename — readers see either the old bytes or the
//! new bytes, never a mix.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Write `contents` to `path`, creating parent directories as needed.
///
/// The temporary file is written next to the target (same filesystem, so the
/// rename stays atomic), flushed, then renamed over the target.
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = sibling_temp_path(path);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// Remove a file this workspace owns. Grouped here (rather than at call
/// sites) so that every filesystem mutation in the codebase has one address.
pub fn remove_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

/// After a rollback deletes freshly created files, remove the now-empty
/// directories they created, walking up to (but not past) `stop`.
/// Best-effort: a directory that is not empty, or a concurrent creator, just
/// stops the walk.
pub fn prune_empty_dirs_up_to(dir: &Path, stop: &Path) {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d == stop || !d.starts_with(stop) {
            break;
        }
        if fs::remove_dir(d).is_err() {
            break;
        }
        cur = d.parent();
    }
}

fn sibling_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    // The process id keeps concurrent writers from sharing a temp name.
    path.with_file_name(format!(".{name}.tmp-{}", std::process::id()))
}

/// The holder of the single-writer lock. Dropping it releases the lock.
#[derive(Debug)]
pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Best-effort: a lock left behind by a crash is reported by
        // `dit doctor` and cleared manually — silently stealing it here would
        // hide the second-writer bug that left it behind.
        let _ = fs::remove_file(&self.path);
    }
}

/// Acquire the workspace write lock, or fail with a message naming the
/// current holder so the user knows which process to stop.
///
/// The lock is a file created exclusively: creation either wins or observes
/// an existing holder. It is not an OS-level lock — if the holding process
/// dies without cleanup the file remains, which is exactly what we want to
/// surface (`dit doctor`) rather than paper over.
pub fn acquire_lock(path: &Path, holder: &str) -> Result<LockGuard, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create the lock directory: {e}"))?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut f) => {
            use std::io::Write as _;
            let _ = writeln!(f, "{holder}");
            Ok(LockGuard {
                path: path.to_owned(),
            })
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let current = fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or("an unknown process")
                .to_owned();
            Err(format!(
                "another DIT process is writing to this workspace: {current}"
            ))
        }
        Err(e) => Err(format!("cannot acquire the write lock: {e}")),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn write_creates_parents_and_hides_temp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a/b/c.md");
        write(&p, "hello\n").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "hello\n");
        assert_eq!(fs::read_dir(tmp.path().join("a/b")).unwrap().count(), 1);
    }

    #[test]
    fn lock_names_the_holder_and_releases_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = tmp.path().join("write.lock");
        let g = acquire_lock(&lock, "cli 1").unwrap();
        assert!(acquire_lock(&lock, "cli 2").unwrap_err().contains("cli 1"));
        drop(g);
        assert!(acquire_lock(&lock, "cli 2").is_ok());
    }
}
