//! Agent onboarding (ADR 0021): one canonical document describing how this
//! workspace expects to be worked in, plus a four-line pointer in each agent
//! file the team actually uses.
//!
//! The specification is generated from this binary, never parsed back, and
//! stamped with the version that produced it — a workspace pinned to a copied
//! spec freezes at the day it was copied and nothing notices. `dit doctor`
//! reads the stamp and says when the binary has moved on.
//!
//! Nothing here is executed or fetched by DIT. The document tells a human or
//! an agent what to run; DIT never runs it (I7).

use std::path::Path;

/// Where the canonical document lives: under `docs/`, so it is visible in the
/// tree, reviewed in pull requests, and reachable from DIT's own Docs screen.
pub const AGENT_DOC_PATH: &str = "docs/dit-for-agents.md";

const SPEC_START: &str = "<!-- dit:agent-spec -->";
const SPEC_END: &str = "<!-- /dit:agent-spec -->";
const POINTER_START: &str = "<!-- dit:agent-pointer -->";
const POINTER_END: &str = "<!-- /dit:agent-pointer -->";
/// The block ADR 0021 absorbs. Recognised so an older workspace upgrades
/// instead of carrying two overlapping sections.
const LEGACY_START: &str = "<!-- dit:workflow-protocol -->";
const LEGACY_END: &str = "<!-- /dit:workflow-protocol -->";

/// Agent files DIT knows how to point. `AGENTS.md` is written even when
/// absent — it is the cross-tool convention, and it is the one file worth
/// creating. The rest are only touched when the team already uses them, or
/// when the caller names them.
const TOOL_FILES: &[(&str, &str)] = &[
    ("agents", "AGENTS.md"),
    ("claude", "CLAUDE.md"),
    ("cursor", ".cursor/rules"),
    ("copilot", ".github/copilot-instructions.md"),
];

/// The tool keys `--only` accepts, for naming them back in a refusal.
pub fn tool_keys() -> Vec<&'static str> {
    TOOL_FILES.iter().map(|(key, _)| *key).collect()
}

/// Tool keys the caller named that DIT does not know. A silent no-op here
/// would answer a request with nothing, which is the one outcome that leaves
/// someone unable to tell whether it worked.
pub(crate) fn unknown_tools(opts: &AgentDocOptions) -> Vec<String> {
    opts.only
        .iter()
        .filter(|k| !TOOL_FILES.iter().any(|(key, _)| *key == k.as_str()))
        .cloned()
        .collect()
}

/// Which agent files to point at the canonical document.
#[derive(Debug, Clone, Default)]
pub struct AgentDocOptions {
    /// Create every known agent file, including ones this repo does not have.
    pub all: bool,
    /// Restrict to these tool keys (`agents`, `claude`, `cursor`, `copilot`).
    /// Empty means "the default set".
    pub only: Vec<String>,
}

/// What a run changed. `changed` is false on a second, identical run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDocReport {
    pub changed: bool,
    /// The canonical document was created or rewritten.
    pub document_written: bool,
    /// Repo-relative paths that now carry a pointer block.
    pub pointers: Vec<String>,
    /// Files whose legacy `dit:workflow-protocol` block was replaced.
    pub legacy_replaced: Vec<String>,
}

/// Replace the text between two markers, or append a fresh block when they
/// are absent. Everything outside the markers survives byte-for-byte — that
/// is the whole contract, and it is why a team's hand-written rules can live
/// in the same file as generated ones.
pub(crate) fn upsert_marked_block(existing: &str, start: &str, end: &str, body: &str) -> String {
    let block = format!("{start}\n{body}\n{end}");
    match (existing.find(start), existing.find(end)) {
        (Some(a), Some(b)) if b > a => {
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..a]);
            out.push_str(&block);
            out.push_str(&existing[b + end.len()..]);
            out
        }
        _ => {
            let mut out = existing.to_owned();
            if !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(&block);
            out.push('\n');
            out
        }
    }
}

/// Remove a marked block entirely, including the blank line that followed it.
fn remove_marked_block(existing: &str, start: &str, end: &str) -> Option<String> {
    let (a, b) = (existing.find(start)?, existing.find(end)?);
    if b < a {
        return None;
    }
    let mut out = String::with_capacity(existing.len());
    out.push_str(&existing[..a]);
    out.push_str(existing[b + end.len()..].trim_start_matches('\n'));
    Some(out)
}

