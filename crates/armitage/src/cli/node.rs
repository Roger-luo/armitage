use std::io::{self, Write};
use std::path::Path;

use rustyline::{DefaultEditor, Editor};

use crate::cli::complete::{CommaCompleteHelper, NodePathHelper};
use crate::error::{Error, Result};
use armitage_core::node::{self, Node, NodeStatus};
use armitage_core::org::Org;
use armitage_core::team::TeamFile;
use armitage_core::tree::{NodeEntry, find_org_root, list_children, read_node, walk_nodes};
use armitage_labels::def::LabelsFile;

// ---------------------------------------------------------------------------
// ANSI color helpers
// ---------------------------------------------------------------------------

mod color {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const CYAN: &str = "\x1b[36m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const RED: &str = "\x1b[31m";
    pub const BLUE: &str = "\x1b[34m";
}

fn status_color(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Active => color::GREEN,
        NodeStatus::Paused => color::YELLOW,
        NodeStatus::Completed => color::BLUE,
        NodeStatus::Cancelled => color::RED,
    }
}

// ---------------------------------------------------------------------------
// Tree printing
// ---------------------------------------------------------------------------

/// Render a list of node entries as a formatted tree with box-drawing lines.
///
/// When `colored` is true, ANSI escape codes are included.  `term_width`
/// controls description wrapping (pass 0 to disable wrapping).
fn render_tree(entries: &[NodeEntry], term_width: usize, colored: bool) -> String {
    use std::fmt::Write;

    if entries.is_empty() {
        return String::new();
    }

    // Color helpers — return the escape sequence when colored, empty string otherwise.
    let dim = if colored { color::DIM } else { "" };
    let bold = if colored { color::BOLD } else { "" };
    let reset = if colored { color::RESET } else { "" };

    let mut out = String::new();

    for (i, entry) in entries.iter().enumerate() {
        let depth = entry.path.matches('/').count();
        let short_name = entry.path.rsplit('/').next().unwrap_or(&entry.path);

        // Determine the parent path of this entry
        let parent = parent_of(&entry.path).unwrap_or("");

        // Is this the last sibling at its depth?
        let is_last = !entries[i + 1..]
            .iter()
            .any(|e| parent_of(&e.path).unwrap_or("") == parent);

        // Check whether the ancestor at depth d+1 has more siblings under
        // the ancestor at depth d.  Used for both prefix bars and description
        // continuation bars.
        let parts: Vec<&str> = entry.path.split('/').collect();
        let ancestor_has_more = |d: usize| -> bool {
            let child_path = parts[..=d + 1].join("/");
            let child_parent = parts[..=d].join("/");
            entries.iter().any(|e| {
                e.path != child_path
                    && parent_of(&e.path).unwrap_or("") == child_parent
                    && e.path > child_path
            })
        };

        // Build the prefix string with tree-drawing characters
        let mut prefix = String::new();
        if depth > 0 {
            for d in 0..depth - 1 {
                if ancestor_has_more(d) {
                    write!(prefix, "{dim}│  ").unwrap();
                } else {
                    prefix.push_str("   ");
                }
            }
            if is_last {
                write!(prefix, "{dim}└─ {reset}").unwrap();
            } else {
                write!(prefix, "{dim}├─ {reset}").unwrap();
            }
        }

        let sc = if colored {
            status_color(&entry.node.status)
        } else {
            ""
        };
        writeln!(
            out,
            "{prefix}{bold}{short_name}{reset} {dim}[{sc}{}{reset}{dim}]{reset}",
            entry.node.status,
        )
        .unwrap();

        // Print description on the next line if present and non-empty
        if !entry.node.description.is_empty() {
            let mut desc_indent = String::new();
            if depth == 0 {
                desc_indent.push_str("  ");
            } else {
                for d in 0..depth - 1 {
                    if ancestor_has_more(d) {
                        write!(desc_indent, "{dim}│  ").unwrap();
                    } else {
                        desc_indent.push_str("   ");
                    }
                }
                if is_last {
                    desc_indent.push_str("   ");
                } else {
                    write!(desc_indent, "{dim}│  ").unwrap();
                }
            }
            let indent_width = console::measure_text_width(&desc_indent);
            let avail = if term_width > 0 {
                term_width.saturating_sub(indent_width)
            } else {
                0
            };
            for line in node::wrap_str(&entry.node.description, avail) {
                writeln!(out, "{desc_indent}{dim}{line}{reset}").unwrap();
            }
        }
    }
    out
}

/// Print a list of node entries as a colored tree to stdout.
fn print_tree(entries: &[NodeEntry]) {
    let term_width = console::Term::stdout().size().1 as usize;
    print!("{}", render_tree(entries, term_width, true));
}

/// Print a flat list of nodes with colors (for `node list` non-recursive mode).
fn print_list(entries: &[NodeEntry]) {
    for entry in entries {
        let sc = status_color(&entry.node.status);
        println!(
            "{path:<40} {dim}[{sc}{status}{reset}{dim}]{reset} {cyan}{name}{reset}",
            path = entry.path,
            dim = color::DIM,
            sc = sc,
            status = entry.node.status,
            reset = color::RESET,
            cyan = color::CYAN,
            name = entry.node.name,
        );
    }
}

