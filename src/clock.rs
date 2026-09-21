//! The current time, overridable through `PROBE_NOW` so runs are reproducible.

use std::env::{self, VarError};

use anyhow::{Context, Result, bail};
use jiff::Zoned;
use jiff::civil::DateTime;

/// Format of `Created:` metadata, poke headings, and `PROBE_NOW`.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M";

/// Format of the date part of log filenames.
pub const DATE_FORMAT: &str = "%Y-%m-%d";

/// Local wall-clock time, or the value of `PROBE_NOW` when it is set.
pub fn now() -> Result<DateTime> {
    match env::var("PROBE_NOW") {
        Ok(s) => parse_timestamp(&s),
        Err(VarError::NotPresent) => Ok(Zoned::now().datetime()),
        Err(VarError::NotUnicode(_)) => bail!("PROBE_NOW is not valid UTF-8"),
    }
}

pub fn parse_timestamp(s: &str) -> Result<DateTime> {
    DateTime::strptime(TIMESTAMP_FORMAT, s.trim())
        .with_context(|| format!("invalid timestamp {s:?}, expected YYYY-MM-DD HH:MM"))
}
