use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::node::Node;

/// A node discovered on the filesystem.
#[derive(Debug, Clone)]
pub struct NodeEntry {
    /// Relative path from org root (e.g. "gemini/auth-service").
    pub path: String,
    /// Absolute path to the node directory.
    pub dir: PathBuf,
    /// Parsed node.toml contents.
    pub node: Node,
}

/// Walk up from `start` looking for `armitage.toml`. Returns the org root directory.
pub fn find_org_root(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    let mut current = start.as_path();
    loop {
        if current.join("armitage.toml").exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Err(Error::NotInOrg),
        }
    }
}

/// Recursively walk the org directory and return all nodes (dirs with `node.toml`).
/// Sorted by path. Skips directories starting with `.`
pub fn walk_nodes(org_root: &Path) -> Result<Vec<NodeEntry>> {
    let mut entries = Vec::new();
    collect_nodes(org_root, org_root, &mut entries)?;
    // Sort by path components so children always immediately follow their
    // parent.  Plain string sort fails when a directory name is a prefix of a
    // sibling (e.g. "rust" vs "rust-python-integration") because `/` (0x2F) >
    // `-` (0x2D) in ASCII, pushing children after the sibling.
    entries.sort_by(|a, b| {
        let a_parts: Vec<&str> = a.path.split('/').collect();
        let b_parts: Vec<&str> = b.path.split('/').collect();
        a_parts.cmp(&b_parts)
    });
    Ok(entries)
}

fn collect_nodes(org_root: &Path, dir: &Path, entries: &mut Vec<NodeEntry>) -> Result<()> {
    let read_dir = fs::read_dir(dir)?;
    for entry in read_dir {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let child_dir = entry.path();
        let node_toml_path = child_dir.join("node.toml");
        if node_toml_path.exists() {
            let rel = child_dir
                .strip_prefix(org_root)
                .expect("child_dir is always under org_root")
                .to_string_lossy()
                .replace('\\', "/");
            let node = parse_node_toml(&node_toml_path)?;
            entries.push(NodeEntry {
                path: rel,
                dir: child_dir.clone(),
                node,
            });
        }
        // Recurse regardless of whether this dir has a node.toml
        collect_nodes(org_root, &child_dir, entries)?;
    }
    Ok(())
}

/// List direct children of a node (or top-level nodes if `parent_path` is empty).
pub fn list_children(org_root: &Path, parent_path: &str) -> Result<Vec<NodeEntry>> {
    let all = walk_nodes(org_root)?;
    let filtered = all
        .into_iter()
        .filter(|e| {
            if parent_path.is_empty() {
                // Top-level: path has no '/' separator
                !e.path.contains('/')
            } else {
                // Direct child: path starts with "parent_path/" and has no further '/'
                if let Some(rest) = e.path.strip_prefix(&format!("{parent_path}/")) {
                    !rest.contains('/')
                } else {
                    false
                }
            }
        })
        .collect();
    Ok(filtered)
}

/// Read a single node at a specific path relative to the org root.
pub fn read_node(org_root: &Path, node_path: &str) -> Result<NodeEntry> {
    let dir = org_root.join(node_path);
    let node_toml_path = dir.join("node.toml");
    if !node_toml_path.exists() {
        return Err(Error::NodeNotFound(node_path.to_string()));
    }
    let node = parse_node_toml(&node_toml_path)?;
    Ok(NodeEntry {
        path: node_path.to_string(),
        dir,
        node,
    })
}

fn parse_node_toml(path: &Path) -> Result<Node> {
    let content = fs::read_to_string(path)?;
    toml::from_str(&content).map_err(|source| Error::TomlParse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_node_toml(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let content = format!("name = \"{name}\"\ndescription = \"test node\"\n");
        std::fs::write(dir.join("node.toml"), content).unwrap();
    }

    fn write_armitage_toml(dir: &Path) {
        let content = "[org]\nname = \"test\"\ngithub_orgs = [\"test\"]\n";
        std::fs::write(dir.join("armitage.toml"), content).unwrap();
    }

    #[test]
    fn find_org_root_from_subdir() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        let nested = root.join("foo").join("bar");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_org_root(&nested).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn find_org_root_at_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        let found = find_org_root(root).unwrap();
        assert_eq!(found, root.canonicalize().unwrap());
    }

    #[test]
    fn find_org_root_not_found() {
        let tmp = TempDir::new().unwrap();
        // No armitage.toml anywhere
        let result = find_org_root(tmp.path());
        assert!(matches!(result, Err(Error::NotInOrg)));
    }

    #[test]
    fn walk_nodes_finds_all_nodes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        write_node_toml(&root.join("alpha"), "alpha");
        write_node_toml(&root.join("gemini").join("auth-service"), "auth-service");
        write_node_toml(&root.join("gemini").join("data-pipeline"), "data-pipeline");

        let nodes = walk_nodes(root).unwrap();
        let paths: Vec<&str> = nodes.iter().map(|n| n.path.as_str()).collect();
        assert!(paths.contains(&"alpha"));
        assert!(paths.contains(&"gemini/auth-service"));
        assert!(paths.contains(&"gemini/data-pipeline"));
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn walk_nodes_skips_dot_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        write_node_toml(&root.join("visible"), "visible");
        // Place a node.toml inside a dot-dir -- should be skipped
        write_node_toml(&root.join(".armitage").join("hidden"), "hidden");

        let nodes = walk_nodes(root).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].path, "visible");
    }

    #[test]
    fn walk_nodes_skips_dirs_without_node_toml() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        // A directory without node.toml
        std::fs::create_dir_all(root.join("not-a-node")).unwrap();
        write_node_toml(&root.join("real-node"), "real-node");

        let nodes = walk_nodes(root).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].path, "real-node");
    }

    #[test]
    fn list_children_returns_direct_children() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        write_node_toml(&root.join("gemini"), "gemini");
        write_node_toml(&root.join("gemini").join("auth"), "auth");
        write_node_toml(&root.join("gemini").join("auth").join("subauth"), "subauth");

        let children = list_children(root, "gemini").unwrap();
        let paths: Vec<&str> = children.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, vec!["gemini/auth"]);
    }

    #[test]
    fn list_top_level_nodes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        write_node_toml(&root.join("alpha"), "alpha");
        write_node_toml(&root.join("beta"), "beta");
        write_node_toml(&root.join("alpha").join("child"), "child");

        let top = list_children(root, "").unwrap();
        let paths: Vec<&str> = top.iter().map(|n| n.path.as_str()).collect();
        assert!(paths.contains(&"alpha"));
        assert!(paths.contains(&"beta"));
        assert!(!paths.contains(&"alpha/child"));
        assert_eq!(top.len(), 2);
    }

    #[test]
    fn read_node_at_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);
        write_node_toml(&root.join("gemini").join("auth"), "auth");

        let entry = read_node(root, "gemini/auth").unwrap();
        assert_eq!(entry.path, "gemini/auth");
        assert_eq!(entry.node.name, "auth");
        assert_eq!(entry.node.description, "test node");
    }

    #[test]
    fn read_node_not_found() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write_armitage_toml(root);

        let result = read_node(root, "does/not/exist");
        assert!(matches!(result, Err(Error::NodeNotFound(_))));
    }
}
