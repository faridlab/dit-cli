//! The frontmatter-aware merge driver: the piece that turns most git
//! conflicts over issue files into automatic, per-field resolutions.
//!
//! Git calls the driver (via `.gitattributes`) with three temporary files —
//! the common ancestor, our version and their version — and expects the
//! merged result written into the second one. Exit 0 means "resolved", any
//! other exit means "a human must look at this".
//!
//! The one rule that outweighs everything else here: if anything at all goes
//! wrong — a file cannot be read, a side does not parse, the code panics —
//! the driver still writes full diff3 conflict markers into the output file
//! and exits 1. Git's default failure mode for a broken driver is to leave
//! the "ours" version in place looking like a normal file; anyone who then
//! commits loses the other side's work permanently and silently. Leaving
//! markers is loud on purpose.

use std::path::Path;

use dit_model::Workflow;
use dit_parse::{serialize_scalar, serialize_seq, Document, FrontmatterError, Value};
use similar::TextDiff;

/// Which side of the merge a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Ours,
    Theirs,
}

impl Side {
    fn flip(self) -> Side {
        match self {
            Side::Ours => Side::Theirs,
            Side::Theirs => Side::Ours,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Side::Ours => "ours",
            Side::Theirs => "theirs",
        }
    }
}

/// How a scalar that both sides changed differently should be settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPolicy {
    /// The side git considers newer wins. The default: it follows the actual
    /// order commits happened in, not wall clocks (which skew between
    /// machines and get poisoned by earlier merges).
    CommitOrder,
    /// The side belonging to the person running this merge wins.
    PreferLocal,
    /// The opposite of `PreferLocal`.
    PreferIncoming,
    /// Never settle automatically; a human decides.
    Conflict,
}

/// What the driver knows about the merge it is deciding. Orientation is not
/// derivable from the temp files alone: during a rebase the file called
/// "ours" is actually the upstream side, the reverse of a plain merge, so
/// whoever builds the context must detect which kind of merge is running.
#[derive(Debug, Clone, Default)]
pub struct MergeContext {
    /// The side whose edits belong to the person running the merge.
    pub local: Side,
    /// The side git considers newer — the winner under `CommitOrder`.
    pub newer: Side,
    /// Per-field policy overrides; every scalar not listed uses
    /// `CommitOrder`.
    pub policies: std::collections::BTreeMap<String, FieldPolicy>,
    /// The workflow the `status` field must respect, read from the
    /// merge-base copy of `schema/workflow.yaml`. `None` means unknown and
    /// the legality check is skipped rather than guessed at.
    pub workflow: Option<Workflow>,
}

/// One field (or the body) that ended up in conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldConflict {
    /// The frontmatter key, or `body`.
    pub key: String,
    /// Human-readable description of the two sides.
    pub detail: String,
}

/// The result of a merge attempt: full file contents plus what, if anything,
/// still needs a human.
#[derive(Debug, Clone, PartialEq)]
pub struct MergeOutcome {
    /// The complete merged file. When `conflicts` is non-empty this contains
    /// conflict markers at exactly the places that need attention.
    pub contents: String,
    pub conflicts: Vec<FieldConflict>,
}

impl MergeOutcome {
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// Why a merge could not even be attempted. This is not a conflict — it is
/// an input the driver refuses to reason about, and the caller's job is to
/// fall back to whole-file diff3 markers.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("the {side} version cannot be read: {source}")]
    Read {
        side: &'static str,
        source: std::io::Error,
    },
    #[error("the {side} version does not parse: {source}")]
    Parse {
        side: &'static str,
        source: FrontmatterError,
    },
}

/// Merge three full file versions into one.
///
/// The output starts from a copy of the "ours" document so that everything
/// the merge does not explicitly decide — unknown fields, comments, key
/// order, blank lines — survives byte-for-byte from our side. Only the keys
/// where the sides disagree are rewritten.
pub fn merge_documents(
    base: &str,
    ours: &str,
    theirs: &str,
    ctx: &MergeContext,
) -> Result<MergeOutcome, MergeError> {
    // An empty base is normal: git calls the driver for add/add conflicts
    // with an empty ancestor, and parsing "" simply yields an empty body.
    let b = parse_side(base, "base")?;
    let o = parse_side(ours, "ours")?;
    let t = parse_side(theirs, "theirs")?;

    let mut merged = o.clone();
    let mut conflicts: Vec<FieldConflict> = Vec::new();

    let mut keys: Vec<String> = o.keys().into_iter().map(str::to_owned).collect();
    for k in t.keys() {
        if !keys.iter().any(|k2| k2 == k) {
            keys.push(k.to_owned());
        }
    }

    for key in &keys {
        let bv = b.get(key);
        let ov = o.get(key);
        let tv = t.get(key);

        if ov == tv {
            // Both sides ended at the same value (including both deleted).
        } else if ov == bv {
            // Only their side moved this key; take theirs.
            apply_value(&mut merged, key, tv.as_ref());
        } else if tv == bv {
            // Only our side moved it; ours is already in the output.
        } else {
            resolve_double_edit(
                &mut merged,
                key,
                bv.as_ref(),
                ov.as_ref(),
                tv.as_ref(),
                ctx,
                &mut conflicts,
            );
        }
    }

    escalate_illegal_status(&mut merged, &b, ctx, &mut conflicts);

    let (body, body_conflicted) = diff3_merge(
        b.body(),
        o.body(),
        t.body(),
        7,
        Side::Ours.label(),
        Side::Theirs.label(),
    );
    merged.set_body(body);
    if body_conflicted {
        conflicts.push(FieldConflict {
            key: "body".to_owned(),
            detail: "both sides edited the same body text".to_owned(),
        });
    }

    Ok(MergeOutcome {
        contents: merged.serialize(),
        conflicts,
    })
}

