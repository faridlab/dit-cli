//! The cross-process watcher (ADR 0017): how a running `dit-server` sees
//! writes made by other processes — each parallel actor's CLI — and turns
//! them into a reindex + one signal.
//!
//! This is the realization of DESIGN §16.2's `subscribe()` as a free
//! function: `Dit` is not Arc-shared by design, and a `&self` method
//! returning a receiver fed by a background thread would be self-referential.
//! The server already holds `Dit` behind exactly this mutex (§16.4).
//!
//! The watcher never reads working files and never writes anything. On a
//! debounced burst it compares HEAD against the state watermark and reindexes
//! from git blobs only when HEAD moved — own-process writes already moved the
//! watermark in `absorb_commit`, so they produce no signal and a write
//! announces exactly once. Any watcher error degrades to HEAD polling, never
//! to silence.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use notify::event::EventKind;
use notify::{RecursiveMode, Watcher};
use tracing::{debug, warn};

use crate::Dit;

/// How long the watcher waits for the event stream to go quiet before acting.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// The degraded mode's HEAD-polling interval (watcher error, incl.
/// `MaxFilesWatch` — §6.3's rule: degrade explicitly, never fail silently).
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Watch the workspace and signal once per externally-committed change.
///
/// Watches the tree root and `.dit/` non-recursively plus the current
/// month's shard recursively (new work lands there; §6.3's scale rules —
/// watching every historical month would exhaust inotify). A signal means
/// "the index moved under you, refetch"; it carries no payload on purpose,
/// the same contract the server's WebSocket frame already has.
pub fn spawn(dit: Arc<Mutex<Dit>>) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    let stopped = Arc::new(AtomicBool::new(false));

    let root = {
        let dit = lock(&dit);
        dit.repo.root().to_owned()
    };
    let month_shard = current_month_shard(&root);

    // The notify side: watch and forward raw events into the debounce loop,
    // or degrade that loop to HEAD polling on any error.
    let (event_tx, event_rx) = mpsc::channel();
    let notify_stopped = Arc::clone(&stopped);
    let notify_dit = Arc::clone(&dit);
    let notify_root = root.clone();
    let notify_signal_tx = tx.clone();
    let notify_thread = std::thread::Builder::new()
        .name("dit-watch".into())
        .spawn(move || {
            let run = |event_tx: mpsc::Sender<()>| -> Result<(), notify::Error> {
                let (tx, rx) = mpsc::channel();
                // macOS: kqueue, not the default FSEvents — see the Cargo
                // workspace note. Everywhere else the recommended backend.
                #[cfg(target_os = "macos")]
                let mut watcher = notify::KqueueWatcher::new(tx, notify::Config::default())?;
                #[cfg(not(target_os = "macos"))]
                let mut watcher = notify::recommended_watcher(tx)?;
                watcher.watch(&notify_root, RecursiveMode::NonRecursive)?;
                if let Some(shard) = &month_shard {
                    // Best effort: a missing shard simply means nothing to
                    // watch there yet; the root watch still fires on the
                    // folder's creation.
                    let _ = watcher.watch(shard, RecursiveMode::Recursive);
                }
                for e in rx.iter().flatten() {
                    // Only fs changes matter; the rest (e.g. errors about
                    // watch limits) take the polling path below.
                    if matches!(
                        e.kind,
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                    ) {
                        let _ = event_tx.send(());
                    }
                }
                Ok(())
            };
            if let Err(e) = run(event_tx) {
                warn!("file watcher unavailable ({e}) — degrading to HEAD polling");
                while !notify_stopped.load(Ordering::Relaxed) {
                    if head_moved(&notify_dit) {
                        let mut dit = lock(&notify_dit);
                        if dit.refresh_state().unwrap_or(false) {
                            let _ = notify_signal_tx.send(());
                        }
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            }
        });
    if let Err(e) = notify_thread {
        // Without threads there is no watcher; the receiver's senders drop
        // with them, so the caller sees a quiet channel rather than a
        // half-alive one. Own-process announces still work.
        warn!("watcher threads could not start: {e}");
        return rx;
    }

    // The debounce + watermark side.
    let mover = Arc::clone(&stopped);
    let mover_dit = Arc::clone(&dit);
    let signal_tx = tx;
    let signal_thread = std::thread::Builder::new()
        .name("dit-watch-signal".into())
        .spawn(move || {
            let mut armed = false;
            while !mover.load(Ordering::Relaxed) {
                // Extend the quiet window while events keep arriving.
                match event_rx.recv_timeout(DEBOUNCE) {
                    Ok(()) => armed = true,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if armed && head_moved(&mover_dit) {
                            let mut dit = lock(&mover_dit);
                            match dit.refresh_state() {
                                Ok(true) => {
                                    let _ = signal_tx.send(());
                                    debug!("external write absorbed; index refreshed");
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    // The index is disposable; the next event
                                    // retries. Never kill the watcher.
                                    warn!("refresh after external write failed: {e}");
                                }
                            }
                        }
                        armed = false;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
    if let Err(e) = signal_thread {
        warn!("watcher signal thread could not start: {e}");
    }

    rx
}

/// Lock the shared facade. A poisoned lock means some thread panicked
/// mid-write; the last known state is still a `Dit` worth reading, which is
/// all the watcher ever does with it.
fn lock(dit: &Arc<Mutex<Dit>>) -> std::sync::MutexGuard<'_, Dit> {
    match dit.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Did HEAD move past the state watermark? Cheap by design: one git call and
/// one SQLite read, and only after a debounced event.
fn head_moved(dit: &Arc<Mutex<Dit>>) -> bool {
    let dit = lock(dit);
    match (
        dit.repo.head().ok(),
        dit.index.watermark("state").ok().flatten(),
    ) {
        (Some(head), Some(watermark)) => head != watermark,
        _ => false,
    }
}

/// `issues/YYYY/MM` for the current UTC month, when the layout has one —
/// whichever side of `.dit/` the content root sits on (ADR 0005).
fn current_month_shard(root: &std::path::Path) -> Option<std::path::PathBuf> {
    let now = time::OffsetDateTime::now_utc();
    let month = format!("{:02}", u8::from(now.month()));
    ["issues", ".dit/issues"]
        .into_iter()
        .map(|base| {
            root.join(base)
                .join(format!("{:04}", now.year()))
                .join(&month)
        })
        .find(|shard| shard.is_dir())
}
