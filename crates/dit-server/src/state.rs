//! Shared state: one workspace behind a mutex, one token, one broadcast
//! channel for live updates.

use std::sync::{Arc, Mutex, RwLock};

use dit_core::Dit;
use tokio::sync::broadcast;

/// The frame every live client refetches on.
pub const INDEX_UPDATED: &str = r#"{"type":"index_updated"}"#;

pub struct AppState {
    /// The facade. Holding it behind one mutex is the single-writer rule
    /// expressed in the server: a write transaction locks the workspace,
    /// and reads queue behind it rather than racing it.
    pub dit: Arc<Mutex<Dit>>,
    /// The alias writes are attributed to. One server process is one
    /// person at one machine — that is the whole v0.1 model. Behind a lock
    /// because the settings panel can change it while the server runs; read
    /// it through [`AppState::me`].
    me: RwLock<String>,
    /// The bearer token every `/api` request must carry.
    pub token: String,
    /// Hostnames the `Host` header may name. The bind address is in the
    /// list so `--host 0.0.0.0` (LAN mode) does not lock out the phone it
    /// was opened for; anything else is a rebinding attempt.
    pub allowed_hosts: Vec<String>,
    /// Fan-out for the WebSocket endpoint.
    events: broadcast::Sender<String>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The token never appears in logs, not even masked — a masked
        // token in a debug dump is a hint about length and format.
        f.debug_struct("AppState")
            .field("me", &self.me())
            .field("allowed_hosts", &self.allowed_hosts)
            .finish_non_exhaustive()
    }
}

impl AppState {
    pub fn new(dit: Dit, me: &str, token: &str) -> Arc<AppState> {
        let (events, _) = broadcast::channel(16);
        Arc::new(AppState {
            dit: Arc::new(Mutex::new(dit)),
            me: RwLock::new(me.to_owned()),
            token: token.to_owned(),
            allowed_hosts: local_host_names(),
            events,
        })
    }

    /// Like `new`, plus a bind host whose name is accepted in the `Host`
    /// header — for a named interface (LAN mode) where the configured name
    /// is how the workspace is reached.
    pub fn with_bind_host(dit: Dit, me: &str, token: &str, host: &str) -> Arc<AppState> {
        let (events, _) = broadcast::channel(16);
        let mut allowed = local_host_names();
        let name = host_hostname(host);
        if !allowed.contains(&name) {
            allowed.push(name);
        }
        Arc::new(AppState {
            dit: Arc::new(Mutex::new(dit)),
            me: RwLock::new(me.to_owned()),
            token: token.to_owned(),
            allowed_hosts: allowed,
            events,
        })
    }

    /// The alias writes are attributed to right now. Empty when the server
    /// knows nobody. A poisoned lock (a panic mid-write of a `String`) is not
    /// a state worth failing a request over — the old value is still a name.
    pub fn me(&self) -> String {
        match self.me.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Point later writes at a new alias. The caller has already persisted
    /// it through the facade; this only updates what the handlers read.
    pub fn set_me(&self, alias: &str) {
        match self.me.write() {
            Ok(mut guard) => *guard = alias.to_owned(),
            Err(poisoned) => *poisoned.into_inner() = alias.to_owned(),
        }
    }

    /// Tell every connected client the index moved. Failures are drops
    /// into a channel with no listeners — nothing to act on.
    pub fn announce(&self) {
        let _ = self.events.send(INDEX_UPDATED.to_owned());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }

    /// Bring the index current and start watching for writes made by other
    /// processes (ADR 0017): each parallel actor's CLI. Every external
    /// commit becomes one announce, so the UI refetches live; this process's
    /// own writes already announce through the write path, and the watcher's
    /// HEAD-watermark check keeps them from announcing twice. Called once by
    /// both server constructors — standalone `dit-server` and `dit ui`.
    pub fn start_live_updates(self: &Arc<Self>) {
        {
            let mut dit = match self.dit.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Err(e) = dit.refresh_state() {
                tracing::warn!("startup index refresh failed: {e}");
            }
        }
        let rx = dit_core::watch::spawn(std::sync::Arc::clone(&self.dit));
        let state = std::sync::Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("dit-live-updates".into())
            .spawn(move || {
                while rx.recv().is_ok() {
                    state.announce();
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("live-update bridge could not start: {e}");
        }
    }
}

/// The hostnames that may appear in a local request's `Host` header.
fn local_host_names() -> Vec<String> {
    ["localhost", "127.0.0.1", "[::1]", "::1"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Strip the port from a host string, keeping IPv6 brackets intact.
///
/// Hostnames are case-insensitive, and a fully-qualified name carries a
/// trailing dot (`localhost.` is `localhost`). Folding both away here means
/// every spelling a browser or terminal may send compares equal against the
/// allowlist — the normalization only ever widens access for names already
/// allowed, never for foreign ones.
pub fn host_hostname(host: &str) -> String {
    let name = if let Some(rest) = host.strip_prefix('[') {
        // "[::1]:7700" → "[::1]"
        format!("[{}]", rest.split(']').next().unwrap_or(rest))
    } else {
        host.split(':').next().unwrap_or(host).to_owned()
    };
    name.trim_end_matches('.').to_ascii_lowercase()
}