fn parse_side(text: &str, side: &'static str) -> Result<Document, MergeError> {
    Document::parse(text).map_err(|source| MergeError::Parse { side, source })
}

/// Write a value taken wholesale from one side into the merged document.
/// `None` means the key is absent there — a deletion wins.
fn apply_value(doc: &mut Document, key: &str, value: Option<&Value>) {
    match value {
        None => doc.remove(key),
        Some(Value::Scalar(Some(s))) => {
            doc.set_raw(key, &serialize_scalar(s));
        }
        Some(Value::Scalar(None)) => {
            doc.set_raw(key, "\"\"");
        }
        Some(Value::Seq(items)) => {
            doc.set_raw(key, &serialize_seq(items));
        }
        Some(Value::Map(lines)) => {
            // A nested block mapping travels as its continuation lines; the
            // first line of the entry stays the bare key.
            let joined = lines.join("\n");
            doc.set_raw(key, &format!("\n{joined}"));
        }
    }
}

fn value_repr(value: Option<&Value>) -> String {
    match value {
        None => "(deleted)".to_owned(),
        Some(Value::Scalar(Some(s))) => serialize_scalar(s),
        Some(Value::Scalar(None)) => "\"\"".to_owned(),
        Some(Value::Seq(items)) => serialize_seq(items),
        Some(Value::Map(lines)) => lines.join(" | "),
    }
}

/// Both sides changed the same key, differently. This is the interesting
/// part: most of these are still automatic.
fn resolve_double_edit(
    merged: &mut Document,
    key: &str,
    bv: Option<&Value>,
    ov: Option<&Value>,
    tv: Option<&Value>,
    ctx: &MergeContext,
    conflicts: &mut Vec<FieldConflict>,
) {
    // Timestamps take the max regardless of who won anything: the field
    // means "when was this last touched", not "whose edit survived".
    if key == "updated" {
        if let (Some(Value::Scalar(Some(o))), Some(Value::Scalar(Some(t)))) = (ov, tv) {
            if let (Ok(ot), Ok(tt)) = (parse_timestamp(o), parse_timestamp(t)) {
                let winner = if ot >= tt { o } else { t };
                merged.set_raw(key, &serialize_scalar(winner));
                return;
            }
        }
        // Not parseable as timestamps — settle it like any other scalar.
    }

    // Lists merge as sets: both sides' additions survive, both sides'
    // removals are honored. Two people adding different labels is the
    // everyday case, and it must not be a conflict.
    if let (Some(Value::Seq(oi)), Some(Value::Seq(ti))) = (ov, tv) {
        let empty: Vec<String> = Vec::new();
        let bi = match bv {
            Some(Value::Seq(items)) => items,
            _ => &empty,
        };
        let merged_set = set_merge(bi, oi, ti);
        merged.set_raw(key, &serialize_seq(&merged_set));
        return;
    }

    // Anything involving a nested mapping is settled by a human for now;
    // recursive merging is future work and guessing here loses data.
    if matches!(ov, Some(Value::Map(_))) && matches!(tv, Some(Value::Map(_))) {
        conflicts.push(mark_field_conflict(merged, key, bv, ov, tv));
        return;
    }

    // One side holds a collection (a list, a nested mapping) and the other
    // holds something of a different shape — a scalar, or a deletion.
    // Settling that by policy would silently change what kind of field this
    // even is, so it always goes to a human.
    let o_shape = value_shape(ov);
    let t_shape = value_shape(tv);
    let involves_a_collection =
        matches!(o_shape, Shape::Seq | Shape::Map) || matches!(t_shape, Shape::Seq | Shape::Map);
    if involves_a_collection && o_shape != t_shape {
        conflicts.push(mark_field_conflict(merged, key, bv, ov, tv));
        return;
    }

    // Two different scalars (or scalar vs deletion): policy decides.
    let policy = ctx
        .policies
        .get(key)
        .copied()
        .unwrap_or(FieldPolicy::CommitOrder);
    let winner = match policy {
        FieldPolicy::CommitOrder => ctx.newer,
        FieldPolicy::PreferLocal => ctx.local,
        FieldPolicy::PreferIncoming => ctx.local.flip(),
        FieldPolicy::Conflict => {
            conflicts.push(mark_field_conflict(merged, key, bv, ov, tv));
            return;
        }
    };
    if winner == Side::Ours {
        // Our value is already in the output byte-for-byte; writing it back
        // would only re-serialize a line that did not need touching.
        return;
    }
    apply_value(merged, key, tv);
}