/// The pointer every agent file gets: short enough that it costs an agent
/// nothing to read, specific enough that it knows why to follow it.
fn pointer_body() -> String {
    format!(
        "## This repository is a DIT workspace\n\
         \n\
         Project data lives in Markdown files under git, not in a database, and it has rules\n\
         that are enforced. Read [`{AGENT_DOC_PATH}`]({AGENT_DOC_PATH}) before touching\n\
         anything here; `dit ai spec` prints it for the DIT you run, and `dit ai spec <topic>`\n\
         goes deeper on one subject. The rules that cost someone else their work:\n\
         \n\
         - **Never edit an issue file by hand** — write through `dit issue new` / `dit issue set`.\n\
         - **Pick with `dit ready --lane <lane>`, then `dit claim <issue>` before the first edit.**\n\
         - **Move the status as you work:** `in_progress`, `review` while a gate is pending,\n\
         \x20 `done` only with evidence in a comment.\n\
         - **Before building on an endpoint, run `dit morse check`** — it says where the seam\n\
         \x20 was proven, and an environment it does not list was never proven there."
    )
}

/// A file carrying a repository's own rules for agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFile {
    /// The `repos:` entry it lives in; `None` for this workspace.
    pub repo: Option<String>,
    pub path: String,
}

/// What the spec says about this particular workspace beyond its workflow:
/// where the rules are, which API specs are registered, whether readiness
/// asks for proof. Gathered by the facade; formatted here, purely.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentContext {
    pub rule_files: Vec<RuleFile>,
    pub specs: Vec<String>,
    pub proof_required: bool,
    /// Registered code roots, one line each: id, repo, what it covers.
    pub code_roots: Vec<String>,
}

/// The file names an agent reads as a repository's rules.
pub const RULE_FILE_NAMES: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];

fn rules_section(files: &[RuleFile]) -> String {
    if files.is_empty() {
        return "No `CLAUDE.md` or `AGENTS.md` was found in this workspace or in the repositories \
                it links under `repos:`. The code is then the only statement of its conventions: \
                read the nearest existing module before writing a new one, and match it."
            .to_owned();
    }
    let mut out = String::from(
        "Each file below is a repository's own rules for agents. Read the ones that govern the \
         code you are about to change **before** changing it — the root file of that repository \
         and every file on the path down to the directory you work in. When two disagree, the \
         nearest one wins: a file in `apps/web/` overrides the repository root, and a linked \
         repository's own rules override this workspace's for code in that repository.\n\n",
    );
    let mut here: Vec<&RuleFile> = files.iter().filter(|f| f.repo.is_none()).collect();
    here.sort_by(|a, b| a.path.cmp(&b.path));
    for f in here {
        out.push_str(&format!("- `{}`\n", f.path));
    }
    let mut linked: Vec<&RuleFile> = files.iter().filter(|f| f.repo.is_some()).collect();
    linked.sort_by(|a, b| (&a.repo, &a.path).cmp(&(&b.repo, &b.path)));
    for f in linked {
        out.push_str(&format!(
            "- {}: `{}`\n",
            f.repo.as_deref().unwrap_or_default(),
            f.path
        ));
    }
    out.push_str(
        "\nThe list is taken when this document is generated; `dit ai init` refreshes it after \
         a rules file is added or moved.",
    );
    out
}

