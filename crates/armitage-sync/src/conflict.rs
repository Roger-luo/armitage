use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::merge::FieldConflict;

// ---------------------------------------------------------------------------
// Stored conflict types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConflict {
    pub node_path: String,
    pub field_conflicts: Vec<StoredFieldConflict>,
    pub body_conflict: Option<BodyConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFieldConflict {
    pub field: String,
    pub local_value: String,
    pub remote_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyConflict {
    pub local: String,
    pub remote: String,
}

// ---------------------------------------------------------------------------
// Filename encoding
// ---------------------------------------------------------------------------

fn conflict_filename(node_path: &str) -> String {
    format!("{}.toml", node_path.replace('/', "--"))
}

fn conflicts_dir(org_root: &Path) -> std::path::PathBuf {
    org_root.join(".armitage").join("sync").join("conflicts")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn write_conflict(
    org_root: &Path,
    node_path: &str,
    field_conflicts: &[FieldConflict],
    body_conflict: Option<(&str, &str)>,
) -> Result<()> {
    let stored = StoredConflict {
        node_path: node_path.to_string(),
        field_conflicts: field_conflicts
            .iter()
            .map(|fc| StoredFieldConflict {
                field: fc.field.clone(),
                local_value: fc.local_value.clone(),
                remote_value: fc.remote_value.clone(),
            })
            .collect(),
        body_conflict: body_conflict.map(|(local, remote)| BodyConflict {
            local: local.to_string(),
            remote: remote.to_string(),
        }),
    };

    let dir = conflicts_dir(org_root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(conflict_filename(node_path));
    let content = toml::to_string(&stored)?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn list_conflicts(org_root: &Path) -> Result<Vec<StoredConflict>> {
    let dir = conflicts_dir(org_root);
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut conflicts = Vec::new();
    let entries = std::fs::read_dir(&dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        let conflict: StoredConflict =
            toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })?;
        conflicts.push(conflict);
    }

    // Sort by node_path for deterministic output
    conflicts.sort_by(|a, b| a.node_path.cmp(&b.node_path));
    Ok(conflicts)
}

pub fn remove_conflict(org_root: &Path, node_path: &str) -> Result<()> {
    let path = conflicts_dir(org_root).join(conflict_filename(node_path));
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn has_conflicts(org_root: &Path) -> Result<bool> {
    let conflicts = list_conflicts(org_root)?;
    Ok(!conflicts.is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_org(tmp: &TempDir) -> std::path::PathBuf {
        let org = tmp.path().join("testorg");
        std::fs::create_dir_all(org.join(".armitage").join("sync").join("conflicts")).unwrap();
        let config = "[org]\nname = \"testorg\"\ngithub_orgs = [\"testorg\"]\n";
        std::fs::write(org.join("armitage.toml"), config).unwrap();
        org
    }

    fn make_field_conflict(field: &str, local: &str, remote: &str) -> FieldConflict {
        FieldConflict {
            field: field.to_string(),
            local_value: local.to_string(),
            remote_value: remote.to_string(),
        }
    }

    #[test]
    fn write_and_read_conflict() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        let fc = make_field_conflict("description", "local desc", "remote desc");
        write_conflict(&org, "gemini/auth", &[fc], None).unwrap();

        let conflicts = list_conflicts(&org).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].node_path, "gemini/auth");
        assert_eq!(conflicts[0].field_conflicts.len(), 1);
        assert_eq!(conflicts[0].field_conflicts[0].field, "description");
        assert_eq!(conflicts[0].field_conflicts[0].local_value, "local desc");
        assert_eq!(conflicts[0].field_conflicts[0].remote_value, "remote desc");
        assert!(conflicts[0].body_conflict.is_none());
    }

    #[test]
    fn write_conflict_with_body() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        let fc = make_field_conflict("name", "local-name", "remote-name");
        write_conflict(
            &org,
            "project/node",
            &[fc],
            Some(("local body text", "remote body text")),
        )
        .unwrap();

        let conflicts = list_conflicts(&org).unwrap();
        assert_eq!(conflicts.len(), 1);
        let c = &conflicts[0];
        assert_eq!(c.node_path, "project/node");
        assert_eq!(c.field_conflicts.len(), 1);
        let body = c
            .body_conflict
            .as_ref()
            .expect("body conflict should be present");
        assert_eq!(body.local, "local body text");
        assert_eq!(body.remote, "remote body text");
    }

    #[test]
    fn remove_conflict() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        let fc = make_field_conflict("name", "a", "b");
        write_conflict(&org, "mynode", std::slice::from_ref(&fc), None).unwrap();
        write_conflict(&org, "other", &[fc], None).unwrap();

        assert_eq!(list_conflicts(&org).unwrap().len(), 2);

        super::remove_conflict(&org, "mynode").unwrap();

        let remaining = list_conflicts(&org).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].node_path, "other");
    }

    #[test]
    fn has_conflicts_returns_correct() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        assert!(!has_conflicts(&org).unwrap());

        let fc = make_field_conflict("description", "x", "y");
        write_conflict(&org, "node1", &[fc], None).unwrap();

        assert!(has_conflicts(&org).unwrap());

        super::remove_conflict(&org, "node1").unwrap();
        assert!(!has_conflicts(&org).unwrap());
    }
}
