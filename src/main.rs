use std::path::Path;

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
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::New { title } => new(&title.join(" ")),
        Command::Poke { id } => poke(&id),
    }
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