/// Collect all known repos from existing nodes, default_repo, and github_orgs.
fn collect_known_repos(org_root: &Path) -> Vec<String> {
    let mut repos = std::collections::BTreeSet::new();

    // Repos from existing nodes
    if let Ok(entries) = walk_nodes(org_root) {
        for entry in &entries {
            for repo in &entry.node.repos {
                repos.insert(repo.clone());
            }
        }
    }

    // default_repo and github_orgs from armitage.toml
    if let Ok(org) = Org::open(org_root) {
        if let Some(ref dr) = org.info().default_repo {
            repos.insert(dr.clone());
        }
        // Add org prefixes as completable hints (e.g. "acme/")
        for org_name in &org.info().github_orgs {
            repos.insert(format!("{org_name}/"));
        }
    }

    repos.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Back-navigation support for interactive prompts
// ---------------------------------------------------------------------------

const BACK_CMD: &str = "<";

enum Input<T> {
    Value(T),
    Back,
}

/// Check if raw input is the back command.
fn is_back(s: &str) -> bool {
    s.trim() == BACK_CMD
}

/// Like rl_with_default but returns Input::Back on "<".
fn rl_field(rl: &mut DefaultEditor, label: &str, default: &str) -> Result<Input<String>> {
    let prompt = format!("{label} [{default}]: ");
    match rl.readline(&prompt) {
        Ok(line) => {
            let trimmed = line.trim();
            if is_back(trimmed) {
                Ok(Input::Back)
            } else if trimmed.is_empty() {
                Ok(Input::Value(default.to_string()))
            } else {
                Ok(Input::Value(trimmed.to_string()))
            }
        }
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            std::process::exit(0);
        }
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

/// Like rl_optional but returns Input::Back on "<".
fn rl_field_optional(rl: &mut DefaultEditor, prompt: &str) -> Result<Input<Option<String>>> {
    match rl.readline(prompt) {
        Ok(line) => {
            let trimmed = line.trim().to_string();
            if is_back(&trimmed) {
                Ok(Input::Back)
            } else if trimmed.is_empty() {
                Ok(Input::Value(None))
            } else {
                Ok(Input::Value(Some(trimmed)))
            }
        }
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            std::process::exit(0);
        }
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

/// Like rl_comma_complete but returns Input::Back on "<".
fn rl_field_comma(items: Vec<String>, prompt: &str) -> Result<Input<Option<String>>> {
    let mut rl = Editor::new().map_err(|e| Error::Other(format!("failed to init editor: {e}")))?;
    rl.set_helper(Some(CommaCompleteHelper { items }));
    match rl.readline(prompt) {
        Ok(line) => {
            let trimmed = line.trim().to_string();
            if is_back(&trimmed) {
                Ok(Input::Back)
            } else if trimmed.is_empty() {
                Ok(Input::Value(None))
            } else {
                Ok(Input::Value(Some(trimmed)))
            }
        }
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            std::process::exit(0);
        }
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

/// Like rl_comma_complete_with_default but returns Input::Back on "<".
fn rl_field_comma_default(items: Vec<String>, label: &str, default: &str) -> Result<Input<String>> {
    let mut rl = Editor::new().map_err(|e| Error::Other(format!("failed to init editor: {e}")))?;
    rl.set_helper(Some(CommaCompleteHelper { items }));
    let prompt = format!("{label} [{default}]: ");
    match rl.readline(&prompt) {
        Ok(line) => {
            let trimmed = line.trim();
            if is_back(trimmed) {
                Ok(Input::Back)
            } else if trimmed.is_empty() {
                Ok(Input::Value(default.to_string()))
            } else {
                Ok(Input::Value(trimmed.to_string()))
            }
        }
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            std::process::exit(0);
        }
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

/// Core create logic, separated for testability.
pub fn create_node(
    org_root: &Path,
    path: &str,
    name: Option<&str>,
    description: Option<&str>,
    github_issue: Option<&str>,
    labels: Option<&str>,
    status: &str,
) -> Result<()> {
    create_node_full(
        org_root,
        path,
        name,
        description,
        github_issue,
        labels,
        &[],
        &[],
        status,
        None,
    )
}

fn parent_of(path: &str) -> Option<&str> {
    let p = Path::new(path);
    p.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_str().expect("path is valid utf-8"))
}

fn parse_node_status(s: &str) -> Result<NodeStatus> {
    Ok(s.parse::<NodeStatus>()?)
}

/// CLI entry point: armitage node create
/// Interactive when no path is given; non-interactive when path (or any option) is provided.
#[allow(clippy::too_many_arguments)]
pub fn run_create(
    path: Option<String>,
    name: Option<String>,
    description: Option<String>,
    github_issue: Option<String>,
    labels: Option<String>,
    repos: Option<String>,
    owners: Option<String>,
    status: Option<String>,
    timeline: Option<String>,
) -> Result<()> {
    let has_any_option = path.is_some()
        || name.is_some()
        || description.is_some()
        || github_issue.is_some()
        || labels.is_some()
        || repos.is_some()
        || owners.is_some()
        || status.is_some()
        || timeline.is_some();

    if has_any_option {
        run_create_noninteractive(
            path,
            name,
            description,
            github_issue,
            labels,
            repos,
            owners,
            status,
            timeline,
        )
    } else {
        run_create_interactive()
    }
}

#[allow(clippy::too_many_arguments)]
fn run_create_noninteractive(
    path: Option<String>,
    name: Option<String>,
    description: Option<String>,
    github_issue: Option<String>,
    labels: Option<String>,
    repos: Option<String>,
    owners: Option<String>,
    status: Option<String>,
    timeline: Option<String>,
) -> Result<()> {
    let path =
        path.ok_or_else(|| Error::Other("path is required in non-interactive mode".to_string()))?;
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let repos_vec: Vec<String> = repos
        .map(|r| {
            r.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let owners_vec: Vec<String> = owners
        .map(|o| {
            o.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let tl = match timeline.as_deref() {
        Some(s) => parse_timeline_input(s)?,
        None => None,
    };

    create_node_full(
        &org_root,
        &path,
        name.as_deref(),
        description.as_deref(),
        github_issue.as_deref(),
        labels.as_deref(),
        &repos_vec,
        &owners_vec,
        &status.unwrap_or_else(|| "active".to_string()),
        tl,
    )
}

fn run_create_interactive() -> Result<()> {
    const LAST_STEP: usize = 7;

    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let existing = walk_nodes(&org_root)?;
    if !existing.is_empty() {
        println!("{}Current roadmap:{}", color::BOLD, color::RESET);
        print_tree(&existing);
        println!();
    }

    println!("(Type < to go back to the previous field)\n");

    // Step 0: Path (no back from here)
    let path = rl_path(&existing, "Path (e.g. backend/auth, Tab to complete): ")?;

    let mut rl = DefaultEditor::new()
        .map_err(|e| Error::Other(format!("failed to init line editor: {e}")))?;

    // Compute parent defaults once
    let parent_node = parent_of(&path).and_then(|pp| existing.iter().find(|e| e.path == pp));
    let parent_repos = parent_node
        .map(|e| e.node.repos.join(", "))
        .unwrap_or_default();
    let parent_labels = parent_node
        .map(|e| e.node.labels.join(", "))
        .unwrap_or_default();
    let default_name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Collected values
    let mut name = String::new();
    let mut description: Option<String> = None;
    let mut status = String::new();
    let mut repos_str: Option<String> = None;
    let mut labels_str: Option<String> = None;
    let mut owners_str: Option<String> = None;
    let mut github_issue: Option<String> = None;
    let mut tl: Option<armitage_core::node::Timeline> = None;

    let mut step: usize = 0;

    while step <= LAST_STEP {
        match step {
            0 => match rl_field(&mut rl, "Name", &default_name)? {
                Input::Value(v) => {
                    name = v;
                    step += 1;
                }
                Input::Back => println!("  (already at first field)"),
            },
            1 => match rl_field_optional(&mut rl, "Description: ")? {
                Input::Value(v) => {
                    description = v;
                    step += 1;
                }
                Input::Back => {
                    step -= 1;
                }
            },
            2 => match rl_field(
                &mut rl,
                "Status (active/paused/completed/cancelled)",
                "active",
            )? {
                Input::Value(v) => {
                    if parse_node_status(&v).is_err() {
                        println!("  Invalid status. Choose: active, paused, completed, cancelled.");
                    } else {
                        status = v;
                        step += 1;
                    }
                }
                Input::Back => {
                    step -= 1;
                }
            },
            3 => {
                let known_repos = collect_known_repos(&org_root);
                let result = if parent_repos.is_empty() {
                    rl_field_comma(known_repos, "Repos (comma-separated, Tab to complete): ")?
                } else {
                    match rl_field_comma_default(
                        known_repos,
                        "Repos (comma-separated, Tab to complete)",
                        &parent_repos,
                    )? {
                        Input::Value(v) => Input::Value(Some(v)),
                        Input::Back => Input::Back,
                    }
                };
                match result {
                    Input::Value(v) => {
                        repos_str = v;
                        step += 1;
                    }
                    Input::Back => {
                        step -= 1;
                    }
                }
            }
            4 => {
                let label_names = LabelsFile::read(&org_root)?.names();
                let result = if parent_labels.is_empty() {
                    rl_field_comma(label_names, "Labels (comma-separated, Tab to complete): ")?
                } else {
                    match rl_field_comma_default(
                        label_names,
                        "Labels (comma-separated, Tab to complete)",
                        &parent_labels,
                    )? {
                        Input::Value(v) => Input::Value(Some(v)),
                        Input::Back => Input::Back,
                    }
                };
                match result {
                    Input::Value(v) => {
                        // Validate and create undefined labels
                        if let Some(ref ls) = v {
                            let entered: Vec<String> = ls
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let mut lf = LabelsFile::read(&org_root)?;
                            let undefined: Vec<&String> =
                                entered.iter().filter(|l| !lf.has(l)).collect();
                            if !undefined.is_empty() {
                                println!(
                                    "\n  The following labels are not defined in labels.toml:"
                                );
                                for n in &undefined {
                                    println!("    - {n}");
                                }
                                println!();
                                for n in &undefined {
                                    let desc = rl_with_default(
                                        &mut rl,
                                        &format!("  Description for '{n}'"),
                                        "",
                                    )?;
                                    lf.add((*n).clone(), desc);
                                }
                                lf.write(&org_root)?;
                                println!("  Labels added to labels.toml.\n");
                            }
                        }
                        labels_str = v;
                        step += 1;
                    }
                    Input::Back => {
                        step -= 1;
                    }
                }
            }
            5 => {
                match rl_field_optional(
                    &mut rl,
                    "Owners (comma-separated GitHub usernames, or blank): ",
                )? {
                    Input::Value(v) => {
                        owners_str = v;
                        step += 1;
                    }
                    Input::Back => {
                        step -= 1;
                    }
                }
            }
            6 => {
                match rl_field_optional(&mut rl, "GitHub issue (owner/repo#number, or blank): ")? {
                    Input::Value(v) => {
                        github_issue = v;
                        step += 1;
                    }
                    Input::Back => {
                        step -= 1;
                    }
                }
            }
            7 => {
                let parent_tl = parent_node.and_then(|e| e.node.timeline.as_ref());

                // Timeline uses its own sub-prompts; support back to skip it entirely
                let input = rl_field(&mut rl, "Set timeline? (yes/no)", "no")?;
                match input {
                    Input::Back => {
                        step -= 1;
                    }
                    Input::Value(v) if v == "yes" || v == "y" => {
                        if let Some(pt) = parent_tl {
                            println!(
                                "  {}Parent timeline: {} to {}{}",
                                color::DIM,
                                pt.start,
                                pt.end,
                                color::RESET
                            );
                        }
                        let default_start = parent_tl
                            .map_or_else(|| chrono::Local::now().date_naive(), |pt| pt.start);
                        println!("  Enter start date:");
                        let start = rl_date_fields(&mut rl, default_start)?;
                        let default_end =
                            parent_tl.map_or_else(|| start + chrono::Months::new(6), |pt| pt.end);
                        let default_end = if default_end < start {
                            start + chrono::Months::new(6)
                        } else {
                            default_end
                        };
                        println!("  Enter end date:");
                        loop {
                            let end = rl_date_fields(&mut rl, default_end)?;
                            if end < start {
                                println!(
                                    "  End date ({end}) must be on or after start date ({start})."
                                );
                                continue;
                            }
                            let child = armitage_core::node::Timeline { start, end };
                            if let Some(pt) = parent_tl
                                && !pt.contains(&child)
                            {
                                println!(
                                    "  {}Timeline {start} to {end} exceeds parent timeline {} to {}.{}",
                                    color::RED,
                                    pt.start,
                                    pt.end,
                                    color::RESET,
                                );
                                println!("  Please enter an end date within the parent range.");
                                continue;
                            }
                            let days = end.signed_duration_since(start).num_days();
                            println!("  Timeline: {start} to {end} ({})", format_duration(days));
                            tl = Some(child);
                            break;
                        }
                        step += 1;
                    }
                    Input::Value(_) => {
                        tl = None;
                        step += 1;
                    }
                }
            }
            _ => break,
        }
    }

    let repos: Vec<String> = repos_str
        .map(|r| {
            r.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let owners: Vec<String> = owners_str
        .map(|o| {
            o.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    create_node_full(
        &org_root,
        &path,
        Some(&name),
        description.as_deref(),
        github_issue.as_deref(),
        labels_str.as_deref(),
        &repos,
        &owners,
        &status,
        tl,
    )
}

/// Full create including repos field.
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_node_full(
    org_root: &Path,
    path: &str,
    name: Option<&str>,
    description: Option<&str>,
    github_issue: Option<&str>,
    labels: Option<&str>,
    repos: &[String],
    owners: &[String],
    status: &str,
    timeline: Option<armitage_core::node::Timeline>,
) -> Result<()> {
    // Check parent exists if nested path
    if let Some(parent_path) = parent_of(path) {
        let parent_dir = org_root.join(parent_path);
        if !parent_dir.join("node.toml").exists() {
            return Err(
                armitage_core::error::Error::ParentNotFound(parent_path.to_string()).into(),
            );
        }
    }

    // Check target doesn't already exist
    let node_dir = org_root.join(path);
    if node_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeExists(path.to_string()).into());
    }

    // Validate child timeline fits within parent timeline
    if let (Some(child_tl), Some(parent_path)) = (&timeline, parent_of(path))
        && let Ok(parent_entry) = read_node(org_root, parent_path)
        && let Some(parent_tl) = &parent_entry.node.timeline
        && !parent_tl.contains(child_tl)
    {
        return Err(Error::Other(format!(
            "timeline {start} to {end} exceeds parent '{parent}' timeline {ps} to {pe}",
            start = child_tl.start,
            end = child_tl.end,
            parent = parent_path,
            ps = parent_tl.start,
            pe = parent_tl.end,
        )));
    }

    let derived_name = name.map_or_else(
        || {
            Path::new(path)
                .file_name()
                .map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string())
        },
        std::string::ToString::to_string,
    );

    let labels_vec: Vec<String> = labels
        .map(|l| {
            l.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // Validate labels against labels.toml
    if !labels_vec.is_empty() {
        let lf = LabelsFile::read(org_root)?;
        let undefined: Vec<&str> = labels_vec
            .iter()
            .filter(|l| !lf.has(l))
            .map(std::string::String::as_str)
            .collect();
        if !undefined.is_empty() {
            return Err(Error::Other(format!(
                "undefined label(s): {}. Define them in labels.toml first, \
                 or use interactive mode (armitage node new) to create them.",
                undefined.join(", ")
            )));
        }
    }

    let node_status = parse_node_status(status)?;

    let node = Node {
        name: derived_name,
        description: description.unwrap_or("").to_string(),
        triage_hint: None,
        github_issue: github_issue.map(std::string::ToString::to_string),
        labels: labels_vec,
        repos: repos.to_vec(),
        owners: owners.to_vec(),
        team: None,
        timeline,
        status: node_status,
    };

    std::fs::create_dir_all(&node_dir)?;
    let toml_content = node.to_toml()?;
    std::fs::write(node_dir.join("node.toml"), toml_content)?;

    println!("Created node at '{path}'");
    Ok(())
}

/// Prompt for a node path with tab-completion from existing nodes.
fn rl_path(entries: &[NodeEntry], prompt: &str) -> Result<String> {
    let helper = NodePathHelper::from_entries(entries);
    let mut rl = Editor::new().map_err(|e| Error::Other(format!("failed to init editor: {e}")))?;
    rl.set_helper(Some(helper));
    loop {
        match rl.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim().trim_end_matches('/').to_string();
                if !trimmed.is_empty() {
                    return Ok(trimmed);
                }
                println!("  This field is required.");
            }
            Err(
                rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
            ) => {
                std::process::exit(0);
            }
            Err(e) => return Err(Error::Other(format!("readline error: {e}"))),
        }
    }
}

fn rl_with_default(rl: &mut DefaultEditor, label: &str, default: &str) -> Result<String> {
    let prompt = format!("{label} [{default}]: ");
    match rl.readline(&prompt) {
        Ok(line) => {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                Ok(default.to_string())
            } else {
                Ok(trimmed.to_string())
            }
        }
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            std::process::exit(0);
        }
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

/// CLI entry point: armitage node list
pub fn run_list(path: Option<String>, recursive: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let entries = if recursive {
        match &path {
            Some(p) => {
                // All nodes under this path
                let all = walk_nodes(&org_root)?;
                all.into_iter()
                    .filter(|e| e.path == *p || e.path.starts_with(&format!("{p}/")))
                    .collect()
            }
            None => walk_nodes(&org_root)?,
        }
    } else {
        list_children(&org_root, path.as_deref().unwrap_or(""))?
    };

    if recursive {
        print_tree(&entries);
    } else {
        print_list(&entries);
    }
    Ok(())
}

/// CLI entry point: armitage node show
pub fn run_show(path: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let entry = read_node(&org_root, &path)?;

    println!("name:        {}", entry.node.name);
    println!("description: {}", entry.node.description);
    if let Some(ref hint) = entry.node.triage_hint {
        println!("triage_hint: {hint}");
    }
    println!("status:      {}", entry.node.status);
    if let Some(ref issue) = entry.node.github_issue {
        println!("github:      {issue}");
    }
    if !entry.node.labels.is_empty() {
        println!("labels:      {}", entry.node.labels.join(", "));
    }
    if !entry.node.owners.is_empty() {
        println!("owners:      {}", entry.node.owners.join(", "));
    }
    if let Some(ref team) = entry.node.team {
        println!("team:        {team}");
    }
    if let Some(ref tl) = entry.node.timeline {
        println!("timeline:    {} — {}", tl.start, tl.end);
    }

    let children = list_children(&org_root, &path)?;
    if !children.is_empty() {
        println!("\nchildren:");
        for child in &children {
            println!("  {} [{}]", child.path, child.node.status);
        }
    }

    // Show milestones if present
    let milestone_path = org_root.join(&path).join("milestones.toml");
    if milestone_path.exists() {
        let content = std::fs::read_to_string(&milestone_path)?;
        let mf: armitage_milestones::milestone::MilestoneFile =
            toml::from_str(&content).map_err(|source| armitage_core::error::Error::TomlParse {
                path: milestone_path,
                source,
            })?;
        if !mf.milestones.is_empty() {
            println!("\nmilestones:");
            for m in &mf.milestones {
                println!("  {} ({}) — {}", m.name, m.date, m.description);
            }
        }
    }

    Ok(())
}

/// CLI entry point: armitage node edit
pub fn run_edit(path: String) -> Result<()> {
    const LAST_STEP: usize = 7;

    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let entry = read_node(&org_root, &path)?;
    let node = entry.node.clone();
    let node_toml_path = entry.dir.join("node.toml");

    // Look up parent for timeline validation
    let parent_tl = parent_of(&path)
        .and_then(|pp| read_node(&org_root, pp).ok())
        .and_then(|e| e.node.timeline);

    println!("Editing '{path}' — press Enter to keep current value, type < to go back.\n");

    let mut rl = DefaultEditor::new()
        .map_err(|e| Error::Other(format!("failed to init line editor: {e}")))?;

    let repos_default = node.repos.join(", ");
    let labels_default = node.labels.join(", ");
    let owners_default = node.owners.join(", ");
    let gh_default = node.github_issue.as_deref().unwrap_or("").to_string();

    let mut name = node.name.clone();
    let mut description = node.description.clone();
    let mut status = node.status.clone();
    let mut repos: Vec<String> = node.repos.clone();
    let mut labels: Vec<String> = node.labels.clone();
    let mut owners: Vec<String> = node.owners.clone();
    let mut github_issue: Option<String> = node.github_issue.clone();
    let mut timeline: Option<armitage_core::node::Timeline> = node.timeline.clone();

    let mut step: usize = 0;

    while step <= LAST_STEP {
        match step {
            0 => match rl_field(&mut rl, "Name", &name)? {
                Input::Value(v) => {
                    name = v;
                    step += 1;
                }
                Input::Back => println!("  (already at first field)"),
            },
            1 => match rl_field(&mut rl, "Description", &description)? {
                Input::Value(v) => {
                    description = v;
                    step += 1;
                }
                Input::Back => {
                    step -= 1;
                }
            },
            2 => match rl_field(
                &mut rl,
                "Status (active/paused/completed/cancelled)",
                &status.to_string(),
            )? {
                Input::Value(v) => match parse_node_status(&v) {
                    Ok(s) => {
                        status = s;
                        step += 1;
                    }
                    Err(_) => {
                        println!("  Invalid status. Choose: active, paused, completed, cancelled.");
                    }
                },
                Input::Back => {
                    step -= 1;
                }
            },
            3 => {
                let known = collect_known_repos(&org_root);
                match rl_field_comma_default(
                    known,
                    "Repos (comma-separated, Tab to complete)",
                    &repos_default,
                )? {
                    Input::Value(v) => {
                        repos = v
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        step += 1;
                    }
                    Input::Back => {
                        step -= 1;
                    }
                }
            }
            4 => {
                let known = LabelsFile::read(&org_root)?.names();
                match rl_field_comma_default(
                    known,
                    "Labels (comma-separated, Tab to complete)",
                    &labels_default,
                )? {
                    Input::Value(v) => {
                        labels = v
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        // Validate and create undefined labels
                        if !labels.is_empty() {
                            let mut lf = LabelsFile::read(&org_root)?;
                            let undefined: Vec<&String> =
                                labels.iter().filter(|l| !lf.has(l)).collect();
                            if !undefined.is_empty() {
                                println!(
                                    "\n  The following labels are not defined in labels.toml:"
                                );
                                for n in &undefined {
                                    println!("    - {n}");
                                }
                                println!();
                                for n in &undefined {
                                    let desc = rl_with_default(
                                        &mut rl,
                                        &format!("  Description for '{n}'"),
                                        "",
                                    )?;
                                    lf.add((*n).clone(), desc);
                                }
                                lf.write(&org_root)?;
                                println!("  Labels added to labels.toml.\n");
                            }
                        }
                        step += 1;
                    }
                    Input::Back => {
                        step -= 1;
                    }
                }
            }
            5 => match rl_field(
                &mut rl,
                "Owners (comma-separated GitHub usernames)",
                &owners_default,
            )? {
                Input::Value(v) => {
                    owners = v
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    step += 1;
                }
                Input::Back => {
                    step -= 1;
                }
            },
            6 => match rl_field(
                &mut rl,
                "GitHub issue (owner/repo#number, or 'none')",
                &gh_default,
            )? {
                Input::Value(v) => {
                    github_issue = match v.as_str() {
                        "" | "none" | "null" => None,
                        s => Some(s.to_string()),
                    };
                    step += 1;
                }
                Input::Back => {
                    step -= 1;
                }
            },
            7 => {
                let has = timeline.is_some();
                match rl_field(
                    &mut rl,
                    "Set timeline? (yes/no)",
                    if has { "yes" } else { "no" },
                )? {
                    Input::Back => {
                        step -= 1;
                    }
                    Input::Value(v) if v == "yes" || v == "y" => {
                        if let Some(ref pt) = parent_tl {
                            println!(
                                "  {}Parent timeline: {} to {}{}",
                                color::DIM,
                                pt.start,
                                pt.end,
                                color::RESET,
                            );
                        }
                        let default_start = parent_tl
                            .as_ref()
                            .map(|pt| pt.start)
                            .or_else(|| timeline.as_ref().map(|t| t.start))
                            .unwrap_or_else(|| chrono::Local::now().date_naive());
                        println!("  Enter start date:");
                        let start = rl_date_fields(&mut rl, default_start)?;
                        let default_end = parent_tl
                            .as_ref()
                            .map(|pt| pt.end)
                            .or_else(|| timeline.as_ref().map(|t| t.end))
                            .unwrap_or_else(|| start + chrono::Months::new(6));
                        let default_end = if default_end < start {
                            start + chrono::Months::new(6)
                        } else {
                            default_end
                        };
                        println!("  Enter end date:");
                        loop {
                            let end = rl_date_fields(&mut rl, default_end)?;
                            if end < start {
                                println!(
                                    "  End date ({end}) must be on or after start date ({start})."
                                );
                                continue;
                            }
                            let child = armitage_core::node::Timeline { start, end };
                            if let Some(ref pt) = parent_tl
                                && !pt.contains(&child)
                            {
                                println!(
                                    "  {}Timeline {start} to {end} exceeds parent timeline {} to {}.{}",
                                    color::RED,
                                    pt.start,
                                    pt.end,
                                    color::RESET,
                                );
                                println!("  Please enter an end date within the parent range.");
                                continue;
                            }
                            let days = end.signed_duration_since(start).num_days();
                            println!("  Timeline: {start} to {end} ({})", format_duration(days));
                            timeline = Some(child);
                            break;
                        }
                        step += 1;
                    }
                    Input::Value(_) => {
                        timeline = None;
                        step += 1;
                    }
                }
            }
            _ => break,
        }
    }

    let updated = Node {
        name,
        description,
        triage_hint: node.triage_hint.clone(),
        github_issue,
        labels,
        repos,
        owners,
        team: node.team,
        timeline,
        status,
    };

    let toml_content = updated.to_toml()?;
    std::fs::write(&node_toml_path, toml_content)?;

    println!("\nSaved '{path}'");
    Ok(())
}

fn format_duration(days: i64) -> String {
    if days >= 365 {
        let years = days / 365;
        let remaining_months = (days % 365) / 30;
        if remaining_months > 0 {
            format!("~{years} year(s), {remaining_months} month(s)")
        } else {
            format!("~{years} year(s)")
        }
    } else if days >= 30 {
        format!("~{} month(s)", (days + 15) / 30)
    } else {
        format!("{days} day(s)")
    }
}

fn parse_timeline_input(input: &str) -> Result<Option<armitage_core::node::Timeline>> {
    let input = input.trim();
    if input.is_empty() || input == "none" || input == "null" {
        return Ok(None);
    }
    let parts: Vec<&str> = input.splitn(2, " to ").collect();
    if parts.len() != 2 {
        return Err(Error::Other(format!(
            "invalid timeline format: '{input}'. Expected: YYYY-MM-DD to YYYY-MM-DD"
        )));
    }
    let start = chrono::NaiveDate::parse_from_str(parts[0].trim(), "%Y-%m-%d").map_err(|_| {
        Error::Other(format!(
            "invalid start date: '{}'. Expected format: YYYY-MM-DD",
            parts[0].trim()
        ))
    })?;
    let end = chrono::NaiveDate::parse_from_str(parts[1].trim(), "%Y-%m-%d").map_err(|_| {
        Error::Other(format!(
            "invalid end date: '{}'. Expected format: YYYY-MM-DD",
            parts[1].trim()
        ))
    })?;
    if end < start {
        return Err(Error::Other(format!(
            "end date ({end}) is before start date ({start})"
        )));
    }
    Ok(Some(armitage_core::node::Timeline { start, end }))
}

/// Prompt for year, month, day with defaults and per-field validation.
fn rl_date_fields(rl: &mut DefaultEditor, default: chrono::NaiveDate) -> Result<chrono::NaiveDate> {
    use chrono::Datelike;

    loop {
        let year_str = rl_with_default(rl, "    Year", &default.year().to_string())?;
        let year: i32 = match year_str.parse() {
            Ok(y) if (2000..=2100).contains(&y) => y,
            _ => {
                println!("    Invalid year. Enter a 4-digit year (2000-2100).");
                continue;
            }
        };

        let month_str =
            rl_with_default(rl, "    Month (1-12)", &format!("{:02}", default.month()))?;
        let month: u32 = match month_str.parse() {
            Ok(m) if (1..=12).contains(&m) => m,
            _ => {
                println!("    Invalid month. Enter 1-12.");
                continue;
            }
        };

        // Calculate max days for this year/month
        let max_day = days_in_month(year, month);
        let default_day = default.day().min(max_day);
        let day_str = rl_with_default(
            rl,
            &format!("    Day (1-{max_day})"),
            &format!("{default_day:02}"),
        )?;
        let day: u32 = match day_str.parse() {
            Ok(d) if (1..=max_day).contains(&d) => d,
            _ => {
                println!("    Invalid day. Enter 1-{max_day}.");
                continue;
            }
        };

        match chrono::NaiveDate::from_ymd_opt(year, month, day) {
            Some(date) => return Ok(date),
            None => {
                println!("    Invalid date. Try again.");
            }
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    use chrono::Datelike;
    if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .and_then(|d| d.pred_opt())
    .map_or(28, |d| d.day())
}

/// Core move logic, separated for testability.
pub fn move_node(org_root: &Path, from: &str, to: &str) -> Result<()> {
    let from_dir = org_root.join(from);
    if !from_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeNotFound(from.to_string()).into());
    }

    let to_dir = org_root.join(to);
    if to_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeExists(to.to_string()).into());
    }

    // Check to's parent exists
    if let Some(to_parent) = parent_of(to) {
        let to_parent_dir = org_root.join(to_parent);
        if !to_parent_dir.join("node.toml").exists() {
            return Err(armitage_core::error::Error::ParentNotFound(to_parent.to_string()).into());
        }
    }

    std::fs::rename(&from_dir, &to_dir)?;
    println!("Moved '{from}' → '{to}'");
    Ok(())
}

/// CLI entry point: armitage node move
pub fn run_move(from: String, to: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    move_node(&org_root, &from, &to)
}

/// CLI entry point: armitage node remove
pub fn run_remove(path: String, yes: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let node_dir = org_root.join(&path);
    if !node_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeNotFound(path).into());
    }

    if !yes {
        print!("Remove '{path}'? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    std::fs::remove_dir_all(&node_dir)?;
    println!("Removed '{path}'");
    Ok(())
}

/// CLI entry point: armitage node merge <from> <to>
/// Merges one node into another: reassigns triage suggestions, moves children, removes source.
pub fn run_merge(from: String, to: String, yes: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let from_dir = org_root.join(&from);
    let to_dir = org_root.join(&to);

    if !from_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeNotFound(from).into());
    }
    if !to_dir.join("node.toml").exists() {
        return Err(armitage_core::error::Error::NodeNotFound(to).into());
    }
    if from == to {
        return Err(Error::Other("cannot merge a node into itself".to_string()));
    }

    // Count what will be affected
    let children: Vec<String> = list_children(&org_root, &from)?
        .into_iter()
        .map(|e| e.path)
        .collect();

    let db_path = org_root.join(".armitage/issues.db");
    let has_db = db_path.exists();
    let suggestion_count = if has_db {
        let conn = armitage_triage::db::open_db(&org_root)?;
        armitage_triage::db::get_suggestions_filtered(
            &conn,
            &armitage_triage::db::SuggestionFilters {
                node_prefix: Some(from.clone()),
                ..Default::default()
            },
        )?
        .len()
    } else {
        0
    };

    println!("Merge '{from}' into '{to}':");
    println!("  {suggestion_count} triage suggestion(s) will be reassigned");
    if !children.is_empty() {
        println!("  {} child node(s) will be moved:", children.len());
        for child in &children {
            let new_path = child.replacen(&from, &to, 1);
            println!("    {child} -> {new_path}");
        }
    }
    println!("  '{from}' will be removed");

    if !yes {
        print!("Proceed? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    // 1. Reassign triage suggestions in DB
    if has_db && suggestion_count > 0 {
        let conn = armitage_triage::db::open_db(&org_root)?;
        let reassigned = armitage_triage::db::reassign_suggestions(&conn, &from, &to)?;
        println!("Reassigned {reassigned} suggestion(s) from '{from}' to '{to}'");
        // Refresh issue cache
        armitage_triage::cache::refresh_all(&conn, &org_root)?;
    }

    // 2. Move children from source to target (deepest first to avoid path conflicts)
    let mut sorted_children = children;
    sorted_children.sort_by_key(|b| std::cmp::Reverse(b.len()));
    for child in &sorted_children {
        let new_path = child.replacen(&from, &to, 1);
        let new_dir = org_root.join(&new_path);
        if new_dir.exists() {
            println!("  Warning: '{new_path}' already exists, skipping move of '{child}'");
            continue;
        }
        if let Some(parent) = new_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(org_root.join(child), &new_dir)?;
        println!("  Moved '{child}' -> '{new_path}'");
    }

    // 3. Remove the source node directory
    std::fs::remove_dir_all(&from_dir)?;
    println!("Removed '{from}'");
    println!("Merge complete.");

    Ok(())
}

/// CLI entry point: armitage node tree
pub fn run_tree(max_depth: Option<usize>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let all = walk_nodes(&org_root)?;
    let filtered: Vec<_> = match max_depth {
        Some(d) => all
            .into_iter()
            .filter(|e| e.path.matches('/').count() < d)
            .collect(),
        None => all,
    };
    print_tree(&filtered);
    Ok(())
}

/// CLI entry point: armitage node set
/// Set fields on a node non-interactively.
#[allow(clippy::too_many_arguments)]
pub fn run_set(
    path: String,
    name: Option<String>,
    description: Option<String>,
    triage_hint: Option<String>,
    owners: Option<String>,
    team: Option<String>,
    repos: Option<String>,
    labels: Option<String>,
    status: Option<String>,
    timeline_start: Option<String>,
    timeline_end: Option<String>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let entry = read_node(&org_root, &path)?;
    let mut node = entry.node.clone();

    if let Some(n) = name {
        node.name = n;
    }
    if let Some(d) = description {
        node.description = d;
    }
    if let Some(h) = triage_hint {
        node.triage_hint = if h.is_empty() || h == "none" {
            None
        } else {
            Some(h)
        };
    }
    if let Some(o) = owners {
        node.owners = o
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(t) = team {
        node.team = if t.is_empty() || t == "none" {
            None
        } else {
            Some(t)
        };
    }
    if let Some(r) = repos {
        node.repos = r
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(l) = labels {
        node.labels = l
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(s) = status {
        node.status = parse_node_status(&s)?;
    }
    if timeline_start.is_some() || timeline_end.is_some() {
        let new_start = timeline_start
            .map(|s| {
                chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| {
                    Error::Other(format!(
                        "invalid --timeline-start '{}': expected YYYY-MM-DD",
                        s.trim()
                    ))
                })
            })
            .transpose()?;
        let new_end = timeline_end
            .map(|s| {
                chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| {
                    Error::Other(format!(
                        "invalid --timeline-end '{}': expected YYYY-MM-DD",
                        s.trim()
                    ))
                })
            })
            .transpose()?;

        let existing = node.timeline.as_ref();
        let start = new_start
            .or_else(|| existing.map(|t| t.start))
            .ok_or_else(|| {
                Error::Other(
                    "cannot set --timeline-end without an existing start date; \
                     also provide --timeline-start"
                        .into(),
                )
            })?;
        let end = new_end.or_else(|| existing.map(|t| t.end)).ok_or_else(|| {
            Error::Other(
                "cannot set --timeline-start without an existing end date; \
                     also provide --timeline-end"
                    .into(),
            )
        })?;
        if end < start {
            return Err(Error::Other(format!(
                "end date ({end}) is before start date ({start})"
            )));
        }
        node.timeline = Some(armitage_core::node::Timeline { start, end });
    }

    let toml_content = node.to_toml()?;
    std::fs::write(entry.dir.join("node.toml"), toml_content)?;
    println!("Updated '{path}'");

    // Warn about any owners not registered in team.toml
    let team_file = TeamFile::read(&org_root).unwrap_or_default();
    for owner in &node.owners {
        if !team_file.has(owner) {
            eprintln!(
                "{}warning:{} owner '{owner}' is not in team.toml",
                color::YELLOW,
                color::RESET,
            );
        }
    }

    Ok(())
}

/// CLI entry point: armitage node fmt
/// Re-serialize node.toml files with canonical formatting (multi-line strings, etc.).
pub fn run_fmt(paths: Vec<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let entries = if paths.is_empty() {
        walk_nodes(&org_root)?
    } else {
        let mut v = Vec::new();
        for p in &paths {
            v.push(read_node(&org_root, p)?);
        }
        v
    };

    let mut formatted = 0;
    for entry in &entries {
        let toml_path = entry.dir.join("node.toml");
        let old = std::fs::read_to_string(&toml_path)?;
        let new = entry.node.to_toml()?;
        if old != new {
            std::fs::write(&toml_path, &new)?;
            println!("Formatted '{}'", entry.path);
            formatted += 1;
        }
    }
    if formatted == 0 {
        println!("All {} node(s) already formatted.", entries.len());
    } else {
        println!("Formatted {formatted} of {} node(s).", entries.len());
    }
    Ok(())
}

/// CLI entry point: armitage node check
/// Scans the whole tree for timeline violations and other issues.
pub fn run_check(check_repos: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let all = walk_nodes(&org_root)?;

    let mut violations = 0;

    // --- Parent-child timeline containment ---
    for entry in &all {
        let Some(parent_path) = parent_of(&entry.path) else {
            continue;
        };
        let Some(parent) = all.iter().find(|e| e.path == parent_path) else {
            continue;
        };

        if let (Some(child_tl), Some(parent_tl)) = (&entry.node.timeline, &parent.node.timeline)
            && !parent_tl.contains(child_tl)
        {
            violations += 1;
            println!(
                "{}timeline violation:{} {bold}{}{reset}",
                color::RED,
                color::RESET,
                entry.path,
                bold = color::BOLD,
                reset = color::RESET,
            );
            println!("  child:  {} to {}", child_tl.start, child_tl.end,);
            println!(
                "  parent: {} to {} {dim}({}){reset}",
                parent_tl.start,
                parent_tl.end,
                parent_path,
                dim = color::DIM,
                reset = color::RESET,
            );
            if child_tl.start < parent_tl.start {
                println!(
                    "  {yellow}start date is {days} day(s) before parent start{reset}",
                    yellow = color::YELLOW,
                    days = parent_tl
                        .start
                        .signed_duration_since(child_tl.start)
                        .num_days(),
                    reset = color::RESET,
                );
            }
            if child_tl.end > parent_tl.end {
                println!(
                    "  {yellow}end date is {days} day(s) after parent end{reset}",
                    yellow = color::YELLOW,
                    days = child_tl.end.signed_duration_since(parent_tl.end).num_days(),
                    reset = color::RESET,
                );
            }
            println!(
                "  {dim}fix with: armitage node edit {}{reset}",
                entry.path,
                dim = color::DIM,
                reset = color::RESET,
            );
            println!();
        }
    }

    // --- Issue project dates vs node timeline ---
    let conn = armitage_triage::db::open_db(&org_root).ok();
    if let Some(conn) = &conn {
        for entry in &all {
            let Some(tl) = &entry.node.timeline else {
                continue;
            };
            let issues_file =
                armitage_core::issues::IssuesFile::read(&entry.dir).unwrap_or_default();
            if issues_file.is_empty() {
                continue;
            }

            for issue_entry in &issues_file.issues {
                let Some((repo, num)) = parse_issue_ref(&issue_entry.issue_ref) else {
                    continue;
                };
                let items = armitage_triage::db::get_project_items_for_issue(conn, &repo, num)
                    .unwrap_or_default();
                for item in &items {
                    let issue_label = issue_entry
                        .title
                        .as_deref()
                        .unwrap_or(&issue_entry.issue_ref);

                    // Check target_date exceeds node end
                    if let Some(target) = &item.target_date
                        && let Ok(target_date) =
                            chrono::NaiveDate::parse_from_str(target, "%Y-%m-%d")
                        && target_date > tl.end
                    {
                        violations += 1;
                        let days = target_date.signed_duration_since(tl.end).num_days();
                        println!(
                            "{}issue target date exceeds node timeline:{} {bold}{}{reset}",
                            color::RED,
                            color::RESET,
                            entry.path,
                            bold = color::BOLD,
                            reset = color::RESET,
                        );
                        println!(
                            "  {dim}{}{reset}",
                            issue_entry.issue_ref,
                            dim = color::DIM,
                            reset = color::RESET,
                        );
                        println!("  {issue_label}");
                        println!(
                            "  {yellow}target {target} is {days} day(s) after node end {end}{reset}",
                            yellow = color::YELLOW,
                            target = target,
                            days = days,
                            end = tl.end,
                            reset = color::RESET,
                        );
                        println!();
                    }

                    // Check start_date before node start
                    if let Some(start) = &item.start_date
                        && let Ok(start_date) = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d")
                        && start_date < tl.start
                    {
                        violations += 1;
                        let days = tl.start.signed_duration_since(start_date).num_days();
                        println!(
                            "{}issue start date precedes node timeline:{} {bold}{}{reset}",
                            color::RED,
                            color::RESET,
                            entry.path,
                            bold = color::BOLD,
                            reset = color::RESET,
                        );
                        println!(
                            "  {dim}{}{reset}",
                            issue_entry.issue_ref,
                            dim = color::DIM,
                            reset = color::RESET,
                        );
                        println!("  {issue_label}");
                        println!(
                            "  {yellow}start {start} is {days} day(s) before node start {node_start}{reset}",
                            yellow = color::YELLOW,
                            start = start,
                            days = days,
                            node_start = tl.start,
                            reset = color::RESET,
                        );
                        println!();
                    }
                }
            }
        }
    }

    // --- Owner validation against team.toml ---
    let team_file = TeamFile::read(&org_root).unwrap_or_default();
    let mut warnings = 0;
    for entry in &all {
        for owner in &entry.node.owners {
            if !team_file.has(owner) {
                warnings += 1;
                println!(
                    "{}warning:{} owner {bold}{owner}{reset} in node {dim}{}{reset} is not in team.toml",
                    color::YELLOW,
                    color::RESET,
                    entry.path,
                    bold = color::BOLD,
                    dim = color::DIM,
                    reset = color::RESET,
                );
            }
        }
    }
    if warnings > 0 {
        println!();
    }

    // --- Repo archived / renamed check (requires --check-repos) ---
    if check_repos {
        use std::collections::{BTreeMap, BTreeSet};

        // Collect unique bare repo names across all nodes.
        let mut repo_nodes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for entry in &all {
            for repo in &entry.node.repos {
                let bare = armitage_triage::fetch::strip_repo_qualifier(repo);
                repo_nodes.entry(bare).or_default().push(entry.path.clone());
            }
        }

        if repo_nodes.is_empty() {
            println!("{}No repos to check.{}", color::DIM, color::RESET);
        } else {
            let gh = armitage_github::require_gh()?;
            let total = repo_nodes.len();
            println!("Checking {total} repo(s)...");
            let mut checked: BTreeSet<String> = BTreeSet::new();
            for (repo, node_paths) in &repo_nodes {
                if checked.contains(repo) {
                    continue;
                }
                checked.insert(repo.clone());
                match armitage_github::issue::fetch_repo_metadata(&gh, repo) {
                    None => {
                        warnings += 1;
                        for node_path in node_paths {
                            println!(
                                "{}warning:{} repo {bold}{repo}{reset} in node {dim}{node_path}{reset} could not be fetched — may not exist",
                                color::YELLOW,
                                color::RESET,
                                bold = color::BOLD,
                                dim = color::DIM,
                                reset = color::RESET,
                            );
                        }
                    }
                    Some(meta) => {
                        // Rename detection: canonical name differs from what's in node.toml.
                        let canonical = meta.name_with_owner.to_lowercase();
                        let stored = repo.to_lowercase();
                        if canonical != stored {
                            warnings += 1;
                            for node_path in node_paths {
                                println!(
                                    "{}warning:{} repo {bold}{repo}{reset} has been renamed to {bold}{}{reset} — update node {dim}{node_path}{reset}",
                                    color::YELLOW,
                                    color::RESET,
                                    meta.name_with_owner,
                                    bold = color::BOLD,
                                    dim = color::DIM,
                                    reset = color::RESET,
                                );
                            }
                        } else if meta.is_archived {
                            warnings += 1;
                            for node_path in node_paths {
                                println!(
                                    "{}warning:{} repo {bold}{repo}{reset} is archived — consider removing from node {dim}{node_path}{reset}",
                                    color::YELLOW,
                                    color::RESET,
                                    bold = color::BOLD,
                                    dim = color::DIM,
                                    reset = color::RESET,
                                );
                            }
                        }
                    }
                }
            }
            println!();
        }
    }

    if violations == 0 && warnings == 0 {
        println!("{}No issues found.{}", color::GREEN, color::RESET);
    } else if violations == 0 {
        println!(
            "{}Found {warnings} warning(s).{}",
            color::YELLOW,
            color::RESET,
        );
    } else {
        println!(
            "{}Found {violations} issue(s), {warnings} warning(s).{}",
            color::YELLOW,
            color::RESET,
        );
    }
    Ok(())
}

/// Parse "owner/repo#123" into ("owner/repo", 123).
fn parse_issue_ref(s: &str) -> Option<(String, u64)> {
    let (repo, num_str) = s.rsplit_once('#')?;
    let num = num_str.parse().ok()?;
    Some((repo.to_string(), num))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn setup_org(tmp: &TempDir) -> PathBuf {
        let org = tmp.path().join("testorg");
        crate::cli::init::init_at(&org, "testorg", &["testorg".to_string()], None).unwrap();
        org
    }

    fn write_node(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let content = format!("name = \"{name}\"\ndescription = \"test\"\n");
        std::fs::write(dir.join("node.toml"), content).unwrap();
    }

    #[test]
    fn create_top_level_node() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_node(&org, "gemini", None, None, None, None, "active").unwrap();

        assert!(org.join("gemini").join("node.toml").exists());
        let content = std::fs::read_to_string(org.join("gemini").join("node.toml")).unwrap();
        let node: Node = toml::from_str(&content).unwrap();
        assert_eq!(node.name, "gemini");
        assert_eq!(node.status, NodeStatus::Active);
    }

    #[test]
    fn create_with_custom_name() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_node(
            &org,
            "gemini",
            Some("Gemini Project"),
            Some("A cool project"),
            None,
            None,
            "active",
        )
        .unwrap();

        let content = std::fs::read_to_string(org.join("gemini").join("node.toml")).unwrap();
        let node: Node = toml::from_str(&content).unwrap();
        assert_eq!(node.name, "Gemini Project");
        assert_eq!(node.description, "A cool project");
    }

    #[test]
    fn create_child_node() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        write_node(&org.join("gemini"), "gemini");
        create_node(&org, "gemini/auth", None, None, None, None, "active").unwrap();

        assert!(org.join("gemini").join("auth").join("node.toml").exists());
        let content =
            std::fs::read_to_string(org.join("gemini").join("auth").join("node.toml")).unwrap();
        let node: Node = toml::from_str(&content).unwrap();
        assert_eq!(node.name, "auth");
    }

    #[test]
    fn create_fails_if_parent_missing() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        // "gemini" directory doesn't have a node.toml
        let result = create_node(&org, "gemini/auth", None, None, None, None, "active");
        assert!(result.is_err());
    }

    #[test]
    fn create_fails_if_already_exists() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        create_node(&org, "gemini", None, None, None, None, "active").unwrap();
        let result = create_node(&org, "gemini", None, None, None, None, "active");
        assert!(result.is_err());
    }

    #[test]
    fn create_with_labels() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        // Define labels first
        let mut lf = LabelsFile::default();
        lf.add("team:alpha".to_string(), "Alpha team".to_string());
        lf.add("priority:high".to_string(), "High priority".to_string());
        lf.write(&org).unwrap();

        create_node(
            &org,
            "gemini",
            None,
            None,
            None,
            Some("team:alpha, priority:high"),
            "active",
        )
        .unwrap();

        let content = std::fs::read_to_string(org.join("gemini").join("node.toml")).unwrap();
        let node: Node = toml::from_str(&content).unwrap();
        assert_eq!(node.labels, vec!["team:alpha", "priority:high"]);
    }

    #[test]
    fn labels_validated_against_labels_toml() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        // Using undefined labels should fail
        let result = create_node(&org, "gemini", None, None, None, Some("bug"), "active");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("undefined label"));
        assert!(err.contains("bug"));

        // Define labels in labels.toml, then create should succeed
        let mut lf = LabelsFile::default();
        lf.add("bug".to_string(), "Something is broken".to_string());
        lf.add("team:alpha".to_string(), "Alpha team".to_string());
        lf.write(&org).unwrap();

        create_node(
            &org,
            "gemini",
            None,
            None,
            None,
            Some("bug, team:alpha"),
            "active",
        )
        .unwrap();

        let content = std::fs::read_to_string(org.join("gemini").join("node.toml")).unwrap();
        let node: Node = toml::from_str(&content).unwrap();
        assert_eq!(node.labels, vec!["bug", "team:alpha"]);
    }

    #[test]
    fn no_labels_skips_validation() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        // No labels.toml and no labels should be fine
        create_node(&org, "gemini", None, None, None, None, "active").unwrap();
        assert!(org.join("gemini").join("node.toml").exists());
    }

    #[test]
    fn wrap_text_fits_in_one_line() {
        assert_eq!(node::wrap_str("short text", 80), vec!["short text"]);
    }

    #[test]
    fn wrap_text_breaks_on_words() {
        let lines = node::wrap_str("hello world foo bar", 11);
        assert_eq!(lines, vec!["hello world", "foo bar"]);
    }

    #[test]
    fn wrap_text_long_word_hard_breaks() {
        let lines = node::wrap_str("abcdefghij", 5);
        assert_eq!(lines, vec!["abcde", "fghij"]);
    }

    #[test]
    fn wrap_text_empty() {
        assert_eq!(node::wrap_str("", 80), vec![""]);
    }

    #[test]
    fn wrap_text_zero_width() {
        assert_eq!(node::wrap_str("hello", 0), vec!["hello"]);
    }

    // -----------------------------------------------------------------------
    // Tree rendering snapshot tests
    // -----------------------------------------------------------------------

    fn node_entry(path: &str, desc: &str, status: NodeStatus) -> NodeEntry {
        NodeEntry {
            path: path.to_string(),
            dir: PathBuf::from(path),
            node: Node {
                name: path.rsplit('/').next().unwrap_or(path).to_string(),
                description: desc.to_string(),
                triage_hint: None,
                status,
                github_issue: None,
                labels: vec![],
                repos: vec![],
                owners: vec![],
                team: None,
                timeline: None,
            },
        }
    }

    #[test]
    fn tree_snapshot_flat_nodes() {
        let entries = vec![
            node_entry("alpha", "First project", NodeStatus::Active),
            node_entry("beta", "Second project", NodeStatus::Completed),
            node_entry("gamma", "", NodeStatus::Paused),
        ];
        let out = render_tree(&entries, 80, false);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn tree_snapshot_nested() {
        let entries = vec![
            node_entry("root", "Top-level initiative", NodeStatus::Active),
            node_entry("root/child-a", "First child", NodeStatus::Active),
            node_entry(
                "root/child-a/grandchild",
                "Deep node",
                NodeStatus::Completed,
            ),
            node_entry("root/child-b", "Second child", NodeStatus::Paused),
        ];
        let out = render_tree(&entries, 80, false);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn tree_snapshot_deep_with_wrapping() {
        let entries = vec![
            node_entry("proj", "A project", NodeStatus::Active),
            node_entry(
                "proj/sub",
                "A sub-project with a very long description that should wrap when the terminal is narrow enough to trigger the wrapping logic",
                NodeStatus::Active,
            ),
            node_entry("proj/sub/leaf", "Leaf node", NodeStatus::Active),
        ];
        let out = render_tree(&entries, 50, false);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn tree_snapshot_prefix_sibling_ordering() {
        let entries = vec![
            node_entry("atlas", "Top", NodeStatus::Active),
            node_entry("atlas/python", "Python side", NodeStatus::Active),
            node_entry("atlas/python/bindings", "Bindings", NodeStatus::Active),
            node_entry("atlas/python/old", "Legacy", NodeStatus::Active),
            node_entry("atlas/python/old/compiler", "Compiler", NodeStatus::Active),
            node_entry("atlas/python/old/ir", "IR core", NodeStatus::Completed),
            node_entry("atlas/rust", "Rust rewrite", NodeStatus::Active),
            node_entry("atlas/rust/interpreter", "Interpreter", NodeStatus::Active),
            node_entry("atlas/rust/vm", "VM", NodeStatus::Active),
            node_entry("atlas/rust-ext", "Extension", NodeStatus::Paused),
        ];
        let out = render_tree(&entries, 80, false);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn tree_snapshot_last_child_no_stray_bars() {
        let entries = vec![
            node_entry("root", "Root", NodeStatus::Active),
            node_entry("root/a", "Child A", NodeStatus::Active),
            node_entry("root/a/deep", "Deep under A", NodeStatus::Active),
        ];
        let out = render_tree(&entries, 80, false);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn tree_snapshot_mixed_statuses() {
        let entries = vec![
            node_entry("proj", "Main project", NodeStatus::Active),
            node_entry("proj/done", "Completed sub", NodeStatus::Completed),
            node_entry("proj/wip", "In progress", NodeStatus::Active),
            node_entry("proj/blocked", "Blocked task", NodeStatus::Paused),
            node_entry("proj/dropped", "Cancelled work", NodeStatus::Cancelled),
        ];
        let out = render_tree(&entries, 80, false);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn tree_snapshot_multiple_roots_with_children() {
        let entries = vec![
            node_entry("alpha", "First root", NodeStatus::Active),
            node_entry("alpha/sub1", "Sub 1", NodeStatus::Active),
            node_entry("alpha/sub2", "Sub 2", NodeStatus::Completed),
            node_entry("beta", "Second root", NodeStatus::Paused),
            node_entry("beta/child", "Beta child", NodeStatus::Active),
        ];
        let out = render_tree(&entries, 80, false);
        insta::assert_snapshot!(out);
    }
}
