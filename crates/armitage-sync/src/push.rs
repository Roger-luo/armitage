use std::path::Path;

use chrono::Utc;

use crate::conflict::has_conflicts;
use crate::error::{Error, Result};
use crate::hash::compute_node_hash;
use crate::state::{NodeSyncEntry, read_sync_state, write_sync_state};
use armitage_core::node::{IssueRef, NodeStatus};
use armitage_core::tree::{NodeEntry, walk_nodes};
use armitage_github::Gh;
use armitage_github::issue::{create_issue, fetch_issue, set_issue_state, update_issue};

// ---------------------------------------------------------------------------
// push_all
// ---------------------------------------------------------------------------

pub fn push_all(gh: &Gh, org_root: &Path, scope: Option<&str>, dry_run: bool) -> Result<()> {
    // 1. Check for unresolved conflicts
    if has_conflicts(org_root)? {
        return Err(Error::UnresolvedConflicts);
    }

    let nodes = walk_nodes(org_root)?;
    tracing::debug!(
        total_nodes = nodes.len(),
        scope = scope,
        dry_run = dry_run,
        "push_all"
    );
    let filtered = filter_by_scope(nodes, scope);

    for entry in &filtered {
        push_node(gh, org_root, entry, dry_run)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// push_node
// ---------------------------------------------------------------------------

fn push_node(gh: &Gh, org_root: &Path, entry: &NodeEntry, dry_run: bool) -> Result<()> {
    let mut sync_state = read_sync_state(org_root)?;
    let stored = sync_state.nodes.get(&entry.path).cloned();

    // Compute current local hash
    let current_hash = compute_node_hash(&entry.dir)?;

    // Check if local changed
    let local_changed = stored
        .as_ref()
        .and_then(|s| s.local_hash.as_deref())
        .is_none_or(|stored_hash| stored_hash != current_hash);

    if !local_changed {
        return Ok(());
    }

    let node = &entry.node;
    let issue_md_body = read_issue_md(&entry.dir).unwrap_or_default();

    if let Some(issue_ref_str) = node.github_issue.as_deref() {
        // --- Existing issue: update ---
        let issue_ref = IssueRef::parse(issue_ref_str)?;

        // Stale push protection: check remote hasn't changed since last pull
        if let Some(stored_entry) = stored.as_ref() {
            let remote_issue = fetch_issue(gh, &issue_ref)?;
            let remote_updated_at: chrono::DateTime<Utc> =
                remote_issue.updated_at.parse().map_err(|_| {
                    Error::Other(format!("invalid updatedAt: {}", remote_issue.updated_at))
                })?;

            if let Some(last_known_remote) = stored_entry.remote_updated_at
                && remote_updated_at > last_known_remote
            {
                return Err(Error::StalePush);
            }
        }

        if dry_run {
            println!("would push: {}", entry.path);
            return Ok(());
        }

        // Compute label diff against stored labels
        let (add_labels, remove_labels) = compute_label_diff(
            stored.as_ref().map(|s| s.github_issue.as_str()),
            entry,
            &sync_state,
        );

        update_issue(
            gh,
            &issue_ref,
            Some(&node.name),
            Some(&issue_md_body),
            &add_labels,
            &remove_labels,
        )?;

        // Update state: closed/open based on status
        let should_be_open = matches!(node.status, NodeStatus::Active | NodeStatus::Paused);
        set_issue_state(gh, &issue_ref, should_be_open)?;

        println!("pushed: {}", entry.path);

        // Update sync state
        let new_hash = compute_node_hash(&entry.dir)?;
        let new_entry = NodeSyncEntry {
            github_issue: issue_ref_str.to_string(),
            last_pulled_at: stored.as_ref().and_then(|s| s.last_pulled_at),
            last_pushed_at: Some(Utc::now()),
            remote_updated_at: stored.as_ref().and_then(|s| s.remote_updated_at),
            local_hash: Some(new_hash),
        };
        sync_state.nodes.insert(entry.path.clone(), new_entry);
    } else {
        // --- No github_issue: create new issue ---
        // We need a repo to create the issue in. Use the first repo in node.repos,
        // or fall back to parsing the org config for a default.
        let repo = match node.repos.first() {
            Some(r) => r.clone(),
            None => {
                // Can't create an issue without knowing which repo
                // Skip silently if no repo is configured
                return Ok(());
            }
        };

        if dry_run {
            println!("would create issue: {} in {}", entry.path, repo);
            return Ok(());
        }

        let created = create_issue(gh, &repo, &node.name, &issue_md_body, &node.labels)?;

        // Extract owner from repo string "owner/repo"
        let issue_ref_str = format!("{}#{}", repo, created.number);
        println!("created issue: {} → {}", entry.path, issue_ref_str);

        // Write github_issue back into node.toml
        let mut updated_node = node.clone();
        updated_node.github_issue = Some(issue_ref_str.clone());
        let content = toml::to_string(&updated_node)?;
        std::fs::write(entry.dir.join("node.toml"), &content)?;

        // Update sync state
        let new_hash = compute_node_hash(&entry.dir)?;
        let new_entry = NodeSyncEntry {
            github_issue: issue_ref_str,
            last_pulled_at: None,
            last_pushed_at: Some(Utc::now()),
            remote_updated_at: None,
            local_hash: Some(new_hash),
        };
        sync_state.nodes.insert(entry.path.clone(), new_entry);
    }

    write_sync_state(org_root, &sync_state)?;
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

/// Compute which labels to add and remove compared to the last pushed state.
/// Since we don't store the previous label set, we fall back to: add all current labels.
fn compute_label_diff(
    _stored_issue: Option<&str>,
    entry: &NodeEntry,
    _sync_state: &crate::state::SyncState,
) -> (Vec<String>, Vec<String>) {
    // Simple approach: push all current labels as "add", no removals.
    // A more sophisticated implementation would compare with the remote labels,
    // but since update_issue uses --add-label / --remove-label, we just send
    // the current desired set as adds and don't remove anything here.
    // The body and title update handles the rest.
    let add_labels = entry.node.labels.clone();
    let remove_labels = vec![];
    (add_labels, remove_labels)
}
