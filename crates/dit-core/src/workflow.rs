//! The lane inbox (ADR 0015): the threads still waiting for a lane's own
//! answer, and the lane helpers the CLI reads. Everything here is computed
//! at read time from the index; nothing is stored.

use dit_index::IndexedIssue;
use dit_model::{Comment, IssueId};

use crate::{Dit, DitError};

impl Dit {
    /// Lanes in use across the data, registry order first, then any
    /// free-form lanes alphabetically, Unlaned last (ADR 0019). A lane
    /// "in use" is any non-empty `lane:` value on any issue.
    pub fn lane_counts(&self) -> Result<Vec<(String, usize)>, DitError> {
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for hit in self.query("", None)? {
            if let Some(lane) = hit.issue.lane.as_deref().filter(|l| !l.is_empty()) {
                *counts.entry(lane.to_owned()).or_default() += 1;
            }
        }
        let registry: Vec<String> = self.workflow.lanes.iter().map(|l| l.id.clone()).collect();
        let mut out: Vec<(String, usize)> = Vec::new();
        let mut remaining = counts.clone();
        for id in registry {
            if let Some(n) = remaining.remove(&id) {
                out.push((id, n));
            }
        }
        let mut rest: Vec<(String, usize)> = remaining.into_iter().collect();
        rest.sort();
        out.extend(rest);
        Ok(out)
    }

    /// The registry's label and owners for a lane, when it declares any.
    pub fn lane_meta(&self, lane: &str) -> Option<(String, Vec<String>)> {
        self.workflow
            .lane(lane)
            .map(|l| (l.label.clone(), l.owners.clone()))
    }

    /// The claim TTL this workspace runs with, in minutes.
    pub fn claim_ttl_minutes(&self) -> u32 {
        self.workflow.coordination.claim_ttl_minutes
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
