//! The workspaces on this machine (ADR 0028): a per-machine list of
//! `{ name, path }` and a default, in DIT's config directory. It is never
//! committed anywhere — like `.dit/morse.local.yaml`, it describes this
//! machine, so it is written atomically with no transaction.

use std::path::{Path, PathBuf};

use crate::{Dit, DitError};

/// The file the registry lives in, inside the config directory.
pub const REGISTRY_FILE: &str = "workspaces.yaml";

/// One registered workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Registry {
    file: PathBuf,
    workspaces: Vec<WorkspaceEntry>,
    default: Option<String>,
}

/// DIT's per-machine config directory: `$XDG_CONFIG_HOME/dit`, else
/// `~/.config/dit`. `None` when there is no home to put it in.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join("dit"));
    }
    home().map(|h| h.join(".config").join("dit"))
}

/// Where "New workspace" puts a workspace: `~/Documents/DIT`, or `~/DIT`
/// on a machine with no Documents folder.
pub fn default_workspace_root() -> Option<PathBuf> {
    let home = home()?;
    let documents = home.join("Documents");
    Some(if documents.is_dir() {
        documents.join("DIT")
    } else {
        home.join("DIT")
    })
}

pub(crate) fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// A workspace name: a lowercase word, letters and digits joined by `-` or
/// `_`, starting with a letter — it is a URL segment and a folder name.
fn check_name(name: &str) -> Result<(), DitError> {
    let mut chars = name.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && name.len() <= 48;
    if ok {
        Ok(())
    } else {
        Err(DitError::Refuse(format!(
            "`{name}` is not a workspace name — a lowercase word like `acme` or `side-project`"
        )))
    }
}

/// A name from a folder's own name: `Side Project` → `side-project`.
fn name_from(path: &Path) -> String {
    let raw = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = String::new();
    let mut dash = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out.insert_str(0, "ws-");
    }
    out.truncate(48);
    out
}

