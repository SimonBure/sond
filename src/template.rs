//! Log templates: where they live and how Sond fills them in.
//!
//! This is deliberately not a template language. Sond substitutes the few
//! variables it owns and leaves everything else in the file exactly as written.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

/// Used when the user has not configured a template of their own.
pub const DEFAULT_TEMPLATE: &str = "\
# {{ title }}

Created: {{ created }}
ID: {{ id }}

## Context

## Investigation

## Next steps
";

pub struct TemplateVars<'a> {
    pub title: &'a str,
    pub id: &'a str,
    pub created: &'a str,
    pub date: &'a str,
}

impl<'a> TemplateVars<'a> {
    fn get(&self, name: &str) -> Option<&'a str> {
        match name {
            "title" => Some(self.title),
            "id" => Some(self.id),
            "created" => Some(self.created),
            "date" => Some(self.date),
            _ => None,
        }
    }
}

/// Replaces `{{ name }}` for every variable in `vars`. Unknown or malformed
/// placeholders are copied through untouched.
pub fn render_template(template: &str, vars: &TemplateVars) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let known = after
            .find("}}")
            .and_then(|end| Some((end, vars.get(after[..end].trim())?)));
        match known {
            Some((end, value)) => {
                out.push_str(value);
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }

    out.push_str(rest);
    out
}

/// Where the user's template lives: `$XDG_CONFIG_HOME/sond/template.md`,
/// falling back to `~/.config/sond/template.md`.
pub fn template_path() -> Option<PathBuf> {
    template_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// `template_path` without the environment lookup. Per the XDG spec, an empty
/// or relative `XDG_CONFIG_HOME` is ignored.
pub fn template_path_from(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let config = xdg_config_home
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(config.join("sond").join("template.md"))
}

/// Reads the template at `path`, or returns the default when there is none.
pub fn load_template(path: Option<&Path>) -> Result<String> {
    let Some(path) = path else {
        return Ok(DEFAULT_TEMPLATE.to_string());
    };
    match fs::read_to_string(path) {
        Ok(template) => Ok(template),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(DEFAULT_TEMPLATE.to_string()),
        Err(e) => Err(e).with_context(|| format!("could not read template {}", path.display())),
    }
}

/// Makes a copy of `source` the template at `dest`. The previous template is
/// replaced atomically, so on failure it is left exactly as it was.
pub fn install_template(source: &Path, dest: &Path) -> Result<()> {
    let bytes = fs::read(source).with_context(|| format!("could not read {}", source.display()))?;
    let content =
        String::from_utf8(bytes).map_err(|_| anyhow!("{} is not valid UTF-8", source.display()))?;
    write_atomically(dest, content.as_bytes())
}

/// Writes `content` to a temporary file beside `dest`, then renames it over
/// `dest`. Readers see either the old file or the new one, never a mix.
fn write_atomically(dest: &Path, content: &[u8]) -> Result<()> {
    let dir = create_parent(dest)?;
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));

    let written = fs::write(&tmp, content).and_then(|()| fs::rename(&tmp, dest));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.with_context(|| format!("could not write {}", dest.display()))
}

/// Writes the default template to `path` unless a template is already there.
pub fn ensure_template(path: &Path) -> Result<()> {
    create_parent(path)?;
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file
            .write_all(DEFAULT_TEMPLATE.as_bytes())
            .with_context(|| format!("could not write {}", path.display())),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            if path.is_dir() {
                bail!("{} is a directory, not a template", path.display());
            }
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("could not create {}", path.display())),
    }
}

fn create_parent(path: &Path) -> Result<&Path> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir)
}
