//! The coordination board and inbox (ADR 0015): lanes as rows, workflow
//! statuses as columns, and the derived per-issue facts a parallel actor or
//! a watching human needs — readiness, per-blocker state, claim liveness.
//! Everything here is computed at read time from the index; nothing is
//! stored.

use dit_index::IndexedIssue;
use dit_model::{ClaimLiveness, Comment, IssueId, Priority, Readiness, StatusCategory};

use crate::{Dit, DitError};
use time::OffsetDateTime;

/// The whole board: one row per registered lane, Unlaned last.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowBoard {
    pub lanes: Vec<WorkflowLane>,
    pub claim_ttl_minutes: u32,
}

/// One swimlane row.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowLane {
    /// `None` = the Unlaned row every workspace has.
    pub id: Option<String>,
    pub label: String,
    pub owners: Vec<String>,
    pub cards: Vec<WorkflowCard>,
}

/// One card. `readiness`, `blockers` and `claim` are the derived facts; the
/// rest is the issue's authored state.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowCard {
    pub id: IssueId,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    pub status_label: String,
    pub category: Option<StatusCategory>,
    pub priority: Option<Priority>,
    pub readiness: Readiness,
    /// Every blocker with its derived state.
    pub blockers: Vec<BlockerState>,
    pub claim: Option<ClaimState>,
}

/// One blocker of a card, with where it stands against the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockerState {
    pub id: IssueId,
    pub short_ref: String,
    pub number: Option<u32>,
    pub title: String,
    pub status: String,
    /// Satisfied = through the gate; Unsatisfied = not yet; Broken =
    /// cancelled or gone — never auto-unblocking (ADR 0015).
    pub state: BlockerDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockerDisposition {
    Satisfied,
    Unsatisfied,
    Broken,
}

/// A claim as the board shows it: who holds it and whether it is still worth
/// respecting. The age is computed, never stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimState {
    pub claimed_by: String,
    pub claimed_at: String,
    pub stale: bool,
}

impl Dit {
    /// The coordination board (ADR 0015). Lanes come from the registry in
    /// declaration order; issues with no lane (or a lane id that is no
    /// longer registered) land in the trailing Unlaned row. Reads only.
    pub fn workflow_board(&self) -> Result<WorkflowBoard, DitError> {
        let all = self.query("", None)?;
        let by_id: std::collections::HashMap<IssueId, &IndexedIssue> =
            all.iter().map(|h| (h.issue.id, h)).collect();
        let gate = &self.workflow.coordination.readiness.gate;
        let satisfying = self.workflow.gate_statuses(gate);
        let exits = self.workflow.exit_statuses();
        let now = OffsetDateTime::now_utc();
        let ttl = self.workflow.coordination.claim_ttl_minutes;

        let mut lanes: Vec<WorkflowLane> = self
            .workflow
            .lanes
            .iter()
            .map(|l| WorkflowLane {
                id: Some(l.id.clone()),
                label: l.label.clone(),
                owners: l.owners.clone(),
                cards: Vec::new(),
            })
            .collect();
        lanes.push(WorkflowLane {
            id: None,
            label: "Unlaned".into(),
            owners: Vec::new(),
            cards: Vec::new(),
        });

        // Urgent first, matching the classic board's sort.
        let mut sorted = all.clone();
        sorted.sort_by_key(|h| h.issue.priority);

        for hit in sorted {
            let issue = &hit.issue;
            let mut blockers = Vec::new();
            let mut unsatisfied = Vec::new();
            let mut broken = Vec::new();
            for b in &issue.blocked_by {
                match by_id.get(b) {
                    Some(blocker) => {
                        let status = blocker.issue.status.as_str();
                        let state = if satisfying.contains(&status) {
                            BlockerDisposition::Satisfied
                        } else if exits.contains(&status) {
                            BlockerDisposition::Broken
                        } else {
                            BlockerDisposition::Unsatisfied
                        };
                        match state {
                            BlockerDisposition::Satisfied => {}
                            BlockerDisposition::Unsatisfied => unsatisfied.push(*b),
                            BlockerDisposition::Broken => broken.push(*b),
                        }
                        blockers.push(BlockerState {
                            id: *b,
                            short_ref: b.short_ref().as_str().to_owned(),
                            number: blocker.issue.number,
                            title: blocker.issue.title.clone(),
                            status: status.to_owned(),
                            state,
                        });
                    }
                    // Gone from the index: a deleted dependency is broken.
                    None => {
                        broken.push(*b);
                        blockers.push(BlockerState {
                            id: *b,
                            short_ref: b.short_ref().as_str().to_owned(),
                            number: None,
                            title: String::new(),
                            status: String::new(),
                            state: BlockerDisposition::Broken,
                        });
                    }
                }
            }
            let readiness = if unsatisfied.is_empty() && broken.is_empty() {
                Readiness::Ready
            } else {
                Readiness::Blocked {
                    unsatisfied,
                    broken,
                }
            };
            let claim = match (issue.claimed_by.as_deref(), issue.claimed_at.as_deref()) {
                (Some(by), Some(at)) => Some(ClaimState {
                    claimed_by: by.to_owned(),
                    claimed_at: at.to_owned(),
                    stale: matches!(
                        dit_model::claim_liveness(Some(by), Some(at), now, ttl),
                        ClaimLiveness::Stale
                    ),
                }),
                _ => None,
            };
            let card = WorkflowCard {
                id: issue.id,
                short_ref: issue.id.short_ref().as_str().to_owned(),
                number: issue.number,
                title: issue.title.clone(),
                status: issue.status.clone(),
                status_label: self
                    .workflow
                    .status(&issue.status)
                    .map(|s| s.label.clone())
                    .unwrap_or_else(|| issue.status.clone()),
                category: self.workflow.status(&issue.status).map(|s| s.category),
                priority: issue.priority,
                readiness,
                blockers,
                claim,
            };
            let row = lanes
                .iter()
                .position(|l| l.id.as_deref() == issue.lane.as_deref())
                .unwrap_or(lanes.len() - 1);
            lanes[row].cards.push(card);
        }
        Ok(WorkflowBoard {
            lanes,
            claim_ttl_minutes: ttl,
        })
    }

