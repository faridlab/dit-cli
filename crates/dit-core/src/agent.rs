//! Agent onboarding (ADR 0021): one canonical document describing how this
//! workspace expects to be worked in, plus a four-line pointer in each agent
//! file the team actually uses.
//!
//! The specification is generated from this binary, never parsed back, and
//! stamped with the version that produced it — a workspace pinned to a copied
//! spec freezes at the day it was copied and nothing notices. `dit doctor`
//! reads the stamp and says when the binary has moved on.
//!
//! Nothing here is executed or fetched by DIT. The document tells a human or
//! an agent what to run; DIT never runs it (I7).

use std::path::Path;

/// Where the canonical document lives: under `docs/`, so it is visible in the
/// tree, reviewed in pull requests, and reachable from DIT's own Docs screen.
pub const AGENT_DOC_PATH: &str = "docs/dit-for-agents.md";

const SPEC_START: &str = "<!-- dit:agent-spec -->";
const SPEC_END: &str = "<!-- /dit:agent-spec -->";
const POINTER_START: &str = "<!-- dit:agent-pointer -->";
const POINTER_END: &str = "<!-- /dit:agent-pointer -->";
/// The block ADR 0021 absorbs. Recognised so an older workspace upgrades
/// instead of carrying two overlapping sections.
const LEGACY_START: &str = "<!-- dit:workflow-protocol -->";
const LEGACY_END: &str = "<!-- /dit:workflow-protocol -->";

/// Agent files DIT knows how to point. `AGENTS.md` is written even when
/// absent — it is the cross-tool convention, and it is the one file worth
/// creating. The rest are only touched when the team already uses them.
const TOOL_FILES: &[(&str, &str)] = &[
    ("agents", "AGENTS.md"),
    ("claude", "CLAUDE.md"),
    ("cursor", ".cursor/rules"),
    ("copilot", ".github/copilot-instructions.md"),
];

/// Which agent files to point at the canonical document.
#[derive(Debug, Clone, Default)]
pub struct AgentDocOptions {
    /// Create every known agent file, including ones this repo does not have.
    pub all: bool,
    /// Restrict to these tool keys (`agents`, `claude`, `cursor`, `copilot`).
    /// Empty means "the default set".
    pub only: Vec<String>,
}

/// What a run changed. `changed` is false on a second, identical run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDocReport {
    pub changed: bool,
    /// The canonical document was created or rewritten.
    pub document_written: bool,
    /// Repo-relative paths that now carry a pointer block.
    pub pointers: Vec<String>,
    /// Files whose legacy `dit:workflow-protocol` block was replaced.
    pub legacy_replaced: Vec<String>,
}

/// Replace the text between two markers, or append a fresh block when they
/// are absent. Everything outside the markers survives byte-for-byte — that
/// is the whole contract, and it is why a team's hand-written rules can live
/// in the same file as generated ones.
pub(crate) fn upsert_marked_block(existing: &str, start: &str, end: &str, body: &str) -> String {
    let block = format!("{start}\n{body}\n{end}");
    match (existing.find(start), existing.find(end)) {
        (Some(a), Some(b)) if b > a => {
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..a]);
            out.push_str(&block);
            out.push_str(&existing[b + end.len()..]);
            out
        }
        _ => {
            let mut out = existing.to_owned();
            if !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(&block);
            out.push('\n');
            out
        }
    }
}

/// Remove a marked block entirely, including the blank line that followed it.
fn remove_marked_block(existing: &str, start: &str, end: &str) -> Option<String> {
    let (a, b) = (existing.find(start)?, existing.find(end)?);
    if b < a {
        return None;
    }
    let mut out = String::with_capacity(existing.len());
    out.push_str(&existing[..a]);
    out.push_str(existing[b + end.len()..].trim_start_matches('\n'));
    Some(out)
}