/// The coarse shape of a value, for the "did the field change kind" check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Missing,
    Scalar,
    Seq,
    Map,
}

fn value_shape(value: Option<&Value>) -> Shape {
    match value {
        None => Shape::Missing,
        Some(Value::Scalar(_)) => Shape::Scalar,
        Some(Value::Seq(_)) => Shape::Seq,
        Some(Value::Map(_)) => Shape::Map,
    }
}

/// `(base ∪ additions_ours ∪ additions_theirs) − removals_ours − removals_theirs`,
/// keeping base order first and then each side's new items in that side's
/// order, so the result is stable no matter which side git lists first.
fn set_merge(base: &[String], ours: &[String], theirs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in base.iter().chain(ours.iter()).chain(theirs.iter()) {
        if !out.contains(item) {
            out.push(item.clone());
        }
    }
    out.retain(|item| {
        let removed_by_ours = base.contains(item) && !ours.contains(item);
        let removed_by_theirs = base.contains(item) && !theirs.contains(item);
        !(removed_by_ours || removed_by_theirs)
    });
    out
}

/// A field-level conflict is written into the value position with markers so
/// the file is visibly, loudly unresolved. A conflicted file intentionally
/// does not parse as YAML — nothing should be able to silently read past an
/// unresolved merge.
fn mark_field_conflict(
    merged: &mut Document,
    key: &str,
    bv: Option<&Value>,
    ov: Option<&Value>,
    tv: Option<&Value>,
) -> FieldConflict {
    merged.set_raw(
        key,
        &format!(
            "<<<<<<< ours\n{}\n||||||| base\n{}\n=======\n{}\n>>>>>>> theirs",
            value_repr(ov),
            value_repr(bv),
            value_repr(tv)
        ),
    );
    FieldConflict {
        key: key.to_owned(),
        detail: format!(
            "ours={} base={} theirs={}",
            value_repr(ov),
            value_repr(bv),
            value_repr(tv)
        ),
    }
}

/// If the merged `status` is not a legal step from the base `status`, force
/// a conflict even when a policy had settled it. Two people each moving a
/// card through the workflow can produce a jump no one chose.
fn escalate_illegal_status(
    merged: &mut Document,
    base: &Document,
    ctx: &MergeContext,
    conflicts: &mut Vec<FieldConflict>,
) {
    let Some(workflow) = &ctx.workflow else {
        return;
    };
    let from = base.get_str("status").flatten();
    let to = merged.get_str("status").flatten();
    let (Some(from), Some(to)) = (from, to) else {
        return;
    };
    if from == to || workflow.is_legal_transition(&from, &to) {
        return;
    }
    conflicts.push(FieldConflict {
        key: "status".to_owned(),
        detail: format!("`{from}` → `{to}` is not a legal workflow transition"),
    });
    // Re-mark the field: the value sitting there passed the policy check but
    // not the workflow check, and it must not look settled.
    merged.set_raw(
        "status",
        &format!(
            "<<<<<<< ours\n{to}\n||||||| base\n{from}\n=======\n(needs a human)\n>>>>>>> theirs"
        ),
    );
}

fn parse_timestamp(s: &str) -> Result<time::OffsetDateTime, time::error::Parse> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
}

// ---------------------------------------------------------------------------
// Three-way text merge (diff3) for the markdown body
// ---------------------------------------------------------------------------

/// A replacement of base lines `[start, end)` with new text.
#[derive(Debug, Clone)]
struct Edit {
    start: usize,
    end: usize,
    repl: Vec<String>,
}

fn split_lines(text: &str) -> Vec<String> {
    // split_inclusive keeps the terminator, so joining the pieces back
    // reproduces the original exactly, including a missing final newline.
    text.split_inclusive('\n').map(str::to_owned).collect()
}

