//! The board view: workflow columns with their issues, computed at read time.

use dit_index::IndexedIssue;

/// A rendered board. Columns appear in workflow declaration order; issues are
/// ordered so the most urgent work is read first.
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    pub columns: Vec<BoardColumn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardColumn {
    pub status: String,
    pub label: String,
    pub wip_limit: Option<u32>,
    pub issues: Vec<IndexedIssue>,
}

impl BoardColumn {
    /// True when the column holds more than its declared limit. Computed
    /// here, never stored — a limit breach is a live fact about today, not a
    /// property of any issue.
    pub fn over_limit(&self) -> bool {
        self.wip_limit
            .is_some_and(|limit| self.issues.len() > limit as usize)
    }
}
