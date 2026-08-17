//! DIT command-line interface. The CLI is not a second-class citizen: it and
//! the server share exactly the same `dit-core`, so anything doable in the
//! browser is doable in a terminal and vice versa.

// The whole workspace bans printing so library crates stay silent; this
// crate IS the printer, so the standard output macros are its job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dit_core::{
    DiagnosticLevel, Dit, DitError, FieldPatch, IndexedIssue, IssueDraft, IssueId, IssueKind,
    Priority, ReindexMode,
};

#[derive(Parser)]
#[command(
    name = "dit",
    version,
    about = "Project management where Markdown files in git are the source of truth"
)]
struct Cli {
    /// The alias your commits are attributed to (default: $DIT_ME, then $USER).
    #[arg(long, global = true)]
    me: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Make the current directory a workspace: git init, merge driver, README.
    Init,
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
    },
    /// Show one issue: fields, body, comments, field history.
    Show { reference: String },
    /// Change fields, e.g. `dit issue set 01K3MA1 status=done labels=a,b`.
    Set {
        reference: String,
        /// field=value pairs; list fields take comma-separated values.
        fields: Vec<String>,
    },
    /// Add a comment.
    Comment {
        reference: String,
        /// The comment text; multiple words are joined into one paragraph.
        text: Vec<String>,
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
            } else if matches!(e, DitError::NotFound(_)) {
                ExitCode::from(2)
            } else {
                ExitCode::from(1)
            }
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, DitError> {
    let me = alias(&cli);
    match cli.command {
        Command::Init => {
            let cwd = std::env::current_dir()?;
            let exe = std::env::current_exe()?;
            let dit = Dit::init(&cwd, &exe)?;
            println!("initialized workspace at {}", dit.root().display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Issue { cmd } => issue(cmd, &me),
        Command::List { query } => {
            let dit = open()?;
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
            let state = dit_server::AppState::with_bind_host(dit, &me, &token, &host);
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

fn issue(cmd: Issue, me: &str) -> Result<ExitCode, DitError> {
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
        } => {
            let title = title.join(" ");
            if title.trim().is_empty() {
                eprintln!("dit: a title is required");
                return Ok(ExitCode::from(2));
            }
            let mut dit = open()?;
            let mut tx = dit.transaction(me)?;
            let id = tx.create_issue(IssueDraft {
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
                blocked_by: vec![],
                body: body.unwrap_or_default(),
            })?;
            tx.commit(&format!("create {title}"))?;
            println!("{} {}", id.short_ref().as_str(), title);
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
        Issue::Set { reference, fields } => {
            let patch = match parse_patch(&fields) {
                Ok(p) => p,
                Err(msg) => {
                    eprintln!("dit: {msg}");
                    return Ok(ExitCode::from(2));
                }
            };
            if patch.is_empty() {
                eprintln!("dit: nothing to set");
                return Ok(ExitCode::from(2));
            }
            let mut dit = open()?;
            let id = resolve(&dit, &reference)?;
            let mut tx = dit.transaction(me)?;
            tx.set_fields(&id, patch)?;
            tx.commit(&format!("update {reference}"))?;
            println!("updated {}", id.short_ref().as_str());
            Ok(ExitCode::SUCCESS)
        }
        Issue::Comment { reference, text } => {
            let body = text.join(" ");
            if body.trim().is_empty() {
                eprintln!("dit: a comment needs text");
                return Ok(ExitCode::from(2));
            }
            let mut dit = open()?;
            let id = resolve(&dit, &reference)?;
            let mut tx = dit.transaction(me)?;
            tx.comment(&id, me, &body)?;
            tx.commit(&format!("comment on {reference}"))?;
            println!("commented on {}", id.short_ref().as_str());
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Turn `field=value` strings into a patch. Values are validated here so the
/// user gets the name of the offending field, not a store error from deep
/// inside the write path.
fn parse_patch(fields: &[String]) -> Result<FieldPatch, String> {
    let mut patch = FieldPatch::default();
    for f in fields {
        let (key, value) = f
            .split_once('=')
            .ok_or_else(|| format!("`{f}` is not field=value"))?;
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
            "assignees" => patch.assignees = Some(split_list(value)),
            "labels" => patch.labels = Some(split_list(value)),
            "sprint" => patch.sprint = Some(value.to_owned()),
            "due" => patch.due = Some(value.to_owned()),
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
    Ok(patch)
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A full id, or a short ref resolved through the index.
fn resolve(dit: &Dit, reference: &str) -> Result<IssueId, DitError> {
    match dit.get(reference)? {
        Some(hit) => Ok(hit.issue.id),
        None => Err(DitError::NotFound(reference.to_owned())),
    }
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

fn alias(cli: &Cli) -> String {
    cli.me
        .clone()
        .or_else(|| std::env::var("DIT_ME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn print_list(hits: &[IndexedIssue]) {
    for hit in hits {
        let i = &hit.issue;
        println!(
            "{}  {:<11} {:<4} {}",
            i.id.short_ref().as_str(),
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
            println!("  {}  {}", i.issue.id.short_ref().as_str(), i.issue.title,);
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
    println!("{}  {}", i.id.short_ref().as_str(), i.title);
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
