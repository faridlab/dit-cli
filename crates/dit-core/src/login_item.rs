//! Open at Login for the menu bar app (ADR 0029): a LaunchAgent that starts
//! it when the person logs in. Like the workspace list, it describes this
//! machine — written atomically, never through a transaction, never
//! committed. Turning it on does not start a second copy now; launchd reads
//! the folder at the next login.

use std::path::{Path, PathBuf};

use crate::DitError;

/// The agent's label, and its file name with `.plist`.
pub const LOGIN_ITEM_LABEL: &str = "dev.dit.tray";

#[derive(Debug, Clone)]
pub struct LoginItem {
    file: PathBuf,
}

impl LoginItem {
    /// The person's own: `~/Library/LaunchAgents/dev.dit.tray.plist`.
    /// `None` when there is no home folder.
    pub fn for_this_user() -> Option<LoginItem> {
        crate::registry::home().map(|h| LoginItem::in_dir(&h.join("Library").join("LaunchAgents")))
    }

    /// The agent in `dir`, a LaunchAgents folder.
    pub fn in_dir(dir: &Path) -> LoginItem {
        LoginItem {
            file: dir.join(format!("{LOGIN_ITEM_LABEL}.plist")),
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn is_enabled(&self) -> bool {
        self.file.is_file()
    }

    /// Start `program` at login.
    pub fn enable(&self, program: &Path) -> Result<(), DitError> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LOGIN_ITEM_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>ProcessType</key>
  <string>Interactive</string>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
</dict>
</plist>
"#,
            xml_escape(&program.to_string_lossy())
        );
        dit_store::atomic::write(&self.file, &plist)?;
        Ok(())
    }

    /// Stop starting at login. Already off is fine.
    pub fn disable(&self) -> Result<(), DitError> {
        match std::fs::remove_file(&self.file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// A path is text inside `<string>`; `&` and `<` would end it early.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
