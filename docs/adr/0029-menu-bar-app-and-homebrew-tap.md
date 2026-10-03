---
id: 0029
title: "A menu bar app for people without a terminal: dit-tray runs `dit ui --all`, ships as DIT.app from our own Homebrew tap"
status: accepted
date: 2026-10-03
supersedes: null
---

## Context

ADR 0028 made one server serve every workspace on the machine, and gave the
browser a first-run page and a workspace switcher. What is still missing for
someone who has never opened a terminal is everything before the browser:
installing DIT, starting it, stopping it, and getting back to it. DESIGN §6.5
chose "local server + browser, not Tauri" and left one door open: "a thin
shell that runs `dit-server` as a sidecar and then points a webview at
localhost — global shortcuts, a tray icon, file associations, and a one-click
installer". This ADR takes the smallest version of that door: a tray icon,
no window.

Three facts were established by running them, on macOS 26.5 and Homebrew 7.0.7:

1. **Homebrew no longer has `--no-quarantine`**, and its official `homebrew/cask`
   tap rejects any cask whose app is not signed and notarized by Apple
   (`cask/audit.rb`: "The homebrew/cask tap requires all casks to be signed and
   notarized by Apple"). DIT has no Apple Developer account, so it cannot be in
   the official tap.
2. **An ad-hoc-signed app carrying the quarantine attribute is refused by
   Gatekeeper** (`gktool scan`: "not signed by a distributor that meets the
   system Gatekeeper requirements"). Opening it means System Settings →
   Privacy & Security → Open Anyway — a step a non-technical person should
   never be asked to find. The same app **without** the attribute opens.
   Files fetched with `curl` carry no quarantine attribute; files fetched by a
   browser or by Homebrew do.
3. **DIT needs `git` on the machine**: `dit-vcs` runs the `git` program. On a
   Mac without the Command Line Tools, `/usr/bin/git` is a stub whose first run
   pops Apple's installer. Installing Homebrew installs the Command Line Tools,
   so a Homebrew user already has git.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| **A Rust menu bar app (`dit-tray`) that starts `dit ui` as a child process** | One small delivery crate; two pinned pre-1.0 crates, macOS only | The server stays the one `dit ui` already is; a crash of either does not take the other down |
| The same app hosting the server in-process | No child process | A server panic kills the menu; the app links axum, SQLite and the whole stack twice over the CLI |
| A Swift menu bar app | No Rust GUI crates | A second language and toolchain in the repository for ~200 lines |
| Tauri window | Signing, notarization, three web engines (DESIGN §6.5) | Everything §6.5 removed comes back |

Distribution, with no Apple Developer account:

| Option | Consequence |
|---|---|
| **Our own tap (`faridlab/tap`), whose cask clears the quarantine attribute on DIT.app after installing it** | Free, works today, one command; the cask is ours to change when notarization arrives |
| Apple Developer account and notarization | The only way a `.dmg` downloaded in a browser opens with a double-click; $99 a year and a signing pipeline |
| A `.dmg` plus instructions for Open Anyway | Asks the people this is for to find a security setting |

## Decision

**`dit-tray`: a menu bar app with no window, macOS first.**

- A new delivery crate, `dit-tray`, building the `dit-tray` binary. It depends
  on `dit-core` only. On other platforms it builds and says the menu bar app is
  macOS-only for now; its GUI dependencies are macOS-only, so Linux and
  Windows CI never need GTK.
- It runs the server as a child process: the `dit` binary next to it in
  `DIT.app/Contents/MacOS/`, as `dit ui --all --port 7700`. `--all` is new: it
  serves every registered workspace whatever the current directory is (a home
  folder that happens to be a git repository must not turn the app into a
  code map). The app learns the address and token from the `open:` line
  `dit ui` already prints, so it never reads the token file itself.
- The menu: a status line (running at an address / stopped / stopping), **Open
  DIT**, the workspaces from the registry (each opens `/w/<name>/`), **Start
  DIT** / **Stop DIT**, **Open at Login**, and **Quit DIT**, which stops the
  server first. The icon is the DIT mark as a template image, drawn in code,
  dimmed while the server is stopped.
- **The server never outlives the app.** The app passes
  `--stop-with-parent`; the server watches its parent and stops, gracefully,
  once the app is gone — even when the app was killed rather than quit.
- **Stopping is graceful.** `dit ui` (and `dit-server`) now stop on SIGINT and
  SIGTERM by refusing new connections and letting requests in flight finish —
  a write is a commit, and a process killed mid-commit leaves
  `.git/index.lock` behind and every later write failing. Long-lived
  connections (live updates) get a few seconds, then the process exits. The
  app sends SIGTERM, and kills only if the server has not exited after that.
- **Open at Login** is a LaunchAgent, `~/Library/LaunchAgents/dev.dit.tray.plist`,
  written by `dit-core` through `dit-store::atomic` like the workspace list
  (per-machine configuration, never committed — I1 governs repository files).
- **Git check.** At start the app looks for git without running it: a `git` on
  `PATH` other than the `/usr/bin` stub, or `xcode-select -p` succeeding. Without
  one, the menu says so, offers **Install Git…** (which runs
  `xcode-select --install`, Apple's own dialog), and does not start the
  server until git is there. (Running `git --version` would itself pop the
  installer — and I3 keeps `git` invocations in `dit-vcs`.)

**Packaging: `DIT.app` from our own Homebrew tap.**

- The release workflow builds `DIT.app` on macOS: `dit-tray` and `dit` as
  universal binaries (`lipo` of aarch64 and x86_64), an `Info.plist` with
  `LSUIElement` (no Dock icon), an `.icns` made from the DIT mark, ad-hoc
  signed, zipped as `DIT-macos.zip` with a `.sha256`, next to the existing
  tarballs.
- The cask template lives in this repository (`packaging/homebrew/dit.rb`);
  the release workflow fills in the version and digest and attaches the
  result to the release, and it is copied to the tap repository
  `faridlab/homebrew-tap` — by hand until a token for that repository is
  configured. It installs `DIT.app`, links `dit` onto `PATH` (terminals and AI
  agents use the same binary the app runs), and in `preflight_steps` removes
  `com.apple.quarantine` from the staged `DIT.app` — after the digest check,
  before Homebrew moves the bundle to Applications (the move copies the
  bundle's attributes as they then are). Our own bundle, from our own tap,
  and nothing else. `uninstall` quits the app and removes the LaunchAgent;
  `zap` removes DIT's per-machine configuration and never touches
  workspaces.
- **When an Apple Developer account exists**, CI signs with a Developer ID and
  notarizes after the bundle is assembled, the quarantine step goes, and the
  cask can move to `homebrew/cask`. `scripts/build-macos-app.sh` signs ad hoc
  in one place, which is where the Developer ID signature replaces it.

Invariant check: I1 — the LaunchAgent is per-machine configuration written
atomically through `dit-store`, like the registry. I3 — the app never runs
`git`; it looks for one. I7 — nothing in a repository names a program the app
runs: the only programs are `dit` (its own sibling), `open`, `kill` and
`xcode-select`. I11 — the app adds no outbound request; the
server stays bound to 127.0.0.1 with its token.

## Consequences

- A person installs with one command (`brew install --cask faridlab/tap/dit`),
  then never needs the terminal again: the icon starts DIT, the first-run
  page makes a workspace by name.
- Clearing the quarantine attribute means Gatekeeper does not check DIT.app.
  Trust rests on the tap and on GitHub releases; the zip's `.sha256` is in the
  cask, so Homebrew refuses a tampered download. This is the cost of having
  no notarization, recorded here so it is removed the day notarization exists.
- Someone who downloads the zip in a browser still meets Open Anyway. The
  README points them at the tap.
- `tray-icon` and `tao` are pre-1.0 and pinned (DESIGN §9), confined to
  `dit-tray` on macOS.
- The server learning to stop gracefully helps every user of `dit ui`, not
  only the app: Ctrl-C no longer interrupts a commit.

## Verification

`dit-core` pins the LaunchAgent: written with the program path, removed, and
read back as on or off. The CLI suite pins `dit ui --all` serving the
workspace list from a directory that is not a workspace, and
`--stop-with-parent` freeing the port once its parent dies. The server suite
pins graceful stop: a request in flight when the signal arrives completes,
and a connection that never ends cannot hold the stop past the grace period.
`dit-tray`'s own tests pin reading the address from `open:`, stopping with
SIGTERM before killing, and finding git without running it.
The app itself is verified by hand on macOS: start, open, switch, stop
(no `index.lock` after stopping during a write), open at login, quit.
