//! The `dit-flow` fence (ADR 0020): the authored shape of a flow diagram,
//! written in the YAML subset this crate already speaks.
//!
//! Two things happen here and nothing else. `fences` finds the blocks in a
//! document body without understanding Markdown — a fence is a run of lines
//! between two ``` markers, and that is all a scanner needs to know.
//! `parse_flow_shape` turns one block's bytes into a `FlowShape`, refusing
//! anything it cannot vouch for.
//!
//! This text arrives by pull request, so every refusal names the line. And
//! the grammar has no key that DIT would execute or fetch: a flow whose
//! steps *do* things is remote code execution by pull request (I7), so the
//! forbidden names are rejected here rather than merely left unimplemented.

use dit_model::{FlowArrowLabel, FlowGroup, FlowPhase, FlowShape};

use crate::yaml::{self, Yaml, YamlError};

/// The info string that marks a flow-shape block.
pub const FLOW_FENCE: &str = "dit-flow";

/// Key names a DIT file may never carry, at any depth. Not "unsupported" —
/// refused, so a fence can never grow into something a checkout executes.
const FORBIDDEN_KEYS: &[&str] = &[
    "run",
    "cmd",
    "command",
    "exec",
    "executable",
    "script",
    "shell",
    "url",
    "hook",
    "webhook",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FlowShapeError {
    #[error("line {line}: {0}", source)]
    Yaml { line: usize, source: YamlError },
    #[error("the fence has no `flow:` — a shape must say which flow it shapes")]
    NoFlow,
    #[error("`{0}` is not a key a DIT file may carry: a field that names something to run or fetch is remote code execution by pull request")]
    Forbidden(String),
    #[error("`phases:` must be a list of `{{ id, label }}` entries")]
    BadPhases,
    #[error("phase `{0}` is declared twice")]
    DuplicatePhase(String),
    #[error("`groups:` must be a list of `{{ id, label, lane, phases }}` entries")]
    BadGroups,
    #[error("group `{group}` spans phase `{phase}`, which this flow does not declare")]
    UnknownPhase { group: String, phase: String },
    #[error("`labels:` must be a list of `{{ from, to, text }}` entries")]
    BadLabels,
}

impl From<YamlError> for FlowShapeError {
    fn from(source: YamlError) -> Self {
        let line = match &source {
            YamlError::BadLine { line, .. }
            | YamlError::Unterminated { line }
            | YamlError::Indent { line, .. } => *line,
        };
        FlowShapeError::Yaml { line, source }
    }
}

/// One fence found in a document body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// The info string, trimmed — `dit-flow`, `mermaid`, `rust`, or empty.
    pub info: String,
    /// The bytes between the markers.
    pub body: String,
    /// 1-based line of the opening marker, so an error can be pointed at.
    pub line: usize,
}

/// Every fenced block in a document body, in order. Deliberately dumber than
/// a Markdown parser: an opening marker starts a block, the next marker with
/// at least as many backticks ends it, and nothing inside is interpreted.
pub fn fences(body: &str) -> Vec<Fence> {
    // A document ending in a newline yields a trailing empty line from the
    // split; it is an artifact of scanning, not something the author wrote.
    let joined = |lines: &[&str]| lines.join("\n").trim_end_matches('\n').to_owned();
    let mut out = Vec::new();
    let mut open: Option<(String, usize, usize, Vec<&str>)> = None;
    for (i, raw) in body.split('\n').enumerate() {
        let line = i + 1;
        let trimmed = raw.trim_start();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        match &mut open {
            None => {
                if ticks >= 3 {
                    let info = trimmed[ticks..].trim().to_owned();
                    open = Some((info, ticks, line, Vec::new()));
                }
            }
            Some((info, want, at, lines)) => {
                // A closing marker carries no info string; anything else is
                // content, even when it starts with backticks.
                if ticks >= *want && trimmed[ticks..].trim().is_empty() {
                    out.push(Fence {
                        info: info.clone(),
                        body: joined(lines),
                        line: *at,
                    });
                    open = None;
                } else {
                    lines.push(raw);
                }
            }
        }
    }
    // An unterminated fence is still a fence: the reader meant it, and a
    // missing back-tick should not make their phases vanish.
    if let Some((info, _, at, lines)) = open {
        out.push(Fence {
            info,
            body: joined(&lines),
            line: at,
        });
    }
    out
}

