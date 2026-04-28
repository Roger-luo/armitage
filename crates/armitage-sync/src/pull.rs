use std::path::Path;

use chrono::{DateTime, Utc};

use crate::conflict::write_conflict;
use crate::error::{Error, Result};
use crate::hash::compute_node_hash;
use crate::merge::{BodyMergeResult, MergeResult, merge_issue_body, merge_nodes};
use crate::state::{NodeSyncEntry, read_sync_state, write_sync_state};
use armitage_core::node::{IssueRef, Node, NodeStatus};
use armitage_core::tree::{NodeEntry, walk_nodes};
use armitage_github::Gh;
use armitage_github::issue::{GitHubIssue, fetch_issue};

// ---------------------------------------------------------------------------
// Result type for a single node pull
// ---------------------------------------------------------------------------

pub enum PullNodeResult {
    /// No track or no remote changes — nothing to do.
    Skipped,
    /// Only remote changed — overwrite local with remote data.
    FastForward,
    /// Both changed — merged cleanly.
    Merged,
    /// Conflicts written to .armitage/sync/conflicts/.
    Conflicted,
}

// ---------------------------------------------------------------------------
// apply_remote_to_local
// ---------------------------------------------------------------------------

/// Write remote GitHub issue data into the node directory:
/// - Update node.toml fields (name from title, labels, status from state)
/// - Write issue.md from body
fn apply_remote_to_local(entry: &NodeEntry, issue: &GitHubIssue) -> Result<()> {
    // Build the updated node
    let mut node = entry.node.clone();
    node.name.clone_from(&issue.title);
    node.labels = issue.labels.iter().map(|l| l.name.clone()).collect();
    node.status = if issue.state.to_uppercase() == "OPEN" {
        NodeStatus::Active
    } else {
        NodeStatus::Completed
    };

    // Write node.toml
    let node_toml_path = entry.dir.join("node.toml");
    let content = node.to_toml()?;
    std::fs::write(&node_toml_path, content)?;

    // Write issue.md
    let issue_md_path = entry.dir.join("issue.md");
    std::fs::write(&issue_md_path, &issue.body)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// pull_node
// ---------------------------------------------------------------------------

pub fn pull_node(gh: &Gh, org_root: &Path, entry: &NodeEntry) -> Result<PullNodeResult> {
    // 1. Skip if no track
    let Some(issue_ref_str) = entry.node.track.as_deref() else {
        return Ok(PullNodeResult::Skipped);
    };
    let issue_ref = IssueRef::parse(issue_ref_str)?;

    // 2. Fetch issue from GitHub
    let remote_issue = fetch_issue(gh, &issue_ref)?;

    // Parse remote updatedAt
    let remote_updated_at: DateTime<Utc> = remote_issue
        .updated_at
        .parse()
        .map_err(|_| Error::Other(format!("invalid updatedAt: {}", remote_issue.updated_at)))?;

    // 3. Load sync state
    let mut sync_state = read_sync_state(org_root)?;
    let stored = sync_state.nodes.get(&entry.path).cloned();

    // 4. Check if remote changed (compare stored remote_updated_at with GitHub's updatedAt)
    let remote_changed = stored
        .as_ref()
        .and_then(|s| s.remote_updated_at)
        .is_none_or(|stored_remote_ts| remote_updated_at > stored_remote_ts);

    // 5. Check if local changed (compare stored local_hash with current hash)
    let current_hash = compute_node_hash(&entry.dir)?;
    let local_changed = stored
        .as_ref()
        .and_then(|s| s.local_hash.as_deref())
        .is_none_or(|stored_hash| stored_hash != current_hash);

    tracing::debug!(
        node = %entry.path,
        remote_changed = remote_changed,
        local_changed = local_changed,
        "pull_node sync check"
    );

    if !remote_changed {
        return Ok(PullNodeResult::Skipped);
    }

    let result = if local_changed {
        // 6b. Both changed: three-way merge
        let local_node = &entry.node;
        let remote_node = Node {
            name: remote_issue.title.clone(),
            description: local_node.description.clone(), // not in GitHub issue
            triage_hint: local_node.triage_hint.clone(), // not in GitHub issue
            track: local_node.track.clone(),             // keep local ref
            labels: remote_issue
                .labels
                .iter()
                .map(|l| l.name.clone())
                .collect::<Vec<_>>(),
            repos: local_node.repos.clone(),   // not in GitHub issue
            owners: local_node.owners.clone(), // not in GitHub issue
            team: local_node.team.clone(),     // not in GitHub issue
            timeline: local_node.timeline.clone(), // not in GitHub issue
            status: if remote_issue.state.to_uppercase() == "OPEN" {
                NodeStatus::Active
            } else {
                NodeStatus::Completed
            },
        };
        // Use local node as base (so only remote's divergences create conflicts)
        let base_node = local_node.clone();

        let merge_result = merge_nodes(&base_node, local_node, &remote_node);

        // Issue body merge
        let local_body = read_issue_md(&entry.dir).unwrap_or_default();
        let remote_body = &remote_issue.body;
        // Base body is empty string if we have no stored base
        let base_body = ""; // We don't have a stored base body; treat as empty
        let body_merge = merge_issue_body(base_body, &local_body, remote_body);

        match merge_result {
            MergeResult::Clean(merged_node) => {
                // Write merged node.toml
                let content = merged_node.to_toml()?;
                std::fs::write(entry.dir.join("node.toml"), content)?;

                match body_merge {
                    BodyMergeResult::Clean(body) => {
                        std::fs::write(entry.dir.join("issue.md"), body)?;
                        PullNodeResult::Merged
                    }
                    BodyMergeResult::Conflict { local, remote } => {
                        write_conflict(org_root, &entry.path, &[], Some((&local, &remote)))?;
                        PullNodeResult::Conflicted
                    }
                }
            }
            MergeResult::Conflict {
                merged: merged_node,
                conflicts,
            } => {
                // Write provisional merged node.toml
                let content = merged_node.to_toml()?;
                std::fs::write(entry.dir.join("node.toml"), content)?;

                let body_conflict_pair = match &body_merge {
                    BodyMergeResult::Conflict { local, remote } => {
                        Some((local.as_str(), remote.as_str()))
                    }
                    BodyMergeResult::Clean(body) => {
                        std::fs::write(entry.dir.join("issue.md"), body)?;
                        None
                    }
                };

                write_conflict(org_root, &entry.path, &conflicts, body_conflict_pair)?;
                PullNodeResult::Conflicted
            }
        }
    } else {
        // 6a. Only remote changed: fast-forward
        apply_remote_to_local(entry, &remote_issue)?;
        PullNodeResult::FastForward
    };

    // 7. Update sync state
    let new_hash = compute_node_hash(&entry.dir)?;
    let entry_state = NodeSyncEntry {
        track: issue_ref_str.to_string(),
        last_pulled_at: Some(Utc::now()),
        last_pushed_at: stored.as_ref().and_then(|s| s.last_pushed_at),
        remote_updated_at: Some(remote_updated_at),
        local_hash: Some(new_hash),
    };
    sync_state.nodes.insert(entry.path.clone(), entry_state);
    write_sync_state(org_root, &sync_state)?;

    Ok(result)
}

// ---------------------------------------------------------------------------
// pull_all
// ---------------------------------------------------------------------------

pub fn pull_all(gh: &Gh, org_root: &Path, scope: Option<&str>, dry_run: bool) -> Result<()> {
    let nodes = walk_nodes(org_root)?;
    tracing::debug!(
        total_nodes = nodes.len(),
        scope = scope,
        dry_run = dry_run,
        "pull_all"
    );
    let filtered = filter_by_scope(nodes, scope);

    for entry in &filtered {
        if dry_run && entry.node.track.is_some() {
            println!("would pull: {}", entry.path);
            continue;
        } else if dry_run {
            continue;
        }

        match pull_node(gh, org_root, entry)? {
            PullNodeResult::Skipped => {
                // silent
            }
            PullNodeResult::FastForward => {
                println!("fast-forward: {}", entry.path);
            }
            PullNodeResult::Merged => {
                println!("merged: {}", entry.path);
            }
            PullNodeResult::Conflicted => {
                println!(
                    "conflicted: {} — run `armitage resolve {}`",
                    entry.path, entry.path
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn filter_by_scope(nodes: Vec<NodeEntry>, scope: Option<&str>) -> Vec<NodeEntry> {
    match scope {
        None => nodes,
        Some(prefix) => nodes
            .into_iter()
            .filter(|e| e.path == prefix || e.path.starts_with(&format!("{prefix}/")))
            .collect(),
    }
}

fn read_issue_md(node_dir: &Path) -> Option<String> {
    let path = node_dir.join("issue.md");
    std::fs::read_to_string(path).ok()
}
