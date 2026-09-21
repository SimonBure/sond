use std::path::Path;

use anyhow::{Result, bail};
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
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::New { title } => new(&title.join(" ")),
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
