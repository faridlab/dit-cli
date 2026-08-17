//! `.dit/config.yaml` — committed project configuration.
//!
//! No field here may name an executable, a shell command, a binary path, or a
//! URL that gets fetched automatically: the file arrives via `git pull`, and
//! anything the local tool runs from it is remote code execution by pull
//! request. The struct below is the exhaustive allowlist of what a config may
//! contain.

use serde::{Deserialize, Serialize};

/// A code repo linked to this workspace. Read through a git ref, never
/// merged in — the workspace's own history must stay about the issues.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoLink {
    pub name: String,
    /// A local path or git remote. Resolved by `dit-vcs` only.
    pub remote: String,
    #[serde(default)]
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// The file-format version. Clients read files up to their `SCHEMA_MAX`,
    /// and refuse to write above it.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    /// Mode A linked code repos. Empty in a standalone non-code workspace.
    #[serde(default)]
    pub repos: Vec<RepoLink>,
}

/// The highest schema this client understands (§18.3).
pub const SCHEMA_MAX: u32 = 1;

pub fn default_schema_version() -> u32 {
    1
}

impl Default for Config {
    fn default() -> Self {
        Config {
            schema_version: default_schema_version(),
            repos: vec![],
        }
    }
}

impl Config {
    /// Reading a newer file is best-effort; writing one is refused — an old
    /// client would silently drop fields it does not know about.
    pub fn writable(&self) -> bool {
        self.schema_version <= SCHEMA_MAX
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_schema_file_is_readable_but_not_writable() {
        let cfg = Config {
            schema_version: 2,
            repos: vec![],
        };
        assert!(!cfg.writable(), "an old client must refuse to write");
    }

    #[test]
    fn defaults_are_writeable() {
        assert!(Config::default().writable());
    }
}