/// The pointer every agent file gets: short enough that it costs an agent
/// nothing to read, specific enough that it knows why to follow it.
fn pointer_body() -> String {
    format!(
        "## This repository is a DIT workspace\n\
         \n\
         Project data lives in Markdown files under git, not in a database, and it has rules\n\
         that are enforced — editing an issue file by hand will be rejected or lost. Read\n\
         [`{AGENT_DOC_PATH}`]({AGENT_DOC_PATH}) before touching anything here, and run\n\
         `dit ai spec` for the version-accurate grammar."
    )
}

/// The canonical specification: what an agent cannot guess. It deliberately
/// does not restate `--help` — the agent can read that itself, and a copy
/// would be a second place to keep in sync.
pub fn agent_spec(version: &str, statuses: &[String], lanes: &[String]) -> String {
    let status_line = if statuses.is_empty() {
        "(none configured)".to_owned()
    } else {
        statuses.join(" · ")
    };
    let lane_line = if lanes.is_empty() {
        "(none registered — lanes are free-form; any name works)".to_owned()
    } else {
        lanes.join(" · ")
    };
    format!(
        r##"# Working in a DIT workspace

_Generated by DIT {version}. Regenerate with `dit ai init`; print the current one with `dit ai spec`._

DIT is project management where the source of truth is Markdown files inside this git
repository. SQLite is only a disposable index, rebuilt from git at any time. Every issue
is a file; every change is a commit; history is `git log`.

## Rules that carry consequences

1. **Never edit an issue file directly.** No `sed`, no text editor, no `Write`. Writes go
   through `dit issue set` / `dit issue new` / `dit claim`, which format the file the one
   way the merge driver understands. A hand-edited file loses its edits at the next merge.
2. **Never store a fact that can be computed.** Related commits, activity history,
   time-in-status, readiness, a flow's stage, whether something is blocked — all of these
   are derived at read time. Writing one into a file is rejected by the test suite.
3. **A merge conflict is a state, not a failure.** `dit sync` reporting a conflict has
   worked correctly. Resolve the file, do not retry the sync.
4. **Fields you do not recognise are preserved, not dropped.** If you are reading a file
   written by a newer DIT, leave what you do not understand exactly as it is.
5. **No field ever names something to be executed or fetched.** A DIT file that could make
   a checkout do work is remote code execution by pull request, and the schema forbids it.

## The data model

An issue's frontmatter carries: `id`, `number`, `title`, `type`, `status`, `priority`,
`reporter`, `assignees`, `labels`, `epic`, `estimate`, `sprint`, `created`, `updated`,
`due`, `start`, `blocked_by`, `fed_by`, `lane`, `flows`, `claimed_by`, `claimed_at`.
Anything else is outside the vocabulary and will fail the invariant tests.

- **Statuses** in this workspace: {status_line}
- **Lanes** registered here: {lane_line}. A lane is a work stream — one actor per lane.
- **`blocked_by`** is the gating relation. It decides what is pickable, how a flow's
  stages are layered and where its critical path runs. Never borrow it to draw a picture.
- **`fed_by`** is the non-gating relation: "that feeds this". It draws an arrow and may
  carry a label. It affects nothing derived. Use it for results, outcomes and return paths.
- **`flows`** lists the orchestrations an issue belongs to. One issue may join several.
- **`labels`** is free-form except for prefixes DIT owns. `phase/<id>` states which phase
  of a flow diagram an issue sits in.

## Coordination between parallel actors

Set your identity once — `export DIT_ME=<alias>` — so claims, comments and commits are
attributed. Then:

- `dit ready --lane <your-lane>` lists what is pickable right now. Empty output means wait.
- `dit claim <issue>` takes exclusive intent before you edit; `--renew` during a long
  session, `--release` when you stop. A claim older than the TTL is takable by someone else.
- Move the issue as you go: `in_progress` before the first edit, `review` while a gate is
  pending, `done` only with evidence in a comment.
- Blocked on another actor? Comment on the blocker with what you expected, what you got,
  and the evidence. Reply in-thread when it lands. `dit inbox` shows what is waiting on you.
- Never edit an issue another actor has claimed without claiming it first.

## Shaping a flow diagram

A flow is a set of issues carrying the same name in `flows:`. Its diagram is derived:
stages from `blocked_by`, rows, readiness and the critical path all computed. The part
that cannot be derived — the names and order of the columns, the groups inside a lane,
the labels on the arrows — is authored in a `dit-flow` fence, in any document:

```dit-flow
flow: register
phases:
  - {{ id: intake, label: Intake }}
  - {{ id: build,  label: "Build + verify" }}
  - {{ id: ship,   label: Ship }}
groups:
  - {{ id: planning, label: "Planning loop", lane: backend, phases: [intake, build] }}
labels:
  - {{ from: "#515", to: "#497", text: "record result" }}
```

An issue joins a phase with a label: `dit issue set '#497' labels=auth,phase/build`.

The fence may not list members, restate status or dependencies, or name anything to be
executed or fetched. A fence that does not parse never costs anyone their diagram: the
flow falls back to computed stages and the screen says which document and line to fix.

## Finding your way

`dit --help` lists every command, and each subcommand explains its own flags. `dit doctor`
checks what silently breaks a workspace when wrong, including whether this document was
written by an older DIT than the one you are using.
"##
    )
}