/// The flow-shape fences in a document, with the line each one starts on.
pub fn flow_fences(body: &str) -> Vec<Fence> {
    fences(body)
        .into_iter()
        .filter(|f| f.info == FLOW_FENCE)
        .collect()
}

pub fn parse_flow_shape(text: &str) -> Result<FlowShape, FlowShapeError> {
    let root = yaml::parse(text)?;
    refuse_forbidden(&root)?;

    let flow = root
        .get("flow")
        .and_then(Yaml::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(FlowShapeError::NoFlow)?
        .to_owned();

    let mut phases = Vec::new();
    if let Some(node) = root.get("phases") {
        let items = node.as_seq().ok_or(FlowShapeError::BadPhases)?;
        for item in items {
            let id = entry_str(item, "id").ok_or(FlowShapeError::BadPhases)?;
            if phases.iter().any(|p: &FlowPhase| p.id == id) {
                return Err(FlowShapeError::DuplicatePhase(id));
            }
            let label = entry_str(item, "label").unwrap_or_else(|| id.clone());
            phases.push(FlowPhase { id, label });
        }
    }

    let mut groups = Vec::new();
    if let Some(node) = root.get("groups") {
        let items = node.as_seq().ok_or(FlowShapeError::BadGroups)?;
        for item in items {
            let id = entry_str(item, "id").ok_or(FlowShapeError::BadGroups)?;
            let label = entry_str(item, "label").unwrap_or_else(|| id.clone());
            let lane = entry_str(item, "lane").filter(|l| !l.is_empty());
            let spans: Vec<String> = match item.get("phases") {
                Some(Yaml::Seq(items)) => items
                    .iter()
                    .filter_map(Yaml::as_str)
                    .map(str::to_owned)
                    .collect(),
                Some(Yaml::Str(one)) if !one.is_empty() => vec![one.clone()],
                _ => Vec::new(),
            };
            for phase in &spans {
                if !phases.iter().any(|p| &p.id == phase) {
                    return Err(FlowShapeError::UnknownPhase {
                        group: id.clone(),
                        phase: phase.clone(),
                    });
                }
            }
            groups.push(FlowGroup {
                id,
                label,
                lane,
                phases: spans,
            });
        }
    }

    let mut labels = Vec::new();
    if let Some(node) = root.get("labels") {
        let items = node.as_seq().ok_or(FlowShapeError::BadLabels)?;
        for item in items {
            let (Some(from), Some(to)) = (entry_str(item, "from"), entry_str(item, "to")) else {
                return Err(FlowShapeError::BadLabels);
            };
            let text = entry_str(item, "text").unwrap_or_default();
            labels.push(FlowArrowLabel { from, to, text });
        }
    }

    Ok(FlowShape {
        flow,
        phases,
        groups,
        labels,
    })
}

fn entry_str(node: &Yaml, key: &str) -> Option<String> {
    node.get(key)
        .and_then(Yaml::as_str)
        .map(str::trim)
        .map(str::to_owned)
}

fn refuse_forbidden(node: &Yaml) -> Result<(), FlowShapeError> {
    match node {
        Yaml::Map(entries) => {
            for (key, value) in entries {
                let lower = key.to_ascii_lowercase();
                if FORBIDDEN_KEYS.contains(&lower.as_str()) {
                    return Err(FlowShapeError::Forbidden(key.clone()));
                }
                refuse_forbidden(value)?;
            }
            Ok(())
        }
        Yaml::Seq(items) => items.iter().try_for_each(refuse_forbidden),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SHAPE: &str = r##"flow: register
phases:
  - { id: intake, label: Intake }
  - { id: build, label: "Build + verify" }
  - { id: ship, label: Ship }
groups:
  - { id: planning, label: "Planning loop", lane: backend, phases: [intake, build] }
labels:
  - { from: "#515", to: "#497", text: "record result" }
"##;

    #[test]
    fn a_shape_carries_its_phases_groups_and_arrow_labels() {
        let shape = parse_flow_shape(SHAPE).unwrap();
        assert_eq!(shape.flow, "register");
        assert_eq!(
            shape
                .phases
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["intake", "build", "ship"]
        );
        assert_eq!(shape.phases[1].label, "Build + verify");
        assert_eq!(shape.groups.len(), 1);
        assert_eq!(shape.groups[0].lane.as_deref(), Some("backend"));
        assert_eq!(shape.groups[0].phases, vec!["intake", "build"]);
        assert_eq!(shape.labels[0].text, "record result");
        assert_eq!(shape.labels[0].from, "#515");
    }

    #[test]
    fn a_shape_without_a_flow_is_refused_rather_than_guessed_at() {
        assert_eq!(
            parse_flow_shape("phases:\n  - { id: a }\n").unwrap_err(),
            FlowShapeError::NoFlow
        );
    }

    #[test]
    fn a_group_may_not_span_a_phase_the_flow_never_declared() {
        let text = "flow: r\nphases:\n  - { id: a }\ngroups:\n  - { id: g, phases: [b] }\n";
        assert_eq!(
            parse_flow_shape(text).unwrap_err(),
            FlowShapeError::UnknownPhase {
                group: "g".into(),
                phase: "b".into(),
            }
        );
    }

    #[test]
    fn a_duplicate_phase_is_named_rather_than_silently_collapsed() {
        let text = "flow: r\nphases:\n  - { id: a }\n  - { id: a }\n";
        assert_eq!(
            parse_flow_shape(text).unwrap_err(),
            FlowShapeError::DuplicatePhase("a".into())
        );
    }

    #[test]
    fn a_key_that_names_something_to_run_is_refused_at_any_depth() {
        for text in [
            "flow: r\nrun: rm -rf /\n",
            "flow: r\nphases:\n  - { id: a, command: curl evil.example }\n",
            "flow: r\ngroups:\n  - { id: g, webhook: \"http://evil.example\" }\n",
        ] {
            let err = parse_flow_shape(text).unwrap_err();
            assert!(
                matches!(err, FlowShapeError::Forbidden(_)),
                "{text} -> {err:?}"
            );
        }
    }

    #[test]
    fn a_broken_fence_names_the_line() {
        let err = parse_flow_shape("flow: r\n   oops\n").unwrap_err();
        assert!(matches!(err, FlowShapeError::Yaml { .. }), "{err:?}");
        assert!(err.to_string().contains("line"), "{err}");
    }

    #[test]
    fn fences_are_found_by_their_info_string_and_nothing_else_is_read() {
        let body = "intro\n\n```mermaid\ngraph TD\n```\n\ntext\n\n```dit-flow\nflow: r\n```\n";
        let all = fences(body);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].info, "mermaid");
        let flow = flow_fences(body);
        assert_eq!(flow.len(), 1);
        assert_eq!(flow[0].body, "flow: r");
        assert_eq!(flow[0].line, 9, "the opening marker's line");
    }

    #[test]
    fn an_unterminated_fence_still_yields_what_the_author_wrote() {
        let flow = flow_fences("```dit-flow\nflow: r\n");
        assert_eq!(flow.len(), 1);
        assert_eq!(flow[0].body, "flow: r");
    }

    #[test]
    fn backticks_inside_a_fence_do_not_close_it() {
        let body = "````dit-flow\nflow: r\n```\nphases:\n  - { id: a }\n````\n";
        let flow = flow_fences(body);
        assert_eq!(flow.len(), 1);
        assert!(flow[0].body.contains("phases:"), "{:?}", flow[0].body);
    }
}
