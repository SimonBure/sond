use std::io::{self, ErrorKind, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use probe::{clock, editor, log, template};

/// Logs live in `./logs`, relative to wherever Probe is run.
const LOGS_DIR: &str = "logs";

/// Append-only research logs, stored as Markdown next to your code.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a new investigation and open it in your editor
    New {
        /// Title of the investigation; quoting it is optional
        #[arg(required = true, num_args = 1..)]
        title: Vec<String>,
    },
    /// Continue an investigation: add a dated section and open the log
    Poke {
        /// ID of the log, e.g. R042 (the R and leading zeros are optional)
        id: String,
    },
    /// Find the logs that mention a phrase (literal, case-insensitive)
    Search {
        /// Text to look for; quoting it is optional
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
    },
    /// List investigations, most recently active first
    Recent {
        /// How many to show
        #[arg(short = 'n', long, default_value = "10")]
        limit: NonZeroUsize,
    },
    /// Manage the template new logs are created from
    Template {
        #[command(subcommand)]
        command: TemplateCommand,
    },
}

#[derive(Subcommand)]
enum TemplateCommand {
    /// Open the template in your editor, starting from the default if there is none
    Edit,
    /// Use a copy of FILE as the template for new logs
    Set {
        /// Markdown file to copy
        file: PathBuf,
    },
}

fn main() -> Result<ExitCode> {
    let done = match Cli::parse().command {
        Command::New { title } => new(&title.join(" ")),
        Command::Poke { id } => poke(&id),
        Command::Search { query } => return search(&query.join(" ")),
        Command::Recent { limit } => recent(limit.get()),
        Command::Template { command } => match command {
            TemplateCommand::Edit => template_edit(),
            TemplateCommand::Set { file } => template_set(&file),
        },
    };
    done.map(|()| ExitCode::SUCCESS)
}

fn new(title: &str) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        bail!("title must not be empty");
    }

    let template = template::load_template(template::template_path().as_deref())?;
    let now = clock::now()?;
    let path = log::create_log(Path::new(LOGS_DIR), title, now, &template)?;

    println!("{}", path.display());
    editor::open(&path, None)
}

fn poke(id: &str) -> Result<()> {
    let id = log::parse_id(id)
        .with_context(|| format!("invalid log ID {id:?}, expected something like R042"))?;
    let path = log::find_log(Path::new(LOGS_DIR), id)?;
    let now = clock::now()?;
    let heading = log::poke_log(&path, now)?;

    println!("{}", path.display());
    editor::open(&path, Some(heading + 1))
}

/// Exits 1 when nothing matches, like `grep`.
fn search(query: &str) -> Result<ExitCode> {
    let query = query.trim();
    if query.is_empty() {
        bail!("search query must not be empty");
    }

    let hits = log::search_logs(Path::new(LOGS_DIR), query)?;
    if hits.is_empty() {
        eprintln!("no log mentions {query:?}");
        return Ok(ExitCode::FAILURE);
    }

    let groups: Vec<String> = hits
        .iter()
        .map(|hit| {
            let mut group = format!("{}  {}\n", log::format_id(hit.log.id), hit.log.title);
            for (n, line) in &hit.lines {
                group.push_str(&format!("{n:>4}: {line}\n"));
            }
            group
        })
        .collect();
    print(&groups.join("\n"))?;
    Ok(ExitCode::SUCCESS)
}

fn recent(limit: usize) -> Result<()> {
    let logs = log::recent_logs(Path::new(LOGS_DIR))?;
    if logs.is_empty() {
        eprintln!("no logs yet; start one with `probe new <title>`");
        return Ok(());
    }

    let shown = &logs[..logs.len().min(limit)];
    let width = shown
        .iter()
        .map(|l| log::format_id(l.id).len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for l in shown {
        let when = l.last_activity.map_or_else(
            || "-".to_string(),
            |t| t.strftime(clock::TIMESTAMP_FORMAT).to_string(),
        );
        out.push_str(&format!(
            "{:<width$}  {when:<16}  {}\n",
            log::format_id(l.id),
            l.title
        ));
    }
    print(&out)
}

/// Writes `text` to stdout. A reader that stops early, as in
/// `probe recent | head`, is not an error.
fn print(text: &str) -> Result<()> {
    match io::stdout().lock().write_all(text.as_bytes()) {
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(()),
        written => Ok(written?),
    }
}

fn template_edit() -> Result<()> {
    let path = template_file()?;
    template::ensure_template(&path)?;

    println!("{}", path.display());
    editor::open(&path, None)
}

fn template_set(source: &Path) -> Result<()> {
    let path = template_file()?;
    template::install_template(source, &path)?;

    println!("{}", path.display());
    Ok(())
}

fn template_file() -> Result<PathBuf> {
    template::template_path()
        .context("cannot locate the config directory: set HOME or XDG_CONFIG_HOME")
}
