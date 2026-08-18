//! `.dit/config.yaml` — committed project configuration.
//!
//! No field here may name an executable, a shell command, a binary path, or a
//! URL that gets fetched automatically: the file arrives via `git pull`, and
//! anything the local tool runs from it is remote code execution by pull
//! request. The struct below is the exhaustive allowlist of what a config may
//! contain.

use serde::{Deserialize, Serialize};

use crate::layout::DataLayout;

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
    /// Where content roots live (ADR 0005). One boolean, never a path.
    #[serde(default)]
    pub layout: DataLayout,
    /// Who assigns `number:` (ADR 0007). `local` — max + 1 at creation, the
    /// single-writer default. `on-merge` — dit-bot assigns after merge
    /// serialization, the team default.
    #[serde(default)]
    pub numbering: Numbering,
    /// Mode A linked code repos. Empty in a standalone non-code workspace.
    #[serde(default)]
    pub repos: Vec<RepoLink>,
}

/// Number-assignment policy (ADR 0007). Closed set of two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Numbering {
    #[default]
    Local,
    OnMerge,
}

impl Numbering {
    pub fn as_str(self) -> &'static str {
        match self {
            Numbering::Local => "local",
            Numbering::OnMerge => "on-merge",
        }
    }

    pub fn parse(s: &str) -> Option<Numbering> {
        match s {
            "local" => Some(Numbering::Local),
            "on-merge" => Some(Numbering::OnMerge),
            _ => None,
        }
    }
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
            layout: DataLayout::Root,
            numbering: Numbering::Local,
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
            ..Config::default()
        };
        assert!(!cfg.writable(), "an old client must refuse to write");
    }

    #[test]
    fn defaults_are_writeable() {
        assert!(Config::default().writable());
    }

    #[test]
    fn layout_and_numbering_survive_a_json_round_trip() {
        let cfg = Config {
            layout: DataLayout::DotDir,
            numbering: Numbering::OnMerge,
            ..Config::default()
        };
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json.get("layout").and_then(|v| v.as_str()), Some("dotdir"));
        assert_eq!(
            json.get("numbering").and_then(|v| v.as_str()),
            Some("on-merge")
        );
        let back: Config = serde_json::from_value(json).unwrap();
        assert_eq!(back.layout, DataLayout::DotDir);
        assert_eq!(back.numbering, Numbering::OnMerge);
    }

    #[test]
    fn absent_layout_and_numbering_default_to_root_and_local() {
        // A config written before ADR 0005/0007 carries neither key.
        let json = serde_json::json!({"schema_version": 1});
        let cfg: Config = serde_json::from_value(json).unwrap();
        assert_eq!(cfg.layout, DataLayout::Root);
        assert_eq!(cfg.numbering, Numbering::Local);
    }
}
