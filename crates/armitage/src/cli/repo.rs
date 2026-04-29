use std::collections::BTreeMap;

use armitage_core::tree::{find_org_root, walk_nodes};
use armitage_triage::fetch::strip_repo_qualifier;
use serde::Serialize;

use crate::error::Result;

#[derive(Debug, Serialize)]
pub struct RepoInfo {
    pub repo: String,
    pub visibility: String,
    pub nodes: Vec<String>,
}

/// `armitage repo list [--format table|json]`
///
/// Lists all repos referenced by node.toml files and their GitHub visibility
/// (public / private / unknown). Queries GitHub once per unique repo.
pub fn run_list(format: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let all_nodes = walk_nodes(&org_root)?;

    // Collect unique bare repos and the nodes that reference them.
    let mut repo_nodes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &all_nodes {
        for repo in &entry.node.repos {
            let bare = strip_repo_qualifier(repo);
            repo_nodes.entry(bare).or_default().push(entry.path.clone());
        }
    }

    if repo_nodes.is_empty() {
        println!("No repos referenced by any node.");
        return Ok(());
    }

    let gh = armitage_github::require_gh()?;

    let mut rows: Vec<RepoInfo> = repo_nodes
        .into_iter()
        .map(|(repo, nodes)| {
            let visibility = match armitage_github::issue::fetch_repo_metadata(&gh, &repo) {
                Some(meta) => {
                    if meta.is_private {
                        "private".to_string()
                    } else {
                        "public".to_string()
                    }
                }
                None => "unknown".to_string(),
            };
            RepoInfo {
                repo,
                visibility,
                nodes,
            }
        })
        .collect();

    // Sort: private first, then public, then unknown; alphabetical within each group.
    rows.sort_by(|a, b| {
        let vis_order = |v: &str| match v {
            "private" => 0,
            "public" => 1,
            _ => 2,
        };
        vis_order(&a.visibility)
            .cmp(&vis_order(&b.visibility))
            .then(a.repo.cmp(&b.repo))
    });

    match format.as_str() {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|e| crate::error::Error::Other(e.to_string()))?
        ),
        _ => print_table(&rows),
    }

    Ok(())
}

fn print_table(rows: &[RepoInfo]) {
    let repo_w = rows
        .iter()
        .map(|r| r.repo.len())
        .max()
        .unwrap_or(10)
        .max(10);
    println!(
        "{:<repo_w$}  {:<8}  nodes",
        "repo",
        "visibility",
        repo_w = repo_w
    );
    println!("{}", "-".repeat(repo_w + 2 + 8 + 2 + 20));
    for row in rows {
        let vis_colored = match row.visibility.as_str() {
            "private" => "\x1b[33mprivate\x1b[0m ",
            "public" => "\x1b[32mpublic\x1b[0m  ",
            _ => "\x1b[2munknown\x1b[0m ",
        };
        println!(
            "{:<repo_w$}  {}  {}",
            row.repo,
            vis_colored,
            row.nodes.join(", "),
            repo_w = repo_w
        );
    }
}
