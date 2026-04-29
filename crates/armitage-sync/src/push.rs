use std::path::Path;

use chrono::Utc;

use crate::conflict::has_conflicts;
use crate::error::{Error, Result};
use crate::hash::compute_node_hash;
use crate::state::{NodeSyncEntry, read_sync_state, write_sync_state};
use armitage_core::node::{IssueRef, Node, NodeStatus};
use armitage_core::tree::{NodeEntry, walk_nodes};
use armitage_github::Gh;
use armitage_github::issue::{
    add_sub_issue, fetch_issue, fetch_issue_database_id, list_sub_issue_ids, set_issue_state,
    update_issue,
};

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

    // Pass 1: sync issue content (title, body, labels, state) for changed nodes.
    for entry in &filtered {
        push_node(gh, org_root, entry, dry_run)?;
    }

    if !dry_run {
        // Pass 2: wire sub-issue relationships for every node in scope that has
        // a tracking issue.  This runs unconditionally (not gated on hash change)
        // so that sub-issues are linked even when a parent's issue was created
        // after the children were last pushed.  No-op detection inside
        // `wire_as_parent_sub_issue` keeps this idempotent.
        for entry in &filtered {
            if let Some(ref issue_ref_str) = entry.node.track {
                wire_as_parent_sub_issue(gh, org_root, &entry.path, issue_ref_str);
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// push_node
// ---------------------------------------------------------------------------

fn push_node(gh: &Gh, org_root: &Path, entry: &NodeEntry, dry_run: bool) -> Result<()> {
    // Initiative-level nodes (depth 0, no '/' in path) don't get tracking issues.
    if !entry.path.contains('/') {
        return Ok(());
    }

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
    let issue_md = read_issue_md(&entry.dir);

    let Some(issue_ref_str) = node.track.as_deref() else {
        return Ok(()); // no tracking issue configured for this node
    };

    let issue_ref = IssueRef::parse(issue_ref_str)?;

    // Stale push protection: check remote hasn't changed since last pull
    if let Some(stored_entry) = stored.as_ref() {
        let remote_issue = fetch_issue(gh, &issue_ref)?;
        let remote_updated_at: chrono::DateTime<Utc> = remote_issue
            .updated_at
            .parse()
            .map_err(|_| Error::Other(format!("invalid updatedAt: {}", remote_issue.updated_at)))?;

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

    let (add_labels, remove_labels) = compute_label_diff(
        stored.as_ref().map(|s| s.track.as_str()),
        entry,
        &sync_state,
    );

    update_issue(
        gh,
        &issue_ref,
        Some(&node.name),
        issue_md.as_deref(),
        &add_labels,
        &remove_labels,
    )?;

    let should_be_open = matches!(node.status, NodeStatus::Active | NodeStatus::Paused);
    set_issue_state(gh, &issue_ref, should_be_open)?;

    println!("pushed: {}", entry.path);

    let new_hash = compute_node_hash(&entry.dir)?;
    let new_entry = NodeSyncEntry {
        track: issue_ref_str.to_string(),
        last_pulled_at: stored.as_ref().and_then(|s| s.last_pulled_at),
        last_pushed_at: Some(Utc::now()),
        remote_updated_at: stored.as_ref().and_then(|s| s.remote_updated_at),
        local_hash: Some(new_hash),
    };
    sync_state.nodes.insert(entry.path.clone(), new_entry);

    write_sync_state(org_root, &sync_state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the repo where a tracking issue should be created.
///
/// Prefers `candidate` when it is a private repo (checked via the GitHub API).
/// Falls back to `default_repo` when the candidate is public or unavailable,
/// so internal tracking issues are never created in public repos.
/// After pushing a node, register it as a sub-issue of its parent's tracking
/// issue (if the parent has one).
///
/// Errors are logged as warnings rather than propagated — sub-issue wiring is
/// best-effort and should not fail the push.
fn wire_as_parent_sub_issue(gh: &Gh, org_root: &Path, node_path: &str, issue_ref_str: &str) {
    // Derive parent path: "a/b/c" → "a/b", "a" → no parent
    let parent_path = match node_path.rsplit_once('/') {
        Some((p, _)) => p,
        None => return, // top-level node, no parent
    };

    // Read parent from disk to pick up any github_issue written this run
    let parent_node_toml = org_root.join(parent_path).join("node.toml");
    let content = match std::fs::read_to_string(&parent_node_toml) {
        Ok(c) => c,
        Err(_) => return, // parent directory has no node.toml
    };
    let parent_node: Node = match toml::from_str(&content) {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(
                parent = parent_path,
                "failed to parse parent node.toml: {e}"
            );
            return;
        }
    };

    let parent_issue_str = match parent_node.track {
        Some(ref s) => s.clone(),
        None => return, // parent has no tracking issue
    };

    let parent_ref = match IssueRef::parse(&parent_issue_str) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(parent = parent_path, "invalid parent track: {e}");
            return;
        }
    };

    let child_ref = match IssueRef::parse(issue_ref_str) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(node = node_path, "invalid child track: {e}");
            return;
        }
    };

    // Fetch child database ID (needed by the sub-issues API)
    let child_db_id = match fetch_issue_database_id(gh, &child_ref) {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(node = node_path, "failed to fetch issue database id: {e}");
            return;
        }
    };

    // No-op: check if already a sub-issue
    let existing = match list_sub_issue_ids(gh, &parent_ref) {
        Ok(ids) => ids,
        Err(e) => {
            tracing::warn!(parent = parent_path, "failed to list sub-issues: {e}");
            return;
        }
    };

    if existing.contains(&child_db_id) {
        tracing::debug!(
            node = node_path,
            parent = parent_path,
            "already a sub-issue, skipping"
        );
        return;
    }

    match add_sub_issue(gh, &parent_ref, child_db_id) {
        Ok(()) => {
            println!("  sub-issue: {} → {}", issue_ref_str, parent_issue_str);
        }
        Err(e) => {
            tracing::warn!(
                node = node_path,
                parent = parent_path,
                "failed to add sub-issue: {e}"
            );
        }
    }
}

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
