//! The live-update chain (ADR 0017) at the server layer: a write made by
//! ANOTHER process (each parallel actor's CLI) must refresh this server's
//! index and reach the broadcast channel the WebSocket forwards — one frame
//! per external commit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Duration;

use dit_core::{Dit, IssueDraft, IssueKind};

const TOKEN: &str = "test-token";

fn draft(title: &str) -> IssueDraft {
    IssueDraft {
        title: title.into(),
        kind: IssueKind::Task,
        status: None,
        priority: None,
        reporter: None,
        assignees: vec![],
        labels: vec![],
        epic: None,
        estimate: None,
        sprint: None,
        due: None,
        start: None,
        blocked_by: vec![],
        fed_by: vec![],
        lane: Some("backend".into()),
        flows: Vec::new(),
        number: None,
        body: String::new(),
    }
}

#[tokio::test]
async fn external_commits_reach_the_broadcast_channel_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut dit = Dit::init(tmp.path(), &std::env::current_exe().unwrap()).unwrap();
    // An issue in the current month shard, so the watcher has a directory to
    // watch from the start.
    {
        let mut tx = dit.transaction("tester").unwrap();
        let id = tx.create_issue(draft("Watched")).unwrap();
        tx.commit("create").unwrap();
        let _ = id;
    }
    let state = dit_server::AppState::new(dit, "tester", TOKEN);
    state.start_live_updates();
    let mut rx = state.subscribe();

    // An external process writes and commits behind this server's back: a
    // second Dit instance on the same workspace. Its commit moves HEAD
    // without moving this server's watermark — exactly the two-process
    // situation the watcher exists for.
    {
        let mut other = Dit::open(tmp.path()).unwrap();
        let mut tx = other.transaction("other-actor").unwrap();
        let id = tx.create_issue(draft("Written elsewhere")).unwrap();
        tx.set_fields(
            &id,
            dit_core::FieldPatch {
                status: Some("in_progress".into()),
                ..Default::default()
            },
        )
        .unwrap();
        tx.commit("external create").unwrap();
    }

    let frame = tokio::time::timeout(Duration::from_secs(15), rx.recv()).await;
    assert!(
        frame.is_ok(),
        "an external commit must refresh the index and announce"
    );
    assert_eq!(frame.unwrap().unwrap(), dit_server::state::INDEX_UPDATED);

    // Exactly one frame for that commit, within a generous quiet window.
    let second = tokio::time::timeout(Duration::from_millis(1500), rx.recv()).await;
    assert!(second.is_err(), "one commit, one announce");

    // And the server's own index really did absorb the external change.
    {
        let dit = state.dit.lock().unwrap();
        let hits = dit.query("status = in_progress", None).unwrap();
        assert_eq!(
            hits.len(),
            1,
            "the watcher refreshed the state index: {:?}",
            dit.query("", None)
                .unwrap()
                .iter()
                .map(|h| h.issue.title.clone())
                .collect::<Vec<_>>()
        );
    }
}
