use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeSyncEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSyncEntry {
    pub github_issue: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pulled_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pushed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_hash: Option<String>,
}

fn state_path(org_root: &Path) -> std::path::PathBuf {
    org_root.join(".armitage").join("sync").join("state.toml")
}

pub fn read_sync_state(org_root: &Path) -> Result<SyncState> {
    let path = state_path(org_root);
    if !path.exists() {
        return Ok(SyncState::default());
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })
}

pub fn write_sync_state(org_root: &Path, state: &SyncState) -> Result<()> {
    let path = state_path(org_root);
    // Ensure .armitage/sync directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string(state)?;
    std::fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_org(tmp: &TempDir) -> std::path::PathBuf {
        let org = tmp.path().join("testorg");
        std::fs::create_dir_all(org.join(".armitage").join("sync")).unwrap();
        let config = "[org]\nname = \"testorg\"\ngithub_orgs = [\"testorg\"]\n";
        std::fs::write(org.join("armitage.toml"), config).unwrap();
        org
    }

    #[test]
    fn roundtrip_sync_state() {
        let mut state = SyncState::default();
        state.nodes.insert(
            "gemini/auth".to_string(),
            NodeSyncEntry {
                github_issue: "owner/repo#42".to_string(),
                last_pulled_at: Some(
                    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                        .unwrap()
                        .into(),
                ),
                last_pushed_at: None,
                remote_updated_at: Some(
                    DateTime::parse_from_rfc3339("2026-01-02T12:00:00Z")
                        .unwrap()
                        .into(),
                ),
                local_hash: Some("abc123".to_string()),
            },
        );

        let serialized = toml::to_string(&state).unwrap();
        let deserialized: SyncState = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.nodes.len(), 1);
        let entry = deserialized.nodes.get("gemini/auth").unwrap();
        assert_eq!(entry.github_issue, "owner/repo#42");
        assert!(entry.last_pulled_at.is_some());
        assert!(entry.last_pushed_at.is_none());
        assert_eq!(entry.local_hash.as_deref(), Some("abc123"));
    }

    #[test]
    fn read_write_sync_state() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        let mut state = SyncState::default();
        state.nodes.insert(
            "project/subnode".to_string(),
            NodeSyncEntry {
                github_issue: "acme/things#7".to_string(),
                last_pulled_at: None,
                last_pushed_at: None,
                remote_updated_at: None,
                local_hash: Some("deadbeef".to_string()),
            },
        );

        write_sync_state(&org, &state).unwrap();

        let loaded = read_sync_state(&org).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        let entry = loaded.nodes.get("project/subnode").unwrap();
        assert_eq!(entry.github_issue, "acme/things#7");
        assert_eq!(entry.local_hash.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn read_missing_sync_state_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        let state = read_sync_state(&org).unwrap();
        assert!(state.nodes.is_empty());
    }
}
