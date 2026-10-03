---
id: 0030
title: "Choosing a folder from the browser: the system's own dialog, opened by the local server"
status: accepted
date: 2026-10-03
supersedes: null
---

## Context

ADR 0028 made "New workspace" a name: the folder is made under
`~/Documents/DIT`. People asked to choose where it goes, and to pick the folder
for "Add existing workspace" instead of typing a path. A web page cannot do
either on its own: `<input webkitdirectory>` hands the page file names, never
the folder's path on disk, and the path is the one thing DIT needs.

Two ways to give the page a folder:

| Option | Consequence |
|---|---|
| **The local server opens the operating system's folder dialog and returns the one path a person chose** | The familiar Finder dialog; the page learns one path, chosen by a human, and nothing else about the disk |
| An API listing folder names, drawn as a tree inside the page | The same look everywhere — and a new way for the page, or a script injected into it, to read the shape of the person's disk |

## Decision

**The hub opens the system's own folder dialog.**

- `POST /api/workspaces/choose-folder` — on the server that serves every
  workspace (ADR 0028), behind its token and host guards — opens the
  operating system's folder chooser on the machine the server runs on and
  answers `{ "path": "<chosen>" }`, or `{ "path": null }` when the person
  cancelled. On macOS that is `osascript`'s `choose folder`; on Linux,
  `zenity --file-selection --directory` when it is installed. Elsewhere, or
  without one, the answer is 404 and the page offers typing alone.
- **Only when the server is bound to this machine.** A server bound to a
  network address (`--host 0.0.0.0`, for a phone in a meeting) never opens a
  dialog: a request from another device would put a window on the screen of
  someone who did not ask for it. `GET /api/workspaces` says whether a
  chooser is available, so the button is shown only when it works.
- One dialog at a time; a second request while one is open is refused.
- **New workspace** gains a location: `~/Documents/DIT` unless a folder is
  chosen. The workspace is made as `<location>/<name>`, as before — the
  location must be an existing folder, and `<location>/<name>` must not already
  hold files.
- **Add existing workspace** gains the same button, filling the path. The
  rule of ADR 0028 stands: the browser adds only a folder that already is a
  DIT workspace.

Invariant check: I7 — the programs run are fixed (`osascript`, `zenity`) with
fixed arguments; nothing in a repository names them. I11 — no outbound
request. The page receives one path a person picked; the disk is never listed.

## Consequences

- The dialog belongs to the server process. With the menu bar app (ADR 0029)
  that is the person's own session, which is the case this is for.
- A `dit ui` started over SSH has no screen to show a dialog on; the chooser
  fails, the page says so, and typing still works.

## Verification

The hub suite pins the chooser's guards: refused on a server bound to a
network address, a cancelled dialog answering `null`, and New workspace
refusing a location that is not a folder. The dialog itself is verified by
hand on macOS.
