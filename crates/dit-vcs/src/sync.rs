//! The optimistic sync loop: fetch, rebase onto the remote, push, retry when
//! the push loses a race.
//!
//! A conflict during the rebase is a *state*, reported on `SyncReport` — not
//! an `Err`. Callers that treat conflicts as failures end up hiding them,
//! and a hidden conflict is how someone else's work gets committed away.

use std::path::PathBuf;
use std::time::Duration;

use crate::git::{RebaseOutcome, Repo, VcsError};

/// What one auto-resolved field looked like before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldResolution {
    pub path: String,
    pub summary: String,
}

/// One file that needs a human before sync can finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub path: PathBuf,
    pub detail: String,
}

/// The outcome of a sync. Never an error for "we got conflicts".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncReport {
    pub pulled: usize,
    pub pushed: usize,
    pub auto_resolved: Vec<FieldResolution>,
    pub needs_human: Vec<ConflictInfo>,
}

/// Tuning knobs for the sync loop.
pub struct SyncOptions {
    pub remote: String,
    pub branch: String,
    /// How many times a rejected push is retried after re-rebasing.
    pub max_retries: usize,
    /// Base delay between retries; doubled each round. Tests pass zero.
    pub retry_delay: Duration,
    /// Runs between fetch and push. Production leaves it `None`; tests use
    /// it to simulate the remote moving in the middle of a sync.
    pub after_fetch: Option<Box<dyn FnMut() + Send>>,
}

// The test hook is a closure, which has no useful debug form — report only
// whether one is installed.
impl std::fmt::Debug for SyncOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncOptions")
            .field("remote", &self.remote)
            .field("branch", &self.branch)
            .field("max_retries", &self.max_retries)
            .field("retry_delay", &self.retry_delay)
            .field("after_fetch", &self.after_fetch.is_some())
            .finish()
    }
}

impl Default for SyncOptions {
    fn default() -> SyncOptions {
        SyncOptions {
            remote: "origin".to_owned(),
            branch: "main".to_owned(),
            max_retries: 5,
            retry_delay: Duration::from_millis(100),
            after_fetch: None,
        }
    }
}

/// Fetch the remote, replay local commits on top of it, push. If the push is
/// rejected because the remote moved again, the whole cycle repeats, up to
/// `max_retries` times.
pub fn sync(repo: &Repo, opts: &mut SyncOptions) -> Result<SyncReport, VcsError> {
    let mut report = SyncReport::default();

    // A workspace with no remote is a plain local repository: nothing to
    // fetch, nothing to push, and definitely nothing to fail.
    if !repo.has_remote(&opts.remote) {
        return Ok(report);
    }

    let mut attempts = 0usize;
    loop {
        repo.fetch(&opts.remote, &opts.branch)?;

        let behind = repo.behind_count(&opts.remote, &opts.branch)?;
        if behind > 0 {
            // Rebasing YAML issue files line-by-line with git's default
            // merge is precisely the corruption this workspace exists to
            // prevent, so refuse rather than proceed half-protected.
            if !repo.merge_driver_ready() {
                return Err(VcsError::MergeDriverNotConfigured);
            }
            match repo.rebase(&format!("{}/{}", opts.remote, opts.branch))? {
                RebaseOutcome::Clean => report.pulled += behind,
                RebaseOutcome::Conflicted(paths) => {
                    report.needs_human = paths
                        .into_iter()
                        .map(|path| ConflictInfo {
                            detail: "both sides changed this file; conflict markers are in place"
                                .to_owned(),
                            path,
                        })
                        .collect();
                    // The repository stays mid-rebase on purpose: that state
                    // is what a human (or the resolve flow) picks up.
                    return Ok(report);
                }
            }
        }

        if let Some(hook) = opts.after_fetch.as_mut() {
            hook();
        }

        let to_push = repo
            .rev_list_count(&format!("{}/{}..HEAD", opts.remote, opts.branch))
            .unwrap_or(0);
        match repo.push(&opts.remote, &opts.branch) {
            Ok(()) => {
                report.pushed = to_push;
                return Ok(report);
            }
            Err(_) if attempts < opts.max_retries && to_push > 0 => {
                // The remote moved between our fetch and our push. Refetch,
                // rebase, try again — with the driver check enforced on the
                // next loop round.
                attempts += 1;
                std::thread::sleep(opts.retry_delay * (1 << attempts.min(4)));
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}
