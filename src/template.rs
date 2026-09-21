//! Log templates: where they live and how Probe fills them in.
//!
//! This is deliberately not a template language. Probe substitutes the few
//! variables it owns and leaves everything else in the file exactly as written.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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

/// Where the user's template lives: `$XDG_CONFIG_HOME/probe/template.md`,
/// falling back to `~/.config/probe/template.md`.
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
    Some(config.join("probe").join("template.md"))
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
