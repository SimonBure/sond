//! Log files: IDs, filenames, and creating logs on disk.
//!
//! The filename is the index. Only its `R<id>-` prefix is load-bearing; the
//! date and slug are there for humans and for `ls` ordering.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jiff::civil::DateTime;

use crate::clock::{DATE_FORMAT, TIMESTAMP_FORMAT};
use crate::template::{TemplateVars, render_template};

const MAX_SLUG_LEN: usize = 60;

/// Turns a title into a lowercase ASCII `a-z0-9-` slug of at most 60 chars.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut separate = false;

    for c in title.chars().flat_map(char::to_lowercase) {
        let ascii = match c {
            'a'..='z' | '0'..='9' => None,
            _ => match fold_to_ascii(c) {
                Some(s) => Some(s),
                None => {
                    separate = true;
                    continue;
                }
            },
        };
        if separate && !slug.is_empty() {
            slug.push('-');
        }
        separate = false;
        match ascii {
            Some(s) => slug.push_str(s),
            None => slug.push(c),
        }
    }

    if slug.len() > MAX_SLUG_LEN {
        // The slug is ASCII, so byte offsets are char offsets.
        let cut = slug[..=MAX_SLUG_LEN].rfind('-').unwrap_or(MAX_SLUG_LEN);
        slug.truncate(cut);
    }
    if slug.is_empty() {
        slug.push_str("untitled");
    }
    slug
}

/// ASCII spelling of the lowercase accented letters of Western European languages.
fn fold_to_ascii(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
        'æ' => "ae",
        'ç' => "c",
        'è' | 'é' | 'ê' | 'ë' => "e",
        'ì' | 'í' | 'î' | 'ï' => "i",
        'ñ' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
        'œ' => "oe",
        'ß' => "ss",
        'ù' | 'ú' | 'û' | 'ü' => "u",
        'ý' | 'ÿ' => "y",
        _ => return None,
    })
}

pub fn format_id(n: u32) -> String {
    format!("R{n:03}")
}

/// Parses an ID as a human types it: `R042`, `r42`, `42`, ...
pub fn parse_id(s: &str) -> Option<u32> {
    let s = s.trim();
    parse_digits(s.strip_prefix(['R', 'r']).unwrap_or(s))
}

fn parse_digits(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

pub fn log_filename(id: u32, date: &str, slug: &str) -> String {
    format!("{}-{date}-{slug}.md", format_id(id))
}

/// What Probe can read back out of a log's filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFile {
    pub id: u32,
    pub date: Option<String>,
    pub slug: String,
}

/// Parses `R<id>-[YYYY-MM-DD-]<slug>.md`; anything else is not a log.
pub fn parse_log_filename(name: &str) -> Option<LogFile> {
    let stem = name.strip_suffix(".md")?.strip_prefix('R')?;
    let (digits, rest) = stem.split_once('-')?;
    let id = parse_digits(digits)?;

    let dated = rest
        .get(..10)
        .filter(|d| has_shape(d, "dddd-dd-dd"))
        .zip(rest.get(10..).and_then(|s| s.strip_prefix('-')))
        .filter(|(_, slug)| !slug.is_empty());
    let (date, slug) = match dated {
        Some((date, slug)) => (Some(date.to_string()), slug),
        None => (None, rest),
    };

    if slug.is_empty() {
        return None;
    }
    Some(LogFile {
        id,
        date,
        slug: slug.to_string(),
    })
}

/// True when `s` matches `shape`, where `d` stands for any ASCII digit.
fn has_shape(s: &str, shape: &str) -> bool {
    s.len() == shape.len()
        && s.bytes().zip(shape.bytes()).all(|(c, p)| match p {
            b'd' => c.is_ascii_digit(),
            _ => c == p,
        })
}

/// The 1-based line of the last `## YYYY-MM-DD HH:MM` heading, if nothing but
/// whitespace follows it.
pub fn trailing_empty_section_line(content: &str) -> Option<usize> {
    let lines: Vec<&str> = content.lines().collect();
    let heading = lines.iter().rposition(|l| is_dated_heading(l))?;
    lines[heading + 1..]
        .iter()
        .all(|l| l.trim().is_empty())
        .then_some(heading + 1)
}

fn is_dated_heading(line: &str) -> bool {
    line.trim_end()
        .strip_prefix("## ")
        .is_some_and(|rest| has_shape(rest, "dddd-dd-dd dd:dd"))
}

/// One more than the highest ID in `logs_dir`, so IDs are never reused while
/// the highest log still exists.
pub fn next_id(logs_dir: &Path) -> Result<u32> {
    let entries = match fs::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(1),
        Err(e) => {
            return Err(e).with_context(|| format!("could not read {}", logs_dir.display()));
        }
    };

    let mut highest = 0;
    for entry in entries {
        let entry = entry.with_context(|| format!("could not read {}", logs_dir.display()))?;
        if let Some(log) = entry.file_name().to_str().and_then(parse_log_filename) {
            highest = highest.max(log.id);
        }
    }
    highest.checked_add(1).context("log IDs are exhausted")
}

/// Creates a new log in `logs_dir` from `template` and returns its path.
/// Never overwrites an existing file.
pub fn create_log(logs_dir: &Path, title: &str, now: DateTime, template: &str) -> Result<PathBuf> {
    fs::create_dir_all(logs_dir)
        .with_context(|| format!("could not create logs directory {}", logs_dir.display()))?;

    let n = next_id(logs_dir)?;
    let id = format_id(n);
    let date = now.strftime(DATE_FORMAT).to_string();
    let created = now.strftime(TIMESTAMP_FORMAT).to_string();
    let content = render_template(
        template,
        &TemplateVars {
            title,
            id: &id,
            created: &created,
            date: &date,
        },
    );

    let path = logs_dir.join(log_filename(n, &date, &slugify(title)));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(path)
}
