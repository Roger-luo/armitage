//! Shared helpers for CLI subcommands.
//!
//! These collapse boilerplate that appears across `run_*` functions:
//! org-root resolution, JSON output, confirmation prompts, and small
//! string/date utilities.

use std::path::PathBuf;

use armitage_core::tree::find_org_root;
use chrono::NaiveDate;
use serde::Serialize;

use crate::error::{Error, Result};

/// Resolve the current working directory and walk up to the org root.
pub fn org_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    Ok(find_org_root(&cwd)?)
}

/// Print a value as pretty JSON.
pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value).map_err(|e| Error::Other(e.to_string()))?;
    println!("{s}");
    Ok(())
}

/// Returns true if format == "json" and emits the JSON. Caller can early-return.
pub fn maybe_print_json<T: Serialize>(format: &str, value: &T) -> Result<bool> {
    if format == "json" {
        print_json(value)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Prompt the user with `[y/N]`. `force` bypasses the prompt.
/// Returns `true` to proceed, `false` if aborted (and prints "Aborted.").
pub fn confirm(prompt: &str, force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }
    eprint!("{prompt} [y/N] ");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if input.trim().eq_ignore_ascii_case("y") {
        Ok(true)
    } else {
        println!("Aborted.");
        Ok(false)
    }
}

/// Byte-truncate to a max length. Not unicode-safe but matches the existing
/// padding-oriented use.
pub fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}

/// Parse a comma-separated CLI argument into a trimmed, non-empty vec.
pub fn parse_csv(s: Option<String>) -> Vec<String> {
    s.map(|v| {
        v.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// Parse an `YYYY-MM-DD` date.
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

/// Build an `Error::Other` from a formatted message.
#[macro_export]
macro_rules! bail_other {
    ($($arg:tt)*) => {
        return Err($crate::error::Error::Other(format!($($arg)*)))
    };
}
