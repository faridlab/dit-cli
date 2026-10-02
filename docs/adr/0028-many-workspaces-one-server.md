---
id: 0028
title: "Many workspaces, one server: a per-machine registry, /w/<name>/, and a workspace every command can name"
status: accepted
date: 2026-10-03
supersedes: null
---

## Context

A DIT workspace is a git repository, and until now DIT has served exactly one:
`dit ui` opens the repository the terminal stands in, `dit-server` takes
`--workspace <path>`, and every CLI command acts on the current directory.
Someone with a work project and a personal one runs two servers on two ports
with two tokens, and finds their way back to each by `cd`.

Two kinds of people hit this wall:

- **People without a terminal.** The desktop app (Phase 3) starts one server
  from a tray icon. There is no directory to stand in, so "which workspace"
  has to be a choice inside DIT — a switcher at the top left, as DESIGN §5
  already drew it — and "a new workspace" has to be a name, not a path and
  `git init`.
- **AI agents.** An agent acting on DIT must know which workspace it acts on,
  and must be able to name one without changing directory — otherwise an
  agent working in a code repository writes issues into whatever workspace
  the shell happens to be in.

DESIGN §5 settled the shape in one line ("the server serves several
workspaces under the path `/w/<name>/`, read from `~/.config/dit/links.toml`.
Each workspace has its own index") but none of it was built, and the threats
it opens were never written down.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| **One server, many workspaces at `/w/<name>/`** | A hub router in front of today's per-workspace router; a registry | One port, one token, one process for the tray to start and stop; the switcher is a link |
| One server per workspace, a launcher in front | Process management in the tray; a token and an origin per workspace | The switcher hops between origins and loses the session each time |
| Keep one workspace per process | None | Non-technical users cannot have more than one project |

The registry's format: DESIGN names `links.toml`. TOML would be DIT's only
reason to carry a TOML parser; the YAML subset `dit-parse` already speaks
reads a two-key list just as well. The file is per-machine and never shared,
so its name and format are DIT's to choose.

## Decision

**One server serves every registered workspace at `/w/<name>/`.**

- **The registry** is `workspaces.yaml` in DIT's per-machine config directory
  (`$XDG_CONFIG_HOME/dit`, else `~/.config/dit`): a list of `{ name, path }`
  and a `default`. It is never committed anywhere. A name is a lowercase word
  (`acme`, `side-project`); a path is absolute.
- **The hub** answers `/w/<name>/…` by handing the request — with the prefix
  taken off — to that workspace's own router, the same one `dit ui` has always
  served, opened on first use with its own index and its own live updates.
  `/api/workspaces` lists, creates, adds, removes and picks the default. `/`
  goes to the default workspace, or to a first-run page when there is none.
  Static assets are shared. One token for the process, kept in the config
  directory; one port.
- **A new workspace is a name.** "New workspace" creates
  `~/Documents/DIT/<name>` (or `~/DIT/<name>` where there is no Documents
  folder), runs `dit init` in it and registers it. Nobody types a path.
- **The browser may add an existing folder only if it is already a DIT
  workspace** (it holds `.dit/config.yaml`). A plain git repository opens as
  a read-only code map (ADR 0025) — through the browser, that would let a
  script injected into the page register any repository on the disk and read
  its source over `/api/code`. Adding a repository that is not a workspace is
  a terminal action (`dit workspace add`), made by a person.
- **Removing a workspace takes it off the list.** No file is touched; the
  repository and its history are exactly where they were.
- **Every command can name its workspace.** `dit --workspace <name>` (or
  `-W`), else `DIT_WORKSPACE`, else the repository the current directory is
  in. `dit workspace list | current | new | add | remove | use` manage the
  registry; `current` prints the name and path an agent is acting on.
  `dit init` registers what it creates. `dit ui` inside a workspace registers
  it and opens `/w/<name>/`; anywhere else it opens the default.

Invariant check: the registry is per-machine configuration outside every
repository — written atomically like `.dit/morse.local.yaml`, never through a
transaction, never committed (I1 governs DIT files in a repository). I2 is
unchanged inside each workspace. I7: a registry path is a local directory the
person chose, not an address anything fetches. I11: the hub adds no egress.

## Consequences

- The tray app (Phase 3) starts one process and points at `/`; switching is a
  page navigation inside one origin and one session.
- Each open workspace holds its own SQLite index and watcher. Workspaces are
  opened on first use, not at start, so a long list costs nothing until used.
- Bookmarks of `/#/…` keep working for a single-workspace server; a hub
  redirects `/` to the default.
- `dit-server --workspace <path>` keeps its single-workspace meaning.
- An agent that runs `dit workspace current` first knows where its writes go;
  the agent guide says so.

## Verification

`dit-core` pins the registry: names are refused if they are not a word or are
taken, a removed workspace leaves its files, `new` creates and initialises the
folder, and resolution order is flag, then environment, then directory. The
server's suites pin `/w/<name>/api/…` reaching the right workspace, a token
required on the hub routes, and the browser's add refusing a folder that is
not a workspace.