/// The canonical specification: what an agent cannot guess. It deliberately
/// does not restate `--help` — the agent can read that itself, and a copy
/// would be a second place to keep in sync.
pub fn agent_spec(
    version: &str,
    statuses: &[String],
    lanes: &[String],
    context: &AgentContext,
) -> String {
    let rules = rules_section(&context.rule_files);
    let spec_line = if context.specs.is_empty() {
        "none registered yet — add one under `specs:` in `.dit/config.yaml`".to_owned()
    } else {
        context.specs.join(" · ")
    };
    let proof_line = if context.proof_required {
        "**This workspace requires proof** (`proof: required` under `coordination.readiness`): \
         an issue whose blockers are done stays out of `dit ready` until every scenario in its \
         `needs_scenarios` holds for its `env`. `dit ready` lists what is held and why."
    } else {
        "**This workspace does not require proof** yet: readiness follows `blocked_by` alone. \
         Naming seams still pays — `dit morse check` reports them — and a workflow turns the gate \
         on with `proof: required` under `coordination.readiness`."
    };
    let code_line = if context.code_roots.is_empty() {
        "none registered yet — add one under `code:` in `.dit/config.yaml`, e.g. \
         `- { id: web, include: [\"src/**\"], generated: [\"src/generated/**\"] }`"
            .to_owned()
    } else {
        context.code_roots.join(" · ")
    };
    let status_line = if statuses.is_empty() {
        "(none configured)".to_owned()
    } else {
        statuses.join(" · ")
    };
    let lane_line = if lanes.is_empty() {
        "(none registered — lanes are free-form; any name works)".to_owned()
    } else {
        lanes.join(" · ")
    };
    format!(
        r##"# Working in a DIT workspace

_Generated by DIT {version}. Regenerate with `dit ai init`; print the current one with `dit ai spec`._

DIT is project management where the source of truth is Markdown files inside this git
repository. SQLite is only a disposable index, rebuilt from git at any time. Every issue
is a file; every change is a commit; history is `git log`.

## Rules that carry consequences

1. **Never edit an issue file directly.** No `sed`, no text editor, no `Write`. Writes go
   through `dit issue set` / `dit issue new` / `dit claim`, which format the file the one
   way the merge driver understands. A hand-edited file loses its edits at the next merge.
2. **Never store a fact that can be computed.** Related commits, activity history,
   time-in-status, readiness, a flow's stage, whether something is blocked — all of these
   are derived at read time. Writing one into a file is rejected by the test suite.
3. **A merge conflict is a state, not a failure.** `dit sync` reporting a conflict has
   worked correctly. Resolve the file, do not retry the sync.
4. **Fields you do not recognise are preserved, not dropped.** If you are reading a file
   written by a newer DIT, leave what you do not understand exactly as it is.
5. **No field ever names something to be executed or fetched.** A DIT file that could make
   a checkout do work is remote code execution by pull request, and the schema forbids it.

## The data model

An issue's frontmatter carries: `id`, `number`, `title`, `type`, `status`, `priority`,
`reporter`, `assignees`, `labels`, `epic`, `estimate`, `sprint`, `created`, `updated`,
`due`, `start`, `blocked_by`, `fed_by`, `lane`, `flows`, `claimed_by`, `claimed_at`,
`needs_scenarios`, `proves`, `env`. Anything else is outside the vocabulary and will fail
the invariant tests.

- **Statuses** in this workspace: {status_line}
- **Lanes** registered here: {lane_line}. A lane is a work stream — one actor per lane.
- **`blocked_by`** is the gating relation. It decides what is pickable, how a flow's
  stages are layered and where its critical path runs. Never borrow it to draw a picture.
- **`fed_by`** is the non-gating relation: "that feeds this". It draws an arrow and may
  carry a label. It affects nothing derived. Use it for results, outcomes and return paths.
- **`flows`** lists the orchestrations an issue belongs to. One issue may join several.
- **`labels`** is free-form except for prefixes DIT owns. `phase/<id>` states which phase
  of a flow diagram an issue sits in.
- **`needs_scenarios`**, **`proves`** and **`env`** name seams: the scenarios this issue
  needs proven before it can start, the ones it delivers, and the environment its lane
  works against. Names only — see *Seams* below.

## The rules of the code you are touching

{rules}

## Working alongside other sessions

Several sessions — human or AI — work one repository at the same time, one lane each.
The flow is where you find out what the others are doing before you start, and how you
tell them what you are doing without interrupting anybody.

Set your identity once — `export DIT_ME=<alias>` — so every claim, comment and commit is
attributed to you. Then work this loop:

1. **Look before you pick.** `dit flow show <flow>` prints the whole orchestration: every
   issue by phase and lane, whether it is `ready`, `blocked`, `in-flight` or `done`, a `*`
   on the critical path, and `[alias]` beside anything another session already holds —
   `[alias stale]` when their claim has expired and the work is takable again. That one
   command answers "who is doing what, and what is waiting on whom", without asking.
2. **Take something nobody holds.** `dit ready --lane <your-lane>` narrows it to what is
   pickable right now. Empty output means wait, not look harder.
3. **Claim it before the first edit.** `dit claim <issue>`, `--renew` if the session runs
   long, `--release` when you stop. Never edit an issue another session holds.
4. **Join the orchestration.** `dit issue set <issue> flows=<flow>` when it is not a
   member yet. Work outside the flow is invisible to everyone reading the flow.
5. **Move the status as you go**, so the others see it without asking: `in_progress`
   before the first edit, `review` while a gate is pending, `done` only with evidence in
   a comment.
6. **Declare the handoff.** When your work produces something another issue consumes, say
   so on the receiving issue: `dit issue set <theirs> fed_by=<yours>`. That draws the
   arrow and changes nothing about readiness, so it cannot block anyone by accident. Use
   `blocked_by` only when the other work genuinely cannot start until yours is through
   the gate — it decides what every other session is allowed to pick up.
7. **Say so when you are stuck.** Comment on the blocker with what you expected, what you
   got, and the evidence. `dit inbox` lists the threads waiting on your lane; answer them
   in-thread.

The loop exists so nobody has to be asked. Another session reads the same flow and sees
your claim, your status and your arrows — which is also why skipping steps 3 to 6 is not
a shortcut: it makes your work invisible to people who are deciding what to touch next.

## Seams: know what answers before you build on it

Most expensive surprises in parallel work are not in the code you read; they are between
lanes — an endpoint marked done that answers 404 on the environment you use, a filter the
server silently ignores, a migration that ran on one tenant and not the other. DIT keeps
those seams in the repository as **scenarios**: `dit-morse` fences in any document, each a
declarative chain of requests against a registered API spec, with no scripts.

- **Registered specs** here: {spec_line}.
- **Before building on an endpoint**, run `dit morse check`. It lists every scenario with
  its health against the spec (fresh, stale, broken) and, per environment, where it was
  **proven** green and whether that proof still holds. An environment that is not listed
  was never proven there — do not assume it works.
- **A proof is made by running it:** `dit morse sync <scenario> --env <name>` fires the
  chain against that environment and, only on green, records `proven.<name>` in the fence.
  Nothing else ever writes a proof — not reindex, not CI.
- **Name the seam on the issues.** The issue that delivers it gets `proves=<scenario>`; the
  issue that depends on it gets `needs_scenarios=<scenario>` and `env=<name>`:
  `dit issue set '#619' needs_scenarios=payslip-pdf env=local-hrperf`.
- {proof_line}
- **Found a seam that does not answer?** Write or extend the scenario, run it, and put the
  failing step's status and error code in a comment on the issue that owns the seam. The
  scenario then proves the fix when it lands.

## Reading the code

DIT keeps a map of the code derived from git: which file imports which, what each file
defines and calls, followed through path aliases and barrel re-exports. It is rebuilt from
HEAD on every `dit code` command, so it is never older than the last commit. Ask it before
grepping — one answer instead of a page of matches.

- **Code roots** here: {code_line}.
- `dit code users <file|symbol>` — who imports it; the blast radius of changing it.
- `dit code uses <file|symbol>` — what it imports and calls.
- `dit code explain <name>`, `dit code where <text>`, `dit code path <a> <b>`,
  `dit code hubs` — a node in full, a name search, the import chain between two, and the
  most depended-on files (generated ones left out).
- **The map of intent** — `dit-map` fences — says what the code cannot: where a task is
  done, the file to copy, the paths never to touch. `dit code check` prints every entry with
  its verdict. Trust an entry that **holds**; re-read one that is **stale** against its
  example before copying it; never follow one that is **broken**.

The map covers committed files only: a file you just wrote appears after it is committed.
`dit ai spec code` has the fence format and the confirm step.

## Recipes

The common tasks as the commands that do them, in order. `dit ai spec <topic>` goes deeper
on `issues`, `flow`, `morse` and `code`.

- **Report something:** `dit issue new "<title>"` prints `#<n>`; check it landed with
  `dit issue show '#<n>'`. Then shape it: `dit issue set '#<n>' labels=<a>,<b> lane=<lane>`.
- **Do the work:** `dit ready --lane <lane>` → `dit claim '#<n>'` →
  `dit issue set <issue> status=in_progress` → the change → `status=review` while a gate is
  pending → `dit issue comment '#<n>' "<evidence>"` → `status=done`.
- **Depend on someone:** `dit issue set <yours> blocked_by=<theirs>` when yours cannot start
  until theirs is through the gate; `fed_by=` when theirs only feeds yours.
- **Join an orchestration:** `dit issue set <issue> flows=<flow>`, then `dit flow show <flow>`.
- **Change a file safely:** `dit code users <file>` lists everything that breaks with it;
  `dit code check` says where the task is done and which file to copy.
- **Rely on an endpoint:** `dit morse check`; if it is not proven for your environment,
  `dit morse sync <scenario> --env <name>`, and name the seam on both issues.

## Shaping a flow diagram

A flow is a set of issues carrying the same name in `flows:`. Its diagram is derived:
stages from `blocked_by`, rows, readiness and the critical path all computed. The part
that cannot be derived — the names and order of the columns, the groups inside a lane,
the labels on the arrows — is authored in a `dit-flow` fence, in any document:

```dit-flow
flow: register
phases:
  - {{ id: intake, label: Intake }}
  - {{ id: build,  label: "Build + verify" }}
  - {{ id: ship,   label: Ship }}
groups:
  - {{ id: planning, label: "Planning loop", lane: backend, phases: [intake, build] }}
labels:
  - {{ from: "#515", to: "#497", text: "record result" }}
```

An issue joins a phase with a label: `dit issue set '#497' labels=auth,phase/build`.

The fence may not list members, restate status or dependencies, or name anything to be
executed or fetched. A fence that does not parse never costs anyone their diagram: the
flow falls back to computed stages and the screen says which document and line to fix.

## How DIT helps you

- `dit ai spec` prints this document for the DIT you are running; `dit doctor` says when the
  committed copy is older.
- `dit ready --lane <lane>` answers "what can I start" — and names what is held for proof.
- `dit flow show <flow>` answers "who is doing what, and what waits on whom".
- `dit inbox --lane <lane>` lists the threads waiting on you.
- `dit issue show <#n>` after `dit issue new` confirms the issue is indexed. `dit reindex`
  names any file it could not read, with the reason — a skipped file is a committed issue no
  command can find, so fix it rather than create a second one.
- `dit morse check` before building on an endpoint; `dit morse sync` to prove one.
- `dit code users <file>` before changing a file; `dit code check` for where a task is done.

## Finding your way

`dit --help` lists every command, and each subcommand explains its own flags.
`dit ai spec <topic>` goes deeper on one subject: `issues`, `flow`, `morse`, `code`. `dit doctor`
checks what silently breaks a workspace when wrong, including whether this document was
written by an older DIT than the one you are using.
"##
    )
}

