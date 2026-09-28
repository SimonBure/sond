use std::io::{self, ErrorKind, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use sond::{clock, editor, log, template};
#[cfg(feature = "ask")]
use {
    sond::chunk::{self, Chunk},
    sond::embed,
    sond::index::{self, Index},
    std::collections::HashMap,
    std::fs,
    std::io::IsTerminal,
};

/// Logs live in `./sond`, relative to wherever Sond is run.
const LOGS_DIR: &str = "sond";

/// Append-only research logs, stored as Markdown next to your code.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a new investigation
    New {
        /// Title of the investigation; quoting it is optional
        #[arg(required = true, num_args = 1..)]
        title: Vec<String>,
        /// Open the new log in $VISUAL / $EDITOR
        #[arg(short, long)]
        edit: bool,
    },
    /// Continue an investigation: add a dated section to the log
    Poke {
        /// ID of the log, e.g. R042 (the R and leading zeros are optional)
        id: String,
        /// Open the log in $VISUAL / $EDITOR, at the new section
        #[arg(short, long)]
        edit: bool,
    },
    /// Find the logs that mention a phrase (literal, case-insensitive)
    Search {
        /// Text to look for; quoting it is optional
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
    },
    /// Build the semantic index that `ask` searches (downloads the model once)
    Index,
    /// Find the sections closest in meaning to a question
    Ask {
        /// The question; quoting it is optional
        #[arg(required = true, num_args = 1..)]
        question: Vec<String>,
        /// How many sections to show
        #[arg(short = 'n', long, default_value = "5")]
        limit: NonZeroUsize,
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
        Command::New { title, edit } => new(&title.join(" "), edit),
        Command::Poke { id, edit } => poke(&id, edit),
        Command::Search { query } => return search(&query.join(" ")),
        Command::Index => index(),
        Command::Ask { question, limit } => return ask(&question.join(" "), limit.get()),
        Command::Recent { limit } => recent(limit.get()),
        Command::Template { command } => match command {
            TemplateCommand::Edit => template_edit(),
            TemplateCommand::Set { file } => template_set(&file),
        },
    };
    done.map(|()| ExitCode::SUCCESS)
}

fn new(title: &str, edit: bool) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        bail!("title must not be empty");
    }

    let template = template::load_template(template::template_path().as_deref())?;
    let now = clock::now()?;
    let path = log::create_log(Path::new(LOGS_DIR), title, now, &template)?;

    println!("{}", path.display());
    if edit {
        editor::open(&path, None)?;
    }
    Ok(())
}

