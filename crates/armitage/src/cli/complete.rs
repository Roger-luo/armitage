use std::fmt::Write as _;

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

use armitage_core::tree::NodeEntry;

// ---------------------------------------------------------------------------
// Tab-completion helper for node paths
// ---------------------------------------------------------------------------

pub struct NodePathHelper {
    /// Known node paths (e.g. ["backend", "backend/auth", "backend/auth/oauth"])
    paths: Vec<String>,
}

impl NodePathHelper {
    pub fn from_entries(entries: &[NodeEntry]) -> Self {
        Self {
            paths: entries.iter().map(|e| e.path.clone()).collect(),
        }
    }
}

impl Completer for NodePathHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let input = &line[..pos];
        let mut candidates = Vec::new();

        for path in &self.paths {
            // Offer "path/" as completion (for creating children under this node)
            let with_slash = format!("{path}/");
            if with_slash.starts_with(input) && with_slash != input {
                candidates.push(Pair {
                    display: with_slash.clone(),
                    replacement: with_slash,
                });
            }
            // Offer exact path (for editing or referencing this node)
            if path.starts_with(input) && path.as_str() != input {
                candidates.push(Pair {
                    display: path.clone(),
                    replacement: path.clone(),
                });
            }
        }

        // Deduplicate and sort
        candidates.sort_by(|a, b| a.display.cmp(&b.display));
        candidates.dedup_by(|a, b| a.display == b.display);

        Ok((0, candidates))
    }
}

impl Hinter for NodePathHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<String> {
        let input = &line[..pos];
        if input.is_empty() {
            return None;
        }

        // Collect all matching paths.
        let matches: Vec<&str> = self
            .paths
            .iter()
            .filter(|p| p.starts_with(input) && p.as_str() != input)
            .map(std::string::String::as_str)
            .collect();

        if matches.is_empty() {
            return None;
        }

        // Inline suffix: the remaining characters of the first match.
        let inline = &matches[0][input.len()..];

        // Build a multi-column list of all matches below the input line.
        let columns = format_columns(&matches, 80);

        Some(format!("{inline}\n{columns}"))
    }
}

impl Highlighter for NodePathHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        highlight_hint_dim(hint)
    }
}
impl Validator for NodePathHelper {}
impl Helper for NodePathHelper {}

// ---------------------------------------------------------------------------
// Comma-separated completion helper (used for labels, repos, etc.)
// ---------------------------------------------------------------------------

/// Generic helper for comma-separated input with tab-completion and inline hints.
pub struct CommaCompleteHelper {
    pub items: Vec<String>,
}

impl Completer for CommaCompleteHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let input = &line[..pos];
        let (prefix_before, current_token) = input
            .rfind(',')
            .map_or((0, input), |i| (i + 1, input[i + 1..].trim_start()));
        let start = if prefix_before == 0 {
            0
        } else {
            prefix_before + (input[prefix_before..].len() - current_token.len())
        };

        let candidates: Vec<Pair> = self
            .items
            .iter()
            .filter(|l| {
                if current_token.is_empty() {
                    true
                } else {
                    l.starts_with(current_token) && l.as_str() != current_token
                }
            })
            .map(|l| Pair {
                display: l.clone(),
                replacement: l.clone(),
            })
            .collect();

        Ok((start, candidates))
    }
}

impl Hinter for CommaCompleteHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<String> {
        let input = &line[..pos];
        let current_token = input
            .rfind(',')
            .map_or(input, |i| input[i + 1..].trim_start());
        if current_token.is_empty() {
            return None;
        }

        let matches: Vec<&str> = self
            .items
            .iter()
            .filter(|l| l.starts_with(current_token) && l.as_str() != current_token)
            .map(std::string::String::as_str)
            .collect();

        if matches.is_empty() {
            return None;
        }

        let inline = &matches[0][current_token.len()..];
        let columns = format_columns(&matches, 80);
        Some(format!("{inline}\n{columns}"))
    }
}

impl Highlighter for CommaCompleteHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        highlight_hint_dim(hint)
    }
}
impl Validator for CommaCompleteHelper {}
impl Helper for CommaCompleteHelper {}

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

/// Format strings into a multi-column layout fitting within `width` characters.
pub fn format_columns(items: &[&str], width: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let col_width = items.iter().map(|s| s.len()).max().unwrap_or(0) + 2;
    let num_cols = (width / col_width).max(1);
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 && i % num_cols == 0 {
            out.push('\n');
        }
        let _ = write!(out, "{item:<col_width$}");
    }
    out
}

/// Render hint text: inline suffix in dim, options list below in dim.
fn highlight_hint_dim(hint: &str) -> std::borrow::Cow<'_, str> {
    if let Some((inline, options)) = hint.split_once('\n') {
        std::borrow::Cow::Owned(format!("\x1b[90m{inline}\x1b[0m\n\x1b[90m{options}\x1b[0m"))
    } else {
        std::borrow::Cow::Owned(format!("\x1b[90m{hint}\x1b[0m"))
    }
}
