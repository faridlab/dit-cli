//! Self-update. `dit upgrade` downloads a release tarball from GitHub,
//! verifies it against the release's `.sha256` asset, and replaces the
//! running binary; `dit doctor` asks the same API whether this binary is
//! current. The URL is a constant of the project's own repo — nothing in a
//! workspace file is ever consulted or fetched (invariant 7 is about repo
//! content naming executables; this is the user explicitly running a command
//! against the binary's own home).
//!
//! The download is refused when the release carries no checksum asset: the
//! first checksummed release is the one that ships after this code, so
//! `dit upgrade <old-version>` to anything older says no instead of
//! installing unverified bytes. `scripts/install.sh` remains the bootstrap
//! path and does not verify.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

// The install chain below is unix-only — Windows ships no prebuilt release,
// so `install` there is a refusal stub — and its imports follow it out.
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::path::PathBuf;

use sha2::{Digest, Sha256};

const REPO_API: &str = "https://api.github.com/repos/faridlab/dit-cli";
const USER_AGENT: &str = concat!(
    "dit/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/faridlab/dit-cli)"
);

/// The release triple this binary was built for, or `None` where no prebuilt
/// release exists (Windows: CI does not publish one).
pub fn triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

/// `v0.1.8` / `0.1.8` → `(0, 1, 8)`. Tags in this repo are always plain
/// `major.minor.patch`, so a hand-rolled tuple beats a semver dependency.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().strip_prefix('v').unwrap_or(text.trim());
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// What `dit doctor` reports about this binary's freshness.
pub enum Freshness {
    Current {
        latest: String,
    },
    Behind {
        current: String,
        latest: String,
    },
    /// The check could not run (offline, rate-limited). Never a failure —
    /// doctor diagnoses the workspace, and the network is not the workspace.
    Unknown {
        current: String,
        reason: String,
    },
}

pub fn freshness() -> Freshness {
    let current = env!("CARGO_PKG_VERSION");
    match latest_tag() {
        Ok(latest) => {
            let same = match (parse_version(current), parse_version(&latest)) {
                (Some(a), Some(b)) => a == b,
                // An unparseable tag is not a reason to warn.
                _ => true,
            };
            if same {
                Freshness::Current { latest }
            } else {
                Freshness::Behind {
                    current: current.to_string(),
                    latest,
                }
            }
        }
        Err(reason) => Freshness::Unknown {
            current: current.to_string(),
            reason,
        },
    }
}

/// Downloads and installs a release, replacing this binary. `None` means the
/// latest release; `Some("0.1.9")` (with or without the `v`) that exact one.
/// Returns a human line for the caller to print.
pub fn upgrade(wanted: Option<&str>) -> Result<String, String> {
    let Some(triple) = triple() else {
        return Err(
            "no prebuilt release for this platform — build from source: scripts/install.sh"
                .to_string(),
        );
    };

    let wanted_tag = match wanted {
        None => None,
        Some(text) => {
            let version = parse_version(text)
                .ok_or_else(|| format!("'{text}' is not a version (expected e.g. 0.1.9)"))?;
            Some(format!("v{}.{}.{}", version.0, version.1, version.2))
        }
    };

    let release = fetch_release(triple, wanted_tag.as_deref())?;
    let current = env!("CARGO_PKG_VERSION");
    if parse_version(&release.tag) == parse_version(current) {
        return Ok(format!("already up to date ({})", release.tag));
    }

    // Resolve the checksum before downloading anything: a release without
    // one is refused, and there is no point pulling the tarball first.
    let checksum_url = release.checksum_url.ok_or_else(|| {
        format!(
            "release {} has no checksum asset — refusing to install unverified bytes \
             (it predates checksummed releases, or the asset is missing); \
             scripts/install.sh is the unverified fallback",
            release.tag
        )
    })?;
    let tarball = http_get(&release.tarball_url, 120)?;
    let checksum = http_get(&checksum_url, 30)?;
    if !checksum_matches(&checksum, &tarball) {
        return Err(format!(
            "checksum mismatch for {} — the download did not arrive intact",
            release.tag
        ));
    }

    let exe = std::env::current_exe().map_err(|e| format!("cannot locate this binary: {e}"))?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Err(
            "refusing to replace a cargo target/ build — install the release binary first \
             (scripts/install.sh), then run 'dit upgrade'"
                .to_string(),
        );
    }
    install(&tarball, &exe)?;
    Ok(format!(
        "upgraded {current} → {} ({})",
        release.tag,
        exe.display()
    ))
}

// -- the network shell -------------------------------------------------------

struct Release {
    tag: String,
    tarball_url: String,
    checksum_url: Option<String>,
}

fn latest_tag() -> Result<String, String> {
    let body = http_get(&format!("{REPO_API}/releases/latest"), 10)?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("bad response from GitHub: {e}"))?;
    json["tag_name"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "the latest release has no tag".to_string())
}

fn fetch_release(triple: &str, tag: Option<&str>) -> Result<Release, String> {
    let url = match tag {
        None => format!("{REPO_API}/releases/latest"),
        Some(tag) => format!("{REPO_API}/releases/tags/{tag}"),
    };
    let body = http_get(&url, 10)?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("bad response from GitHub: {e}"))?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or("the release has no tag")?
        .to_string();

    let mut tarball_url = None;
    let mut checksum_url = None;
    let assets = json["assets"]
        .as_array()
        .ok_or("the release lists no assets")?;
    for asset in assets {
        let name = asset["name"].as_str().unwrap_or_default();
        let download = asset["browser_download_url"].as_str().unwrap_or_default();
        if name == format!("dit-{triple}.tar.gz") {
            tarball_url = Some(download.to_string());
        } else if name == format!("dit-{triple}.tar.gz.sha256") {
            checksum_url = Some(download.to_string());
        }
    }
    let tarball_url =
        tarball_url.ok_or_else(|| format!("release {tag} has no dit-{triple}.tar.gz asset"))?;
    Ok(Release {
        tag,
        tarball_url,
        checksum_url,
    })
}