impl Registry {
    /// Read the registry from `dir`; a missing file is an empty registry.
    pub fn load(dir: &Path) -> Result<Registry, DitError> {
        let file = dir.join(REGISTRY_FILE);
        let mut registry = Registry {
            file: file.clone(),
            workspaces: Vec::new(),
            default: None,
        };
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(registry),
            Err(e) => return Err(e.into()),
        };
        let bad = |why: &str| DitError::Refuse(format!("{}: {why}", file.display()));
        let root = dit_parse::parse_yaml(&text).map_err(|e| bad(&e.to_string()))?;
        if let Some(items) = root.get("workspaces").and_then(|w| w.as_seq()) {
            for item in items {
                let (Some(name), Some(path)) = (
                    item.get("name").and_then(|n| n.as_str()),
                    item.get("path").and_then(|p| p.as_str()),
                ) else {
                    return Err(bad("every workspace needs a `name` and a `path`"));
                };
                registry.workspaces.push(WorkspaceEntry {
                    name: name.to_owned(),
                    path: PathBuf::from(path),
                });
            }
        }
        registry.default = root
            .get("default")
            .and_then(|d| d.as_str())
            .map(str::to_owned);
        Ok(registry)
    }

    pub fn workspaces(&self) -> &[WorkspaceEntry] {
        &self.workspaces
    }

    pub fn get(&self, name: &str) -> Option<&WorkspaceEntry> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    /// The default workspace: the one marked so, else the first.
    pub fn default_entry(&self) -> Option<&WorkspaceEntry> {
        self.default
            .as_deref()
            .and_then(|d| self.get(d))
            .or_else(|| self.workspaces.first())
    }

    /// The workspace a command names: `--workspace`, then `DIT_WORKSPACE`.
    /// `Ok(None)` when neither names one — the current directory decides.
    pub fn resolve(
        &self,
        flag: Option<&str>,
        env: Option<&str>,
    ) -> Result<Option<&WorkspaceEntry>, DitError> {
        let Some(name) = flag.or(env).map(str::trim).filter(|n| !n.is_empty()) else {
            return Ok(None);
        };
        self.get(name).map(Some).ok_or_else(|| {
            DitError::Missing(format!(
                "workspace `{name}` — `dit workspace list` shows the ones on this machine"
            ))
        })
    }

    /// Make a new workspace called `name` under `root` (ADR 0028): the
    /// folder, `dit init` in it, and the registry entry. Nobody types a path.
    pub fn create(&mut self, name: &str, root: &Path, driver: &Path) -> Result<PathBuf, DitError> {
        check_name(name)?;
        if self.get(name).is_some() {
            return Err(DitError::Refuse(format!(
                "a workspace called `{name}` already exists"
            )));
        }
        let path = root.join(name);
        if path.exists() && std::fs::read_dir(&path)?.next().is_some() {
            return Err(DitError::Refuse(format!(
                "{} already holds files — choose another name, or add that folder instead",
                path.display()
            )));
        }
        std::fs::create_dir_all(&path)?;
        // Stored canonical, as `add` stores it, so one folder is one entry
        // however it was reached (`/var` and `/private/var` on macOS).
        let path = path.canonicalize()?;
        Dit::init(&path, driver)?;
        self.push(name.to_owned(), path.clone())?;
        Ok(path)
    }

    /// Register an existing folder. `must_be_workspace` is the browser's
    /// rule: only a folder that already holds `.dit/config.yaml` — a plain
    /// repository opens as a code map, and a page must not be able to point
    /// DIT at any repository on the disk (ADR 0028). Returns the name; a
    /// folder already registered keeps the name it has.
    pub fn add(
        &mut self,
        name: Option<&str>,
        path: &Path,
        must_be_workspace: bool,
    ) -> Result<String, DitError> {
        if !path.is_dir() {
            return Err(DitError::Refuse(format!(
                "{} is not a folder on this machine",
                path.display()
            )));
        }
        let path = path.canonicalize()?;
        if let Some(existing) = self.workspaces.iter().find(|w| w.path == path) {
            return Ok(existing.name.clone());
        }
        if must_be_workspace && !Dit::is_workspace(&path).unwrap_or(false) {
            return Err(DitError::Refuse(format!(
                "{} is not a DIT workspace — create one with New workspace, or add this folder from a terminal with `dit workspace add`",
                path.display()
            )));
        }
        let mut name = name.map(str::to_owned).unwrap_or_else(|| name_from(&path));
        check_name(&name)?;
        let base = name.clone();
        let mut n = 2;
        while self.get(&name).is_some() {
            name = format!("{base}-{n}");
            n += 1;
        }
        self.push(name.clone(), path)?;
        Ok(name)
    }

    /// Take a workspace off the list. Its files are not touched.
    pub fn remove(&mut self, name: &str) -> Result<(), DitError> {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w.name != name);
        if self.workspaces.len() == before {
            return Err(DitError::Missing(format!("workspace `{name}`")));
        }
        if self.default.as_deref() == Some(name) {
            self.default = None;
        }
        self.save()
    }

    pub fn set_default(&mut self, name: &str) -> Result<(), DitError> {
        if self.get(name).is_none() {
            return Err(DitError::Missing(format!("workspace `{name}`")));
        }
        self.default = Some(name.to_owned());
        self.save()
    }

    fn push(&mut self, name: String, path: PathBuf) -> Result<(), DitError> {
        self.workspaces.push(WorkspaceEntry { name, path });
        if let Err(e) = self.save() {
            self.workspaces.pop();
            return Err(e);
        }
        Ok(())
    }

    fn save(&self) -> Result<(), DitError> {
        let mut out =
            String::from("# DIT workspaces on this machine (ADR 0028). Never committed.\n");
        if let Some(default) = &self.default {
            out.push_str(&format!("default: {default}\n"));
        }
        out.push_str("workspaces:\n");
        for w in &self.workspaces {
            out.push_str(&format!(
                "  - {{ name: {}, path: {} }}\n",
                w.name,
                quoted_path(&w.path)?
            ));
        }
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        dit_store::atomic::write(&self.file, &out)?;
        Ok(())
    }
}

/// A path the YAML reader reads back exactly: it strips one pair of quotes
/// and processes no escapes, so a path holding both kinds is refused.
fn quoted_path(path: &Path) -> Result<String, DitError> {
    let text = path.to_string_lossy();
    if !text.contains('"') {
        Ok(format!("\"{text}\""))
    } else if !text.contains('\'') {
        Ok(format!("'{text}'"))
    } else {
        Err(DitError::Refuse(format!(
            "{text} holds both kinds of quote, which the workspace list cannot keep — rename the folder"
        )))
    }
}
