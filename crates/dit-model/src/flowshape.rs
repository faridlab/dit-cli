//! The authored half of a flow diagram (ADR 0020): the part no derivation
//! can produce — what the columns are called and in what order they run,
//! which small groups sit inside a lane, and what the arrows say.
//!
//! Everything else about a flow stays derived: stages when there is no
//! shape, rows, readiness, the critical path. This module is pure data; the
//! grammar that produces it lives in `dit-parse`, and the index stores it.

/// The label namespace DIT owns for phase membership. An issue states its
/// phase with `labels: [phase/<id>]` rather than a new field, because
/// `labels` merges as a set union: two branches that disagree produce two
/// visible labels instead of one edit silently winning.
pub const PHASE_LABEL_PREFIX: &str = "phase/";

/// The phase id a label states, if it states one.
pub fn phase_of_label(label: &str) -> Option<&str> {
    label
        .strip_prefix(PHASE_LABEL_PREFIX)
        .filter(|id| !id.is_empty())
}

/// Every phase an issue claims, in the order its labels are written. More
/// than one is legal — a merge of two branches produces exactly that — and
/// the screen shows it rather than hiding it.
pub fn phases_of(labels: &[String]) -> Vec<&str> {
    labels.iter().filter_map(|l| phase_of_label(l)).collect()
}

/// One flow's authored shape.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlowShape {
    /// The flow this shape belongs to. The fence names it, so the document
    /// holding it may live anywhere.
    pub flow: String,
    /// The columns, left to right.
    pub phases: Vec<FlowPhase>,
    /// Frames inside a lane, one level deep.
    pub groups: Vec<FlowGroup>,
    /// What the arrows say.
    pub labels: Vec<FlowArrowLabel>,
}

impl FlowShape {
    pub fn phase_index(&self, id: &str) -> Option<usize> {
        self.phases.iter().position(|p| p.id == id)
    }

    /// The column an issue draws in, given the phases its labels claim.
    /// Several claims resolve to the earliest, so a node still draws once.
    pub fn column_of(&self, labels: &[String]) -> Option<usize> {
        phases_of(labels)
            .iter()
            .filter_map(|id| self.phase_index(id))
            .min()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowPhase {
    pub id: String,
    pub label: String,
}

/// A frame around some of one lane's nodes, spanning a run of phases.
/// Exactly one level deep and scoped to one lane: a frame drawn around a
/// scattered set is a frame that includes nodes it does not mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowGroup {
    pub id: String,
    pub label: String,
    /// `None` means the unlaned band.
    pub lane: Option<String>,
    /// Phase ids the frame covers, in shape order.
    pub phases: Vec<String>,
}

/// A caption on the arrow between two issues. The two ends are written as
/// people write them — `#12` or a short ref — and resolved at read time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowArrowLabel {
    pub from: String,
    pub to: String,
    pub text: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn only_the_reserved_prefix_states_a_phase() {
        assert_eq!(phase_of_label("phase/build"), Some("build"));
        assert_eq!(phase_of_label("phase/"), None, "an empty id states nothing");
        assert_eq!(phase_of_label("phases/build"), None);
        assert_eq!(phase_of_label("build"), None);
    }

    #[test]
    fn two_phase_labels_resolve_to_the_earliest_column() {
        let shape = FlowShape {
            flow: "register".into(),
            phases: vec![
                FlowPhase {
                    id: "intake".into(),
                    label: "Intake".into(),
                },
                FlowPhase {
                    id: "ship".into(),
                    label: "Ship".into(),
                },
            ],
            ..Default::default()
        };
        let labels = vec!["phase/ship".to_owned(), "phase/intake".to_owned()];
        assert_eq!(shape.column_of(&labels), Some(0), "earliest wins, once");
        assert_eq!(shape.column_of(&["auth".to_owned()]), None);
    }
}