/// The subjects `dit ai spec <topic>` goes deeper on.
pub const AGENT_TOPICS: [&str; 4] = ["issues", "flow", "morse", "code"];

/// One topic, or `None` for a name that is not one — the caller lists
/// [`AGENT_TOPICS`]. Longer than the main spec on purpose: an agent opens a
/// topic when it is about to do that thing, not before.
pub fn agent_topic(topic: &str, context: &AgentContext) -> Option<String> {
    match topic {
        "issues" => Some(TOPIC_ISSUES.to_owned()),
        "flow" => Some(TOPIC_FLOW.to_owned()),
        "morse" => {
            let specs = if context.specs.is_empty() {
                "none registered yet".to_owned()
            } else {
                context.specs.join(" · ")
            };
            Some(TOPIC_MORSE.replace("{specs}", &specs))
        }
        "code" => {
            let roots = if context.code_roots.is_empty() {
                "none registered yet".to_owned()
            } else {
                context.code_roots.join(" · ")
            };
            Some(TOPIC_CODE.replace("{roots}", &roots))
        }
        _ => None,
    }
}

const TOPIC_ISSUES: &str = r##"# Issues in depth

An issue is `issues/<yyyy>/<mm>/<id>-<slug>/README.md`: YAML frontmatter, then a Markdown
body. You never write that file — every change goes through a command, which formats it
the one way the merge driver can merge.

