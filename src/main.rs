use std::io::{IsTerminal, Read};
use std::path::Path;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use serde_json::json;

use driftr::output::Format;
use driftr::watch::Settle;
use driftr::{Repo, SnapshotOpts, Untrack, hook, profile, registry, report, status, usage};

const EXIT_CODES: &str = "\
Exit codes:
  0   success
  1   empty result (log, files); not enabled (status)
  2   error
  64  usage error";

/// Automatic, recoverable working history, stored in plain git.
#[derive(Parser)]
#[command(version, after_help = EXIT_CODES)]
struct Cli {
    /// Pretty JSON
    #[arg(short = 'j', long, global = true)]
    json: bool,
    /// One compact JSON object per line
    #[arg(short = 'J', long, global = true, conflicts_with = "json")]
    ndjson: bool,
    /// Print where the time went (per git command and phase) to stderr
    #[arg(long, global = true)]
    profile: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Opt this repo in; outside git, track the directory via a store kept elsewhere
    Init,
    /// Add files or directories to the snapshot scope
    Track {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Narrow the scope: drop a matching include, else exclude the path
    Untrack {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// List the files currently in scope
    Files,
    /// Show scope, snapshot counts, and whether the Claude Code hook is wired up
    Status,
    /// Snapshot the in-scope files; prints the snapshot id, or nothing if unchanged
    Snapshot {
        #[arg(short, long)]
        message: Option<String>,
        /// Agent or editing session responsible for these changes
        #[arg(long)]
        session: Option<String>,
    },
    /// List snapshots, oldest first (newest last)
    Log {
        path: Option<String>,
        /// Only snapshots tagged with this session
        #[arg(long)]
        session: Option<String>,
    },
    /// Diff a snapshot (`@` = latest, `@~1` the one before) against the current in-scope files
    Diff { id: String, path: Option<String> },
    /// Restore a path from a snapshot (what it overwrites is snapshotted first)
    Restore {
        /// File or directory, relative to the current directory. A directory is restored
        /// whole: in-scope files added since the snapshot are removed
        path: String,
        /// Snapshot to restore from: an id from `driftr log`, or `@~N`
        #[arg(long)]
        at: String,
        /// Print the diff the restore would apply, and change nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Poll the working tree and snapshot whenever it settles (-j streams like -J)
    Watch {
        /// Seconds between polls; a tree unchanged across one poll has settled
        #[arg(long, default_value_t = 30)]
        idle: u64,
        /// Snapshot at least this often (seconds) while changes keep coming
        #[arg(long, default_value_t = 300)]
        max: u64,
    },
    /// Rewrite snapshot history to the current scope, dropping files no longer tracked
    Gc {
        /// Report what would be removed, and change nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Summarize what each hook costs and catches, from the hook usage log
    ///
    /// One row per hook event and tool, busiest first. Columns:
    ///
    ///   runs    hook invocations
    ///   caught  runs that took at least one snapshot
    ///   swept   runs where the end-of-turn sweep caught a change the targeted hooks missed
    ///   fast    runs settled by the stat fingerprint alone, without running git
    ///   busy    runs that skipped a repo another driftr held; the next hook catches up
    ///   errors  runs that hit an error (the hook still never fails the tool call)
    ///   p50 ms  median run time; p95 ms, the slowest 5%
    #[command(verbatim_doc_comment)]
    Report {
        /// Only runs from the last N hours
        #[arg(long)]
        hours: Option<u64>,
    },
    /// Claude Code hook: reads the hook JSON on stdin and snapshots in opted-in repos
    Hook,
}

fn main() -> ExitCode {
    let start = Instant::now();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { 64 } else { 0 });
        }
    };
    let fmt = match (cli.json, cli.ndjson) {
        (true, _) => Format::Json,
        (_, true) => Format::Ndjson,
        _ => Format::Text,
    };
    if let Command::Hook = cli.command {
        return run_hook(start, cli.profile);
    }
    let code = match run(cli.command, fmt) {
        Ok(code) => code,
        Err(e) => {
            match fmt {
                Format::Text => eprintln!("driftr: {e:#}"),
                _ => eprintln!("{}", json!({ "error": format!("{e:#}") })),
            }
            ExitCode::from(2)
        }
    };
    if cli.profile {
        eprint!("{}", profile::summary(start.elapsed()).render());
    }
    code
}

fn run(command: Command, fmt: Format) -> Result<ExitCode> {
    if let Command::Report { hours } = command {
        let since = hours.map_or(0, |h| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
                .saturating_sub(h * 3600)
        });
        let rows = report::summarize(&usage::read()?, since);
        fmt.rows_or_table(&rows, report::render)?;
        return Ok(found(!rows.is_empty()));
    }
    if let Command::Init = command {
        let (repo, created) = Repo::init_dir(Path::new("."))?;
        registry::register(repo.common_dir())?;
        let config = repo.config_path().display().to_string();
        if !created {
            eprintln!("already initialized");
        }
        let value = json!({ "config": config, "store": repo.git_dir(), "created": created });
        fmt.one(&value, |_| Some(config.clone()))?;
        return Ok(ExitCode::SUCCESS);
    }
    let repo = Repo::open(Path::new("."))?;
    match command {
        Command::Init | Command::Report { .. } => unreachable!("handled before opening a repo"),
        Command::Track { paths } => {
            let patterns = paths
                .iter()
                .map(|p| repo.track(p))
                .collect::<Result<Vec<_>>>()?;
            fmt.rows(&patterns, |p| p.clone())?;
        }
        Command::Untrack { paths } => {
            let changes = paths
                .iter()
                .map(|p| repo.untrack(p))
                .collect::<Result<Vec<_>>>()?;
            fmt.rows(&changes, |c| match c {
                Untrack::Removed(p) => format!("removed {p}"),
                Untrack::Excluded(p) => format!("excluded {p}"),
            })?;
        }
        Command::Files => {
            let files = repo.files()?;
            fmt.rows(&files, |f| f.clone())?;
            return Ok(found(!files.is_empty()));
        }
        Command::Status => {
            let s = status::status(&repo)?;
            fmt.one(&s, |s| Some(status_text(s)))?;
            return Ok(found(s.enabled));
        }
        Command::Snapshot { message, session } => {
            let mut opts = session
                .as_deref()
                .map(SnapshotOpts::session)
                .unwrap_or_default();
            opts.message = message;
            let id = repo.snapshot(&opts)?;
            // Repos opted in before the registry existed join on their next snapshot.
            let _ = registry::register(repo.common_dir());
            fmt.one(&json!({ "snapshot": id }), |_| id.clone())?;
        }
        Command::Log { path, session } => {
            let entries = repo.log(path.as_deref(), session.as_deref())?;
            fmt.rows(&entries, |e| format!("{}\t{}\t{}", e.id, e.time, e.subject))?;
            return Ok(found(!entries.is_empty()));
        }
        Command::Diff { id, path } => {
            let diff = repo.diff(&id, path.as_deref())?;
            fmt.one(&json!({ "diff": diff }), |_| Some(diff.clone()))?;
        }
        Command::Restore { path, at, dry_run } => {
            if dry_run {
                let diff = repo.preview_restore(&at, &path)?;
                fmt.one(&json!({ "diff": diff }), |_| Some(diff.clone()))?;
            } else {
                let before = repo.last()?;
                let safety = repo.restore(&at, &path)?;
                // Unchanged since the last snapshot means that snapshot already holds it.
                let undo = safety.clone().or(before).map(|id| id[..7].to_string());
                let value = json!({ "restored": path, "from": at, "safety": safety, "undo": undo });
                fmt.one(&value, |_| {
                    Some(match &undo {
                        Some(id) => format!(
                            "restored {path} from {at}; undo with: driftr restore --at {id} {path}"
                        ),
                        None => format!("restored {path} from {at}"),
                    })
                })?;
            }
        }
        Command::Watch { idle, max } => {
            let fmt = fmt.streaming();
            let idle = idle.max(1);
            let mut settle = Settle::new((max / idle) as u32);
            loop {
                // Held across stage + commit so a concurrent writer can't land in between.
                let lock = repo.lock()?;
                let tree = repo.stage()?;
                if settle.observe(tree.clone())
                    && let Some(id) = repo.commit(&tree, &SnapshotOpts::default())?
                {
                    fmt.one(&json!({ "snapshot": id }), |_| Some(id.clone()))?;
                }
                drop(lock);
                thread::sleep(Duration::from_secs(idle));
            }
        }
        Command::Gc { dry_run } => {
            let report = repo.gc(dry_run)?;
            fmt.one(&report, |r| {
                let verb = if r.dry_run { "would keep" } else { "kept" };
                let mut text = format!(
                    "{verb} {} of {} snapshots; {} paths {}removed from history",
                    r.kept,
                    r.snapshots,
                    r.removed_paths.len(),
                    if r.dry_run { "would be " } else { "" }
                );
                if r.dry_run {
                    for p in &r.removed_paths {
                        text.push_str(&format!("\n  {p}"));
                    }
                }
                Some(text)
            })?;
        }
        Command::Hook => unreachable!("handled before opening a repo"),
    }
    Ok(ExitCode::SUCCESS)
}

