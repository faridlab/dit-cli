//! DIT command-line interface. The CLI is not a second-class citizen: it and
//! the server share exactly the same `dit-core`, so anything doable in the
//! browser is doable in a terminal and vice versa.

// The whole workspace bans printing so library crates stay silent; this
// crate IS the printer, so the standard output macros are its job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

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
    /// in workflow.yaml, peer protocol section in CLAUDE.md. Idempotent.
    Init {
        /// Lane ids to register, comma-separated. Default: backend,frontend.
        #[arg(long, value_delimiter = ',')]
        lanes: Vec<String>,
    },
    /// List the registered lanes and the coordination knobs.
    Lanes,
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
    /// Add a comment, or a reply with `--reply <comment ref>`.
    Comment {
        reference: String,
        /// The comment text; multiple words are joined into one paragraph.
        text: Vec<String>,
        /// The parent comment this replies to: its id or 7-char short form.
        #[arg(long)]
        reply: Option<String>,
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
        Command::Init { layout } => {
            let cwd = std::env::current_dir()?;
            let exe = std::env::current_exe()?;
            let dit = Dit::init_with_layout(&cwd, &exe, layout.into())?;
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
            let dit = open()?;
            // The same token file the standalone server reads, so `dit ui`
            // and `dit-server` hand the same URL shape for one workspace.
            let token = dit_server::config::load_or_create_token(&dit.root().join(".dit-cache"))?;
            let me = me_for(&dit, explicit.as_deref());
            let state = dit_server::AppState::with_bind_host(dit, &me, &token, &host);
            // Catch the index up, then watch for other processes' writes
            // (ADR 0017) — `dit ui` must live-update just like the server.
            state.start_live_updates();
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
        Command::Workflow { cmd } => match cmd {
            WorkflowCmd::Init { lanes } => {
                let mut dit = open()?;
                let specs: Vec<LaneSpec> = (if lanes.is_empty() {
                    vec!["backend".to_owned(), "frontend".to_owned()]
                } else {
                    lanes
                })
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
                if report.protocol_written {
                    println!("wrote the peer protocol section into CLAUDE.md");
                }
                if !report.schema_created
                    && !report.lanes_written
                    && !report.coordination_written
                    && !report.protocol_written
                {
                    println!("nothing to do — the coordination plane is already scaffolded");
                }
                Ok(ExitCode::SUCCESS)
            }
            WorkflowCmd::Lanes => {
                let dit = open()?;
                let board = dit.workflow_board()?;
                if board
                    .lanes
                    .iter()
                    .all(|l| l.cards.is_empty() && l.id.is_some())
                {
                    // Still worth listing: the registry exists even when empty.
                }
                for lane in &board.lanes {
                    match &lane.id {
                        Some(id) => println!(
                            "{:<10} {:<12} owners: {}  issues: {}",
                            id,
                            lane.label,
                            if lane.owners.is_empty() {
                                "-".to_owned()
                            } else {
                                lane.owners.join(", ")
                            },
                            lane.cards.len()
                        ),
                        None => println!(
                            "{:<10} {:<12} owners: -  issues: {}",
                            "(none)",
                            lane.label,
                            lane.cards.len()
                        ),
                    }
                }
                println!("claim TTL: {} minutes", board.claim_ttl_minutes);
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
            if ready.is_empty() {
                println!(
                    "nothing ready{}",
                    lane.as_deref()
                        .map(|l| format!(" in lane {l}"))
                        .unwrap_or_default()
                );
                return Ok(ExitCode::SUCCESS);
            }
            for hit in &ready {
                let issue = &hit.issue.issue;
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
            }
            Ok(ExitCode::SUCCESS)
        }
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
                lane,
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
                    println!(
                        "{} {}:\n  {}",
                        c.author,
                        c.created,
                        c.body.replace('\n', "\n  ")
                    );
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
        } => {
            let body = text.join(" ");
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
/// `blocked_by=`): `#N` and short refs are accepted there, and an ambiguous
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
            "assignees" => patch.assignees = Some(split_list(value)),
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
            "blocked_by" => {
                let mut ids = Vec::new();
                for one in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    match dit.resolve(one) {
                        Ok(id) => ids.push(id),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                patch.blocked_by = Some(ids);
            }
            _ => unreachable!("parse_patch only emits epic and blocked_by refs"),
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