## Creating

    dit issue new "<title>" --kind bug --lane backend --label hr --flow <flow> --body "<text>"

It prints `#<n> <short-ref> <title>`. If it prints no `#<n>`, or `dit issue show '#<n>'`
cannot find it, run `dit reindex`: it names any file it could not read, with the reason.

## Changing

    dit issue set '#<n>' status=review priority=p1 labels=a,b lane=frontend
    dit issue set '#<n>' blocked_by='#12','#14' fed_by='#9' flows=release
    dit issue set '#<n>' needs_scenarios=payslip-pdf env=local-hrperf
    dit issue set '#<n>' proves=payslip-pdf
    dit issue set '#<n>' env=            # an empty value clears a single-valued field

A list field is replaced whole: `labels=a,b` sets exactly those two. A status must be one
of the workflow's; the transition rules are checked. `--force` exists for repairs, not for
routine work.

## The fields that coordinate

- `blocked_by` gates: the issue is not ready until each blocker reaches the workflow's
  gate. A cancelled blocker never satisfies it — someone must re-point the dependency.
- `fed_by` draws an arrow and changes nothing derived.
- `lane` is the work stream; `claimed_by` / `claimed_at` are written by `dit claim` only.
- `needs_scenarios` / `proves` / `env` name seams (`dit ai spec morse`). With
  `proof: required` in the workflow, an issue waits until each scenario it needs is proven
  for its `env`.

## Talking on an issue

    dit issue comment '#<n>' "what you expected, what you got, the evidence"

Comments are files too, merged the same way. `dit inbox --lane <lane>` lists threads
waiting on you.
"##;

const TOPIC_FLOW: &str = r##"# Flows in depth

A flow is every issue carrying the same name in `flows:`. Its board is derived: stages from
`blocked_by`, the critical path, readiness and who holds what. `dit flow show <flow>` prints
it; `[alias]` marks a live claim and `[alias stale]` one that has expired.

## The two relations

- `blocked_by` decides what is pickable and how stages layer. Use it only when the work
  genuinely cannot start before the other is through the gate.
- `fed_by` is "that feeds this": an arrow, optionally labelled, that blocks nothing. Use it
  for results, handoffs and return paths.

## The authored shape

Columns, lane groups and arrow labels cannot be derived, so they are written once in a
`dit-flow` fence in any document:

```dit-flow
flow: register
phases:
  - { id: intake, label: Intake }
  - { id: build,  label: "Build + verify" }
groups:
  - { id: loop, label: "Planning loop", lane: backend, phases: [intake, build] }
labels:
  - { from: "#515", to: "#497", text: "record result" }
```