/// The edits that turn `base` into `other`, as line ranges over base.
fn line_edits(base: &[String], other: &[String]) -> Vec<Edit> {
    let base_ref: Vec<&str> = base.iter().map(String::as_str).collect();
    let other_ref: Vec<&str> = other.iter().map(String::as_str).collect();
    let diff = TextDiff::from_slices(&base_ref, &other_ref);
    let mut edits = Vec::new();
    for op in diff.ops() {
        match *op {
            similar::DiffOp::Equal { .. } => {}
            similar::DiffOp::Delete {
                old_index, old_len, ..
            } => {
                edits.push(Edit {
                    start: old_index,
                    end: old_index + old_len,
                    repl: Vec::new(),
                });
            }
            similar::DiffOp::Insert {
                old_index,
                new_index,
                new_len,
                ..
            } => {
                edits.push(Edit {
                    start: old_index,
                    end: old_index,
                    repl: other_ref[new_index..new_index + new_len]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                });
            }
            similar::DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                edits.push(Edit {
                    start: old_index,
                    end: old_index + old_len,
                    repl: other_ref[new_index..new_index + new_len]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                });
            }
        }
    }
    edits
}

/// Render a slice of base lines with one side's edits applied.
fn render(base: &[String], edits: &[Edit]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut pos = 0;
    for e in edits {
        out.extend_from_slice(&base[pos..e.start]);
        out.extend(e.repl.iter().cloned());
        pos = e.end;
    }
    out.extend_from_slice(&base[pos..]);
    out
}

