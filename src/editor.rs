//! Opening a log in the user's editor, following `$VISUAL` / `$EDITOR`.

use std::env;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// The first non-blank of `$VISUAL` and `$EDITOR`, falling back to `vi`.
pub fn choose_editor<'a>(visual: Option<&'a str>, editor: Option<&'a str>) -> &'a str {
    [visual, editor]
        .into_iter()
        .flatten()
        .find(|e| !e.trim().is_empty())
        .unwrap_or("vi")
}

/// Builds the command that opens `path`, at `line` when the editor has a
/// syntax for it we know. `editor` is split on whitespace, so flags such as
/// `code --wait` work but quoted paths containing spaces do not.
pub fn editor_command(editor: &str, path: &Path, line: Option<usize>) -> Option<EditorCommand> {
    let mut words = editor.split_whitespace();
    let program = words.next()?.to_string();
    let mut args: Vec<String> = words.map(str::to_string).collect();

    let path = path.to_string_lossy().into_owned();
    let name = Path::new(&program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&program);

    match (name, line) {
        ("vi" | "vim" | "nvim" | "nano" | "emacs", Some(n)) => {
            args.extend([format!("+{n}"), path]);
        }
        ("code" | "code-insiders" | "codium", Some(n)) => {
            args.extend(["--goto".to_string(), format!("{path}:{n}")]);
        }
        ("hx" | "helix", Some(n)) => args.push(format!("{path}:{n}")),
        _ => args.push(path),
    }

    Some(EditorCommand { program, args })
}

/// Opens `path` in the user's editor and waits for it to exit.
pub fn open(path: &Path, line: Option<usize>) -> Result<()> {
    let visual = env::var("VISUAL").ok();
    let editor = env::var("EDITOR").ok();
    let editor = choose_editor(visual.as_deref(), editor.as_deref());
    let cmd = editor_command(editor, path, line).context("no editor configured")?;

    let status = Command::new(&cmd.program)
        .args(&cmd.args)
        .status()
        .with_context(|| format!("could not launch editor `{editor}`"))?;
    if !status.success() {
        bail!("editor `{editor}` exited with {status}");
    }
    Ok(())
}