/// True when a generated document was written by a different version than the
/// one asking — what `dit doctor` reports.
pub fn agent_doc_stamp(text: &str) -> Option<String> {
    let marker = "_Generated by DIT ";
    let start = text.find(marker)? + marker.len();
    // A version has dots in it, so the sentence's full stop is not a
    // delimiter: take the whole word and drop the punctuation it ends on.
    let word = text[start..].split_whitespace().next()?;
    let stamp = word.trim_end_matches('.');
    (!stamp.is_empty()).then(|| stamp.to_owned())
}

/// The tool files to touch, as repo-relative paths.
pub(crate) fn targets(root: &Path, opts: &AgentDocOptions) -> Vec<String> {
    TOOL_FILES
        .iter()
        .filter(|(key, _)| opts.only.is_empty() || opts.only.iter().any(|k| k == key))
        .filter(|(key, path)| {
            // AGENTS.md is the convention, so it is created rather than
            // waited for. Everything else is only pointed when the team
            // already uses it — DIT never litters a repo with files for
            // tools nobody here runs.
            opts.all || *key == "agents" || root.join(path).exists()
        })
        .map(|(_, path)| (*path).to_owned())
        .collect()
}

/// The canonical document, preserving anything the team wrote around the
/// generated block.
pub(crate) fn render_document(existing: &str, spec: &str) -> String {
    upsert_marked_block(existing, SPEC_START, SPEC_END, spec)
}

pub(crate) fn render_pointer(existing: &str) -> (String, bool) {
    // An older workspace carries the protocol inline; it is replaced, not
    // left beside the pointer to rot.
    let (base, had_legacy) = match remove_marked_block(existing, LEGACY_START, LEGACY_END) {
        Some(cleaned) => (cleaned, true),
        None => (existing.to_owned(), false),
    };
    (
        upsert_marked_block(&base, POINTER_START, POINTER_END, &pointer_body()),
        had_legacy,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_marked_block_leaves_everything_around_it_alone() {
        let before = "top\n\n<!-- a -->\nold\n<!-- /a -->\n\nbottom\n";
        let after = upsert_marked_block(before, "<!-- a -->", "<!-- /a -->", "new");
        assert!(after.starts_with("top\n"), "{after}");
        assert!(after.contains("new"), "{after}");
        assert!(!after.contains("old"), "{after}");
        assert!(after.ends_with("bottom\n"), "{after}");
    }

    #[test]
    fn a_missing_block_is_appended_without_eating_the_last_line() {
        let after = upsert_marked_block("hand written\n", "<!-- a -->", "<!-- /a -->", "body");
        assert!(after.starts_with("hand written\n\n"), "{after}");
        assert!(after.trim_end().ends_with("<!-- /a -->"), "{after}");
    }

    #[test]
    fn the_stamp_round_trips() {
        let doc = agent_spec("9.9.9", &[], &[]);
        assert_eq!(agent_doc_stamp(&doc).as_deref(), Some("9.9.9"));
    }
}
