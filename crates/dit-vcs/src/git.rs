//! A thin, honest wrapper around the `git` binary.
//!
//! Git operations that change history — push, merge, rebase — genuinely do
//! not exist in the pure-Rust git libraries, so this workspace shells out to
//! the real git instead of pretending otherwise. Everything git-related
//! lives in this one crate: no other crate may spawn a git command, which
//! keeps the boundary between "reads the index" and "talks to git" visible
//! in every dependency graph.
//!
//! Conventions:
//! - every command captures its output; nothing is ever printed by a call
//!   here (printing is the CLI's job),
//! - failures carry the command and git's own stderr verbatim, because
//!   "merge failed" with no stderr has cost more debugging hours than any
//!   other error shape,
//! - paths passed to git are always repo-relative strings, never absolute
//!   paths, so the errors quote what git saw.

use std::path::{Path, PathBuf};
use std::process::Command;

/// One diagnostic line per error, with git's own words attached.
#[derive(Debug, thiserror::Error)]
pub enum VcsError {
    #[error("`git {args}` failed:\n{stderr}")]
    Git { args: String, stderr: String },
    #[error("`{0}` is not inside a git repository")]
    NotARepository(String),
    #[error("git is not installed or not on PATH")]
    GitMissing,
    #[error(
        "the frontmatter merge driver is not configured — refusing to rebase, because a \
         rebase without it can silently discard someone's changes. Run `dit install-hooks` \
         first."
    )]
    MergeDriverNotConfigured,
    #[error("cannot rebase: the working tree has uncommitted changes ({0})")]
    DirtyWorktree(String),
}

/// A git repository on disk.
#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
}

/// What happened when a rebase ran.
#[derive(Debug, PartialEq)]
pub enum RebaseOutcome {
    /// The rebase completed; every commit was replayed.
    Clean,
    /// The rebase stopped on conflicts. The listed files contain markers and
    /// the repository is mid-rebase — that is a state to report, not an
    /// error to unwind.
    Conflicted(Vec<PathBuf>),
}

impl Repo {
    /// `git init` a fresh repository with a deterministic initial branch.
    pub fn init(path: &Path) -> Result<Repo, VcsError> {
        run_in(path, &["init", "--initial-branch=main"])?;
        Repo::open(path)
    }

    /// Open the repository containing `path` (resolves to the worktree root).
    pub fn open(path: &Path) -> Result<Repo, VcsError> {
        let out = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(path)
            .output()
            .map_err(|_| VcsError::GitMissing)?;
        if !out.status.success() {
            return Err(VcsError::NotARepository(path.display().to_string()));
        }
        let root = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        Ok(Repo {
            root: PathBuf::from(root),
        })
    }

    /// Clone `url` into `to` (a fresh directory that must not exist yet).
    pub fn clone(url: &Path, to: &Path) -> Result<Repo, VcsError> {
        let url = url.display().to_string();
        let to_str = to.display().to_string();
        let parent = to.parent().unwrap_or(Path::new(".")).to_path_buf();
        run_in(&parent, &["clone", &url, &to_str])?;
        Repo::open(to)
    }

    /// Clone a working repository into a bare one — how test and fixture
    /// remotes are built. Production remotes are real URLs, not local paths.
    pub fn clone_to_bare(from: &Path, to: &Path) -> Result<(), VcsError> {
        let from = from.display().to_string();
        let to_str = to.display().to_string();
        let parent = to.parent().unwrap_or(Path::new(".")).to_path_buf();
        run_in(&parent, &["clone", "--bare", &from, &to_str])
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run an arbitrary git command in this repository, for the operations
    /// that do not need their own wrapper yet. Captured output, stderr in
    /// the error — same contract as every wrapper here.
    pub fn git(&self, args: &[&str]) -> Result<String, VcsError> {
        self.run(args)
    }

    /// Run git, capturing output. Non-zero exit becomes `VcsError::Git`
    /// carrying stderr verbatim.
    fn run(&self, args: &[&str]) -> Result<String, VcsError> {
        let out = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .map_err(|_| VcsError::GitMissing)?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        } else {
            Err(VcsError::Git {
                args: args.join(" "),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            })
        }
    }

    // -- identity -----------------------------------------------------------

    /// Set repo-local identity. Local, not global: a tool that quietly
    /// rewrites someone's global git config is unacceptable.
    pub fn set_identity(&self, name: &str, email: &str) -> Result<(), VcsError> {
        self.run(&["config", "user.name", name])?;
        self.run(&["config", "user.email", email])?;
        Ok(())
    }