An issue joins a phase by label: `dit issue set '#497' labels=phase/build`. The fence may
not list members or restate status or dependencies; a fence that fails to parse falls back
to computed stages and the board names the document and line to fix.
"##;

const TOPIC_CODE: &str = r##"# The code map in depth

Two layers, kept apart on purpose.

**The derived graph** is computed from the code at HEAD and never written into a file:
imports, re-exports, definitions, calls and trait/class relations, for TypeScript and Rust.
Registered code roots: {roots}.

A root is registered in `.dit/config.yaml`:

```yaml
code:
  - id: web
    repo: frontend            # a `repos:` entry; omit for this repository
    include: ["src/**"]
    exclude: ["src/**/*.test.ts"]
    generated: ["src/generated/**"]
```

Files under `generated:` are mapped but left out of `dit code hubs`, and `dit code explain`
says when a file is generated — edit its source, not it. Every `dit code` command first
brings the map up to HEAD, reading only the files whose content changed; `dit code refresh
--full` reads every file again, and `dit reindex` refreshes it too.

| Command | Answers |
|---|---|
| `dit code users <file\|symbol>` | who imports it, through barrels — what breaks if it changes |
| `dit code uses <file\|symbol>` | what it imports (resolved, external or unresolved) and calls |
| `dit code explain <name>` | what it defines, whether it is generated, who uses it |
| `dit code path <a> <b>` | the shortest import chain from one to the other |
| `dit code where <text>` | files and symbols whose name contains the text |
| `dit code hubs [--root r]` | the most depended-on files |

Name a file by its path (`src/crud/hooks.ts`, or `web:src/crud/hooks.ts` when two roots
share it) or a symbol by name (`useList`, `Repo::head`).

**The map of intent** is authored, in a `dit-map` fence in any document, and says what no
parser can infer:

```dit-map
map: web
confirmed: {{ web: 3f2a9c1e }}
entries:
  - task: add a list screen for an entity
    change: [web:src/resources/**]
    example: web:src/resources/product/index.ts
    never: [web:src/generated/**]
    why: the engine renders every entity; a screen is configuration, not code
```

Every path is `<root>:<glob>`. An entry needs a `task` and at least one of `change`,
`example` or `never`. `dit code check` judges each entry against the code:

- **broken** — a path matches no file at HEAD, or names an unregistered root. Fix the entry.
- **unconfirmed** — nobody has confirmed the map against the code yet.
- **stale** — the example changed since the map was confirmed. Read it before copying it.
- **holds** — confirmed, and the example is unchanged since.

After reading the map against the code, `dit code map confirm <map>` pins each root it names
to HEAD, in one commit. Nothing else moves the pin: a map is a person's claim, and a claim
nobody re-read must not look fresh. The fence may not name anything to run or fetch.
"##;

const TOPIC_MORSE: &str = r##"# Morse in depth: seams you can prove

Morse keeps API scenarios in the repository as `dit-morse` fences: declarative chains of
requests against a registered OpenAPI spec. No scripts, no URLs in the file, no secrets.

Registered specs here: {specs}.

## Reading what exists

    dit morse specs                 # registered specs and their catalogues
    dit morse operations <spec>     # operationIds you can call
    dit morse check                 # every scenario: fresh / stale / broken, and where proven

A scenario proven on one environment says nothing about another. `check` lists each proof
by environment; an environment it does not list was never proven.

## Writing a scenario

```dit-morse
scenario: payslip-pdf
spec: { id: payroll, commit: <sha> }
requires: [slip_id]
steps:
  - id: pdf
    operation: payroll/getSalarySlip
    params: { id: "{{slip_id}}" }
    expect: { status: 200 }
    capture: { slip: $.data.id }
```

`requires:` names the variables the environment must supply — names only. A route no spec
describes is declared inline under `requests:` with a method and a path.

## Environments and proof

Environment addresses and values live in `.dit/morse.local.yaml` (gitignored), never in a
committed file; `dit morse allow <host>` trusts a host on this machine.

    dit morse run <scenario> --env <name>     # fire it, record nothing
    dit morse sync <scenario> --env <name>    # fire it; on green, pin the spec commit and
                                              # record proven.<name> in the fence

A proof appears in the fence as:

```
proven:
  local-hrperf: { commit: <sha>, on: <yyyy-mm-dd> }
```

Only `sync` writes it, only on green. Reindex judges each proof against the spec: it goes
stale when the spec moves, and needs proving again.

## Naming seams on issues

