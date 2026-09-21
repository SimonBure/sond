//! Log files: IDs, filenames, and creating logs on disk.
//!
//! The filename is the index. Only its `R<id>-` prefix is load-bearing; the
//! date and slug are there for humans and for `ls` ordering.

use std::cmp::Reverse;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use jiff::civil::DateTime;

use crate::clock::{DATE_FORMAT, TIMESTAMP_FORMAT, parse_timestamp};
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

/// The text of the first `# ` heading, if it has any.
pub fn log_title(content: &str) -> Option<&str> {
    content
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// When the log was last worked on, according to what Probe wrote into it:
/// the latest of its `Created:` line, its dated sections, and `file_date` (the
/// date from its filename, at midnight). Unparseable timestamps are skipped.
pub fn last_activity(content: &str, file_date: Option<&str>) -> Option<DateTime> {
    let created = content
        .lines()
        .find_map(|l| l.strip_prefix("Created:"))
        .and_then(|t| parse_timestamp(t).ok());
    let sections = content
        .lines()
        .filter(|l| is_dated_heading(l))
        .filter_map(|l| parse_timestamp(&l.trim_end()["## ".len()..]).ok());
    let filed = file_date.and_then(|d| parse_timestamp(&format!("{d} 00:00")).ok());

    created.into_iter().chain(sections).chain(filed).max()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogSummary {
    pub id: u32,
    pub title: String,
    pub last_activity: Option<DateTime>,
}

/// Every log in `logs_dir` with its content, most recently active first. Logs
/// with no known activity come last; ties go to the higher ID.
fn load_logs(logs_dir: &Path) -> Result<Vec<(LogSummary, String)>> {
    let mut logs = Vec::new();
    for (file, path) in list_logs(logs_dir)? {
        let bytes =
            fs::read(&path).with_context(|| format!("could not read {}", path.display()))?;
        let content = String::from_utf8_lossy(&bytes).into_owned();
        let summary = LogSummary {
            id: file.id,
            title: log_title(&content).unwrap_or(&file.slug).to_string(),
            last_activity: last_activity(&content, file.date.as_deref()),
        };
        logs.push((summary, content));
    }
    logs.sort_by_key(|(l, _)| Reverse((l.last_activity, l.id)));
    Ok(logs)
}

/// Every log in `logs_dir`, most recently active first.
pub fn recent_logs(logs_dir: &Path) -> Result<Vec<LogSummary>> {
    Ok(load_logs(logs_dir)?.into_iter().map(|(l, _)| l).collect())
}

/// The lines of `content` containing `query`, ignoring case, with their
/// 1-based line numbers. The query is literal text, not a pattern.
pub fn matching_lines<'a>(content: &'a str, query: &str) -> Vec<(usize, &'a str)> {
    let query = query.to_lowercase();
    content
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&query))
        .map(|(i, line)| (i + 1, line.trim_end()))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub log: LogSummary,
    pub lines: Vec<(usize, String)>,
}

/// The logs in `logs_dir` mentioning `query`, most recently active first.
pub fn search_logs(logs_dir: &Path, query: &str) -> Result<Vec<SearchHit>> {
    Ok(load_logs(logs_dir)?
        .into_iter()
        .filter_map(|(log, content)| {
            let lines: Vec<(usize, String)> = matching_lines(&content, query)
                .into_iter()
                .map(|(n, line)| (n, line.to_string()))
                .collect();
            (!lines.is_empty()).then_some(SearchHit { log, lines })
        })
        .collect())
}

/// Every log directly inside `logs_dir`, in no particular order. A missing
/// directory has no logs.
pub fn list_logs(logs_dir: &Path) -> Result<Vec<(LogFile, PathBuf)>> {
    let entries = match fs::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("could not read {}", logs_dir.display()));
        }
    };

    let mut logs = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("could not read {}", logs_dir.display()))?;
        if let Some(log) = entry.file_name().to_str().and_then(parse_log_filename) {
            logs.push((log, entry.path()));
        }
    }
    Ok(logs)
}

/// One more than the highest ID in `logs_dir`, so IDs are never reused while
/// the highest log still exists.
pub fn next_id(logs_dir: &Path) -> Result<u32> {
    let highest = list_logs(logs_dir)?
        .iter()
        .map(|(log, _)| log.id)
        .max()
        .unwrap_or(0);
    highest.checked_add(1).context("log IDs are exhausted")
}

/// The path of the log with this ID. Two files claiming the same ID is an
/// error rather than a guess.
pub fn find_log(logs_dir: &Path, id: u32) -> Result<PathBuf> {
    let mut matches: Vec<PathBuf> = list_logs(logs_dir)?
        .into_iter()
        .filter(|(log, _)| log.id == id)
        .map(|(_, path)| path)
        .collect();
    matches.sort();

    match matches.len() {
        0 => bail!("no log with ID {} in {}", format_id(id), logs_dir.display()),
        1 => Ok(matches.remove(0)),
        _ => {
            let names: Vec<String> = matches.iter().map(|p| p.display().to_string()).collect();
            bail!(
                "ID {} is used by more than one log; rename all but one:\n  {}",
                format_id(id),
                names.join("\n  ")
            )
        }
    }
}

/// Opens a new dated section at the end of the log and returns the 1-based
/// line of its heading. If the last section is a dated one the user left
/// empty, that section is reused instead. The file is only ever appended to.
pub fn poke_log(path: &Path, now: DateTime) -> Result<usize> {
    let existing = fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
    if let Some(line) = trailing_empty_section_line(&String::from_utf8_lossy(&existing)) {
        return Ok(line);
    }

    let mut section = String::new();
    if !existing.is_empty() && !existing.ends_with(b"\n") {
        section.push('\n');
    }
    // Everything before the heading now ends in a newline, so the heading is
    // four lines past the last existing line: blank, `---`, blank, heading.
    let lines_before = existing.iter().filter(|&&b| b == b'\n').count() + section.len();
    section.push_str(&format!(
        "\n---\n\n## {}\n\n",
        now.strftime(TIMESTAMP_FORMAT)
    ));

    OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(section.as_bytes()))
        .with_context(|| format!("could not append to {}", path.display()))?;
    Ok(lines_before + 4)
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
