//! Session token and bind settings.

use std::path::Path;

/// Load the workspace's server token, creating it on first use. The token
/// is the only thing standing between arbitrary web pages and this server,
/// so it is 256 bits from the OS entropy source, stored next to the
/// disposable index (never inside the repo), and readable by the owning
/// user only.
pub fn load_or_create_token(cache_dir: &Path) -> Result<String, std::io::Error> {
    let file = cache_dir.join("server-token");
    if let Ok(text) = std::fs::read_to_string(&file) {
        let token = text.trim();
        if !token.is_empty() {
            // The file may predate this run — restored from a backup, copied
            // over by hand — with permissions looser than a secret deserves.
            restrict_to_owner(&file);
            return Ok(token.to_owned());
        }
    }
    let token = dit_core::generate_token().map_err(std::io::Error::other)?;
    std::fs::create_dir_all(cache_dir)?;
    std::fs::write(&file, &token)?;
    restrict_to_owner(&file);
    Ok(token)
}

/// Drop group and world permissions on the token file. Unix only, because
/// that is where permission bits exist; on other platforms the file lives
/// in the user's own cache directory.
#[cfg(unix)]
fn restrict_to_owner(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) {}
