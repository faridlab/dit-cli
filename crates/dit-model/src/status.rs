use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusCategory {
    Todo,
    Doing,
    Done,
}

impl StatusCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            StatusCategory::Todo => "todo",
            StatusCategory::Doing => "doing",
            StatusCategory::Done => "done",
        }
    }
}

/// Workflow statuses are configured per project in `.dit/schema/workflow.yaml`,
/// so this is a thin wrapper rather than a fixed enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub id: String,
    pub category: StatusCategory,
    pub terminal: bool,
}
