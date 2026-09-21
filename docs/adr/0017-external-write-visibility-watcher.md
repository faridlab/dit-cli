---
id: 0017
title: "External-write visibility: a file watcher in dit-core feeds the existing event channel"
status: accepted
date: 2026-09-21
supersedes: null
---

## Context

The live-update channel exists: `dit-server` holds a tokio broadcast,
`AppState::announce()` fires it after every write that went through the
server process, and the browser refetches on the WebSocket frame
`{"type":"index_updated"}`. But the whole point of a multi-actor workspace
(ADR 0015) is that writes also arrive from *other* processes — a `dit` CLI
run by each lane. Those writes announce nothing: the channel is in-process
only, so a browser watching the board goes stale the moment a second actor
exists.

DESIGN §16.2 specifies `Dit::subscribe()` "from the file watcher & our own
writes", and §6.3 specifies the watcher's survival rules (watch the tree
root and `.dit/` non-recursively plus the current month shard; degrade to
polling on `Error::MaxFilesWatch`; no feedback loop on DIT's own git
operations). Neither exists; `notify` is not a dependency of anything today.

The constraint: crate edges are enforced by `tests/architecture.rs`, and the
watcher's single job — "HEAD moved → `reindex(State)` → signal" — is a
facade operation. dit-store/dit-index cannot call upward; duplicating it in
each delivery crate multiplies the most stateful code in the design.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| UI polls REST on a timer | None server-side | Refresh lag and idle request noise; "auto-refresh on any write" degrades to "auto-refresh sometimes" |
| A second transport (SSE) next to the WebSocket | Two event paths to keep in sync | The WS endpoint and its client hook already work; a parallel channel doubles the surface for the same frame |
| Watcher inside `dit-server` only | Duplicated the day the CLI (or any other delivery) needs events | The watcher is facade logic; parking it in one delivery crate bakes in the duplication |
| **`notify` watcher in a new `dit-core::watch` module, bridged to the existing broadcast** | One new pinned dependency (`notify`, dit-core only — not in the wasm set) | Zero new crate edges; one watcher; the server's announce path stays the single fan-out |

## Decision

**`dit_core::watch::spawn(dit: Arc<Mutex<Dit>>) -> Receiver<()>` is the
realization of §16.2's `subscribe()`** (a `&self` method returning a
receiver off a watcher thread would be self-referential; `Dit` is not
Arc-shared by design, and the server already holds it behind exactly this
mutex — §16.4).

Event flow, CLI write in another process → UI:

```
CLI commits + absorbs into its own process's index
  → notify event lands in the server process's watcher
  → 300 ms debounce collapses the burst
  → lock the Dit mutex; repo.head() vs the state watermark
  → moved: refresh_state() (reindex State from HEAD — blobs, never the
    working tree) + send () on the channel
  → the server's bridge task calls AppState::announce()
  → WS frame → clients refetch
```

Dedupe and safety, each answering a §6.3 rule:

- **The gate is the watcher's own in-memory seen-head, not the index
  watermark.** `.dit-cache/index.sqlite` is shared between processes: a CLI
  in another process absorbs its own commit into that shared index, so a
  watermark gate finds head == watermark at the watcher and stays silent
  exactly when a browser is waiting for a frame — found live in the pilot,
  fixed by tracking what this watcher has already signalled. When HEAD
  moved, the watcher reindexes from git blobs only if the shared index is
  stale (a raw `git commit` never absorbs; any facade writer already did)
  and signals once per debounced burst.
- **Own writes may add one watcher frame** on top of the write path's
  immediate announce — a refetch hint, never a loop: frames cause no
  writes. The cost is one extra fetch per own-write burst.
- **Mid-transaction wakeups are benign**: a commit is atomic, so HEAD is
  either the old or the new value, and reindex reads `HEAD:` blobs, never
  half-written working files.
- **Watcher errors never kill the feature**: any `notify` error — including
  `MaxFilesWatch` — degrades to a 2 s HEAD-polling loop doing the same
  seen-head check. Reindex errors are logged and skipped; the index is
  disposable (Principle 2) and the next event retries.
- **Startup closes the old gap**: `dit-server` and `dit ui` call the new
  `Dit::refresh_state()` once before binding — today neither reindexes at
  startup unless `sync` pulled something, so a stale or version-bumped index
  stayed stale until the first write.

## Consequences

- `notify` becomes a dependency of `dit-core`, pinned, recorded in §6.3
  (dit-core is not in the wasm32 check set; the pure crates stay I/O-free).
- Announce rate is bounded structurally: at most one frame per commit batch
  per debounce window, zero for in-process writes. The broadcast's capacity
  of 16 keeps breathing room; lagged clients already recover by refetching
  on the next frame.
- The watcher never writes, never locks for longer than a reindex, and is
  read-only with respect to the source of truth — it cannot corrupt
  anything, at worst it announces late.

## Verification

`dit-core` tests with real tempdir workspaces pin the three behaviors: a
burst of external `git commit`s produces exactly one channel signal; a write
through the facade in the watcher's own process produces none (watermark
already moved); a forced reindex error leaves the watcher thread alive and
polling. The inotify-scale limits remain governed by the §6.3 rules the
watcher implements.