The issue that delivers a seam: `dit issue set '#<n>' proves=<scenario>`. The issue that
depends on it: `dit issue set '#<n>' needs_scenarios=<scenario> env=<name>`. With
`proof: required` under `coordination.readiness`, the dependent stays out of `dit ready`
until the proof holds, and `dit ready` says which scenario and which environment.
"##;

/// True when a generated document was written by a different version than the
/// one asking — what `dit doctor` reports.
pub fn agent_doc_stamp(text: &str) -> Option<String> {
    let marker = "_Generated by DIT ";
    let start = text.find(marker)? + marker.len();
    // A version has dots in it, so the sentence's full stop is not a
    // delimiter: take the whole word and drop the punctuation it ends on.
    let word = text[start..].split_whitespace().next()?;
    let stamp = word.trim_end_matches('.');
    (!stamp.is_empty()).then(|| stamp.to_owned())
}

/// The tool files to touch, as repo-relative paths.
pub(crate) fn targets(root: &Path, opts: &AgentDocOptions) -> Vec<String> {
    TOOL_FILES
        .iter()
        .filter(|(key, _)| opts.only.is_empty() || opts.only.iter().any(|k| k == key))
        .filter(|(key, path)| {
            // Naming a tool is the clearest statement of intent there is, so
            // a named tool's file is created. AGENTS.md is the cross-tool
            // convention and is created too. Everything else is only pointed
            // when the team already uses it — the guard exists to keep DIT
            // from littering a repo with files for tools nobody here runs,
            // not to second-guess someone who asked.
            opts.only.iter().any(|k| k == key)
                || opts.all
                || *key == "agents"
                || root.join(path).exists()
        })
        .map(|(_, path)| (*path).to_owned())
        .collect()
}

/// The canonical document, preserving anything the team wrote around the
/// generated block.
pub(crate) fn render_document(existing: &str, spec: &str) -> String {
    upsert_marked_block(existing, SPEC_START, SPEC_END, spec)
}

