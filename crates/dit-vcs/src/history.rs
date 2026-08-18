//! The `field_events` backfill: walk every commit, diff every issue file
//! against every parent, and report each frontmatter change as an event.
//!
//! The obvious shortcuts here are all wrong in ways that fail silently, which
//! is why this walker looks heavier than "read the log":
//!
//! - **No squashed diffs.** One `git diff old new` spanning N commits turns
//!   three status changes into one event, and every cycle-time number built
//!   on top becomes fiction. So the walk is per commit.
//! - **Merge commits diff against *every* parent.** A merge's tree equals
//!   each parent's tree plus that side's resolutions; diffing only against
//!   the first parent would attribute the other side's work to nobody, and
//!   the merge driver's decisions — the changes most worth auditing — would
//!   produce zero events. Per-parent diffs can describe the same change
//!   twice; the event table's uniqueness key includes `parent_sha`, so a
//!   re-run never doubles anything.
//! - **Events key to the issue id, never the path.** Folders move (`dit
//!   archive`, refactors), and a path-keyed history loses everything before
//!   the move.
//! - **The root commit diffs against the empty tree**, so file creation is
//!   an event like any other, with an empty `parent_sha`.
//!
//! Events come back oldest-first in topological commit order; the index
//! stamps them with `seq` in exactly the order received, which is what makes
//! a rebuild reproduce the same numbering.

use dit_model::{looks_like_issue_body, DataLayout, EventSource, FieldEvent};
use dit_parse::frontmatter::{Document, Value};

use crate::git::{Repo, VcsError};

/// The empty tree object's sha is a fixed constant for structural reasons
/// (every git computes it the same way), which makes it a usable "diff from
/// nothing" base for root commits.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Walk commits reachable from HEAD (optionally only those after `since`,
/// exclusive — pass the previously recorded watermark) and produce one event
/// per changed frontmatter field per parent diff.
///
/// `layout` decides which paths are DIT data (ADR 0005): it narrows the diff
/// to the roots this workspace actually uses and classifies issue bodies by
/// shape, so `README.md` bodies under `issues/` are read and doc pages are
/// not. The layout is injected — this crate never reads config itself.
pub fn walk_field_events(
    repo: &Repo,
    since: Option<&str>,
    layout: DataLayout,
) -> Result<Vec<FieldEvent>, VcsError> {
    let range = match since {
        Some(watermark) => format!("{watermark}..HEAD"),
        None => "HEAD".to_owned(),
    };
    // %x00 separates fields: shas and dates never contain it, and an author
    // name with spaces stays one field. The last field is the Dit-Author
    // trailer — the DIT alias the transaction stamped into the commit message
    // — preferred over %an because git's configured identity is machine-level
    // ("DIT Test", "farid@laptop") and tells you nothing about who clicked.
    // Topo order (not date order) makes the sequence deterministic regardless
    // of clock skew; --reverse turns newest-first into oldest-first so seq
    // counts up through history.
    let out = repo.git(&[
        "log",
        "--topo-order",
        "--reverse",
        "--format=%H%x00%P%x00%an%x00%aI%x00%(trailers:key=Dit-Author,valueonly)",
        &range,
    ])?;
    let commits: Vec<&str> = out.lines().collect();

    let mut events = Vec::new();
    for line in commits {
        // The trailer field ends with its own newline, so a commit without
        // one simply leaves the field empty. Splitting on all separators is
        // safe: nothing before the trailer ever contains %x00.
        let parts: Vec<&str> = line.split('\u{0}').collect();
        if parts.len() < 5 {
            continue;
        }
        let sha = parts[0];
        let parents = parts[1].trim();
        let trailer = parts[4].trim();
        let author = if trailer.is_empty() {
            parts[2]
        } else {
            trailer
        };
        let date = parts[3];
        let bases: Vec<&str> = if parents.is_empty() {
            vec![EMPTY_TREE]
        } else {
            parents.split_whitespace().collect()
        };
        for base in bases {
            diff_one(repo, sha, base, author, date, &mut events, layout)?;
        }
    }
    Ok(events)
}

