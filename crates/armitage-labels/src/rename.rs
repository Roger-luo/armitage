use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Rename ledger (tracks old->new label mappings for repo sync)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelRename {
    pub old_name: String,
    pub new_name: String,
    pub recorded_at: String,
    #[serde(default)]
    pub synced_repos: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelRenameLedger {
    #[serde(default)]
    pub renames: Vec<LabelRename>,
}

fn renames_path(org_root: &Path) -> PathBuf {
    org_root
        .join(".armitage")
        .join("labels")
        .join("renames.toml")
}

pub fn read_rename_ledger(org_root: &Path) -> Result<LabelRenameLedger> {
    let path = renames_path(org_root);
    if !path.exists() {
        return Ok(LabelRenameLedger::default());
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })
}

pub fn write_rename_ledger(org_root: &Path, ledger: &LabelRenameLedger) -> Result<()> {
    let path = renames_path(org_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string(ledger)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Record rename mappings. Collapses chains (A->B + B->C keeps both B->C and updates A->B to A->C).
/// If the same `old_name` already has a mapping, the target is updated (not duplicated).
pub fn record_renames(org_root: &Path, mappings: &[(String, String)]) -> Result<()> {
    let mut ledger = read_rename_ledger(org_root)?;
    let now = chrono::Utc::now().to_rfc3339();

    for (old, new) in mappings {
        // Collapse chains: if an existing rename targets `old`, update it to target `new`
        for existing in &mut ledger.renames {
            if existing.new_name == *old {
                existing.new_name.clone_from(new);
                existing.recorded_at.clone_from(&now);
                existing.synced_repos.clear();
            }
        }

        // Update existing mapping for same old_name, or add a new one
        if let Some(existing) = ledger.renames.iter_mut().find(|r| r.old_name == *old) {
            if existing.new_name != *new {
                existing.new_name.clone_from(new);
                existing.recorded_at.clone_from(&now);
                existing.synced_repos.clear();
            }
        } else {
            ledger.renames.push(LabelRename {
                old_name: old.clone(),
                new_name: new.clone(),
                recorded_at: now.clone(),
                synced_repos: vec![],
            });
        }
    }

    write_rename_ledger(org_root, &ledger)
}

pub fn mark_rename_synced(ledger: &mut LabelRenameLedger, old: &str, new: &str, repo: &str) {
    if let Some(entry) = ledger
        .renames
        .iter_mut()
        .find(|r| r.old_name == old && r.new_name == new)
        && !entry.synced_repos.iter().any(|r| r == repo)
    {
        entry.synced_repos.push(repo.to_string());
    }
}

pub fn pending_renames_for_repo(ledger: &LabelRenameLedger, repo: &str) -> Vec<LabelRename> {
    ledger
        .renames
        .iter()
        .filter(|r| !r.synced_repos.iter().any(|sr| sr == repo))
        .cloned()
        .collect()
}

/// Remove duplicate entries for the same `old_name`, keeping the latest one.
/// This cleans up ledgers where multiple reconciliation runs added conflicting targets.
pub fn dedup_rename_ledger(ledger: &mut LabelRenameLedger) {
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut to_remove = Vec::new();
    // Walk backwards so we keep the latest entry (last in the vec = most recently recorded)
    for i in (0..ledger.renames.len()).rev() {
        if let Some(&existing_idx) = seen.get(ledger.renames[i].old_name.as_str()) {
            // We already saw a later entry for this old_name -- remove the earlier one
            to_remove.push(i);
            // But check if the later entry is actually older by timestamp
            if ledger.renames[i].recorded_at > ledger.renames[existing_idx].recorded_at {
                // This one is newer despite being earlier in the vec -- swap which one to keep
                to_remove.pop(); // undo removing i
                to_remove.push(existing_idx);
                seen.insert(&ledger.renames[i].old_name, i);
            }
        } else {
            seen.insert(&ledger.renames[i].old_name, i);
        }
    }
    to_remove.sort_unstable();
    to_remove.dedup();
    for i in to_remove.into_iter().rev() {
        ledger.renames.remove(i);
    }
}

pub fn prune_fully_synced(ledger: &mut LabelRenameLedger, all_repos: &[String]) {
    ledger.renames.retain(|r| {
        !all_repos
            .iter()
            .all(|repo| r.synced_repos.iter().any(|sr| sr == repo))
    });
}

/// Translate a list of label names through the rename ledger.
///
/// Any label matching an `old_name` in the ledger is replaced with its `new_name`.
/// Duplicates that arise from the mapping are removed (preserving first occurrence order).
pub fn translate_labels(labels: &[String], ledger: &LabelRenameLedger) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(labels.len());
    for label in labels {
        let translated = ledger
            .renames
            .iter()
            .find(|r| r.old_name == *label)
            .map_or(label.as_str(), |r| r.new_name.as_str());
        if seen.insert(translated.to_string()) {
            out.push(translated.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rename_ledger_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let ledger = LabelRenameLedger {
            renames: vec![LabelRename {
                old_name: "A-service".to_string(),
                new_name: "A-cloud".to_string(),
                recorded_at: "2026-04-03T12:00:00Z".to_string(),
                synced_repos: vec!["owner/repo".to_string()],
            }],
        };
        write_rename_ledger(tmp.path(), &ledger).unwrap();
        let loaded = read_rename_ledger(tmp.path()).unwrap();
        assert_eq!(loaded.renames.len(), 1);
        assert_eq!(loaded.renames[0].old_name, "A-service");
        assert_eq!(loaded.renames[0].synced_repos, vec!["owner/repo"]);
    }

    #[test]
    fn record_renames_collapses_chains() {
        let tmp = TempDir::new().unwrap();
        // First rename: A -> B
        record_renames(tmp.path(), &[("A".into(), "B".into())]).unwrap();
        // Second rename: B -> C (should collapse A -> C)
        record_renames(tmp.path(), &[("B".into(), "C".into())]).unwrap();

        let ledger = read_rename_ledger(tmp.path()).unwrap();
        // A -> C (collapsed) and B -> C (new)
        assert!(
            ledger
                .renames
                .iter()
                .any(|r| r.old_name == "A" && r.new_name == "C")
        );
        assert!(
            ledger
                .renames
                .iter()
                .any(|r| r.old_name == "B" && r.new_name == "C")
        );
    }

    #[test]
    fn record_renames_updates_existing_mapping_for_same_old_name() {
        let tmp = TempDir::new().unwrap();
        // First rename: analysis -> category: analysis
        record_renames(
            tmp.path(),
            &[("analysis".into(), "category: analysis".into())],
        )
        .unwrap();
        // Second rename with same old_name but different target
        record_renames(tmp.path(), &[("analysis".into(), "area: analysis".into())]).unwrap();

        let ledger = read_rename_ledger(tmp.path()).unwrap();
        // Should have exactly one entry for "analysis", updated to the latest target
        let analysis_renames: Vec<_> = ledger
            .renames
            .iter()
            .filter(|r| r.old_name == "analysis")
            .collect();
        assert_eq!(analysis_renames.len(), 1);
        assert_eq!(analysis_renames[0].new_name, "area: analysis");
    }

    #[test]
    fn pending_renames_excludes_synced_repos() {
        let ledger = LabelRenameLedger {
            renames: vec![
                LabelRename {
                    old_name: "A".to_string(),
                    new_name: "B".to_string(),
                    recorded_at: "2026-04-03T12:00:00Z".to_string(),
                    synced_repos: vec!["owner/repo1".to_string()],
                },
                LabelRename {
                    old_name: "C".to_string(),
                    new_name: "D".to_string(),
                    recorded_at: "2026-04-03T12:00:00Z".to_string(),
                    synced_repos: vec![],
                },
            ],
        };
        let pending = pending_renames_for_repo(&ledger, "owner/repo1");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].old_name, "C");
    }

    #[test]
    fn prune_removes_fully_synced() {
        let mut ledger = LabelRenameLedger {
            renames: vec![
                LabelRename {
                    old_name: "A".to_string(),
                    new_name: "B".to_string(),
                    recorded_at: "2026-04-03T12:00:00Z".to_string(),
                    synced_repos: vec!["repo1".to_string(), "repo2".to_string()],
                },
                LabelRename {
                    old_name: "C".to_string(),
                    new_name: "D".to_string(),
                    recorded_at: "2026-04-03T12:00:00Z".to_string(),
                    synced_repos: vec!["repo1".to_string()],
                },
            ],
        };
        prune_fully_synced(&mut ledger, &["repo1".to_string(), "repo2".to_string()]);
        assert_eq!(ledger.renames.len(), 1);
        assert_eq!(ledger.renames[0].old_name, "C");
    }

    #[test]
    fn translate_labels_applies_renames() {
        let ledger = LabelRenameLedger {
            renames: vec![
                LabelRename {
                    old_name: "A-frontend".to_string(),
                    new_name: "area: frontend".to_string(),
                    recorded_at: "2026-04-05T00:00:00Z".to_string(),
                    synced_repos: vec![],
                },
                LabelRename {
                    old_name: "D-CI-CD".to_string(),
                    new_name: "devops: ci".to_string(),
                    recorded_at: "2026-04-05T00:00:00Z".to_string(),
                    synced_repos: vec![],
                },
            ],
        };

        let input = vec![
            "A-frontend".to_string(),
            "bug".to_string(),
            "D-CI-CD".to_string(),
        ];
        let result = translate_labels(&input, &ledger);
        assert_eq!(result, vec!["area: frontend", "bug", "devops: ci"]);
    }

    #[test]
    fn translate_labels_deduplicates() {
        let ledger = LabelRenameLedger {
            renames: vec![LabelRename {
                old_name: "old-bug".to_string(),
                new_name: "bug".to_string(),
                recorded_at: "2026-04-05T00:00:00Z".to_string(),
                synced_repos: vec![],
            }],
        };

        let input = vec!["bug".to_string(), "old-bug".to_string()];
        let result = translate_labels(&input, &ledger);
        assert_eq!(result, vec!["bug"]);
    }
}