pub(crate) fn render_pointer(existing: &str) -> (String, bool) {
    // An older workspace carries the protocol inline; it is replaced, not
    // left beside the pointer to rot.
    let (base, had_legacy) = match remove_marked_block(existing, LEGACY_START, LEGACY_END) {
        Some(cleaned) => (cleaned, true),
        None => (existing.to_owned(), false),
    };
    (
        upsert_marked_block(&base, POINTER_START, POINTER_END, &pointer_body()),
        had_legacy,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_marked_block_leaves_everything_around_it_alone() {
        let before = "top\n\n<!-- a -->\nold\n<!-- /a -->\n\nbottom\n";
        let after = upsert_marked_block(before, "<!-- a -->", "<!-- /a -->", "new");
        assert!(after.starts_with("top\n"), "{after}");
        assert!(after.contains("new"), "{after}");
        assert!(!after.contains("old"), "{after}");
        assert!(after.ends_with("bottom\n"), "{after}");
    }

    #[test]
    fn a_missing_block_is_appended_without_eating_the_last_line() {
        let after = upsert_marked_block("hand written\n", "<!-- a -->", "<!-- /a -->", "body");
        assert!(after.starts_with("hand written\n\n"), "{after}");
        assert!(after.trim_end().ends_with("<!-- /a -->"), "{after}");
    }

    fn context() -> AgentContext {
        AgentContext {
            rule_files: vec![
                RuleFile {
                    repo: None,
                    path: "CLAUDE.md".into(),
                },
                RuleFile {
                    repo: None,
                    path: "apps/web/CLAUDE.md".into(),
                },
                RuleFile {
                    repo: Some("backend".into()),
                    path: "services/auth/AGENTS.md".into(),
                },
            ],
            specs: vec!["auth".into(), "payroll".into()],
            proof_required: true,
            code_roots: vec!["web (src/**; generated src/generated/**)".into()],
        }
    }

    // The agent must be told where each repository's own rules are, and that
    // the nearer one wins — the rules are the part of a codebase an agent
    // cannot infer from the code, and following the wrong file's is the
    // expensive mistake.
    #[test]
    fn the_spec_names_every_rule_file_and_says_the_nearest_wins() {
        let doc = agent_spec("9.9.9", &[], &[], &context());
        assert!(doc.contains("`CLAUDE.md`"), "{doc}");
        assert!(doc.contains("`apps/web/CLAUDE.md`"), "{doc}");
        assert!(doc.contains("backend: `services/auth/AGENTS.md`"), "{doc}");
        assert!(doc.to_lowercase().contains("nearest"), "{doc}");
    }

    #[test]
    fn with_no_rule_files_the_spec_says_so_rather_than_listing_nothing() {
        let doc = agent_spec("9.9.9", &[], &[], &AgentContext::default());
        assert!(doc.contains("No `CLAUDE.md` or `AGENTS.md`"), "{doc}");
    }

    // ADR 0024: an agent about to build against an endpoint must know how to
    // find out whether it answers, where, and what holds its issue back.
    #[test]
    fn the_spec_teaches_proven_seams() {
        let doc = agent_spec("9.9.9", &[], &[], &context());
        for needle in [
            "dit morse check",
            "dit morse sync <scenario> --env <name>",
            "needs_scenarios",
            "proves",
            "`env`",
            "proof: required",
            "auth · payroll",
        ] {
            assert!(doc.contains(needle), "missing `{needle}`:\n{doc}");
        }
        assert!(doc.contains("This workspace requires proof"), "{doc}");
        let off = agent_spec(
            "9.9.9",
            &[],
            &[],
            &AgentContext {
                proof_required: false,
                ..context()
            },
        );
        assert!(
            off.contains("This workspace does not require proof"),
            "{off}"
        );
    }

    #[test]
    fn the_data_model_lists_the_seam_keys() {
        let doc = agent_spec("9.9.9", &[], &[], &AgentContext::default());
        assert!(doc.contains("`needs_scenarios`, `proves`, `env`"), "{doc}");
    }

    // The pointer is what an agent reads first, and often the only thing it
    // reads before acting — so the few rules whose breach costs someone else
    // their work live here, not only behind the link.
    #[test]
    fn the_pointer_carries_the_rules_that_cost_most_when_broken() {
        let body = pointer_body();
        for needle in [
            "docs/dit-for-agents.md",
            "Never edit an issue file by hand",
            "dit ready",
            "dit claim",
            "dit morse check",
            "dit ai spec",
        ] {
            assert!(body.contains(needle), "missing `{needle}`:\n{body}");
        }
        assert!(
            body.lines().count() <= 20,
            "a pointer, not a manual:\n{body}"
        );
    }

    // Recipes: the common tasks as the commands that do them, in order —
    // what an agent needs to act, which `--help` lists but never sequences.
    #[test]
    fn the_spec_carries_recipes_for_the_common_tasks() {
        let doc = agent_spec("9.9.9", &[], &[], &AgentContext::default());
        for needle in [
            "## Recipes",
            "dit issue new",
            "dit issue show",
            "dit issue set <issue> status=in_progress",
            "dit issue comment",
            "dit flow show",
            "dit morse sync",
            "dit ai spec <topic>",
        ] {
            assert!(doc.contains(needle), "missing `{needle}`:\n{doc}");
        }
    }

    // A topic goes deeper on one subject; the main spec stays the part an
    // agent reads whole.
    #[test]
    fn each_topic_goes_deeper_and_an_unknown_one_is_named() {
        let ctx = context();
        let issues = agent_topic("issues", &ctx).unwrap();
        assert!(
            issues.contains("needs_scenarios") && issues.contains("blocked_by"),
            "{issues}"
        );
        let flow = agent_topic("flow", &ctx).unwrap();
        assert!(
            flow.contains("dit-flow") && flow.contains("fed_by"),
            "{flow}"
        );
        let morse = agent_topic("morse", &ctx).unwrap();
        for needle in [
            "dit-morse",
            "proven:",
            "--env",
            "morse.local.yaml",
            "requires:",
            "auth · payroll",
        ] {
            assert!(morse.contains(needle), "missing `{needle}`:\n{morse}");
        }
        let code = agent_topic("code", &ctx).unwrap();
        for needle in [
            "dit-map",
            "confirmed:",
            "dit code map confirm",
            "dit code users",
            "generated:",
            "web (src/**; generated src/generated/**)",
        ] {
            assert!(code.contains(needle), "missing `{needle}`:\n{code}");
        }
        assert!(agent_topic("nonsense", &ctx).is_none());
        assert_eq!(AGENT_TOPICS, ["issues", "flow", "morse", "code"]);
    }

    // An agent that does not know the map exists greps instead; one that
    // does not know a map entry can be broken copies a file that is gone.
    #[test]
    fn the_spec_points_at_the_code_map_and_its_verdicts() {
        let spec = agent_spec("9.9.9", &[], &[], &context());
        for needle in [
            "dit code users",
            "dit code check",
            "web (src/**; generated src/generated/**)",
            "**broken**",
        ] {
            assert!(spec.contains(needle), "missing `{needle}`");
        }
        let bare = agent_spec("9.9.9", &[], &[], &AgentContext::default());
        assert!(bare.contains("add one under `code:`"), "{bare}");
    }

    #[test]
    fn the_stamp_round_trips() {
        let doc = agent_spec("9.9.9", &[], &[], &AgentContext::default());
        assert_eq!(agent_doc_stamp(&doc).as_deref(), Some("9.9.9"));
    }
}