/// Diff one commit against one base and append the events found in it.
fn diff_one(
    repo: &Repo,
    sha: &str,
    base: &str,
    author: &str,
    date: &str,
    events: &mut Vec<FieldEvent>,
    layout: DataLayout,
) -> Result<(), VcsError> {
    // -M notices folder moves and reports them as renames; the handler reads
    // each side from its own path either way. The pathspec keeps code-repo
    // commits (Mode C) out of the walk.
    let mut argv = vec![
        "diff".to_owned(),
        "--name-status".to_owned(),
        "-M".to_owned(),
        base.to_owned(),
        sha.to_owned(),
        "--".to_owned(),
    ];
    argv.extend(layout.diff_pathspecs().iter().map(|s| s.to_string()));
    let argv_ref: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let diff = repo.git(&argv_ref)?;
    for line in diff.lines() {
        // <status>\t<path>[\t<new path> for renames/copies]
        let mut fields = line.split('\t');
        let Some(status) = fields.next() else {
            continue;
        };
        let (old_path, new_path) = match (fields.next(), fields.next()) {
            (Some(a), Some(b)) if status.starts_with('R') || status.starts_with('C') => {
                (Some(a), Some(b))
            }
            (Some(a), _) => (Some(a), Some(a)),
            _ => continue,
        };
        let Some(old_path) = old_path else { continue };
        let Some(new_path) = new_path else { continue };
        if !looks_like_issue_body(old_path, layout) && !looks_like_issue_body(new_path, layout) {
            continue;
        }
        // A file appearing or vanishing is handled by the same None-aware
        // field diff: absent → present is a creation event, present → absent
        // a removal.
        let old_text = if status.starts_with('A') || status.starts_with('C') {
            None
        } else {
            repo.show_text(&format!("{base}:{old_path}"))
        };
        let new_text = if status.starts_with('D') {
            None
        } else {
            repo.show_text(&format!("{sha}:{new_path}"))
        };
        push_field_diff(
            old_text.as_deref(),
            new_text.as_deref(),
            sha,
            base_if_real(base),
            author,
            date,
            events,
        );
    }
    Ok(())
}

/// Root-commit diffs name the empty tree, but no such object is recorded in
/// the history — events carry `''` for "no parent", which is also the value
/// the event table's uniqueness contract expects.
fn base_if_real(base: &str) -> &str {
    if base == EMPTY_TREE {
        ""
    } else {
        base
    }
}

/// Compare two revisions of one issue file and append an event per changed
/// frontmatter key. A revision that fails to parse counts as absent; a pair
/// with no `id` on either side contributes nothing — a half-written
/// historical file has no identity to attribute events to.
fn push_field_diff(
    old_text: Option<&str>,
    new_text: Option<&str>,
    sha: &str,
    parent_sha: &str,
    author: &str,
    date: &str,
    events: &mut Vec<FieldEvent>,
) {
    let old = old_text.and_then(|t| Document::parse(t).ok());
    let new = new_text.and_then(|t| Document::parse(t).ok());
    // A missing side (file created or deleted) behaves as a document with no
    // keys: every field on the other side surfaces as a change to or from
    // nothing. The id may come from either side — a deleted file still owns
    // its history.
    let id = new
        .as_ref()
        .and_then(|d| d.get("id"))
        .or_else(|| old.as_ref().and_then(|d| d.get("id")));
    let Some(Value::Scalar(Some(id))) = id else {
        return;
    };

    let mut keys: Vec<&str> = old.as_ref().map(|d| d.keys()).unwrap_or_default();
    if let Some(new) = &new {
        for key in new.keys() {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    for key in keys {
        if key == "id" {
            continue;
        }
        let before = old.as_ref().and_then(|d| d.get(key));
        let after = new.as_ref().and_then(|d| d.get(key));
        if before == after {
            continue;
        }
        events.push(FieldEvent {
            issue_id: id.clone(),
            field: key.to_owned(),
            old_value: before.as_ref().map(value_text),
            new_value: after.as_ref().map(value_text),
            author: author.to_owned(),
            commit_sha: sha.to_owned(),
            parent_sha: parent_sha.to_owned(),
            ts: date.to_owned(),
            source: EventSource::File,
        });
    }
}

/// A display form for a frontmatter value, shared by both sides of every
/// diff so equal values always compare equal.
fn value_text(v: &Value) -> String {
    match v {
        Value::Scalar(Some(s)) => s.clone(),
        Value::Scalar(None) => String::new(),
        Value::Seq(items) => format!("[{}]", items.join(", ")),
        Value::Map(lines) => format!("{{{}\n}}", lines.join("\n")),
    }
}
