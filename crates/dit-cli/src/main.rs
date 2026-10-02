//! DIT command-line interface. The CLI is not a second-class citizen: it and
//! the server share exactly the same `dit-core`, so anything doable in the
//! browser is doable in a terminal and vice versa.

// The whole workspace bans printing so library crates stay silent; this
// crate IS the printer, so the standard output macros are its job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

mod upgrade;
use dit_core::{
    ClaimOptions, DataLayout, DiagnosticLevel, Dit, DitError, FieldPatch, IndexedIssue, IssueDraft,
    IssueId, IssueKind, LaneSpec, Priority, ReindexMode,
};

#[derive(Parser)]
#[command(
    name = "dit",
    version,
    about = "Project management where Markdown files in git are the source of truth"
)]
struct Cli {
    /// The alias your commits are attributed to (default: $DIT_ME, then the
    /// alias saved in this clone by `dit ui`'s settings panel, then $USER).
    #[arg(long, global = true)]
    me: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make the current directory a workspace: git init, merge driver, README.
    Init {
        /// Where issue content lives (ADR 0005): `root` keeps `issues/`
        /// visible at the tree root, `dotdir` tucks everything under `.dit/`.
        #[arg(long, default_value = "root")]
        layout: LayoutArg,
        /// Also install the agent guide (`dit ai init`) once the workspace
        /// exists, so agents are told how it works from the first commit.
        #[arg(long)]
        ai: bool,
    },
    /// Create, read and edit issues.
    Issue {
        #[command(subcommand)]
        cmd: Issue,
    },
    /// List issues matching a DQL query (no query = all issues).
    List { query: Vec<String> },
    /// The board: one column per workflow status.
    Board,
    /// Branch, head and working-tree state.
    Status,
    /// Fetch, rebase onto the remote, push. Exits 1 when files need a human.
    Sync {
        #[arg(long, default_value = "origin")]
        remote: String,
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Rebuild the local index from git.
    Reindex {
        #[arg(long, default_value = "all")]
        mode: Mode,
    },
    /// Check everything that silently breaks a workspace when wrong.
    Doctor,
    /// Serve this workspace to the browser and open it: one binary, no
    /// separate installation.
    Ui {
        /// Interface to bind. 127.0.0.1 keeps it on this machine; anything
        /// else opens it to the network the interface sits on.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 7700)]
        port: u16,
    },
    /// Register this binary as the repository's merge driver.
    InstallDriver,
    /// Upgrade this binary to the latest release, or an exact one
    /// (`dit upgrade 0.1.9`). Downloads are checksum-verified.
    #[command(alias = "update")]
    Upgrade {
        /// An exact version, e.g. 0.1.9 or v0.1.9. Default: the latest release.
        version: Option<String>,
    },
    /// List and edit the issue templates `.dit/templates/` holds.
    Templates {
        #[command(subcommand)]
        cmd: Templates,
    },
    /// Build generated documents (ADR 0008).
    Docs {
        #[command(subcommand)]
        cmd: Docs,
    },
    /// Move the workspace's content between layouts (ADR 0005):
    /// `git mv` + reindex, history intact.
    MigrateLayout { to: LayoutArg },
    /// Backfill `#numbers` onto issues created before numbering (ADR 0009):
    /// append-only, one commit, existing numbers never move.
    Renumber,
    /// Teach AI sessions how this workspace works (ADR 0019, 0020, 0021):
    /// install the agent guide, or print the current specification.
    Ai {
        #[command(subcommand)]
        cmd: AiCmd,
    },
    /// The coordination plane (ADR 0015): scaffold lanes, list them.
    Workflow {
        #[command(subcommand)]
        cmd: WorkflowCmd,
    },
    /// Claim an issue as your exclusive intent, renew, take over or release
    /// (ADR 0015). Refuses protocol violations, naming the way out.
    Claim {
        reference: String,
        /// Refresh your own claim's timestamp (no commit when still live).
        #[arg(long)]
        renew: bool,
        /// Take the issue over from another actor's live claim.
        #[arg(long)]
        takeover: bool,
        /// Clear the claim pair.
        #[arg(long)]
        release: bool,
        /// Write the claim regardless of any guard.
        #[arg(long)]
        force: bool,
    },
    /// List issues pickable right now (ADR 0015): `pick_from` status, every
    /// blocker through the gate. Empty output means wait.
    Ready {
        /// Only issues of this lane.
        #[arg(long)]
        lane: Option<String>,
        /// Override the gate for this call: this status "or later".
        #[arg(long)]
        until: Option<String>,
    },
    /// The lane's inbox (ADR 0015): threads on its issues whose latest
    /// comment is not from the lane's own voice — the questions still
    /// waiting for an answer. Empty output means nobody is waiting on you.
    Inbox {
        /// Only issues of this lane.
        #[arg(long)]
        lane: Option<String>,
    },
    /// The orchestration flows (ADR 0019): list them, or show one as a
    /// stage-by-stage text tree. Membership is `dit issue set flows=...`.
    Flow {
        #[command(subcommand)]
        cmd: FlowCmd,
    },
    /// Morse (§20): the API scenarios this repository states, and whether
    /// they still match the specs they were written against. Reads only —
    /// nothing here sends a request.
    Morse {
        #[command(subcommand)]
        cmd: MorseCmd,
    },
    /// The code map (ADR 0025): how the registered code roots connect — what
    /// a file imports and calls, who uses a file or symbol, the chain between
    /// two, the most depended-on files. Derived from source at HEAD into the
    /// index; each command brings it up to date first.
    Code {
        #[command(subcommand)]
        cmd: CodeCmd,
    },
    /// Called by git during merges; humans never type this.
    #[command(hide = true)]
    MergeDriver {
        /// %O — the common ancestor version.
        base: PathBuf,
        /// %A — the current version; the result is written here.
        ours: PathBuf,
        /// %B — the incoming version.
        theirs: PathBuf,
        /// %L — conflict marker size.
        marker_size: String,
        /// %P — the path being merged (may be empty).
        #[arg(allow_hyphen_values = true)]
        label: String,
    },
}

#[derive(Subcommand)]
enum WorkflowCmd {
    /// Scaffold the coordination plane: lane registry + coordination block
    /// in workflow.yaml, and the evidence-report template. Idempotent.
    /// Agent-facing rules live in `dit ai` (ADR 0021).
    Init {
        /// Lane ids to register, comma-separated — purely an ordering
        /// hint; lanes are free-form (ADR 0019) and none is registered by
        /// default.
        #[arg(long, value_delimiter = ',')]
        lanes: Vec<String>,
    },
    /// List the registered lanes and the coordination knobs.
    Lanes,
}

#[derive(Subcommand)]
enum AiCmd {
    /// Write `docs/dit-for-agents.md` and point every agent file this repo
    /// uses at it. Idempotent — running it again is how you update it.
    Init {
        /// Also create agent files for tools this repo does not use yet.
        #[arg(long)]
        all: bool,
        /// Restrict to these tools: agents, claude, cursor, copilot.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
    /// Point one named tool's file at the guide, creating that file if the
    /// repo does not have it yet — naming a tool is the whole request.
    Add {
        /// Tools to point: agents, claude, cursor, copilot.
        #[arg(required = true, value_delimiter = ',')]
        tools: Vec<String>,
    },
    /// Print the specification to stdout, generated by this binary so it
    /// always matches this binary. Name a topic to go deeper on one subject:
    /// `issues`, `flow`, `morse`.
    Spec {
        /// A topic: issues, flow, morse.
        topic: Option<String>,
    },
}

#[derive(Subcommand)]
enum CodeCmd {
    /// What a file — or the file defining a symbol — imports, by the file
    /// each import reaches.
    Uses {
        name: String,
        /// Also list the calls it makes beyond what its imports name.
        #[arg(long)]
        calls: bool,
    },
    /// Who imports a file, or uses a symbol (followed through barrels).
    Users { name: String },
    /// The shortest import chain from one node to another.
    Path { from: String, to: String },
    /// A node, what it defines, whether it is generated, and who uses it.
    Explain { name: String },
    /// The most depended-on files.
    Hubs {
        /// Only this code root.
        #[arg(long)]
        root: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Include generated files (left out by default).
        #[arg(long)]
        generated: bool,
    },
    /// The files to read for a question: `dit code where token refresh`.
    /// Ranked by how many words a file answers in its path or the names it
    /// defines, then by how much of the code imports it.
    Where {
        #[arg(required = true, num_args = 1..)]
        words: Vec<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Every `dit-map` entry with its verdict: holds, unconfirmed, stale,
    /// broken. Exits non-zero when an entry is broken or a map cannot be
    /// read — stale is a prompt to re-read, not a failure.
    Check,
    /// Path literals in the code matched against the registered specs:
    /// orphan calls no spec describes, and operations called but proven
    /// nowhere. A heuristic over literals — a path built at runtime from
    /// variables is not seen.
    Api {
        /// Every call, including those proven somewhere.
        #[arg(long)]
        all: bool,
    },
    /// Maps of intent (`dit-map` fences).
    Map {
        #[command(subcommand)]
        cmd: MapCmd,
    },
    /// Opt in to refreshing the map in the background after every commit,
    /// merge, checkout and rebase — so the first question after a pull does
    /// not wait for the parse.
    Hook {
        #[command(subcommand)]
        cmd: HookCmd,
    },
    /// Bring the map up to HEAD and say what changed.
    Refresh {
        /// Read every file again, not only those that changed.
        #[arg(long)]
        full: bool,
    },
}

#[derive(Subcommand)]
enum HookCmd {
    /// Add the background refresh to this repository's git hooks.
    Install,
    /// Take it back out.
    Uninstall,
}

#[derive(Subcommand)]
enum MapCmd {
    /// You read the map against the code and it still holds: pin every root
    /// it names to HEAD, in one commit.
    Confirm { map: String },
}

#[derive(Subcommand)]
enum FlowCmd {
    /// Every flow with its member count.
    List,
    /// One flow, stage by stage: the text form of the diagram. `all`
    /// renders the union of every flow.
    Show { name: String },
}

#[derive(Subcommand)]
enum MorseCmd {
    /// The registered specs and what each one describes.
    Specs,
    /// One spec's operations — the catalogue a scenario draws from.
    Operations {
        /// The spec id, as `specs:` in .dit/config.yaml registers it.
        spec: String,
    },
    /// Every scenario with its verdict: fresh, stale, broken, unreadable.
    /// Exits non-zero when anything is broken or unreadable — stale is a
    /// fact about the world, not a failure, so it does not fail the check.
    Check,
    /// Fire one scenario against a live environment. This is one of the two
    /// things in DIT that sends a request, and it only ever sends to a host
    /// this machine allows (§20.5).
    Run {
        scenario: String,
        /// The environment to use, overriding the fence's own `env:`.
        #[arg(long)]
        env: Option<String>,
    },
    /// Run a scenario and, only if every step passes, move its `commit:` pin
    /// to where the spec stands now and record `proven.<env>` in the fence —
    /// "proven to work at this commit, on this environment" (ADR 0024). Needs
    /// a live environment, so it cannot run in CI; CI runs `check`, which
    /// only reads.
    Sync {
        scenario: String,
        #[arg(long)]
        env: Option<String>,
    },
    /// Fire one operation from a spec, without a scenario — the terminal twin
    /// of the Send control (ADR 0023). The method and path come from the
    /// spec, the server from the spec or the environment, and the host must
    /// be allowed on this machine. Prints the response here; nothing is
    /// written.
    Send {
        /// `<spec>/<operationId>`, e.g. `party/getParty`.
        operation: String,
        #[arg(long)]
        env: Option<String>,
        /// A path parameter, `name=value` — fills `{name}` in the spec's path.
        #[arg(long = "param", value_name = "NAME=VALUE")]
        params: Vec<String>,
        #[arg(long = "query", value_name = "NAME=VALUE")]
        query: Vec<String>,
        /// `Name: value`. Use `{{name}}` for anything secret.
        #[arg(long = "header", value_name = "NAME: VALUE")]
        headers: Vec<String>,
        /// A JSON request body.
        #[arg(long)]
        body: Option<String>,
        /// The status to expect. Without one, any response counts as sent.
        #[arg(long)]
        status: Option<u16>,
    },
    /// Trust a host on this machine. Written to `.dit/morse.local.yaml`,
    /// which is gitignored: a scenario arriving in a pull request cannot
    /// bring its own permission with it. With no host, lists what is
    /// allowed.
    Allow { host: Option<String> },
    /// Convert a curl command, a Postman collection or a Postman environment
    /// (ADR 0027). Addresses become a spec's server, credentials become
    /// `{{variables}}`, and scripts are dropped — each listed as it goes.
    Import {
        #[command(subcommand)]
        what: MorseImport,
    },
    /// Change this machine's environments in `.dit/morse.local.yaml`. The
    /// allowlist is not one of them — that is `dit morse allow`.
    Env {
        #[command(subcommand)]
        what: MorseEnv,
    },
}

#[derive(Subcommand)]
enum MorseImport {
    /// One curl command, as a scenario of one step.
    Curl {
        /// The command, from `curl` on — quote it, or pass it after `--`.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        /// The document the scenario goes in.
        #[arg(long)]
        doc: String,
        /// What the scenario is called.
        #[arg(long, default_value = "imported")]
        scenario: String,
        /// The spec a host nothing names goes to.
        #[arg(long)]
        spec: Option<String>,
        /// Show what would be written, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// A Postman collection (v2.1): one scenario per folder.
    Postman {
        file: std::path::PathBuf,
        #[arg(long)]
        doc: String,
        /// The spec `{{baseUrl}}` stands for.
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// A Postman environment, into this machine's local file only.
    PostmanEnv { file: std::path::PathBuf },
}

#[derive(Subcommand)]
enum MorseEnv {
    /// Create or change an environment. A value given here lands in your
    /// shell history — for secrets, prefer `dit ui` or editing the file.
    Set {
        name: String,
        /// The server this environment sends to; `--server ''` clears it.
        #[arg(long)]
        server: Option<String>,
        #[arg(long = "var", value_name = "NAME=VALUE")]
        vars: Vec<String>,
        #[arg(long = "unset", value_name = "NAME")]
        unset: Vec<String>,
    },
    /// Remove an environment and its values.
    Rm { name: String },
}

#[derive(Subcommand)]
enum Issue {
    /// Create an issue.
    New {
        /// The title; multiple words are joined into one line.
        title: Vec<String>,
        #[arg(short, long, default_value = "task")]
        kind: Kind,
        #[arg(short, long)]
        status: Option<String>,
        #[arg(short = 'P', long)]
        priority: Option<Pri>,
        #[arg(short = 'a', long)]
        assignee: Vec<String>,
        #[arg(short, long)]
        label: Vec<String>,
        #[arg(long)]
        estimate: Option<u32>,
        #[arg(long)]
        body: Option<String>,
        /// Seed the body from `.dit/templates/<name>.md` instead of `--body`.
        #[arg(long)]
        template: Option<String>,
        /// The lane this issue is born into (ADR 0015).
        #[arg(long)]
        lane: Option<String>,
        /// The flow(s) this issue joins at birth (ADR 0019).
        #[arg(long = "flow")]
        flows: Vec<String>,
    },
    /// Show one issue: fields, body, comments, field history.
    Show { reference: String },
    /// Change fields, e.g. `dit issue set 01K3MA1 status=done labels=a,b`.
    Set {
        reference: String,
        /// field=value pairs; list fields take comma-separated values.
        fields: Vec<String>,
        /// Write a status the workflow does not declare (ADR 0015's escape).
        #[arg(long)]
        force: bool,
    },
    /// Add a comment, a reply with `--reply <comment ref>`, or an evidence
    /// report drafted from a template with `--template <name>`.
    Comment {
        reference: String,
        /// The comment text; multiple words are joined into one paragraph.
        text: Vec<String>,
        /// The parent comment this replies to: its id or 7-char short form.
        #[arg(long)]
        reply: Option<String>,
        /// Seed the comment from `.dit/templates/<name>.md`, edit it in
        /// $EDITOR, and post the result (refused if left untouched).
        #[arg(long)]
        template: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Task,
    Bug,
    Story,
    Spike,
    Chore,
}

impl From<Kind> for IssueKind {
    fn from(k: Kind) -> IssueKind {
        match k {
            Kind::Task => IssueKind::Task,
            Kind::Bug => IssueKind::Bug,
            Kind::Story => IssueKind::Story,
            Kind::Spike => IssueKind::Spike,
            Kind::Chore => IssueKind::Chore,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Pri {
    P0,
    P1,
    P2,
    P3,
    P4,
}

impl From<Pri> for Priority {
    fn from(p: Pri) -> Priority {
        match p {
            Pri::P0 => Priority::P0,
            Pri::P1 => Priority::P1,
            Pri::P2 => Priority::P2,
            Pri::P3 => Priority::P3,
            Pri::P4 => Priority::P4,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    All,
    State,
    Events,
}

#[derive(Subcommand)]
enum Templates {
    /// Print the template names `issue new --template` accepts.
    List,
    /// Open a template in $EDITOR.
    Edit { name: String },
}

#[derive(Subcommand)]
enum Docs {
    /// Regenerate `issues/README.md`, the human-browsable issue index.
    Build {
        /// Build the issue index README (the one target, named for the ones
        /// that follow).
        #[arg(long)]
        index: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum LayoutArg {
    Root,
    Dotdir,
}

impl From<LayoutArg> for DataLayout {
    fn from(l: LayoutArg) -> DataLayout {
        match l {
            LayoutArg::Root => DataLayout::Root,
            LayoutArg::Dotdir => DataLayout::DotDir,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("dit: {e}");
            // A busy lock is not an error in the work — it is a different
            // process holding it. Scripts want to tell the two apart.
            if matches!(e, DitError::Busy { .. }) {
                ExitCode::from(3)
            // "You asked for something that isn't there" is its own code:
            // a script looping over refs can skip and continue on 2.
            } else if matches!(e, DitError::NotFound(_) | DitError::TemplateMissing(_)) {
                ExitCode::from(2)
            } else {
                ExitCode::from(1)
            }
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, DitError> {
    let explicit = alias(&cli);
    match cli.command {
        Command::Init { layout, ai } => {
            let cwd = std::env::current_dir()?;
            let exe = std::env::current_exe()?;
            let mut dit = Dit::init_with_layout(&cwd, &exe, layout.into())?;
            println!("initialized workspace at {}", dit.root().display());
            // Say where the files went — a workspace whose layout is a
            // surprise is a workspace nobody trusts (ADR 0005).
            match dit.layout() {
                DataLayout::Root => println!(
                    "layout: root — {} at the tree root, machinery in .dit/",
                    dit_core::CONTENT_ROOTS.join("/")
                ),
                DataLayout::DotDir => println!("layout: dotdir — everything under .dit/"),
            }
            println!(
                "numbering: {} — issues get a #number {}",
                dit.config().numbering.as_str(),
                match dit.config().numbering {
                    dit_core::Numbering::Local => "when created",
                    dit_core::Numbering::OnMerge => "when their branch merges",
                },
            );
            println!(
                "templates: {} (.dit/templates/)",
                dit.templates().join(", ")
            );
            if ai {
                let report = dit.write_agent_docs(&dit_core::AgentDocOptions::default())?;
                println!(
                    "agent guide: {} — pointed at from {}",
                    dit_core::AGENT_DOC_PATH,
                    report.pointers.join(", ")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Issue { cmd } => issue(cmd, explicit.as_deref()),
        Command::List { query } => {
            let dit = open()?;
            let me = me_for(&dit, explicit.as_deref());
            let hits = dit.query(&query.join(" "), Some(&me))?;
            print_list(&hits);
            println!(
                "{} issue{}",
                hits.len(),
                if hits.len() == 1 { "" } else { "s" }
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Board => {
            let dit = open()?;
            print_board(&dit.board()?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Status => {
            let dit = open()?;
            print_status(&dit);
            Ok(ExitCode::SUCCESS)
        }
        Command::Sync { remote, branch } => {
            let mut dit = open()?;
            let report = dit.sync(dit_core::SyncOptions {
                remote,
                branch,
                ..dit_core::SyncOptions::default()
            })?;
            println!(
                "pulled {} commit{}, pushed {}, {} field{} auto-merged",
                report.pulled,
                if report.pulled == 1 { "" } else { "s" },
                report.pushed,
                report.auto_resolved.len(),
                if report.auto_resolved.len() == 1 {
                    ""
                } else {
                    "s"
                },
            );
            for f in &report.auto_resolved {
                println!("  merged  {} ({})", f.path, f.summary);
            }
            if report.needs_human.is_empty() {
                Ok(ExitCode::SUCCESS)
            } else {
                for c in &report.needs_human {
                    println!("needs a human: {} — {}", c.path.display(), c.detail);
                }
                Ok(ExitCode::from(1))
            }
        }
        Command::Reindex { mode } => {
            let mut dit = open()?;
            let r = dit.reindex(match mode {
                Mode::All => ReindexMode::All,
                Mode::State => ReindexMode::State,
                Mode::Events => ReindexMode::Events,
            })?;
            println!(
                "indexed {} issues, {} comments, {} events at {} ({} file{} skipped)",
                r.issues,
                r.comments,
                r.events,
                &r.head[..7.min(r.head.len())],
                r.skipped,
                if r.skipped == 1 { "" } else { "s" },
            );
            // Name what was passed over: a committed issue that no command
            // can find is worse than an error, because nothing says it exists.
            for f in &r.skipped_files {
                println!("  skipped {}: {}", f.path, f.reason);
            }
            // The code map (ADR 0025) rides an explicit reindex — never the
            // one every command runs on open, which a first parse would stall.
            if !dit.config().code.is_empty() {
                let code = dit.refresh_code()?;
                println!(
                    "code map: {} file(s) in {} root(s), {} parsed",
                    code.files, code.roots, code.parsed
                );
                for problem in &code.problems {
                    println!("  {problem}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            let dit = open()?;
            let mut failed = false;
            for d in dit.doctor() {
                let tag = match d.level {
                    DiagnosticLevel::Ok => " ok  ",
                    DiagnosticLevel::Warn => "WARN ",
                    DiagnosticLevel::Error => {
                        failed = true;
                        "ERROR"
                    }
                };
                println!("[{tag}] {}: {}", d.code, d.message);
            }
            // The one diagnostic that is about this binary, not the workspace.
            // Freshness is a warning (never a failure) and an unreachable
            // check is a note — doctor must stay useful offline.
            match upgrade::freshness() {
                upgrade::Freshness::Current { latest } => {
                    println!(
                        "[ ok  ] version: {} (latest is {latest})",
                        env!("CARGO_PKG_VERSION")
                    );
                }
                upgrade::Freshness::Behind { current, latest } => {
                    println!("[WARN ] version: {current} — latest is {latest}; run 'dit upgrade'");
                }
                upgrade::Freshness::Unknown { current, reason } => {
                    println!("[ ok  ] version: {current} (freshness check skipped: {reason})");
                }
            }
            if failed {
                Ok(ExitCode::from(1))
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
        Command::Ui { host, port } => {
            // A repository that is not a workspace opens as its code map:
            // the Code screen only, read-only (ADR 0025).
            let dit = Dit::open_for_ui(&std::env::current_dir()?)?;
            // The same token file the standalone server reads, so `dit ui`
            // and `dit-server` hand the same URL shape for one workspace; a
            // code map keeps it beside its index, which ignores itself, so
            // nothing lands in the repository's tree.
            let cache = if dit.code_only() {
                dit.root().join(dit_core::CODE_DIR)
            } else {
                dit.root().join(".dit-cache")
            };
            let token = dit_server::config::load_or_create_token(&cache)?;
            let me = me_for(&dit, explicit.as_deref());
            let code_only = dit.code_only();
            let state = dit_server::AppState::with_bind_host(dit, &me, &token, &host);
            // Catch the index up, then watch for other processes' writes
            // (ADR 0017) — `dit ui` must live-update just like the server. A
            // code map has no workspace files to watch; it refreshes on read.
            if !code_only {
                state.start_live_updates();
            }
            let app = dit_server::app(state);
            let display_host = if host == "0.0.0.0" {
                "127.0.0.1"
            } else {
                host.as_str()
            };
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(dit_server::serve(app, &host, port, move || {
                    let url = format!("http://{display_host}:{port}/#token={token}");
                    println!("DIT listening on http://{display_host}:{port}/");
                    println!("open: {url}");
                    open_browser(&url);
                }))
                .map_err(|e| {
                    // A taken port is almost always another dit ui or
                    // dit-server still holding it; say so instead of a bare
                    // OS error.
                    if e.kind() == std::io::ErrorKind::AddrInUse {
                        std::io::Error::new(
                            e.kind(),
                            format!("{e} — is another dit ui or dit-server on port {port}?"),
                        )
                    } else {
                        e
                    }
                })?;
            Ok(ExitCode::SUCCESS)
        }
        Command::InstallDriver => {
            let dit = open()?;
            let exe = std::env::current_exe()?;
            dit.install_merge_driver(&exe)?;
            println!("merge driver registered: {}", exe.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Upgrade { version } => {
            // No workspace needed: this command is about the binary itself.
            // Its failures are download/verify problems, not workspace
            // errors, so they print and fail the exit code directly.
            match upgrade::upgrade(version.as_deref()) {
                Ok(line) => {
                    println!("{line}");
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("dit upgrade: {e}");
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        Command::Templates { cmd } => templates(cmd),
        Command::Docs { cmd } => match cmd {
            Docs::Build { index } => {
                if !index {
                    eprintln!("dit: nothing to build — pass --index for the issue index README");
                    return Ok(ExitCode::from(2));
                }
                let mut dit = open()?;
                if dit.build_docs_index()? {
                    println!("wrote the issue index (issues/README.md)");
                } else {
                    println!("issue index already current (issues/README.md)");
                }
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::MigrateLayout { to } => {
            let mut dit = open()?;
            let from = dit.layout();
            let target = DataLayout::from(to);
            let report = dit.migrate_layout(target)?;
            println!("layout: {} -> {}", from.as_str(), target.as_str());
            println!(
                "moved {} content root{}, renamed {} legacy issue.md file{}",
                report.roots_moved,
                if report.roots_moved == 1 { "" } else { "s" },
                report.bodies_renamed,
                if report.bodies_renamed == 1 { "" } else { "s" },
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Renumber => {
            let mut dit = open()?;
            match dit.renumber()? {
                0 => println!("every issue already has a number — nothing to do"),
                n => println!(
                    "assigned {n} number(s) in creation order, one commit; existing numbers untouched"
                ),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Ai { cmd } => match cmd {
            AiCmd::Spec { topic: None } => {
                let dit = open()?;
                print!("{}", dit.agent_spec());
                Ok(ExitCode::SUCCESS)
            }
            AiCmd::Spec { topic: Some(topic) } => {
                let dit = open()?;
                match dit.agent_topic(&topic) {
                    Some(text) => {
                        print!("{text}");
                        Ok(ExitCode::SUCCESS)
                    }
                    None => Err(DitError::Refuse(format!(
                        "`{topic}` is not a topic — the topics are {}",
                        dit_core::AGENT_TOPICS.join(", ")
                    ))),
                }
            }
            AiCmd::Add { tools } => {
                ensure_workspace()?;
                let mut dit = open()?;
                let report = dit.write_agent_docs(&dit_core::AgentDocOptions {
                    all: false,
                    only: tools,
                })?;
                if report.document_written {
                    println!("wrote {}", dit_core::AGENT_DOC_PATH);
                }
                for path in &report.legacy_replaced {
                    println!("replaced the old peer-protocol block in {path}");
                }
                if report.changed {
                    println!("pointed at it from: {}", report.pointers.join(", "));
                } else {
                    println!(
                        "nothing to do — {} already points at the guide",
                        report.pointers.join(", ")
                    );
                }
                Ok(ExitCode::SUCCESS)
            }
            AiCmd::Init { all, only } => {
                ensure_workspace()?;
                let mut dit = open()?;
                let report = dit.write_agent_docs(&dit_core::AgentDocOptions { all, only })?;
                if report.document_written {
                    println!("wrote {}", dit_core::AGENT_DOC_PATH);
                }
                for path in &report.legacy_replaced {
                    println!("replaced the old peer-protocol block in {path}");
                }
                if report.changed {
                    println!("pointed at it from: {}", report.pointers.join(", "));
                } else {
                    println!("nothing to do — the agent guide is already current");
                }
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Workflow { cmd } => match cmd {
            WorkflowCmd::Init { lanes } => {
                let mut dit = open()?;
                let specs: Vec<LaneSpec> = lanes
                    .into_iter()
                    .map(|id| LaneSpec {
                        label: id[..1].to_uppercase() + &id[1..],
                        id,
                        owners: Vec::new(),
                    })
                    .collect();
                let report = dit.init_workflow(&specs)?;
                if report.schema_created {
                    println!("wrote .dit/schema/workflow.yaml (statuses, lanes, coordination)");
                } else {
                    if report.lanes_written {
                        println!("appended lanes: to .dit/schema/workflow.yaml");
                    }
                    if report.coordination_written {
                        println!("appended coordination: to .dit/schema/workflow.yaml");
                    }
                }
                if report.report_template_written {
                    println!("wrote .dit/templates/integration-report.md (the evidence report)");
                }
                if !report.schema_created
                    && !report.lanes_written
                    && !report.coordination_written
                    && !report.report_template_written
                {
                    println!("nothing to do — the coordination plane is already scaffolded");
                }
                Ok(ExitCode::SUCCESS)
            }
            WorkflowCmd::Lanes => {
                let dit = open()?;
                // Lanes are free-form now (ADR 0019): the registry only hints
                // order, labels, and owners — so the listing is over the data,
                // joined with whatever the registry knows.
                let counts = dit.lane_counts()?;
                for (id, count) in &counts {
                    let registered = dit
                        .lane_meta(id)
                        .map(|(label, owners)| {
                            format!(
                                "{}{}",
                                label,
                                if owners.is_empty() {
                                    String::new()
                                } else {
                                    format!(" owners: {}", owners.join(", "))
                                }
                            )
                        })
                        .unwrap_or_default();
                    println!("{:<16} {:<4} {}", id, count, registered);
                }
                if counts.is_empty() {
                    println!("no lanes in use — set one with `dit issue set REF lane=name`");
                }
                println!("claim TTL: {} minutes", dit.claim_ttl_minutes());
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::Claim {
            reference,
            renew,
            takeover,
            release,
            force,
        } => {
            let mut dit = open()?;
            let id = resolve(&dit, &reference)?;
            let me = me_for(&dit, explicit.as_deref());
            let opts = ClaimOptions {
                renew,
                takeover,
                release,
                force,
            };
            match dit.claim(&id, &me, opts) {
                Ok(report) => {
                    if report.wrote {
                        println!("{} {}", report.note, id.short_ref().as_str());
                    } else {
                        println!("{} ({})", report.note, id.short_ref().as_str());
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Err(DitError::Refuse(why)) => {
                    eprintln!("dit: {why}");
                    Ok(ExitCode::from(2))
                }
                Err(e) => Err(e),
            }
        }
        Command::Ready { lane, until } => {
            let dit = open()?;
            let ready = dit.ready(lane.as_deref(), until.as_deref())?;
            // Held back for proof (ADR 0024): blockers are through, but a
            // scenario the issue needs does not hold for its env. Named,
            // because "nothing ready" hides what the lane is waiting on.
            let held = dit.unproven(lane.as_deref())?;
            let handle = |issue: &dit_core::Issue| {
                issue
                    .number
                    .map(|n| format!("#{n}"))
                    .unwrap_or_else(|| issue.id.short_ref().as_str().to_owned())
            };
            if ready.is_empty() {
                println!(
                    "nothing ready{}",
                    lane.as_deref()
                        .map(|l| format!(" in lane {l}"))
                        .unwrap_or_default()
                );
            }
            for hit in &ready {
                let issue = &hit.issue.issue;
                println!(
                    "{:<10} {:<9} {}",
                    handle(issue),
                    issue.lane.as_deref().unwrap_or("(unlaned)"),
                    issue.title
                );
            }
            if !held.is_empty() {
                println!("\nheld until proven (blockers are through; the seam is not):");
                for hit in &held {
                    let issue = &hit.issue.issue;
                    let scenarios = match &hit.readiness {
                        dit_core::Readiness::Unproven { scenarios } => scenarios.join(", "),
                        _ => String::new(),
                    };
                    let env = issue.env.as_deref().map_or_else(
                        || "no env set — `dit issue set <issue> env=<name>`".to_owned(),
                        |e| format!("not proven on {e} — `dit morse sync <scenario> --env {e}`"),
                    );
                    println!(
                        "{:<10} {:<9} {}\n           needs {scenarios}: {env}",
                        handle(issue),
                        issue.lane.as_deref().unwrap_or("(unlaned)"),
                        issue.title
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Inbox { lane } => {
            let dit = open()?;
            let items = dit.inbox(lane.as_deref())?;
            if items.is_empty() {
                println!(
                    "inbox empty{} — no thread is waiting on this lane",
                    lane.as_deref()
                        .map(|l| format!(" for lane {l}"))
                        .unwrap_or_default()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for item in &items {
                let issue = &item.issue.issue;
                let handle = issue
                    .number
                    .map(|n| format!("#{n}"))
                    .unwrap_or_else(|| issue.id.short_ref().as_str().to_owned());
                println!(
                    "{:<10} {:<9} {}",
                    handle,
                    issue.lane.as_deref().unwrap_or("(unlaned)"),
                    issue.title
                );
                let excerpt: String = item
                    .root
                    .body
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(72)
                    .collect();
                println!(
                    "  last {} {} ({} replies)",
                    item.last_author,
                    &item.last_at[..10.min(item.last_at.len())],
                    item.replies
                );
                println!("  thread: {excerpt}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Code { cmd } => code(cmd, explicit.as_deref()),
        Command::Morse { cmd } => match cmd {
            MorseCmd::Specs => {
                let dit = open()?;
                let report = dit.morse_report()?;
                if report.specs.is_empty() {
                    println!(
                        "no specs registered — add one under `specs:` in .dit/config.yaml, \
                         e.g. `- {{ id: auth, path: api/openapi.yaml }}`"
                    );
                    return Ok(ExitCode::SUCCESS);
                }
                for spec in &report.specs {
                    let where_ = spec
                        .repo
                        .as_deref()
                        .map_or_else(|| spec.path.clone(), |r| format!("{r}:{}", spec.path));
                    match &spec.problem {
                        None => println!(
                            "{:<12} {:>4} op(s)  {}  {}",
                            spec.id,
                            spec.operations.len(),
                            spec.title.as_deref().unwrap_or("-"),
                            where_
                        ),
                        Some(problem) => {
                            println!("{:<12} unreadable  {where_}", spec.id);
                            println!("             {problem}");
                        }
                    }
                }
                Ok(ExitCode::SUCCESS)
            }
            MorseCmd::Operations { spec } => {
                let dit = open()?;
                let report = dit.morse_report()?;
                let Some(found) = report.specs.iter().find(|s| s.id == spec) else {
                    eprintln!("no spec `{spec}` — `dit morse specs` lists the registered ones");
                    return Ok(ExitCode::FAILURE);
                };
                if let Some(problem) = &found.problem {
                    eprintln!("spec `{spec}` could not be read: {problem}");
                    return Ok(ExitCode::FAILURE);
                }
                for op in &found.operations {
                    println!(
                        "{:<7} {:<32} {:<24} {}",
                        op.method,
                        op.path,
                        op.operation_id,
                        op.summary.as_deref().unwrap_or("")
                    );
                }
                Ok(ExitCode::SUCCESS)
            }
            MorseCmd::Allow { host } => {
                let dit = open()?;
                let Some(host) = host else {
                    let hosts = dit.morse_allowed_hosts()?;
                    if hosts.is_empty() {
                        println!(
                            "no hosts allowed on this machine — nothing can be run until one is. \
                             `dit morse allow <host>` adds it"
                        );
                    }
                    for host in hosts {
                        println!("{host}");
                    }
                    return Ok(ExitCode::SUCCESS);
                };
                if dit.morse_allow(&host)? {
                    println!("{host} is now allowed on this machine, and nowhere else");
                } else {
                    println!("{host} was already allowed");
                }
                Ok(ExitCode::SUCCESS)
            }
            MorseCmd::Import { what } => {
                let mut dit = open()?;
                let me = me_for(&dit, explicit.as_deref());
                let (source, doc, spec, dry_run) = match what {
                    MorseImport::Curl {
                        command,
                        doc,
                        scenario,
                        spec,
                        dry_run,
                    } => (
                        // The shell already split the words; quote each one
                        // back so the importer reads the same words, not the
                        // pieces of a header that held a space.
                        dit_core::ImportSource::Curl {
                            command: shell_join(&command),
                            scenario,
                        },
                        doc,
                        spec,
                        dry_run,
                    ),
                    MorseImport::Postman {
                        file,
                        doc,
                        spec,
                        dry_run,
                    } => (
                        dit_core::ImportSource::Postman {
                            json: std::fs::read_to_string(&file)?,
                        },
                        doc,
                        spec,
                        dry_run,
                    ),
                    MorseImport::PostmanEnv { file } => {
                        let imported = dit.morse_import_env(&std::fs::read_to_string(&file)?)?;
                        println!(
                            "environment {} is on this machine, in {}",
                            imported.name,
                            dit_core::MORSE_LOCAL_PATH
                        );
                        for note in imported.notes {
                            println!("  note: {note}");
                        }
                        return Ok(ExitCode::SUCCESS);
                    }
                };
                let preview = dit.morse_import_preview(&source, spec.as_deref())?;
                for scenario in &preview.scenarios {
                    println!(
                        "{} ({}, {} step(s))",
                        scenario.name,
                        scenario.spec,
                        scenario.steps.len()
                    );
                    for step in &scenario.steps {
                        println!("  {}  {}", step.id, step.operation.qualified());
                    }
                    if !scenario.requires.is_empty() {
                        println!("  requires {}", scenario.requires.join(", "));
                    }
                }
                for note in &preview.notes {
                    println!("note: {note}");
                }
                if dry_run {
                    println!("\n--dry-run: nothing written");
                    return Ok(ExitCode::SUCCESS);
                }
                let done = dit.morse_import(&source, spec.as_deref(), &doc, &me)?;
                println!(
                    "\nwrote {} into {doc} as one commit",
                    done.scenarios.join(", ")
                );
                Ok(ExitCode::SUCCESS)
            }
            MorseCmd::Env { what } => {
                let dit = open()?;
                match what {
                    MorseEnv::Set {
                        name,
                        server,
                        vars,
                        unset,
                    } => {
                        let mut set = Vec::new();
                        for pair in vars {
                            let (k, v) = pair.split_once('=').ok_or_else(|| {
                                DitError::Refuse(format!("`{pair}` is not NAME=VALUE"))
                            })?;
                            set.push((k.trim().to_owned(), Some(v.to_owned())));
                        }
                        set.extend(unset.into_iter().map(|k| (k, None)));
                        let server = server.map(|s| Some(s).filter(|s| !s.trim().is_empty()));
                        dit.morse_edit_env(&name, dit_core::EnvEdit::Upsert { server, set })?;
                        println!("environment {name} saved in {}", dit_core::MORSE_LOCAL_PATH);
                    }
                    MorseEnv::Rm { name } => {
                        dit.morse_edit_env(&name, dit_core::EnvEdit::Delete)?;
                        println!("environment {name} removed");
                    }
                }
                Ok(ExitCode::SUCCESS)
            }
            MorseCmd::Run { scenario, env } => {
                let mut dit = open()?;
                let outcome = dit.morse_run(&scenario, env.as_deref())?;
                print_run(&outcome);
                Ok(if outcome.passed() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
            MorseCmd::Send {
                operation,
                env,
                params,
                query,
                headers,
                body,
                status,
            } => {
                let operation = dit_core::OperationRef::parse(&operation).ok_or_else(|| {
                    DitError::Refuse(format!("`{operation}` is not `<spec>/<operationId>`"))
                })?;
                let pairs = |items: &[String], sep: char| -> Result<Vec<_>, DitError> {
                    items
                        .iter()
                        .map(|item| {
                            let (k, v) = item.split_once(sep).ok_or_else(|| {
                                DitError::Refuse(format!("`{item}` is not `name{sep}value`"))
                            })?;
                            Ok((
                                k.trim().to_owned(),
                                dit_core::MorseValue::Str(v.trim().to_owned()),
                            ))
                        })
                        .collect()
                };
                let draft = dit_core::SendDraft {
                    target: dit_core::SendTarget::Operation(operation),
                    params: pairs(&params, '=')?,
                    query: pairs(&query, '=')?,
                    headers: pairs(&headers, ':')?,
                    body: body
                        .as_deref()
                        .map(dit_core::morse_value_from_json)
                        .transpose()?
                        .map(dit_core::RequestBody::Json),
                    expect: dit_core::Expect {
                        status,
                        json: Vec::new(),
                    },
                    capture: Vec::new(),
                };
                let mut dit = open()?;
                let outcome = dit.morse_send(&draft, env.as_deref())?;
                print_run(&outcome);
                Ok(if outcome.passed() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
            MorseCmd::Sync { scenario, env } => {
                let mut dit = open()?;
                let me = me_for(&dit, explicit.as_deref());
                let synced = dit.morse_sync(&scenario, env.as_deref(), &me)?;
                print_run(&synced.run);
                match &synced.moved_to {
                    Some(commit) => println!(
                        "\npinned {scenario} to {} and recorded it proven on {} in {}",
                        &commit[..7.min(commit.len())],
                        synced.env,
                        synced.path
                    ),
                    None => println!(
                        "\nthe pin was left where it was and no proof was recorded for {} — \
                         both move only on a green run, because each is a claim that this was proven",
                        synced.env
                    ),
                }
                Ok(if synced.moved_to.is_some() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
            MorseCmd::Check => {
                let dit = open()?;
                let report = dit.morse_report()?;
                for spec in report.specs.iter().filter(|s| s.problem.is_some()) {
                    println!(
                        "spec {} — {}",
                        spec.id,
                        spec.problem.as_deref().unwrap_or("")
                    );
                }
                if report.scenarios.is_empty() {
                    println!(
                        "no scenarios yet — write a `dit-morse` fence in a document under docs/"
                    );
                }
                for s in &report.scenarios {
                    let at = format!("{}:{}", s.path, s.line);
                    match &s.health {
                        dit_core::ScenarioHealth::Fresh => {
                            println!("fresh       {:<24} {at}", s.scenario);
                        }
                        dit_core::ScenarioHealth::Stale { commits } => {
                            println!(
                                "stale       {:<24} {at}  — `{}` has moved {commits} commit(s) \
                                 since this was checked",
                                s.scenario, s.spec_id
                            );
                        }
                        dit_core::ScenarioHealth::Broken { reasons } => {
                            println!("broken      {:<24} {at}", s.scenario);
                            for reason in reasons {
                                println!("            {reason}");
                            }
                        }
                        dit_core::ScenarioHealth::Unreadable { detail } => {
                            println!("unreadable  {:<24} {at}", s.scenario);
                            println!("            {detail}");
                        }
                    }
                    // Where it was proven, per environment (ADR 0024). An
                    // environment absent here was never proven — not assumed.
                    if s.proofs.is_empty() {
                        println!(
                            "            proven nowhere yet — `dit morse sync {} --env <name>`",
                            s.scenario
                        );
                    }
                    for p in &s.proofs {
                        let state = match &p.health {
                            dit_core::ProofHealth::Fresh => "holds".to_owned(),
                            dit_core::ProofHealth::Stale { commits } => {
                                format!("stale — the spec moved {commits} commit(s) since")
                            }
                            dit_core::ProofHealth::Broken { reason } => {
                                format!("broken — {reason}")
                            }
                        };
                        println!(
                            "            proven on {:<16} {} at {}: {state}",
                            p.env,
                            p.on,
                            &p.commit[..7.min(p.commit.len())]
                        );
                    }
                }
                // The callers' side (ADR 0025): read from the code map as
                // the last `dit code` command or reindex left it — a check
                // never starts a first parse of a large root.
                if !dit.config().code.is_empty() {
                    let api = dit.code_api()?;
                    if !api.calls.is_empty() {
                        println!(
                            "\ncode: {} orphan call(s) no spec describes, {} operation(s) called and \
                             proven nowhere — `dit code api`",
                            api.orphans().count(),
                            api.unproven().len()
                        );
                    }
                }
                // Stale never fails the check: it reports that the world
                // moved, which is the thing to know, not a thing to fix here.
                Ok(if report.is_clean() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                })
            }
        },
        Command::Flow { cmd } => match cmd {
            FlowCmd::List => {
                let dit = open()?;
                let flows = dit.flows()?;
                if flows.is_empty() {
                    println!(
                        "no flows yet — put an issue in one with `dit issue set REF flows=name`"
                    );
                    return Ok(ExitCode::SUCCESS);
                }
                for f in &flows {
                    println!("{:<3} {}", f.issues, f.name);
                }
                Ok(ExitCode::SUCCESS)
            }
            FlowCmd::Show { name } => {
                let dit = open()?;
                let want: Option<&str> = if name == "all" { None } else { Some(&name) };
                let board = dit.flow_board(want)?;
                if board.nodes.is_empty() {
                    println!(
                        "flow `{name}` has no members — `dit issue set REF flows={name}` adds one"
                    );
                    return Ok(ExitCode::SUCCESS);
                }
                println!(
                    "{} — {} member(s), {} {}(s), {} lane(s)",
                    want.unwrap_or("(all flows)"),
                    board.nodes.len(),
                    board.stages,
                    if board.phases.is_empty() {
                        "stage"
                    } else {
                        "phase"
                    },
                    board.lanes.len()
                );
                // A fence that is there and unreadable is said out loud: the
                // columns below are computed, and the reader should know why.
                if let Some(problem) = &board.shape_problem {
                    println!(
                        "\n! {}:{} — {} (columns below are computed from blocked_by)",
                        problem.path, problem.line, problem.detail
                    );
                }
                let by_id: std::collections::HashMap<dit_core::IssueId, &dit_core::FlowNode> =
                    board.nodes.iter().map(|n| (n.id, n)).collect();
                let on_main: std::collections::HashSet<&dit_core::IssueId> =
                    board.main_path.iter().collect();
                for stage in 0..board.stages {
                    // With a fence the columns carry the names the team
                    // chose; without one they are the computed ranks.
                    match board.phases.get(stage) {
                        Some(phase) => println!("\n{} ({})", phase.label, phase.id),
                        None if board.phases.is_empty() => println!("\nstage {stage}"),
                        None => println!("\nunphased"),
                    }
                    let mut stage_nodes: Vec<&dit_core::FlowNode> =
                        board.nodes.iter().filter(|n| n.stage == stage).collect();
                    stage_nodes.sort_by_key(|n| (n.lane.clone().unwrap_or_default(), n.row));
                    for n in stage_nodes {
                        let handle = n
                            .number
                            .map(|h| format!("#{h}"))
                            .unwrap_or_else(|| n.short_ref.clone());
                        // `NotPickable` covers both "someone is on it" and
                        // "it is finished", and reading a done issue as
                        // in-flight is the difference between a flow that is
                        // landing and one that is stuck.
                        let done = n.category == Some(dit_core::StatusCategory::Done);
                        let mark = match n.readiness {
                            _ if done => "done",
                            dit_core::Readiness::Ready => "ready",
                            dit_core::Readiness::NotPickable => "in-flight",
                            dit_core::Readiness::Blocked { .. } => "blocked",
                            dit_core::Readiness::Unproven { .. } => "unproven",
                        };
                        let main = if on_main.contains(&n.id) { "*" } else { " " };
                        let claim = n
                            .claim
                            .as_ref()
                            .map(|c| {
                                format!(
                                    " [{}{}]",
                                    c.claimed_by,
                                    if c.stale { " stale" } else { "" }
                                )
                            })
                            .unwrap_or_default();
                        println!(
                            " {main} {handle:<9} {:<8} {mark:<8} {}{claim}",
                            n.lane.as_deref().unwrap_or("-"),
                            n.title
                        );
                        let _ = &by_id;
                    }
                }
                if board.main_path.len() > 1 {
                    let handles: Vec<String> = board
                        .main_path
                        .iter()
                        .filter_map(|id| {
                            by_id.get(id).map(|n| {
                                n.number
                                    .map(|h| format!("#{h}"))
                                    .unwrap_or_else(|| n.short_ref.clone())
                            })
                        })
                        .collect();
                    println!("\ncritical path (*): {}", handles.join(" -> "));
                }
                Ok(ExitCode::SUCCESS)
            }
        },
        Command::MergeDriver {
            base,
            ours,
            theirs,
            marker_size,
            label,
        } => {
            let args = vec![
                base.to_string_lossy().into_owned(),
                ours.to_string_lossy().into_owned(),
                theirs.to_string_lossy().into_owned(),
                marker_size,
                label,
            ];
            Ok(ExitCode::from(
                u8::try_from(dit_core::run_merge_driver(&args)).unwrap_or(1),
            ))
        }
    }
}

fn issue(cmd: Issue, explicit: Option<&str>) -> Result<ExitCode, DitError> {
    match cmd {
        Issue::New {
            title,
            kind,
            status,
            priority,
            assignee,
            label,
            estimate,
            body,
            template,
            lane,
            flows,
        } => {
            let title = title.join(" ");
            if title.trim().is_empty() {
                eprintln!("dit: a title is required");
                return Ok(ExitCode::from(2));
            }
            let mut dit = open()?;
            let me = me_for(&dit, explicit);
            let mut tx = dit.transaction(&me)?;
            let draft = IssueDraft {
                title: title.clone(),
                kind: kind.into(),
                status,
                priority: priority.map(Into::into),
                reporter: Some(me.to_owned()),
                assignees: assignee,
                labels: label,
                epic: None,
                estimate,
                sprint: None,
                due: None,
                start: None,
                blocked_by: vec![],
                fed_by: vec![],
                lane,
                flows,
                body: body.unwrap_or_default(),
                // The number is facade-owned (ADR 0007): numbering policy
                // assigns it inside the transaction, never the caller.
                number: None,
            };
            let id = match template {
                Some(name) => tx.create_issue_from_template(draft, &name)?,
                None => tx.create_issue(draft)?,
            };
            tx.commit(&format!("create {title}"))?;
            // Read the stored issue back for the number the facade assigned;
            // `#N` is the handle a human reads, the short ref the one a
            // script can rely on forever.
            let stored = dit.get(id.as_str())?;
            let short = id.short_ref().as_str().to_owned();
            match stored.and_then(|hit| hit.issue.number) {
                Some(n) => println!("#{n} {short} {title}"),
                None => println!("{short} {title}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        Issue::Show { reference } => {
            let dit = open()?;
            let Some(hit) = dit.get(&reference)? else {
                eprintln!("dit: no issue matches `{reference}`");
                return Ok(ExitCode::from(2));
            };
            let id = hit.issue.id;
            print_issue(&hit);
            let comments = dit.comments(&id)?;
            if !comments.is_empty() {
                println!("\n-- comments --");
                for c in &comments {
                    // The id prefix is what `--reply` takes, so it belongs in
                    // the listing — a thread you cannot reference you cannot
                    // answer (§4.4).
                    let handle = &c.id.as_str()[..10];
                    match c.reply_to {
                        Some(parent) => {
                            let parent = &parent.as_str()[..10];
                            println!(
                                "{} {} ({} -> {}):\n  {}",
                                c.author,
                                c.created,
                                handle,
                                parent,
                                c.body.replace('\n', "\n  ")
                            );
                        }
                        None => {
                            println!(
                                "{} {} ({}):\n  {}",
                                c.author,
                                c.created,
                                handle,
                                c.body.replace('\n', "\n  ")
                            );
                        }
                    }
                }
            }
            let history = dit.history(&id, None)?;
            if !history.is_empty() {
                println!("\n-- history --");
                for e in history.iter().rev().take(15).rev() {
                    println!(
                        "  {} {}: {} -> {}  ({})",
                        &e.ts[..10.min(e.ts.len())],
                        e.field,
                        e.old_value.as_deref().unwrap_or("-"),
                        e.new_value.as_deref().unwrap_or("-"),
                        e.author,
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Issue::Set {
            reference,
            fields,
            force,
        } => {
            let (mut patch, refs) = match parse_patch(&fields) {
                Ok(p) => p,
                Err(msg) => {
                    eprintln!("dit: {msg}");
                    return Ok(ExitCode::from(2));
                }
            };
            if patch.is_empty() && refs.is_empty() {
                eprintln!("dit: nothing to set");
                return Ok(ExitCode::from(2));
            }
            let mut dit = open()?;
            // Reference-typed values (`epic=`, `blocked_by=`) resolve through
            // the workspace's index after open: `#N` and short refs work, and
            // an ambiguous number is refused naming its candidates (ADR 0018).
            if let Err(msg) = resolve_reference_fields(&dit, &mut patch, refs) {
                eprintln!("dit: {msg}");
                return Ok(ExitCode::from(2));
            }
            let id = resolve(&dit, &reference)?;
            let me = me_for(&dit, explicit);
            let mut tx = dit.transaction(&me)?;
            tx.set_fields_opts(&id, patch, force)?;
            tx.commit(&format!("update {reference}"))?;
            println!("updated {}", id.short_ref().as_str());
            Ok(ExitCode::SUCCESS)
        }
        Issue::Comment {
            reference,
            text,
            reply,
            template,
        } => {
            let mut body = text.join(" ");
            if let Some(name) = &template {
                // An evidence report (or any templated comment) is drafted in
                // the editor and posted from what comes back; posting the
                // untouched skeleton would be noise wearing a report's shape.
                if !body.trim().is_empty() {
                    eprintln!("dit: pass either text or --template, not both");
                    return Ok(ExitCode::from(2));
                }
                let dit = open()?;
                let Some(path) = dit.template_path(name) else {
                    return Err(DitError::TemplateMissing(name.clone()));
                };
                let skeleton = std::fs::read_to_string(&path)?;
                let draft = edit_temp_draft(&skeleton)?;
                if draft.trim() == skeleton.trim() {
                    eprintln!(
                        "dit: the `{name}` template came back untouched — fill it in, or drop \
                         --template to write freely"
                    );
                    return Ok(ExitCode::from(2));
                }
                body = draft;
            }
            if body.trim().is_empty() {
                eprintln!("dit: a comment needs text");
                return Ok(ExitCode::from(2));
            }
            let mut dit = open()?;
            let id = resolve(&dit, &reference)?;
            // A reply must name a comment on this very issue — the thread
            // lives on the issue under discussion, nowhere else.
            let parent = match &reply {
                Some(needle) => {
                    let comments = dit.comments(&id)?;
                    let hits: Vec<_> = comments
                        .iter()
                        .filter(|c| {
                            c.id.as_str() == needle || c.id.as_str().starts_with(needle.as_str())
                        })
                        .collect();
                    match hits.as_slice() {
                        [one] => Some(one.id),
                        [] => {
                            eprintln!("dit: no comment on this issue matches `{needle}`");
                            return Ok(ExitCode::from(2));
                        }
                        many => {
                            eprintln!(
                                "dit: `{needle}` matches {} comments — use more of the id",
                                many.len()
                            );
                            return Ok(ExitCode::from(2));
                        }
                    }
                }
                None => None,
            };
            let me = me_for(&dit, explicit);
            let mut tx = dit.transaction(&me)?;
            tx.comment(&id, &me, parent.as_ref(), &body)?;
            tx.commit(&format!("comment on {reference}"))?;
            println!("commented on {}", id.short_ref().as_str());
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Draft a comment in $EDITOR: write `seed` to a temp file, hand it to the
/// editor, return what survives. The file lives in the system temp dir — a
/// draft is scratch, never workspace content — and is written through the
/// sanctioned atomic writer like everything else (invariant I1).
fn edit_temp_draft(seed: &str) -> Result<String, DitError> {
    let path = std::env::temp_dir().join(format!("dit-comment-{}.md", std::process::id()));
    dit_core::atomic::write(&path, seed)?;
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_owned());
    let status = std::process::Command::new(&editor).arg(&path).status()?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    if !status.success() {
        return Err(DitError::Refuse(format!("editor ({editor}) failed")));
    }
    Ok(text)
}

/// The `templates` subcommand. Templates are plain files: `list` reads the
/// directory the facade seeds, `edit` hands one to $EDITOR. The edit lands
/// as an uncommitted working-tree change the user reviews — same as any
/// other file they edit.
fn templates(cmd: Templates) -> Result<ExitCode, DitError> {
    match cmd {
        Templates::List => {
            let dit = open()?;
            let names = dit.templates();
            if names.is_empty() {
                println!("(no templates in .dit/templates/)");
            } else {
                for name in names {
                    println!("{name}");
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Templates::Edit { name } => {
            let dit = open()?;
            let Some(path) = dit.template_path(&name) else {
                return Err(DitError::TemplateMissing(name));
            };
            let editor = std::env::var("EDITOR")
                .or_else(|_| std::env::var("VISUAL"))
                .unwrap_or_else(|_| "vi".to_owned());
            let status = std::process::Command::new(&editor).arg(&path).status()?;
            if !status.success() {
                eprintln!("dit: editor ({editor}) failed");
                return Ok(ExitCode::from(1));
            }
            println!(
                "edited {} — commit it to share the template",
                path.display()
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The handle a human reads for an issue (ADR 0007): `#12` when the
/// workspace numbered it, the short ref otherwise.
fn handle(i: &dit_core::Issue) -> String {
    match i.number {
        Some(n) => format!("#{n}"),
        None => i.id.short_ref().as_str().to_owned(),
    }
}
/// Turn `field=value` strings into a patch plus the reference-typed values
/// that still need the workspace to resolve (`epic=`, comma-separated
/// `blocked_by=`, `fed_by=`): `#N` and short refs are accepted there, and an ambiguous
/// number is refused naming its candidates (ADR 0018). Values are validated
/// here so the user gets the name of the offending field, not a store error
/// from deep inside the write path.
/// A reference-typed field value awaiting workspace resolution.
type RefValue = (&'static str, String);

fn parse_patch(fields: &[String]) -> Result<(FieldPatch, Vec<RefValue>), String> {
    let mut patch = FieldPatch::default();
    let mut refs: Vec<(&'static str, String)> = Vec::new();
    for f in fields {
        let (key, value) = f
            .split_once('=')
            .ok_or_else(|| format!("`{f}` is not field=value"))?;
        // `due=` with nothing after it clears an optional field — the same
        // three states the API offers, spelled the shell's way.
        if value.is_empty() {
            let field = match key {
                "priority" => dit_core::ClearableField::Priority,
                "epic" => dit_core::ClearableField::Epic,
                "estimate" => dit_core::ClearableField::Estimate,
                "sprint" => dit_core::ClearableField::Sprint,
                "due" => dit_core::ClearableField::Due,
                "start" => dit_core::ClearableField::Start,
                "lane" => dit_core::ClearableField::Lane,
                "env" => dit_core::ClearableField::Env,
                other => return Err(format!("`{other}` cannot be cleared — give it a value")),
            };
            patch.clear.push(field);
            continue;
        }
        match key {
            "title" => patch.title = Some(value.to_owned()),
            "type" | "kind" => {
                patch.kind = Some(match value {
                    "task" => IssueKind::Task,
                    "bug" => IssueKind::Bug,
                    "story" => IssueKind::Story,
                    "spike" => IssueKind::Spike,
                    "chore" => IssueKind::Chore,
                    other => return Err(format!("`{other}` is not a type")),
                });
            }
            "status" => patch.status = Some(value.to_owned()),
            "priority" => {
                patch.priority = Some(match value {
                    "p0" => Priority::P0,
                    "p1" => Priority::P1,
                    "p2" => Priority::P2,
                    "p3" => Priority::P3,
                    "p4" => Priority::P4,
                    other => return Err(format!("`{other}` is not a priority")),
                });
            }
            "reporter" => patch.reporter = Some(value.to_owned()),
            // Reference-typed: resolve against the index after open.
            "epic" => refs.push(("epic", value.to_owned())),
            "blocked_by" => refs.push(("blocked_by", value.to_owned())),
            "fed_by" => refs.push(("fed_by", value.to_owned())),
            "assignees" => patch.assignees = Some(split_list(value)),
            "flows" => patch.flows = Some(split_list(value)),
            "needs_scenarios" => patch.needs_scenarios = Some(split_list(value)),
            "proves" => patch.proves = Some(split_list(value)),
            "env" => patch.env = Some(value.to_owned()),
            "labels" => patch.labels = Some(split_list(value)),
            "sprint" => patch.sprint = Some(value.to_owned()),
            "due" => patch.due = Some(value.to_owned()),
            "start" => patch.start = Some(value.to_owned()),
            "lane" => patch.lane = Some(value.to_owned()),
            "estimate" => {
                patch.estimate = Some(
                    value
                        .parse()
                        .map_err(|_| format!("`{value}` is not a number"))?,
                );
            }
            other => return Err(format!("unknown field `{other}`")),
        }
    }
    Ok((patch, refs))
}

/// Fill the reference-typed patch fields from their raw `field=value`
/// strings: each `#N`/short-ref/full-id resolves through the workspace.
fn resolve_reference_fields(
    dit: &Dit,
    patch: &mut FieldPatch,
    refs: Vec<(&'static str, String)>,
) -> Result<(), String> {
    for (key, raw) in refs {
        match key {
            "epic" => match dit.resolve(&raw) {
                Ok(id) => patch.epic = Some(id),
                Err(e) => return Err(e.to_string()),
            },
            "blocked_by" | "fed_by" => {
                let mut ids = Vec::new();
                for one in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    match dit.resolve(one) {
                        Ok(id) => ids.push(id),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                if key == "blocked_by" {
                    patch.blocked_by = Some(ids);
                } else {
                    patch.fed_by = Some(ids);
                }
            }
            _ => unreachable!("parse_patch only emits epic, blocked_by and fed_by refs"),
        }
    }
    Ok(())
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Resolve a reference to exactly one issue (ADR 0018): full id, short ref
/// or `#N`, with ambiguity rejected naming the candidates.
fn resolve(dit: &Dit, reference: &str) -> Result<IssueId, DitError> {
    dit.resolve(reference)
}

/// Ask the OS to open `url`. A failure here must not take the server down:
/// the URL is already on stdout, and a detached opener process is not worth
/// an exit code.
fn open_browser(url: &str) {
    use std::io::IsTerminal;
    // Piped stdout means a script or a test is driving dit — a browser
    // window opening on behalf of a pipeline is a surprise nobody wants.
    if !std::io::stdout().is_terminal() {
        return;
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(url).spawn();
}

fn open() -> Result<Dit, DitError> {
    let cwd = std::env::current_dir()?;
    Dit::open(&cwd)
}

/// `dit code …`: refresh the map to HEAD, then answer from the index.
fn code(cmd: CodeCmd, explicit: Option<&str>) -> Result<ExitCode, DitError> {
    let cwd = std::env::current_dir()?;
    if let CodeCmd::Hook { cmd } = &cmd {
        let (verb, hooks) = match cmd {
            HookCmd::Install => ("installed in", dit_core::code::install_code_hooks(&cwd)?),
            HookCmd::Uninstall => ("removed from", dit_core::code::uninstall_code_hooks(&cwd)?),
        };
        if hooks.is_empty() {
            println!("nothing to change — the hooks are already as asked");
        } else {
            println!("background refresh {verb}: {}", hooks.join(", "));
        }
        return Ok(ExitCode::SUCCESS);
    }
    let mut dit = Dit::open_code(&cwd)?;
    if matches!(cmd, CodeCmd::Refresh { full: true }) {
        dit.invalidate_code_map()?;
    }
    let report = dit.refresh_code()?;
    for problem in &report.problems {
        eprintln!("{problem}");
    }
    if report.roots == 0 {
        println!(
            "no code roots registered — add one under `code:` in .dit/config.yaml, e.g.\n  code:\n    - {{ id: web, include: [\"src/**\"] }}"
        );
        return Ok(ExitCode::SUCCESS);
    }
    // One root: the prefix says nothing, and every line of every answer
    // would pay for it.
    let single = report.roots == 1;
    let at = |root: &str, path: &str| {
        if single {
            path.to_owned()
        } else {
            format!("{root}:{path}")
        }
    };
    match cmd {
        // Answered above, before the map is opened.
        CodeCmd::Hook { .. } => {}
        CodeCmd::Check => return code_check(&dit),
        CodeCmd::Api { all } => print_code_api(&dit.code_api()?, all),
        CodeCmd::Map {
            cmd: MapCmd::Confirm { map },
        } => {
            let me = me_for(&dit, explicit);
            let pins = dit.code_map_confirm(&map, &me)?;
            let said: Vec<String> = pins
                .iter()
                .map(|(root, c)| format!("{root} at {}", &c[..7.min(c.len())]))
                .collect();
            println!("confirmed map {map}: {}", said.join(", "));
        }
        CodeCmd::Refresh { .. } => {
            println!(
                "{} root(s), {} file(s): {} parsed, {} from cache, {} removed, {} unresolved import(s)",
                report.roots,
                report.files,
                report.parsed,
                report.reused,
                report.removed,
                report.unresolved
            );
        }
        CodeCmd::Uses {
            name,
            calls: show_calls,
        } => {
            let uses = dit.code_uses(&name)?;
            println!("{}", at(&uses.root, &uses.path));
            let braces = |names: &[String]| {
                if names.is_empty() {
                    String::new()
                } else {
                    format!(" {{{}}}", names.join(", "))
                }
            };
            // Resolved imports by the file they reach — the specifier is how
            // it was spelled, the target is what an agent opens.
            let mut external = Vec::new();
            let mut unresolved = Vec::new();
            for i in &uses.imports {
                let mark = if i.reexport { "re-exports " } else { "" };
                match (&i.target, i.external) {
                    (Some(t), _) => println!("  {mark}{t}{}", braces(&i.names)),
                    (None, true) => external.push(format!("{}{}", i.specifier, braces(&i.names))),
                    (None, false) => unresolved.push(i.specifier.clone()),
                }
            }
            if !external.is_empty() {
                println!("  external: {}", external.join(", "));
            }
            if !unresolved.is_empty() {
                println!("  unresolved: {}", unresolved.join(", "));
            }
            // Calls beyond what the imports already name.
            let imported: HashSet<&str> = uses
                .imports
                .iter()
                .flat_map(|i| i.names.iter().map(String::as_str))
                .collect();
            let mut calls: Vec<&str> = Vec::new();
            for c in &uses.calls {
                if !imported.contains(c.as_str()) && !calls.contains(&c.as_str()) {
                    calls.push(c);
                }
            }
            if show_calls && !calls.is_empty() {
                let shown: Vec<&str> = calls.iter().take(MAX_CALLS_SHOWN).copied().collect();
                let more = calls.len().saturating_sub(MAX_CALLS_SHOWN);
                let tail = if more > 0 {
                    format!(" +{more} more")
                } else {
                    String::new()
                };
                println!("  also calls: {}{tail}", shown.join(", "));
            }
        }
        CodeCmd::Users { name } => {
            let users = dit.code_users(&name)?;
            if users.is_empty() {
                println!("nothing indexed uses `{name}`");
            }
            for u in &users {
                let via = u
                    .via
                    .as_deref()
                    .map(|v| format!("  via {v}"))
                    .unwrap_or_default();
                println!("{}:{}{via}", at(&u.root, &u.path), u.line);
            }
            println!("{} user(s)", users.len());
        }
        CodeCmd::Path { from, to } => {
            let chain = dit.code_path(&from, &to)?;
            if chain.is_empty() {
                println!("no import chain from `{from}` to `{to}`");
            } else {
                println!("{}", chain.join("\n  → "));
            }
        }
        CodeCmd::Explain { name } => {
            let e = dit.code_explain(&name)?;
            let what = e
                .symbol
                .as_deref()
                .map(|s| format!("{s} in "))
                .unwrap_or_default();
            println!("{what}{}", at(&e.root, &e.path));
            if e.generated {
                println!("  GENERATED — never the place to edit; change its source and regenerate");
            }
            if !e.defines.is_empty() {
                println!("  exports {}", e.defines.join(", "));
            }
            println!(
                "  imports {} module(s); used by {} file(s)",
                e.imports,
                e.users.len()
            );
            for u in e.users.iter().take(10) {
                println!("    {}", at(&u.root, &u.path));
            }
        }
        CodeCmd::Hubs {
            root,
            limit,
            generated,
        } => {
            for h in dit.code_hubs(root.as_deref(), limit, generated)? {
                println!("{:>5}  {}", h.users, at(&h.root, &h.path));
            }
        }
        CodeCmd::Where { words, limit } => {
            let text = words.join(" ");
            let hits = dit.code_where(&text, limit)?;
            if hits.is_empty() {
                println!("nothing indexed matches `{text}`");
            }
            for h in &hits {
                let line = h
                    .symbols
                    .first()
                    .map_or(String::new(), |(_, l)| format!(":{l}"));
                let names: Vec<&str> = h.symbols.iter().take(4).map(|(n, _)| n.as_str()).collect();
                let more = h.symbols.len().saturating_sub(4);
                let tail = if more > 0 {
                    format!(" +{more}")
                } else {
                    String::new()
                };
                let generated = if h.generated { " [generated]" } else { "" };
                println!(
                    "{}{line}  {}{tail}{generated}",
                    at(&h.root, &h.path),
                    names.join(", ")
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// How many further calls `dit code uses` names before it counts the rest.
const MAX_CALLS_SHOWN: usize = 12;

fn print_code_api(report: &dit_core::ApiReport, all: bool) {
    if report.operations == 0 {
        println!(
            "no spec operations to match against — register a spec under `specs:` and run `dit reindex`"
        );
        return;
    }
    let orphans: Vec<_> = report.orphans().collect();
    let unproven = report.unproven();
    let matched = report.calls.len() - orphans.len();
    println!(
        "{} literal call(s) into {}: {matched} matched, {} orphan; {} operation(s) called and proven nowhere",
        report.calls.len(),
        report.roots.iter().map(|r| format!("/{r}")).collect::<Vec<_>>().join(", "),
        orphans.len(),
        unproven.len()
    );
    if !orphans.is_empty() {
        println!("\norphan — no registered spec describes it:");
        for c in &orphans {
            println!("  {}:{}:{}  {}", c.root, c.path, c.line, c.resolved);
        }
    }
    if !unproven.is_empty() {
        println!("\ncalled, proven nowhere:");
        // One line per path: the methods sharing it are one seam to prove.
        let mut paths: Vec<(String, String, Vec<String>)> = Vec::new();
        for o in &unproven {
            match paths
                .iter_mut()
                .find(|(s, p, _)| s == &o.spec && p == &o.path)
            {
                Some(entry) => entry.2.push(o.method.clone()),
                None => paths.push((o.spec.clone(), o.path.clone(), vec![o.method.clone()])),
            }
        }
        for (spec, path, methods) in paths {
            println!("  {:<24} {path}  ({spec})", methods.join(","));
        }
    }
    if all {
        println!("\nevery call:");
        for c in &report.calls {
            let ops: Vec<String> = c
                .operations
                .iter()
                .map(|o| {
                    let proven = if o.proven.is_empty() {
                        "unproven".to_owned()
                    } else {
                        format!("proven on {}", o.proven.join(", "))
                    };
                    format!("{} {} ({proven})", o.method, o.path)
                })
                .collect();
            let target = if ops.is_empty() {
                "orphan".to_owned()
            } else {
                ops.join("; ")
            };
            println!(
                "  {}:{}:{}  {}  → {target}",
                c.root, c.path, c.line, c.resolved
            );
        }
    }
    println!(
        "\nread from literals: a path assembled at runtime from variables is not seen, a call is matched by path, not method, and {} literal(s) too generic to name one path were left out",
        report.generic
    );
}

fn code_check(dit: &dit_core::Dit) -> Result<ExitCode, DitError> {
    use dit_core::MapHealth;
    let problems = dit.code_map_problems()?;
    let entries = dit.code_map_report()?;
    if problems.is_empty() && entries.is_empty() {
        println!("no `dit-map` fences yet — see `dit ai spec code`");
        return Ok(ExitCode::SUCCESS);
    }
    let mut failed = false;
    for (map, path, line, problem) in &problems {
        failed = true;
        println!("unreadable  {map}  {path}:{line}\n    {problem}");
    }
    for e in &entries {
        let verdict = match &e.health {
            MapHealth::Holds => "holds".to_owned(),
            MapHealth::Unconfirmed => "unconfirmed".to_owned(),
            MapHealth::Stale { commits } => format!("stale ({commits} commit(s) to the example)"),
            MapHealth::Broken { .. } => {
                failed = true;
                "broken".to_owned()
            }
        };
        println!(
            "{verdict:<11} {}  {}  ({}:{})",
            e.map, e.task, e.path, e.line
        );
        if let MapHealth::Broken { reasons } = &e.health {
            for r in reasons {
                println!("    {r}");
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Refuse a workspace-only command outside a workspace — before `open`,
/// which would leave a `.dit-cache/` behind in a repository that is not one.
fn ensure_workspace() -> Result<(), DitError> {
    let cwd = std::env::current_dir()?;
    if Dit::is_workspace(&cwd)? {
        Ok(())
    } else {
        Err(Dit::not_a_workspace())
    }
}

/// The alias the user named explicitly — the flag, then `$DIT_ME`. `None`
/// means "ask the workspace", which needs an open `Dit` (see [`me_for`]).
fn alias(cli: &Cli) -> Option<String> {
    cli.me.clone().or_else(|| std::env::var("DIT_ME").ok())
}

/// The alias writes are attributed to: what the user said, else what this
/// clone saved (the settings panel's `me`), else the machine's `$USER`.
fn me_for(dit: &Dit, explicit: Option<&str>) -> String {
    explicit
        .map(str::to_owned)
        .or_else(|| dit.me())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// One run, step by step. A red step says what it wanted and what it got,
/// because "failed" on its own sends someone back to the terminal to guess.
fn print_run(outcome: &dit_core::RunOutcome) {
    if let Some(refused) = &outcome.refused {
        eprintln!("{refused}");
        return;
    }
    for step in &outcome.steps {
        let status = step
            .status
            .map_or_else(|| "---".to_owned(), |s| s.to_string());
        println!(
            "{}  {:<6} {status:<4} {:>5}ms  {}",
            if step.passed() { "ok  " } else { "FAIL" },
            step.method,
            step.duration_ms,
            step.url
        );
        if let Some(error) = &step.error {
            println!("      {error}");
        }
        for failure in &step.failures {
            println!("      {failure}");
        }
        for (name, value) in &step.captured {
            // Captured values are printed: a run is a debugging session, and
            // hiding what was carried forward is what makes one long.
            println!("      captured {name} = {value}");
        }
        // So is the body — this terminal asked, and it is the one place a
        // response may be read (§20.7). The page never receives it.
        if let Some(body) = step.body.as_deref().filter(|b| !b.is_empty()) {
            for line in body.lines() {
                println!("      | {line}");
            }
        }
    }
    if outcome.passed() {
        println!("\n{} step(s), all green", outcome.steps.len());
    }
}

fn print_list(hits: &[IndexedIssue]) {
    for hit in hits {
        let i = &hit.issue;
        println!(
            "{:<7}  {:<11} {:<4} {}",
            handle(i),
            i.status,
            i.priority
                .map(|p| p.as_str().to_owned())
                .unwrap_or_else(|| "-".into()),
            i.title,
        );
    }
}

fn print_board(board: &dit_core::Board) {
    for col in &board.columns {
        println!("{} ({})", col.label, col.issues.len());
        for i in &col.issues {
            println!("  {}  {}", handle(&i.issue), i.issue.title);
        }
    }
}

fn print_status(dit: &Dit) {
    let s = dit.status();
    println!(
        "branch {} head {} {}",
        s.branch,
        &s.head[..7.min(s.head.len())],
        if s.dirty { "(dirty)" } else { "(clean)" },
    );
}

fn print_issue(hit: &IndexedIssue) {
    let i = &hit.issue;
    println!("{}  {}", handle(i), i.title);
    if i.number.is_some() {
        // The short ref is the permanent identifier; the number is only the
        // display handle, so `show` is where both meet.
        println!("ref: {}", i.id.short_ref().as_str());
    }
    println!(
        "type: {}  status: {}  priority: {}",
        i.kind.as_str(),
        i.status,
        i.priority.map(|p| p.as_str()).unwrap_or("-"),
    );
    if !i.assignees.is_empty() {
        println!("assignees: {}", i.assignees.join(", "));
    }
    if !i.labels.is_empty() {
        println!("labels: {}", i.labels.join(", "));
    }
    println!("created: {}  updated: {}", i.created, i.updated);
    if !i.body.trim().is_empty() {
        println!();
        for line in i.body.lines() {
            println!("{line}");
        }
    }
}

/// Words joined back into one command line that splits into the same words:
/// each one single-quoted when it holds anything a shell would treat
/// specially, a `'` inside written as `'\''`.
fn shell_join(words: &[String]) -> String {
    words
        .iter()
        .map(|w| {
            let plain = !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
            if plain {
                w.clone()
            } else {
                format!("'{}'", w.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::shell_join;

    #[test]
    fn words_the_shell_split_are_joined_back_into_the_same_words() {
        // `dit morse import curl -- curl -H 'Authorization: Bearer x'`
        // arrives as separate words; joined plainly, the header split in two.
        let words: Vec<String> = [
            "curl",
            "-H",
            "Authorization: Bearer x",
            "it's",
            "https://a.test/p?q=1",
        ]
        .iter()
        .map(|w| (*w).to_owned())
        .collect();
        assert_eq!(
            shell_join(&words),
            "curl -H 'Authorization: Bearer x' 'it'\\''s' 'https://a.test/p?q=1'"
        );
    }
}