/// grep's convention for queries: 1 means "nothing".
fn found(any: bool) -> ExitCode {
    if any {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn status_text(s: &status::Status) -> String {
    let hooks = if s.hooks.is_empty() {
        "not installed".to_string()
    } else {
        s.hooks.join(", ")
    };
    [
        ("repo", s.repo.clone()),
        ("store", s.store.clone()),
        ("enabled", s.enabled.to_string()),
        ("config", s.config.clone()),
        ("include", s.include.join(" ")),
        ("exclude", s.exclude.join(" ")),
        ("files", s.files.to_string()),
        (
            "snapshots",
            format!("{} on {}", s.snapshots, s.snapshot_ref),
        ),
        ("last", s.last.clone().unwrap_or_else(|| "-".into())),
        ("hook", hooks),
    ]
    .iter()
    .map(|(k, v)| format!("{k:<10} {v}"))
    .collect::<Vec<_>>()
    .join("\n")
}

/// Always exits 0 and never writes stdout: Claude Code parses hook stdout, and a
/// failing hook must not block or interrupt the agent. Every run is logged with its
/// timings, so what hooks cost and catch can be judged from real use (`driftr report`).
fn run_hook(start: Instant, show_profile: bool) -> ExitCode {
    let result = (|| {
        if std::io::stdin().is_terminal() {
            bail!("expects Claude Code's hook JSON on stdin");
        }
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        // The registry lets Stop sweep every opted-in repo and match paths in commands.
        let ctx = hook::Context {
            registered: registry::worktrees().unwrap_or_default(),
        };
        let outcome = hook::run(&input, &ctx)?;
        for visit in &outcome.visits {
            let _ = registry::register(&visit.common_dir);
        }
        Ok(outcome)
    })();
    let profile = profile::summary(start.elapsed());
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut record =
        json!({ "time": time, "version": env!("CARGO_PKG_VERSION"), "profile": profile });
    match &result {
        Ok(outcome) => {
            record["event"] = json!(outcome.event);
            record["tool"] = json!(outcome.tool);
            record["session"] = json!(outcome.session);
            record["visits"] = json!(outcome.visits);
        }
        Err(e) => {
            eprintln!("driftr hook: {e:#}");
            record["error"] = json!(format!("{e:#}"));
        }
    }
    if let Err(e) = usage::append(&record) {
        eprintln!("driftr hook: usage log: {e:#}");
    }
    if show_profile {
        eprint!("{}", profile.render());
    }
    ExitCode::SUCCESS
}
