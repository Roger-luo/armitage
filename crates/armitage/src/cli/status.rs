use crate::error::Result;
use armitage_core::org::Org;
use armitage_core::tree::{find_org_root, walk_nodes};
use armitage_sync::conflict::list_conflicts;
use armitage_sync::hash::compute_node_hash;
use armitage_sync::state::read_sync_state;

// ---------------------------------------------------------------------------
// Timeline violation detection
// ---------------------------------------------------------------------------

fn find_timeline_violations(nodes: &[armitage_core::tree::NodeEntry]) -> Vec<(&str, &str)> {
    let mut violations = Vec::new();

    for child in nodes {
        let Some(child_tl) = child.node.timeline.as_ref() else {
            continue;
        };

        // Find the parent path (strip the last component)
        let parent_path = match child.path.rfind('/') {
            Some(idx) => &child.path[..idx],
            None => continue, // top-level node has no parent
        };

        // Find the parent node
        if let Some(parent) = nodes.iter().find(|n| n.path == parent_path)
            && let Some(parent_tl) = parent.node.timeline.as_ref()
            && !parent_tl.contains(child_tl)
        {
            violations.push((parent.path.as_str(), child.path.as_str()));
        }
    }

    violations
}

pub fn run() -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let org = Org::open(&org_root)?;
    let nodes = walk_nodes(&org_root)?;
    let sync_state = read_sync_state(&org_root)?;
    let conflicts = list_conflicts(&org_root)?;

    // --- Org info ---
    println!("Org:         {}", org.info().name);
    if org.info().github_orgs.is_empty() {
        println!("GitHub orgs: (none)");
    } else {
        println!("GitHub orgs: {}", org.info().github_orgs.join(", "));
    }
    println!();

    // --- Node counts ---
    let total = nodes.len();
    let linked: Vec<_> = nodes.iter().filter(|n| n.node.track.is_some()).collect();
    let local_only = total - linked.len();

    println!(
        "Nodes:      {} total ({} linked to GitHub, {} local-only)",
        total,
        linked.len(),
        local_only
    );
    println!();

    // --- Modified nodes (local hash differs from stored) ---
    let mut modified = Vec::new();
    for entry in &nodes {
        if let Some(stored) = sync_state.nodes.get(&entry.path)
            && let Some(stored_hash) = stored.local_hash.as_deref()
            && let Ok(current_hash) = compute_node_hash(&entry.dir)
            && current_hash != stored_hash
        {
            modified.push(entry.path.as_str());
        }
    }

    if !modified.is_empty() {
        println!("Modified (local changes not pushed):");
        for path in &modified {
            println!("  M  {path}");
        }
        println!();
    }

    // --- New/unlinked nodes (no entry in sync state) ---
    let new_nodes: Vec<_> = nodes
        .iter()
        .filter(|e| !sync_state.nodes.contains_key(&e.path) && e.node.track.is_none())
        .collect();

    if !new_nodes.is_empty() {
        println!("New / unlinked nodes (not synced to GitHub):");
        for entry in &new_nodes {
            println!("  ?  {}", entry.path);
        }
        println!();
    }

    // --- Unresolved conflicts ---
    if !conflicts.is_empty() {
        println!("Unresolved conflicts ({}):", conflicts.len());
        for c in &conflicts {
            println!("  !  {}", c.node_path);
        }
        println!();
    }

    // --- Timeline violations ---
    let violations = find_timeline_violations(&nodes);
    if !violations.is_empty() {
        println!("Timeline violations (child timeline exceeds parent):");
        for (parent, child) in &violations {
            println!("  T  {child} exceeds {parent}");
        }
        println!();
    }

    // Summary line
    if modified.is_empty() && new_nodes.is_empty() && conflicts.is_empty() && violations.is_empty()
    {
        println!("Everything is in sync.");
    }

    Ok(())
}