fn poke(id: &str, edit: bool) -> Result<()> {
    let id = log::parse_id(id)
        .with_context(|| format!("invalid log ID {id:?}, expected something like R042"))?;
    let path = log::find_log(Path::new(LOGS_DIR), id)?;
    let now = clock::now()?;
    let heading = log::poke_log(&path, now)?;

    println!("{}", path.display());
    if edit {
        editor::open(&path, Some(heading + 1))?;
    }
    Ok(())
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

#[cfg(not(feature = "ask"))]
const NO_ASK: &str = "semantic search is not in this build of sond; install it with `cargo install sond --features ask`";

#[cfg(not(feature = "ask"))]
fn index() -> Result<()> {
    bail!(NO_ASK)
}

#[cfg(not(feature = "ask"))]
fn ask(_question: &str, _limit: usize) -> Result<ExitCode> {
    bail!(NO_ASK)
}

#[cfg(feature = "ask")]
fn index() -> Result<()> {
    let logs = log::load_logs(Path::new(LOGS_DIR))?;
    if logs.is_empty() {
        bail!("no logs in ./{LOGS_DIR} to index");
    }
    let dir = model_dir()?;
    if !embed::is_installed(&embed::MODEL, &dir)? {
        confirm_download(&dir)?;
    }
    let mut model = embed::Model::load(&embed::MODEL, &dir, true)?;

    let path = index_path();
    let created = !path.exists();
    // A damaged index is rebuilt from scratch.
    let old = read_index(&path).ok().flatten().unwrap_or(Index {
        model: String::new(),
        entries: Vec::new(),
    });
    let new = index::refresh(old, embed::MODEL.name, log_chunks(&logs), |texts| {
        model.passages(texts)
    })?;
    write_index(&path, &new)?;
    if created {
        ignore_index()?;
    }

    println!(
        "indexed {} sections from {} logs into {}",
        new.entries.len(),
        logs.len(),
        path.display()
    );
    Ok(())
}

/// Exits 1 when the index has no sections at all.
#[cfg(feature = "ask")]
fn ask(question: &str, limit: usize) -> Result<ExitCode> {
    let question = question.trim();
    if question.is_empty() {
        bail!("question must not be empty");
    }
    let path = index_path();
    let Some(old) = read_index(&path)? else {
        bail!("no semantic index in ./{LOGS_DIR}; build it with `sond index`");
    };
    let dir = model_dir()?;
    if !embed::is_installed(&embed::MODEL, &dir)? {
        bail!("the embedding model is not installed; install it with `sond index`");
    }
    let mut model = embed::Model::load(&embed::MODEL, &dir, false)?;

    let logs = log::load_logs(Path::new(LOGS_DIR))?;
    let current = index::refresh(old.clone(), embed::MODEL.name, log_chunks(&logs), |texts| {
        model.passages(texts)
    })?;
    if current != old {
        write_index(&path, &current)?;
    }

    let hits = index::top_k(&model.query(question)?, &current.entries, limit);
    if hits.is_empty() {
        eprintln!("the index has no sections to search");
        return Ok(ExitCode::FAILURE);
    }
    let titles: HashMap<u32, &str> = logs.iter().map(|(l, _)| (l.id, l.title.as_str())).collect();
    let hits: Vec<String> = hits
        .iter()
        .map(|&(i, score)| {
            let e = &current.entries[i];
            format!(
                "{}  {}\n  L{}  ## {}  {score:.2}\n",
                log::format_id(e.log),
                titles.get(&e.log).unwrap_or(&""),
                e.chunk.line,
                e.chunk.heading
            )
        })
        .collect();
    print(&hits.join("\n"))?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(feature = "ask")]
fn model_dir() -> Result<PathBuf> {
    embed::cache_dir().context("cannot locate the cache directory: set HOME or XDG_CACHE_HOME")
}

/// Asks before downloading the model; without a terminal to ask in, refuses.
#[cfg(feature = "ask")]
fn confirm_download(dir: &Path) -> Result<()> {
    let m = &embed::MODEL;
    if !io::stdin().is_terminal() {
        bail!(
            "the embedding model {} ({}) is not installed; run `sond index` in a terminal to download it",
            m.name,
            m.size
        );
    }
    eprint!(
        "Semantic search needs the embedding model {} ({}), downloaded once into {}.\nDownload it now? [y/N] ",
        m.name,
        m.size,
        dir.display()
    );
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
        bail!("the model was not downloaded; `sond ask` needs it");
    }
    Ok(())
}

#[cfg(feature = "ask")]
fn index_path() -> PathBuf {
    Path::new(LOGS_DIR).join(index::INDEX_FILE)
}

/// The index at `path`, or `None` when there is none.
#[cfg(feature = "ask")]
fn read_index(path: &Path) -> Result<Option<Index>> {
    match fs::read(path) {
        Ok(bytes) => index::decode(&bytes).map(Some).with_context(|| {
            format!(
                "{} is damaged; rebuild it with `sond index`",
                path.display()
            )
        }),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

/// Writes through a temporary file, so an interrupted write leaves the old
/// index whole.
#[cfg(feature = "ask")]
fn write_index(path: &Path, index: &Index) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, index::encode(index))
        .and_then(|()| fs::rename(&tmp, path))
        .with_context(|| format!("could not write {}", path.display()))
}

#[cfg(feature = "ask")]
fn log_chunks(logs: &[(log::LogSummary, String)]) -> Vec<(u32, Chunk)> {
    logs.iter()
        .flat_map(|(l, content)| {
            chunk::chunks(&l.title, content)
                .into_iter()
                .map(|c| (l.id, c))
        })
        .collect()
}

/// Adds the index to `./.gitignore`, when there is one.
#[cfg(feature = "ask")]
fn ignore_index() -> Result<()> {
    let path = Path::new(".gitignore");
    let current = match fs::read_to_string(path) {
        Ok(current) => current,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("could not read {}", path.display())),
    };
    if let Some(updated) = index::with_index_ignored(&current) {
        fs::write(path, updated).with_context(|| format!("could not write {}", path.display()))?;
    }
    Ok(())
}

fn recent(limit: usize) -> Result<()> {
    let logs = log::recent_logs(Path::new(LOGS_DIR))?;
    if logs.is_empty() {
        eprintln!("no logs yet; start one with `sond new <title>`");
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
/// `sond recent | head`, is not an error.
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
