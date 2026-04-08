use crate::error::Result;
use armitage_core::tree::find_org_root;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;
    armitage_sync::pull::pull_all(&gh, &org_root, path.as_deref(), dry_run)?;

    // After pull, translate labels using the rename ledger
    if !dry_run {
        let ledger = armitage_labels::rename::read_rename_ledger(&org_root)?;
        if !ledger.renames.is_empty() {
            let nodes = armitage_core::tree::walk_nodes(&org_root)?;
            for entry in &nodes {
                let translated =
                    armitage_labels::rename::translate_labels(&entry.node.labels, &ledger);
                if translated != entry.node.labels {
                    let mut node = entry.node.clone();
                    node.labels = translated;
                    let content = toml::to_string(&node)?;
                    std::fs::write(entry.dir.join("node.toml"), content)?;
                }
            }
        }
    }
    Ok(())
}