    /// The lane's inbox (ADR 0015, phase 2): threads on this lane's issues
    /// whose latest comment is not from the lane's own voice — the questions
    /// still waiting for an answer. The lane's voice is its registered
    /// `owners`; a lane without owners falls back to the issue's assignees
    /// plus its current claimant, so an inbox still works before anyone has
    /// curated the registry. Newest activity first.
    pub fn inbox(&self, lane: Option<&str>) -> Result<Vec<InboxItem>, DitError> {
        let mut items = Vec::new();
        for hit in self.query("", None)? {
            let issue = &hit.issue;
            if let Some(want) = lane {
                if issue.lane.as_deref() != Some(want) {
                    continue;
                }
            }
            let comments = self.comments(&issue.id)?;
            if comments.is_empty() {
                continue;
            }
            let voice = self.voice_of(issue);
            for (root, thread) in threads(&comments) {
                // The thread's pulse is its newest comment, by `created`
                // then id — the same order the index lists them in.
                let last = thread.last().unwrap_or(&root);
                if voice.contains(last.author.as_str()) {
                    continue;
                }
                items.push(InboxItem {
                    issue: hit.clone(),
                    root: root.clone(),
                    last_author: last.author.clone(),
                    last_at: last.created.clone(),
                    replies: thread.len().saturating_sub(1),
                });
            }
        }
        items.sort_by(|a, b| {
            b.last_at
                .cmp(&a.last_at)
                .then_with(|| b.root.id.cmp(&a.root.id))
        });
        Ok(items)
    }

    /// Who speaks for this issue's lane: the registered owners, or — before
    /// anyone registered any — the issue's assignees plus its claimant.
    fn voice_of(&self, issue: &dit_model::Issue) -> std::collections::BTreeSet<String> {
        let mut voice: std::collections::BTreeSet<String> = self
            .workflow
            .lane(issue.lane.as_deref().unwrap_or(""))
            .map(|l| l.owners.iter().cloned().collect())
            .unwrap_or_default();
        if voice.is_empty() {
            voice.extend(issue.assignees.iter().cloned());
            if let Some(claimant) = &issue.claimed_by {
                voice.insert(claimant.clone());
            }
        }
        voice
    }
}

/// One unanswered thread: the root comment, who spoke last and when, and how
/// many replies the thread carries.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxItem {
    pub issue: IndexedIssue,
    pub root: Comment,
    pub last_author: String,
    /// RFC3339 — for sorting and display only, never ordering semantics.
    pub last_at: String,
    pub replies: usize,
}

/// Group comments into threads. Returns (root, whole thread including the
/// root, in index order). A reply whose parent is missing from the set
/// becomes its own root — an orphan is still a conversation, not noise to
/// drop.
fn threads(comments: &[Comment]) -> Vec<(Comment, Vec<Comment>)> {
    fn root_of(
        comments: &[Comment],
        c: &Comment,
        by_id: &std::collections::HashMap<IssueId, usize>,
    ) -> IssueId {
        let mut current = c.id;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 64 {
                // A reply cycle cannot happen through the write path (a reply
                // must target an existing comment), but a hand-edited file
                // could; break it by treating where we stand as the root.
                return current;
            }
            match by_id.get(&current) {
                Some(&ix) => match comments[ix].reply_to {
                    Some(parent) if by_id.contains_key(&parent) => current = parent,
                    _ => return current,
                },
                None => return current,
            }
        }
    }

    let by_id: std::collections::HashMap<IssueId, usize> = comments
        .iter()
        .enumerate()
        .map(|(ix, c)| (c.id, ix))
        .collect();
    let mut order: Vec<IssueId> = Vec::new();
    let mut grouped: std::collections::HashMap<IssueId, Vec<Comment>> =
        std::collections::HashMap::new();
    for c in comments {
        let root = if c.reply_to.is_some() {
            root_of(comments, c, &by_id)
        } else {
            c.id
        };
        if !grouped.contains_key(&root) {
            order.push(root);
        }
        grouped.entry(root).or_default().push(c.clone());
    }
    order
        .into_iter()
        .filter_map(|root_id| {
            let thread = grouped.remove(&root_id)?;
            let root = comments[*by_id.get(&root_id)?].clone();
            Some((root, thread))
        })
        .collect()
}
