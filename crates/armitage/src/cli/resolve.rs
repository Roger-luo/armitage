use std::io::{self, BufRead, Write};

use crate::error::Result;
use armitage_core::node::Node;
use armitage_core::tree::{find_org_root, read_node};
use armitage_labels::rename::{read_rename_ledger, translate_labels};
use armitage_sync::conflict::{StoredConflict, list_conflicts, remove_conflict};

pub fn run(path: Option<String>, list: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;

    if list || path.is_none() {
        // List all conflicts
        let conflicts = list_conflicts(&org_root)?;
        if conflicts.is_empty() {
            println!("No unresolved conflicts.");
        } else {
            println!("Unresolved conflicts:");
            for c in &conflicts {
                print!("  {}", c.node_path);
                if !c.field_conflicts.is_empty() {
                    let fields: Vec<&str> =
                        c.field_conflicts.iter().map(|f| f.field.as_str()).collect();
                    print!(" — fields: {}", fields.join(", "));
                }
                if c.body_conflict.is_some() {
                    print!(" — body conflict");
                }
                println!();
            }
        }
        return Ok(());
    }

    let node_path = path.as_deref().unwrap();

    // Find the conflict for this path
    let conflicts = list_conflicts(&org_root)?;
    let conflict = conflicts
        .into_iter()
        .find(|c| c.node_path == node_path)
        .ok_or_else(|| armitage_core::error::Error::NodeNotFound(node_path.to_string()))?;

    resolve_conflict_interactive(&org_root, node_path, &conflict)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive resolution
// ---------------------------------------------------------------------------

fn resolve_conflict_interactive(
    org_root: &std::path::Path,
    node_path: &str,
    conflict: &StoredConflict,
) -> Result<()> {
    println!("Resolving conflicts for: {node_path}");
    println!();

    // Read the current node
    let entry = read_node(org_root, node_path)?;
    let mut node = entry.node.clone();

    // Resolve field conflicts
    for fc in &conflict.field_conflicts {
        println!("Field: {}", fc.field);
        println!("  [L] local:  {}", fc.local_value);
        println!("  [R] remote: {}", fc.remote_value);

        let choice = prompt_lr("Choose [L]ocal or [R]emote: ")?;
        if choice == 'R' || choice == 'r' {
            apply_field_to_node(&mut node, &fc.field, &fc.remote_value);
        }
        // 'L' keeps local (already in node.toml)
        println!();
    }

    // Translate labels through rename ledger before writing
    if !conflict.field_conflicts.is_empty() {
        let ledger = read_rename_ledger(org_root)?;
        node.labels = translate_labels(&node.labels, &ledger);
        let content = toml::to_string(&node)?;
        std::fs::write(entry.dir.join("node.toml"), content)?;
    }

    // Resolve body conflict
    if let Some(ref body_conflict) = conflict.body_conflict {
        let local_lines = body_conflict.local.lines().count();
        let remote_lines = body_conflict.remote.lines().count();
        println!("Body conflict:");
        println!("  [L] local:  {} lines", local_lines);
        println!("  [R] remote: {} lines", remote_lines);

        let choice = prompt_lr("Choose [L]ocal or [R]emote: ")?;
        let chosen_body = if choice == 'R' || choice == 'r' {
            &body_conflict.remote
        } else {
            &body_conflict.local
        };
        std::fs::write(entry.dir.join("issue.md"), chosen_body)?;
        println!();
    }

    // Remove conflict file
    remove_conflict(org_root, node_path)?;
    println!("Conflict resolved for: {node_path}");

    Ok(())
}

fn prompt_lr(message: &str) -> Result<char> {
    let stdin = io::stdin();
    loop {
        print!("{message}");
        io::stdout().flush()?;
        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("L") {
            return Ok('L');
        }
        if trimmed.eq_ignore_ascii_case("R") {
            return Ok('R');
        }
        println!("Please enter 'L' or 'R'.");
    }
}

fn apply_field_to_node(node: &mut Node, field: &str, value: &str) {
    match field {
        "name" => node.name = value.to_string(),
        "description" => node.description = value.to_string(),
        "github_issue" => {
            node.github_issue = if value == "(none)" {
                None
            } else {
                Some(value.to_string())
            }
        }
        "status" => {
            use armitage_core::node::NodeStatus;
            node.status = match value {
                "active" => NodeStatus::Active,
                "completed" => NodeStatus::Completed,
                "paused" => NodeStatus::Paused,
                "cancelled" => NodeStatus::Cancelled,
                _ => NodeStatus::Active,
            };
        }
        "labels" => {
            let raw: Vec<String> = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            // apply_field_to_node doesn't have access to org_root, so store raw;
            // translation happens in resolve_conflict_interactive after merge.
            node.labels = raw;
        }
        "repos" => {
            node.repos = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        "timeline" => {
            // Format: "YYYY-MM-DD — YYYY-MM-DD"
            if value == "(none)" {
                node.timeline = None;
            } else if let Some((start_str, end_str)) = value.split_once(" — ") {
                use armitage_core::node::Timeline;
                use chrono::NaiveDate;
                if let (Ok(start), Ok(end)) = (
                    NaiveDate::parse_from_str(start_str.trim(), "%Y-%m-%d"),
                    NaiveDate::parse_from_str(end_str.trim(), "%Y-%m-%d"),
                ) {
                    node.timeline = Some(Timeline { start, end });
                }
            }
        }
        _ => {} // Unknown field — ignore
    }
}
