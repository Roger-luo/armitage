use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::Result;

/// Compute a SHA-256 hash of a node directory.
///
/// Hashes `node.toml` and `issue.md` (if it exists), in that order,
/// feeding each file's path (relative) and content into the digest.
/// Returns the hex-encoded digest string.
pub fn compute_node_hash(node_dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    // Always hash node.toml
    let node_toml = node_dir.join("node.toml");
    let toml_bytes = std::fs::read(&node_toml)?;
    hasher.update(b"node.toml\0");
    hasher.update(&toml_bytes);

    // Hash issue.md if present
    let issue_md = node_dir.join("issue.md");
    if issue_md.exists() {
        let md_bytes = std::fs::read(&issue_md)?;
        hasher.update(b"issue.md\0");
        hasher.update(&md_bytes);
    }

    let result = hasher.finalize();
    Ok(hex::encode(result))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_node_toml(dir: &Path, content: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("node.toml"), content).unwrap();
    }

    #[test]
    fn hash_stable_without_issue_md() {
        let tmp = TempDir::new().unwrap();
        let node_dir = tmp.path().join("mynode");
        write_node_toml(&node_dir, "name = \"mynode\"\ndescription = \"test\"\n");

        let h1 = compute_node_hash(&node_dir).unwrap();
        let h2 = compute_node_hash(&node_dir).unwrap();

        assert_eq!(h1, h2, "hash should be stable");
        assert!(!h1.is_empty());
        // SHA-256 hex is 64 chars
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn hash_changes_when_node_toml_changes() {
        let tmp = TempDir::new().unwrap();
        let node_dir = tmp.path().join("mynode");
        write_node_toml(&node_dir, "name = \"mynode\"\ndescription = \"original\"\n");

        let h1 = compute_node_hash(&node_dir).unwrap();

        // Modify node.toml
        std::fs::write(
            node_dir.join("node.toml"),
            "name = \"mynode\"\ndescription = \"changed\"\n",
        )
        .unwrap();
        let h2 = compute_node_hash(&node_dir).unwrap();

        assert_ne!(h1, h2, "hash should change when node.toml changes");
    }

    #[test]
    fn hash_changes_when_issue_md_changes() {
        let tmp = TempDir::new().unwrap();
        let node_dir = tmp.path().join("mynode");
        write_node_toml(&node_dir, "name = \"mynode\"\ndescription = \"test\"\n");

        let h1 = compute_node_hash(&node_dir).unwrap();

        // Add issue.md
        std::fs::write(node_dir.join("issue.md"), "# My Issue\n\nSome content.\n").unwrap();
        let h2 = compute_node_hash(&node_dir).unwrap();

        assert_ne!(h1, h2, "hash should change when issue.md is added");

        // Modify issue.md
        std::fs::write(
            node_dir.join("issue.md"),
            "# My Issue\n\nDifferent content.\n",
        )
        .unwrap();
        let h3 = compute_node_hash(&node_dir).unwrap();

        assert_ne!(h2, h3, "hash should change when issue.md content changes");
    }
}
