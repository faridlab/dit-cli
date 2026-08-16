#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented
)]

//! One test per invariant from ARCHITECTURE.md §1.
//!
//! Everything here is deterministic and fast, because every invariant must be
//! gated by `just check` — not by nightly fuzzing, not by human review.
//!
//! Tests for crates that do not exist yet are `#[ignore]`d with the reason
//! stated. Removing an `#[ignore]` is part of the definition of done for the
//! milestone that introduces the crate.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under `crates/`, excluding `#[cfg(test)]`-only files.
fn source_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(Path::new("crates"), &mut out);
    out
}

/// Strips `#[cfg(test)] mod tests { .. }` blocks so test code may use the
/// constructs production code may not.
fn production_source(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    match text.find("#[cfg(test)]") {
        Some(i) => text[..i].to_owned(),
        None => text,
    }
}

fn offenders(needles: &[&str], allow: &[&str]) -> Vec<String> {
    source_files()
        .into_iter()
        .filter(|p| {
            let s = p.to_string_lossy().replace('\\', "/");
            !allow.iter().any(|a| s.contains(a))
        })
        .filter_map(|p| {
            let src = production_source(&p);
            needles
                .iter()
                .find(|n| src.contains(**n))
                .map(|n| format!("{}: contains `{n}`", p.display()))
        })
        .collect()
}

/// I1 — every write to disk goes through `Transaction` + `dit fmt`.
#[test]
fn i1_no_direct_filesystem_writes() {
    let bad = offenders(
        &["std::fs::write", "File::create", "fs::OpenOptions"],
        &["crates/dit-store/src/atomic"],
    );
    assert!(
        bad.is_empty(),
        "I1 violated — write only through dit-store::atomic:\n  {}",
        bad.join("\n  ")
    );
}

/// I3 — only `dit-vcs` talks to git.
#[test]
fn i3_git_access_is_contained() {
    let bad = offenders(
        &["Command::new(\"git\")", "gix::", "git2::"],
        &["crates/dit-vcs/"],
    );
    assert!(
        bad.is_empty(),
        "I3 violated — git access belongs in dit-vcs:\n  {}",
        bad.join("\n  ")
    );
}

/// I9 — `field_events` is ordered topologically, never by wall clock.
#[test]
fn i9_field_events_never_ordered_by_ts() {
    let bad = offenders(&["ORDER BY ts", "order by ts", "MAX(ts)"], &[]);
    assert!(
        bad.is_empty(),
        "I9 violated — order field_events by `seq`, not `ts`:\n  {}",
        bad.join("\n  ")
    );
}

/// I10 — comrak raw HTML rendering is never enabled.
#[test]
fn i10_comrak_unsafe_is_never_enabled() {
    let bad = offenders(
        &[
            "unsafe_ = true",
            "render.unsafe_ = true",
            "set_unsafe(true)",
        ],
        &[],
    );
    assert!(
        bad.is_empty(),
        "I10 violated — raw HTML rendering is an XSS vector:\n  {}",
        bad.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Pending — un-ignore these as the corresponding crate lands.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs dit-index + dit-core (v0.1)"]
fn i2_reads_survive_worktree_wipe() {
    unimplemented!()
}

#[test]
#[ignore = "needs dit-parse (v0.1)"]
fn i5_frontmatter_has_no_derived_fields() {
    unimplemented!()
}

#[test]
#[ignore = "needs the merge driver (v0.1) — this is Risk #0, do not ship without it"]
fn i6_merge_driver_failsafe() {
    unimplemented!()
}

#[test]
#[ignore = "needs the schema loader (v0.1)"]
fn i7_no_executable_fields_in_schema() {
    unimplemented!()
}

#[test]
#[ignore = "needs dit-parse (v0.1)"]
fn i8_unknown_fields_survive_roundtrip() {
    unimplemented!()
}

#[test]
#[ignore = "needs dit-server (v0.3)"]
fn i10_csp_header_present() {
    unimplemented!()
}