/// One GET, whole body in memory. The timeout is per-call because a doctor
/// freshness probe and a tarball download should not wait equally long.
fn http_get(url: &str, timeout_secs: u64) -> Result<Vec<u8>, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .build()
        .new_agent();
    let response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("{e}"))?;
    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("download failed: {e}"))?;
    Ok(body)
}

// -- the verified install (no network below this line) -----------------------

/// `sha256sum` writes `<hex>  <filename>\n`; the digest is the first token.
fn checksum_matches(checksum_file: &[u8], tarball: &[u8]) -> bool {
    let file = String::from_utf8_lossy(checksum_file);
    let Some(expected) = file.split_whitespace().next() else {
        return false;
    };
    let actual = hex(&Sha256::digest(tarball));
    expected.eq_ignore_ascii_case(&actual)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Pulls the `dit` entry out of the release tarball and stages it next to the
/// executable — same directory, so the final rename is atomic and never
/// crosses a filesystem boundary. Returns the staged path.
#[cfg(unix)]
fn stage_binary(tarball: &[u8], exe: &Path) -> Result<PathBuf, String> {
    let dir = exe
        .parent()
        .ok_or_else(|| "the executable has no parent directory".to_string())?;
    let staged = dir.join(".dit-upgrade.tmp");
    extract_binary(tarball, &staged)?;
    Ok(staged)
}

#[cfg(unix)]
fn extract_binary(tarball: &[u8], dest: &Path) -> Result<(), String> {
    let gz = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(gz);
    for entry in archive
        .entries()
        .map_err(|e| format!("the release archive is corrupt: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("the release archive is corrupt: {e}"))?;
        let is_file =
            entry.header().entry_type().is_file() || entry.header().entry_type().is_contiguous();
        let name = entry
            .path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_os_string()))
            .unwrap_or_default();
        if !is_file || name != "dit" {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("the release archive is corrupt: {e}"))?;
        fs::write(dest, &bytes).map_err(|e| format!("cannot stage the new binary: {e}"))?;
        set_executable(dest, entry.header().mode().unwrap_or(0o755));
        return Ok(());
    }
    Err("the release archive has no 'dit' binary".to_string())
}

#[cfg(unix)]
fn set_executable(dest: &Path, tar_mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    // The tar entry's own mode, forced executable — an installer that lands
    // a non-runnable binary has failed worse than one that landed nothing.
    let mode = tar_mode | 0o755;
    let _ = fs::set_permissions(dest, fs::Permissions::from_mode(mode));
}

/// Stages, then renames over the executable. On the unix platforms we ship,
/// renaming over a running binary is allowed: the old inode lives as long as
/// the process does, the path now points at the new one.
#[cfg(unix)]
fn install(tarball: &[u8], exe: &Path) -> Result<(), String> {
    let staged = stage_binary(tarball, exe)?;
    fs::rename(&staged, exe).map_err(|e| {
        // A failed rename must not leave the staged file behind.
        let _ = fs::remove_file(&staged);
        format!(
            "cannot replace {} — {e} (a install root you cannot write needs the \
             same rights for 'dit upgrade')",
            exe.display()
        )
    })
}

#[cfg(not(unix))]
fn install(_tarball: &[u8], _exe: &Path) -> Result<(), String> {
    Err("no prebuilt release for this platform".to_string())
}

// -- tests: everything below the network shell, no mocks needed ---------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_with_or_without_the_v() {
        assert_eq!(parse_version("0.1.8"), Some((0, 1, 8)));
        assert_eq!(parse_version("v0.1.8"), Some((0, 1, 8)));
        assert_eq!(parse_version(" 1.2.3 "), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("latest"), None);
    }

    fn sample_tarball() -> Vec<u8> {
        let mut tarball = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tarball);
            let mut header = tar::Header::new_gnu();
            header.set_size(5);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "dit", &b"dit!\n"[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, &tarball).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn checksum_files_verify_by_first_token() {
        let tarball = sample_tarball();
        let digest = hex(&Sha256::digest(&tarball));
        let file = format!("{digest}  dit-aarch64-apple-darwin.tar.gz\n");
        assert!(checksum_matches(file.as_bytes(), &tarball));
        assert!(checksum_matches(file.to_uppercase().as_bytes(), &tarball));
        assert!(!checksum_matches(b"0000  dit.tar.gz\n", &tarball));
        assert!(!checksum_matches(b"\n", &tarball));
    }

    #[cfg(unix)]
    #[test]
    fn extraction_pulls_the_dit_entry_and_marks_it_executable() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dit");
        extract_binary(&sample_tarball(), &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"dit!\n");
        use std::os::unix::fs::PermissionsExt;
        assert!(fs::metadata(&dest).unwrap().permissions().mode() & 0o111 != 0);
    }

    #[cfg(unix)]
    #[test]
    fn install_replaces_the_executable_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("dit");
        fs::write(&exe, b"old").unwrap();
        install(&sample_tarball(), &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"dit!\n");
        // A failed or finished install never leaves the staged file behind.
        assert!(!dir.path().join(".dit-upgrade.tmp").exists());
    }
}
