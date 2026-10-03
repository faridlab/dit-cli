//! The server the menu bar app runs (ADR 0029): `dit ui --all` as a child
//! process. The app learns the address and token from the `open:` line
//! `dit ui` prints, so it never reads the token file itself, and stops the
//! server with SIGTERM — which `dit ui` answers by finishing requests in
//! flight — before anything harsher.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where the server is, as the menu shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Started, not yet listening.
    Starting,
    /// Listening; `url` carries the token in its fragment.
    Running { url: String },
    /// Stopped by the app, or exited cleanly.
    Stopped,
    /// Exited on its own with an error — the last thing it said.
    Failed(String),
}

#[derive(Debug, Default)]
struct Said {
    url: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug)]
pub struct Server {
    child: Child,
    said: Arc<Mutex<Said>>,
}

/// Folders a Mac keeps programs in that an app started from Finder or at
/// login does not have on its `PATH` — Homebrew's among them.
const EXTRA_PATH: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

impl Server {
    /// Start `dit ui --all --stop-with-parent --port <port>` from the `dit`
    /// binary at `dit`.
    pub fn start(dit: &Path, port: u16) -> std::io::Result<Server> {
        let mut path = std::env::var("PATH").unwrap_or_default();
        for extra in EXTRA_PATH {
            if !path.split(':').any(|p| p == *extra) {
                if !path.is_empty() {
                    path.push(':');
                }
                path.push_str(extra);
            }
        }
        let mut child = Command::new(dit)
            // --stop-with-parent: if this app is killed, the server does not
            // live on, orphaned and holding the port.
            .args([
                "ui",
                "--all",
                "--stop-with-parent",
                "--port",
                &port.to_string(),
            ])
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let said = Arc::new(Mutex::new(Said::default()));
        if let Some(out) = child.stdout.take() {
            let said = said.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if let Some(url) = line.strip_prefix("open: ") {
                        lock(&said).url = Some(url.trim().to_owned());
                    }
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            let said = said.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    let line = line.trim();
                    if !line.is_empty() {
                        let line = line.strip_prefix("dit: ").unwrap_or(line);
                        lock(&said).last_error = Some(line.to_owned());
                    }
                }
            });
        }
        Ok(Server { child, said })
    }

    pub fn state(&mut self) -> ServerState {
        match self.child.try_wait() {
            Ok(None) => match lock(&self.said).url.clone() {
                Some(url) => ServerState::Running { url },
                None => ServerState::Starting,
            },
            Ok(Some(status)) if status.success() => ServerState::Stopped,
            Ok(Some(status)) => {
                // stderr may still be draining the moment the process exits.
                std::thread::sleep(Duration::from_millis(50));
                ServerState::Failed(
                    lock(&self.said)
                        .last_error
                        .clone()
                        .unwrap_or_else(|| format!("DIT stopped ({status})")),
                )
            }
            Err(e) => ServerState::Failed(e.to_string()),
        }
    }

    /// Ask the server to stop and wait up to `patience` for it to finish
    /// what it is doing; kill it only after that.
    pub fn stop(mut self, patience: Duration) -> ServerState {
        if let Ok(Some(_)) = self.child.try_wait() {
            return self.state();
        }
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &self.child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return ServerState::Stopped;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        ServerState::Stopped
    }
}

fn lock(said: &Mutex<Said>) -> std::sync::MutexGuard<'_, Said> {
    match said.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
#[cfg(unix)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in for `dit` — a shell script, so the test needs no build of
    /// the real binary and can say exactly what a server would.
    fn fake_dit(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("dit");
        let script = format!("#!/bin/sh\n{body}\n");
        std::fs::write(&path, script).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    fn wait_for(server: &mut Server, want: impl Fn(&ServerState) -> bool) -> ServerState {
        for _ in 0..100 {
            let state = server.state();
            if want(&state) {
                return state;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        server.state()
    }

    #[test]
    fn the_address_comes_from_the_open_line_and_a_stop_is_a_term() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("got-term");
        let dit = fake_dit(
            tmp.path(),
            &format!(
                "trap 'echo yes > {}; exit 0' TERM\necho \"DIT listening on http://127.0.0.1:$5/\"\necho \"open: http://127.0.0.1:$5/#token=abc\"\nwhile true; do sleep 0.05; done",
                marker.display()
            ),
        );
        let mut server = Server::start(&dit, 7799).unwrap();
        let state = wait_for(&mut server, |s| matches!(s, ServerState::Running { .. }));
        assert_eq!(
            state,
            ServerState::Running {
                url: "http://127.0.0.1:7799/#token=abc".into()
            }
        );
        assert_eq!(server.stop(Duration::from_secs(5)), ServerState::Stopped);
        assert!(marker.exists(), "the server was asked, not killed");
    }

    #[test]
    fn a_server_that_cannot_start_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let dit = fake_dit(
            tmp.path(),
            "echo 'dit: Address already in use — is another dit ui or dit-server on port 7700?' >&2\nexit 1",
        );
        let mut server = Server::start(&dit, 7700).unwrap();
        let state = wait_for(&mut server, |s| matches!(s, ServerState::Failed(_)));
        assert_eq!(
            state,
            ServerState::Failed(
                "Address already in use — is another dit ui or dit-server on port 7700?".into()
            )
        );
    }

    #[test]
    fn a_server_that_ignores_the_term_is_killed_after_the_patience_runs_out() {
        let tmp = tempfile::tempdir().unwrap();
        let dit = fake_dit(
            tmp.path(),
            "trap '' TERM\necho \"open: http://127.0.0.1:1/#token=x\"\nwhile true; do sleep 0.05; done",
        );
        let mut server = Server::start(&dit, 1).unwrap();
        wait_for(&mut server, |s| matches!(s, ServerState::Running { .. }));
        let started = Instant::now();
        assert_eq!(
            server.stop(Duration::from_millis(300)),
            ServerState::Stopped
        );
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