/// Three-way line merge. Returns the merged text and whether any region
/// conflicted. Regions only one side edited are taken from that side;
/// regions both edited merge only when the two results are identical,
/// otherwise markers are left behind.
///
/// Edits that merely touch (one ends exactly where the other begins) are
/// treated as one region: git behaves the same way, and silently
/// interleaving two edits written at the same spot produces an order nobody
/// chose.
fn diff3_merge(
    base: &str,
    ours: &str,
    theirs: &str,
    marker_size: usize,
    ours_label: &str,
    theirs_label: &str,
) -> (String, bool) {
    let base_lines = split_lines(base);
    let ours_edits = line_edits(&base_lines, &split_lines(ours));
    let theirs_edits = line_edits(&base_lines, &split_lines(theirs));

    let mut out: Vec<String> = Vec::new();
    let mut conflicted = false;
    let mut pos = 0usize; // next base line not yet consumed
    let mut oi = 0usize;
    let mut ti = 0usize;

    while oi < ours_edits.len() || ti < theirs_edits.len() {
        // Start a region from whichever side's next edit begins earlier.
        let ours_next = oi < ours_edits.len();
        let take_ours = ours_next
            && (ti >= theirs_edits.len() || ours_edits[oi].start <= theirs_edits[ti].start);
        let start = if take_ours {
            ours_edits[oi].start
        } else {
            theirs_edits[ti].start
        };
        let mut end = start;
        let mut o_cluster: Vec<Edit> = Vec::new();
        let mut t_cluster: Vec<Edit> = Vec::new();

        // Grow the region while the next unconsumed edit on either side
        // starts at or before the region's current end.
        loop {
            let mut grew = false;
            if oi < ours_edits.len() && ours_edits[oi].start <= end {
                end = end.max(ours_edits[oi].end);
                o_cluster.push(ours_edits[oi].clone());
                oi += 1;
                grew = true;
            }
            if ti < theirs_edits.len() && theirs_edits[ti].start <= end {
                end = end.max(theirs_edits[ti].end);
                t_cluster.push(theirs_edits[ti].clone());
                ti += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }

        out.extend_from_slice(&base_lines[pos..start]);
        pos = end;

        let region = &base_lines[start..end];
        let ours_region = render(region, &shift_edits(&o_cluster, start));
        let theirs_region = render(region, &shift_edits(&t_cluster, start));

        if o_cluster.is_empty() {
            out.extend(theirs_region);
        } else if t_cluster.is_empty() {
            out.extend(ours_region);
        } else if ours_region == theirs_region {
            // Both sides made the same change; take it once.
            out.extend(ours_region);
        } else {
            conflicted = true;
            out.push(marker_line('<', marker_size, &format!(" {ours_label}")));
            out.extend(ours_region);
            out.push(marker_line('|', marker_size, " base"));
            out.extend(region.iter().cloned());
            out.push(marker_line('=', marker_size, ""));
            out.extend(theirs_region);
            out.push(marker_line('>', marker_size, &format!(" {theirs_label}")));
        }
    }
    out.extend_from_slice(&base_lines[pos..]);

    (out.join(""), conflicted)
}

fn shift_edits(edits: &[Edit], by: usize) -> Vec<Edit> {
    edits
        .iter()
        .map(|e| Edit {
            start: e.start.saturating_sub(by),
            end: e.end.saturating_sub(by),
            repl: e.repl.clone(),
        })
        .collect()
}

fn marker_line(ch: char, size: usize, suffix: &str) -> String {
    let mut s = String::new();
    for _ in 0..size {
        s.push(ch);
    }
    s.push_str(suffix);
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------------------
// The driver entry point git calls
// ---------------------------------------------------------------------------

/// The function git invokes: `dit merge-driver %O %A %B %L %P`.
///
/// Returns the process exit code: 0 = merged cleanly, 1 = conflicts remain
/// (markers are in the file). Every failure path also returns 1 — after
/// writing whole-file markers. The output file is never left untouched on
/// failure.
pub fn drive(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    marker_size: usize,
    _path_label: &str,
) -> i32 {
    drive_with(base, ours, theirs, marker_size, merge_entry)
}

/// What a merge attempt looks like to `drive_with`, so tests can substitute
/// a panicking one and prove the fail-safe catches even that.
pub type MergeFn = fn(&str, &str, &str) -> Result<MergeOutcome, MergeError>;

pub fn drive_with(
    base: &Path,
    ours: &Path,
    theirs: &Path,
    marker_size: usize,
    merge_fn: MergeFn,
) -> i32 {
    // A missing base file is the normal add/add case (git passes an empty
    // temp file); a missing side is a genuine anomaly — either way the
    // merge function gets "" rather than the whole driver aborting.
    let b = std::fs::read_to_string(base).unwrap_or_default();

    let attempt = std::panic::catch_unwind(|| {
        let o = read_side(ours, "ours")?;
        let t = read_side(theirs, "theirs")?;
        merge_fn(&b, &o, &t)
    });

    let (contents, clean) = match attempt {
        Ok(Ok(outcome)) => {
            let clean = outcome.is_clean();
            (outcome.contents, clean)
        }
        // Unreadable input, unparseable side, or a panic — degrade to
        // whole-file markers built from whatever could be read. Loud,
        // never empty, never the "ours" file left as-is.
        _ => {
            let o = std::fs::read_to_string(ours).unwrap_or_default();
            let t = std::fs::read_to_string(theirs).unwrap_or_default();
            (whole_file_markers(&b, &o, &t, marker_size), false)
        }
    };

    if std::fs::write(ours, contents).is_err() {
        // Not even markers could be written; git keeps its conflict state.
        return 1;
    }
    if clean {
        0
    } else {
        1
    }
}

fn read_side(path: &Path, side: &'static str) -> Result<String, MergeError> {
    std::fs::read_to_string(path).map_err(|source| MergeError::Read { side, source })
}

/// Whole-file diff3 markers for the fail-safe path: every line of all three
/// versions, so a human resolving by hand loses nothing.
fn whole_file_markers(base: &str, ours: &str, theirs: &str, marker_size: usize) -> String {
    let mut s = marker_line('<', marker_size, " ours");
    s.push_str(&ensure_trailing_newline(ours));
    s.push_str(&marker_line('|', marker_size, " base"));
    s.push_str(&ensure_trailing_newline(base));
    s.push_str(&marker_line('=', marker_size, ""));
    s.push_str(&ensure_trailing_newline(theirs));
    s.push_str(&marker_line('>', marker_size, " theirs"));
    s
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}

/// The production merge entry: build the context from the repository git
/// already has us running inside, then merge.
fn merge_entry(base: &str, ours: &str, theirs: &str) -> Result<MergeOutcome, MergeError> {
    let ctx = context_from_cwd();
    merge_documents(base, ours, theirs, &ctx)
}

/// Detect which kind of merge is in progress and which side is which.
///
/// During a rebase, the temp file called "ours" holds the upstream version
/// and "theirs" holds your own edits — the reverse of a plain merge. Getting
/// this backwards makes "prefer local" hand every conflict to the other
/// side, so orientation is detected from git's state directories, not
/// guessed.
fn context_from_cwd() -> MergeContext {
    let Ok(repo) = crate::git::Repo::open(Path::new(".")) else {
        return MergeContext::default();
    };

    // The workflow is read from the index's base stage, never from the
    // working tree — the working copy may itself be half-merged.
    let workflow = repo
        .show_text(":1:.dit/schema/workflow.yaml")
        .and_then(|text| dit_parse::parse_workflow(&text).ok());

    if repo.rebase_in_progress() {
        // Our own commits are replayed on top of upstream, so they are the
        // newest by construction — and they arrive in the "theirs" slot.
        return MergeContext {
            local: Side::Theirs,
            newer: Side::Theirs,
            workflow,
            ..MergeContext::default()
        };
    }

    if let Some(merge_head) = repo.merge_head() {
        // Plain merge: "ours" really is ours. Newer side = later commit date.
        let newer = match (
            repo.commit_timestamp("HEAD"),
            repo.commit_timestamp(&merge_head),
        ) {
            (Some(head), Some(other)) if head >= other => Side::Ours,
            _ => Side::Theirs,
        };
        return MergeContext {
            local: Side::Ours,
            newer,
            workflow,
            ..MergeContext::default()
        };
    }

    MergeContext::default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> MergeContext {
        MergeContext {
            local: Side::Ours,
            newer: Side::Theirs,
            ..MergeContext::default()
        }
    }

    fn doc(frontmatter: &str, body: &str) -> String {
        format!("---\n{frontmatter}---\n{body}")
    }

    #[test]
    fn identical_sides_round_trip_byte_exact() {
        let text = doc(
            "id: 01K3M9ZXQ2R7VN8P4TDBCEFGHJ\ntitle: Fix login\n# a comment\nlabels: [auth]\nunknown: keep me\n",
            "\nBody.\n",
        );
        let out = merge_documents(&text, &text, &text, &ctx()).unwrap();
        assert!(out.is_clean());
        assert_eq!(out.contents, text);
    }

    #[test]
    fn only_theirs_changed_a_scalar_takes_theirs() {
        let base = doc("status: todo\npriority: p1\n", "x\n");
        let ours = base.clone();
        let theirs = doc("status: in_progress\npriority: p1\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("status: in_progress"));
    }

    #[test]
    fn only_ours_changed_a_scalar_keeps_ours() {
        let base = doc("status: todo\n", "x\n");
        let ours = doc("status: in_progress\n", "x\n");
        let theirs = base.clone();
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("status: in_progress"));
    }

    #[test]
    fn both_changed_different_scalars_merges_both() {
        let base = doc("status: todo\npriority: p1\n", "x\n");
        let ours = doc("status: in_progress\npriority: p1\n", "x\n");
        let theirs = doc("status: todo\npriority: p0\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("status: in_progress"));
        assert!(out.contents.contains("priority: p0"));
    }

    #[test]
    fn both_made_the_same_change_takes_it_once() {
        let base = doc("status: todo\n", "x\n");
        let ours = doc("status: review\n", "x\n");
        let theirs = doc("status: review\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("status: review"));
    }

    #[test]
    fn double_edit_of_a_scalar_resolves_by_commit_order() {
        let base = doc("status: todo\nestimate: 3\n", "x\n");
        let ours = doc("status: todo\nestimate: 5\n", "x\n");
        let theirs = doc("status: todo\nestimate: 8\n", "x\n");

        // Context says theirs is newer: theirs is written in (quoted, since
        // a bare number is not a canonical plain scalar).
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        let parsed = Document::parse(&out.contents).unwrap();
        assert_eq!(parsed.get_str("estimate").unwrap().unwrap(), "8");

        // Flipped context says ours is newer — same inputs, other winner,
        // and our line is left byte-identical.
        let flipped = MergeContext {
            local: Side::Ours,
            newer: Side::Ours,
            ..MergeContext::default()
        };
        let out = merge_documents(&base, &ours, &theirs, &flipped).unwrap();
        assert!(out.is_clean());
        assert!(
            out.contents.contains("estimate: 5\n"),
            "our winning line must not be rewritten:\n{}",
            out.contents
        );
    }

    #[test]
    fn prefer_local_wins_regardless_of_commit_order() {
        let base = doc("status: todo\n", "x\n");
        let ours = doc("status: in_progress\n", "x\n");
        let theirs = doc("status: review\n", "x\n");
        // Rebase orientation: local edits arrive in the "theirs" slot.
        let rebase_ctx = MergeContext {
            local: Side::Theirs,
            newer: Side::Ours,
            ..MergeContext::default()
        };
        let out = merge_documents(&base, &ours, &theirs, &rebase_ctx).unwrap();
        assert!(out.is_clean());
        assert!(
            out.contents.contains("status: in_progress"),
            "local (theirs slot during rebase) must win:\n{}",
            out.contents
        );
    }

    #[test]
    fn conflict_policy_leaves_markers_and_reports_the_field() {
        let base = doc("priority: p1\n", "x\n");
        let ours = doc("priority: p0\n", "x\n");
        let theirs = doc("priority: p2\n", "x\n");
        let mut c = ctx();
        c.policies
            .insert("priority".to_owned(), FieldPolicy::Conflict);
        let out = merge_documents(&base, &ours, &theirs, &c).unwrap();
        assert!(!out.is_clean());
        assert_eq!(out.conflicts[0].key, "priority");
        assert!(out.contents.contains("<<<<<<< ours"));
        assert!(out.contents.contains("p0"));
        assert!(out.contents.contains("p2"));
    }

    #[test]
    fn updated_takes_the_newer_timestamp_even_when_losing_elsewhere() {
        let base = doc("status: todo\nupdated: 2026-08-16T09:00:00Z\n", "x\n");
        let ours = doc(
            "status: in_progress\nupdated: 2026-08-16T10:00:00Z\n",
            "x\n",
        );
        let theirs = doc("status: review\nupdated: 2026-08-17T08:00:00Z\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        // Status goes to the commit-order winner (theirs)...
        assert!(out.contents.contains("status: review"));
        // ...but updated is simply the later of the two times.
        assert!(out.contents.contains("updated: 2026-08-17T08:00:00Z"));
    }

    #[test]
    fn set_fields_union_both_sides_additions() {
        let base = doc("labels: [auth]\n", "x\n");
        let ours = doc("labels: [auth, api]\n", "x\n");
        let theirs = doc("labels: [auth, perf]\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean(), "{:?}", out.conflicts);
        assert!(out.contents.contains("labels: [auth, api, perf]"));
    }

    #[test]
    fn set_removals_are_honored_alongside_additions() {
        // Base has a and b; ours dropped b; theirs kept b and added c.
        // b was removed by one side, so it goes; c survives.
        let base = doc("labels: [a, b]\n", "x\n");
        let ours = doc("labels: [a]\n", "x\n");
        let theirs = doc("labels: [a, b, c]\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("labels: [a, c]"), "{}", out.contents);
    }

    #[test]
    fn unknown_fields_follow_the_same_rules() {
        let base = doc("status: todo\nwatcher: budi\n", "x\n");
        let ours = doc("status: todo\nwatcher: budi\n", "x\n");
        let theirs = doc("status: todo\nwatcher: farid\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("watcher: farid"));
    }

    #[test]
    fn a_key_theirs_added_and_ours_never_had_is_taken() {
        let base = doc("status: todo\n", "x\n");
        let ours = base.clone();
        let theirs = doc("status: todo\nsprint: aug\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(out.contents.contains("sprint: aug"));
    }

    #[test]
    fn a_key_ours_deleted_stays_deleted_when_theirs_did_not_touch_it() {
        let base = doc("status: todo\nestimate: 3\n", "x\n");
        let ours = doc("status: todo\n", "x\n");
        let theirs = base.clone();
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean());
        assert!(!out.contents.contains("estimate"));
    }

    #[test]
    fn type_clash_between_scalar_and_list_conflicts() {
        let base = doc("assignees: [a]\n", "x\n");
        let ours = doc("assignees: [a, b]\n", "x\n");
        let theirs = doc("assignees: solo\n", "x\n");
        let out = merge_documents(&base, &ours, &theirs, &ctx()).unwrap();
        assert!(!out.is_clean());
        assert_eq!(out.conflicts[0].key, "assignees");
    }

    #[test]
    fn body_edits_in_different_places_merge_cleanly() {
        let base = "---\nstatus: todo\n---\nfirst\nsecond\nthird\nfourth\n";
        let ours = "---\nstatus: todo\n---\nfirst\nsecond (edited)\nthird\nfourth\n";
        let theirs = "---\nstatus: todo\n---\nfirst\nsecond\nthird\nfourth (edited)\n";
        let out = merge_documents(base, ours, theirs, &ctx()).unwrap();
        assert!(out.is_clean(), "{}", out.contents);
        assert!(out.contents.contains("second (edited)"));
        assert!(out.contents.contains("fourth (edited)"));
    }

    #[test]
    fn body_edited_differently_in_the_same_place_leaves_markers() {
        let base = "---\nstatus: todo\n---\nfirst\nsecond\nthird\n";
        let ours = "---\nstatus: todo\n---\nfirst\nours-line\nthird\n";
        let theirs = "---\nstatus: todo\n---\nfirst\ntheirs-line\nthird\n";
        let out = merge_documents(base, ours, theirs, &ctx()).unwrap();
        assert!(!out.is_clean());
        assert_eq!(out.conflicts[0].key, "body");
        assert!(out.contents.contains("<<<<<<< ours"));
        assert!(out.contents.contains("ours-line"));
        assert!(out.contents.contains("theirs-line"));
        assert!(out.contents.contains(">>>>>>> theirs"));
    }

    #[test]
    fn identical_body_edits_on_both_sides_are_taken_once() {
        let base = "---\nk: 1\n---\nline\n";
        let both = "---\nk: 1\n---\nline\nnew line\n";
        let out = merge_documents(base, both, both, &ctx()).unwrap();
        assert!(out.is_clean());
        assert_eq!(out.contents.matches("new line").count(), 1);
    }

    #[test]
    fn add_add_with_empty_base_merges_the_two_sides() {
        // git passes an empty %O for add/add conflicts; the merge must not
        // choke on it.
        let ours = doc("status: todo\nlabels: [api]\n", "\nFrom ours.\n");
        let theirs = doc("status: todo\nlabels: [perf]\n", "\nFrom ours.\n");
        let out = merge_documents("", &ours, &theirs, &ctx()).unwrap();
        assert!(out.is_clean(), "{:?}", out.conflicts);
        assert!(out.contents.contains("labels: [api, perf]"));
    }

    #[test]
    fn an_illegal_status_jump_becomes_a_conflict_even_after_a_policy_pick() {
        let base = doc("status: todo\n", "x\n");
        let ours = doc("status: in_progress\n", "x\n");
        let theirs = doc("status: done\n", "x\n");
        let c = MergeContext {
            workflow: Some(Workflow::default_workflow()),
            ..ctx()
        };
        let out = merge_documents(&base, &ours, &theirs, &c).unwrap();
        assert!(!out.is_clean());
        assert!(out.conflicts.iter().any(|f| f.key == "status"));
        assert!(out.contents.contains("needs a human"));
    }

    #[test]
    fn a_legal_status_transition_stays_resolved() {
        let base = doc("status: todo\n", "x\n");
        let ours = doc("status: todo\n", "x\n");
        let theirs = doc("status: in_progress\n", "x\n");
        let c = MergeContext {
            workflow: Some(Workflow::default_workflow()),
            ..ctx()
        };
        let out = merge_documents(&base, &ours, &theirs, &c).unwrap();
        assert!(out.is_clean(), "{}", out.contents);
    }

    #[test]
    fn an_unparseable_side_is_an_error_not_a_guess() {
        // A body-only document is legal, so use a genuinely broken input:
        // frontmatter that never closes.
        let broken = "---\na: 1\n";
        let err =
            merge_documents("---\na: 1\n---\n", broken, "---\na: 1\n---\n", &ctx()).unwrap_err();
        assert!(matches!(err, MergeError::Parse { side: "ours", .. }));
    }

    // --- drive: the file-level entry point ---

    fn write_all(dir: &Path, base: &str, ours: &str, theirs: &str) -> (PathBuf, PathBuf, PathBuf) {
        let b = dir.join("base");
        let o = dir.join("ours");
        let t = dir.join("theirs");
        std::fs::write(&b, base).unwrap();
        std::fs::write(&o, ours).unwrap();
        std::fs::write(&t, theirs).unwrap();
        (b, o, t)
    }

    #[test]
    fn drive_resolves_a_clean_merge_and_exits_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let base = doc("status: todo\n", "x\n");
        let ours = base.clone();
        let theirs = doc("status: in_progress\n", "x\n");
        let (b, o, t) = write_all(tmp.path(), &base, &ours, &theirs);
        let code = drive_with(&b, &o, &t, 7, |_, _, _| {
            Ok(MergeOutcome {
                contents: doc("status: in_progress\n", "x\n"),
                conflicts: vec![],
            })
        });
        assert_eq!(code, 0);
        assert!(std::fs::read_to_string(&o).unwrap().contains("in_progress"));
    }

    #[test]
    fn drive_exits_one_when_the_merge_reports_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let (b, o, t) = write_all(tmp.path(), "b\n", "o\n", "t\n");
        let code = drive_with(&b, &o, &t, 7, |_, _, _| {
            Ok(MergeOutcome {
                contents: "<<<<<<< ours\n".to_owned(),
                conflicts: vec![FieldConflict {
                    key: "body".to_owned(),
                    detail: String::new(),
                }],
            })
        });
        assert_eq!(code, 1);
    }

    #[test]
    fn a_panicking_merge_still_writes_markers_and_exits_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (b, o, t) = write_all(tmp.path(), "base text\n", "ours text\n", "theirs text\n");
        let code = drive_with(&b, &o, &t, 7, |_, _, _| panic!("driver bug"));
        assert_eq!(code, 1);
        let left = std::fs::read_to_string(&o).unwrap();
        assert!(left.contains("<<<<<<< ours"), "markers missing: {left}");
        assert!(left.contains("ours text"));
        assert!(left.contains("base text"));
        assert!(left.contains("theirs text"));
        assert!(left.contains(">>>>>>> theirs"));
    }

    #[test]
    fn an_unreadable_side_still_leaves_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let (b, o, t) = write_all(tmp.path(), "base\n", "ours\n", "theirs\n");
        std::fs::remove_file(&t).unwrap();
        let code = drive_with(&b, &o, &t, 7, merge_entry);
        assert_eq!(code, 1, "a missing side must not look like a clean merge");
        let left = std::fs::read_to_string(&o).unwrap();
        assert!(left.contains("<<<<<<< ours"));
        assert!(left.contains("base\n"));
        assert!(left.contains("ours\n"));
        assert!(
            !left.contains("theirs text"),
            "theirs is gone; nothing may pretend otherwise"
        );
    }

    #[test]
    fn an_unparseable_base_falls_back_to_whole_file_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let (b, o, t) = write_all(tmp.path(), "---\nbroken\n", "ours\n", "theirs\n");
        let code = drive_with(&b, &o, &t, 7, merge_entry);
        assert_eq!(code, 1);
        let left = std::fs::read_to_string(&o).unwrap();
        assert!(left.contains("<<<<<<< ours"));
        assert!(left.contains("ours\n"));
        assert!(left.contains("theirs\n"));
    }
}