    pub fn has_identity(&self) -> bool {
        self.run(&["config", "user.email"]).is_ok()
    }

    // -- day-to-day plumbing -------------------------------------------------

    pub fn current_branch(&self) -> Result<String, VcsError> {
        self.run(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Stage specific paths (`.` for everything).
    pub fn add(&self, pathspec: &str) -> Result<(), VcsError> {
        self.run(&["add", pathspec])?;
        Ok(())
    }

    /// Commit staged changes. `Ok(None)` when there was nothing to commit —
    /// callers treat that as success, not failure.
    pub fn commit(&self, message: &str) -> Result<Option<String>, VcsError> {
        let out = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.root)
            .output()
            .map_err(|_| VcsError::GitMissing)?;
        if out.status.success() {
            return Ok(Some(self.run(&["rev-parse", "HEAD"])?));
        }
        // Depending on git version and state, "nothing to commit" lands on
        // either stream — and it is not an error. A dirty tree with nothing
        // staged gets its own phrase, meaning the same thing.
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let nothing_staged = ["nothing to commit", "no changes added to commit"]
            .iter()
            .any(|phrase| stderr.contains(phrase) || stdout.contains(phrase));
        if nothing_staged {
            return Ok(None);
        }
        Err(VcsError::Git {
            args: format!("commit -m {message:?}"),
            stderr: stderr.trim().to_owned(),
        })
    }

    pub fn head(&self) -> Result<String, VcsError> {
        self.run(&["rev-parse", "HEAD"])
    }

    /// True when no tracked file differs from HEAD and nothing is staged.
    pub fn is_clean(&self) -> Result<bool, VcsError> {
        let status = self.run(&["status", "--porcelain"])?;
        Ok(status.is_empty())
    }

    pub fn rev_list_count(&self, range: &str) -> Result<usize, VcsError> {
        let out = self.run(&["rev-list", "--count", range])?;
        out.parse::<usize>().map_err(|_| VcsError::Git {
            args: format!("rev-list --count {range}"),
            stderr: format!("unexpected output: {out:?}"),
        })
    }

    /// Commit time of a revision, in seconds since the epoch — used to work
    /// out which side of a merge git considers newer.
    pub fn commit_timestamp(&self, rev: &str) -> Option<i64> {
        self.run(&["log", "-1", "--format=%ct", rev])
            .ok()?
            .parse()
            .ok()
    }

    /// The text of a blob addressed the way `git show` addresses them, e.g.
    /// `:1:.dit/schema/workflow.yaml` for stage 1 (the common ancestor) of a
    /// file during a merge.
    pub fn show_text(&self, revspec: &str) -> Option<String> {
        self.run(&["show", revspec]).ok()
    }

    /// Walk `git log` with a custom format — the commit-trailer indexing
    /// pipeline builds on this.
    pub fn log_lines(&self, range: &str, format: &str) -> Result<Vec<String>, VcsError> {
        let out = self.run(&["log", &format!("--format={format}"), range])?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// One `(path, blob_sha)` per file under a prefix, at HEAD — the state
    /// side of the index walks this to know what changed.
    pub fn ls_tree(&self, prefix: &str) -> Result<Vec<(String, String)>, VcsError> {
        let out = self.run(&["ls-tree", "-r", "HEAD", "--", prefix])?;
        let mut files = Vec::new();
        for line in out.lines() {
            // <mode> <type> <sha>\t<path>
            let Some((meta, path)) = line.split_once('\t') else {
                continue;
            };
            let sha = meta.rsplit(' ').next().unwrap_or_default();
            if !sha.is_empty() {
                files.push((path.to_owned(), sha.to_owned()));
            }
        }
        Ok(files)
    }

    // -- remotes --------------------------------------------------------------

    pub fn has_remote(&self, name: &str) -> bool {
        self.run(&["remote", "get-url", name]).is_ok()
    }

    pub fn add_remote(&self, name: &str, url: &Path) -> Result<(), VcsError> {
        let url = url.display().to_string();
        self.run(&["remote", "add", name, &url])?;
        Ok(())
    }

    pub fn fetch(&self, remote: &str, branch: &str) -> Result<(), VcsError> {
        self.run(&["fetch", remote, branch])?;
        Ok(())
    }

    /// How many commits `remote/branch` has that HEAD does not.
    pub fn behind_count(&self, remote: &str, branch: &str) -> Result<usize, VcsError> {
        self.rev_list_count(&format!("HEAD..{remote}/{branch}"))
    }

    /// Push, refusing to overwrite remote history.
    pub fn push(&self, remote: &str, branch: &str) -> Result<(), VcsError> {
        self.run(&["push", remote, branch])?;
        Ok(())
    }

    // -- merge machinery --------------------------------------------------------

    /// Files git considers unmerged right now (both-modified, add/add, …).
    pub fn unmerged_paths(&self) -> Result<Vec<PathBuf>, VcsError> {
        let status = self.run(&["status", "--porcelain"])?;
        Ok(status
            .lines()
            .filter(|l| {
                let x = l.as_bytes().first().copied().unwrap_or(b' ');
                let y = l.as_bytes().get(1).copied().unwrap_or(b' ');
                x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D')
            })
            .map(|l| PathBuf::from(l.get(3..).unwrap_or("").trim()))
            .collect())
    }

    /// A rebase is mid-flight (its state directory exists). The merge driver
    /// checks this to learn which temp file is which side.
    pub fn rebase_in_progress(&self) -> bool {
        let Some(git_dir) = self.run(&["rev-parse", "--git-dir"]).ok() else {
            return false;
        };
        self.root.join(&git_dir).join("rebase-merge").exists()
            || self.root.join(&git_dir).join("rebase-apply").exists()
    }

    /// MERGE_HEAD while a merge is in progress.
    pub fn merge_head(&self) -> Option<String> {
        self.run(&["rev-parse", "--verify", "--quiet", "MERGE_HEAD"])
            .ok()
            .filter(|s| !s.is_empty())
    }

    /// Rebase the current branch onto `onto`. Conflict stops are reported as
    /// a state, never as an error — the repository stays mid-rebase for a
    /// human (or the UI) to finish.
    pub fn rebase(&self, onto: &str) -> Result<RebaseOutcome, VcsError> {
        if !self.is_clean()? {
            return Err(VcsError::DirtyWorktree(
                self.run(&["status", "--porcelain"])
                    .unwrap_or_default()
                    .lines()
                    .next()
                    .unwrap_or("uncommitted changes")
                    .to_owned(),
            ));
        }
        let out = Command::new("git")
            .args(["rebase", onto])
            .current_dir(&self.root)
            .output()
            .map_err(|_| VcsError::GitMissing)?;
        if out.status.success() {
            return Ok(RebaseOutcome::Clean);
        }
        let conflicted = self.unmerged_paths()?;
        if !conflicted.is_empty() {
            return Ok(RebaseOutcome::Conflicted(conflicted));
        }
        Err(VcsError::Git {
            args: format!("rebase {onto}"),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        })
    }

    // -- merge driver installation -----------------------------------------------

    /// Register the frontmatter merge driver in this repository's local
    /// config. The driver command must be an ABSOLUTE path to the binary:
    /// git runs the command through a shell whose PATH differs between a
    /// terminal, a GUI app and a CI runner, and a driver that fails to start
    /// silently leaves "ours" as the merge result. Absolute is judged per
    /// platform — `/usr/local/bin/dit` on unix, `D:/bin/dit.exe` on Windows.
    pub fn configure_merge_driver(&self, driver_command: &str) -> Result<(), VcsError> {
        let binary = driver_command.split_whitespace().next().unwrap_or("");
        if !Path::new(binary).is_absolute() {
            return Err(VcsError::Git {
                args: "config merge.dit-md.driver".to_owned(),
                stderr: format!(
                    "the driver command must be an absolute path, got `{driver_command}`"
                ),
            });
        }
        self.run(&["config", "merge.dit-md.name", "DIT frontmatter-aware merge"])?;
        self.run(&["config", "merge.dit-md.driver", driver_command])?;
        Ok(())
    }

    /// True when a merge driver command is configured. Checked before any
    /// rebase or merge that would need it: discovering a missing driver
    /// after the fact is exactly the silent-data-loss scenario the driver
    /// exists to prevent.
    pub fn merge_driver_ready(&self) -> bool {
        match self.run(&["config", "--get", "merge.dit-md.driver"]) {
            Ok(cmd) => {
                !cmd.is_empty() && Path::new(cmd.split_whitespace().next().unwrap_or("")).exists()
            }
            Err(_) => false,
        }
    }
}

fn run_in(dir: &Path, args: &[&str]) -> Result<(), VcsError> {
    // `git init <new-subdir>` and friends: the directory may not exist yet,
    // and spawning a process with a missing cwd fails before git ever runs.
    std::fs::create_dir_all(dir).map_err(|e| VcsError::Git {
        args: args.join(" "),
        stderr: format!("cannot create {}: {e}", dir.display()),
    })?;
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|_| VcsError::GitMissing)?;
    if out.status.success() {
        Ok(())
    } else {
        Err(VcsError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        })
    }
}
