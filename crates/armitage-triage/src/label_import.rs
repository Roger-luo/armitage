use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fetch::strip_repo_qualifier;
use armitage_labels::def::{LabelDef, LabelsFile};

// ---------------------------------------------------------------------------
// Reconcile types (LLM-driven label consolidation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSuggestion {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeGroup {
    pub labels: Vec<String>,
    pub reason: String,
    pub suggestions: Vec<LabelSuggestion>,
    /// LLM-recommended label name (from suggestions or existing labels).
    #[serde(default)]
    pub recommended: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReconcileResponse {
    pub merge_groups: Vec<MergeGroup>,
}

// ---------------------------------------------------------------------------
// Import session types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateStatus {
    New,
    Unchanged,
    MetadataDrift,
    DuplicateRemote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFetchedLabel {
    pub name: String,
    pub description: String,
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoLabels {
    pub repo: String,
    pub labels: Vec<RemoteFetchedLabel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLabelVariant {
    pub repo: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelImportCandidate {
    pub name: String,
    pub status: CandidateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<LabelDef>,
    #[serde(default)]
    pub remote_variants: Vec<RemoteLabelVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelImportSession {
    pub id: String,
    pub fetched_at: String,
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub candidates: Vec<LabelImportCandidate>,
}

pub struct MergeSelection {
    pub selected_names: std::collections::BTreeSet<String>,
    pub prefer_repo: Option<String>,
}

// ---------------------------------------------------------------------------
// Session I/O
// ---------------------------------------------------------------------------

fn import_sessions_dir(org_root: &Path) -> PathBuf {
    org_root
        .join(".armitage")
        .join("triage")
        .join("label-imports")
}

pub fn write_import_session(org_root: &Path, session: &LabelImportSession) -> Result<()> {
    let dir = import_sessions_dir(org_root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.toml", session.id));
    let content = toml::to_string(session)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn read_import_session(org_root: &Path, session_id: &str) -> Result<LabelImportSession> {
    let path = import_sessions_dir(org_root).join(format!("{}.toml", session_id));
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })
}

pub fn list_import_session_ids(org_root: &Path) -> Result<Vec<String>> {
    let dir = import_sessions_dir(org_root);
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut ids = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml")
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                ids.push(stem.to_string());
            }
        }
    }
    ids.sort();
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Session construction
// ---------------------------------------------------------------------------

pub fn build_import_session(
    id: &str,
    fetched_at: &str,
    local: &LabelsFile,
    repos: Vec<RepoLabels>,
) -> LabelImportSession {
    let mut grouped: std::collections::BTreeMap<String, Vec<RemoteLabelVariant>> =
        std::collections::BTreeMap::new();
    let repo_names: Vec<String> = repos.iter().map(|repo| repo.repo.clone()).collect();

    for repo_labels in repos {
        for label in repo_labels.labels {
            grouped
                .entry(label.name)
                .or_default()
                .push(RemoteLabelVariant {
                    repo: repo_labels.repo.clone(),
                    description: label.description,
                    color: label.color,
                });
        }
    }

    let candidates = grouped
        .into_iter()
        .map(|(name, remote_variants)| {
            let local_label = local
                .labels
                .iter()
                .find(|label| label.name == name)
                .cloned();
            let status = classify_candidate(local_label.as_ref(), &remote_variants);
            LabelImportCandidate {
                name,
                status,
                local: local_label,
                remote_variants,
            }
        })
        .collect();

    LabelImportSession {
        id: id.to_string(),
        fetched_at: fetched_at.to_string(),
        repos: repo_names,
        candidates,
    }
}

// ---------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------

pub fn merge_selected_candidates(
    local: &mut LabelsFile,
    session: &LabelImportSession,
    selection: &MergeSelection,
) -> Result<()> {
    for candidate in &session.candidates {
        if !selection.selected_names.contains(&candidate.name) {
            continue;
        }

        // Never overwrite a pinned label
        if let Some(existing) = local.labels.iter().find(|l| l.name == candidate.name)
            && existing.pinned
        {
            tracing::debug!(name = candidate.name, "skipping merge of pinned label");
            continue;
        }

        let chosen = choose_remote_variant(candidate, selection.prefer_repo.as_deref())?;
        local.upsert(LabelDef {
            name: candidate.name.clone(),
            description: chosen.description.clone(),
            color: chosen.color.clone(),
            repos: candidate
                .local
                .as_ref()
                .map_or_else(Vec::new, |l| l.repos.clone()),
            pinned: false,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Label filtering
// ---------------------------------------------------------------------------

/// Filter labels applicable to a given repo.
/// A label applies if its `repos` list is empty (universal) or contains the repo.
/// Comparison strips `@qualifier` suffixes so `owner/repo@branch` matches `owner/repo`.
pub fn labels_for_repo(labels: &LabelsFile, repo: &str) -> Vec<LabelDef> {
    let bare = strip_repo_qualifier(repo);
    labels
        .labels
        .iter()
        .filter(|l| l.repos.is_empty() || l.repos.iter().any(|r| strip_repo_qualifier(r) == bare))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Selection helpers
// ---------------------------------------------------------------------------

pub fn default_interactive_selection(
    session: &LabelImportSession,
) -> std::collections::BTreeSet<String> {
    session
        .candidates
        .iter()
        .filter(|candidate| candidate.status == CandidateStatus::New)
        .map(|candidate| candidate.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn classify_candidate(
    local: Option<&LabelDef>,
    remote_variants: &[RemoteLabelVariant],
) -> CandidateStatus {
    if remote_variants.len() > 1 {
        return CandidateStatus::DuplicateRemote;
    }

    match (local, remote_variants.first()) {
        (None, Some(_)) => CandidateStatus::New,
        (Some(local), Some(remote)) => {
            if local.description == remote.description && local.color == remote.color {
                CandidateStatus::Unchanged
            } else {
                CandidateStatus::MetadataDrift
            }
        }
        (_, None) => CandidateStatus::New,
    }
}

pub fn choose_remote_variant<'a>(
    candidate: &'a LabelImportCandidate,
    prefer_repo: Option<&str>,
) -> Result<&'a RemoteLabelVariant> {
    let Some(first) = candidate.remote_variants.first() else {
        return Err(Error::Other(format!(
            "label import candidate has no remote variants: {}",
            candidate.name
        )));
    };

    if candidate.remote_variants.len() == 1 {
        return Ok(first);
    }

    if let Some(repo) = prefer_repo {
        if let Some(matching) = candidate
            .remote_variants
            .iter()
            .find(|variant| variant.repo == repo)
        {
            return Ok(matching);
        }
        return Err(Error::Other(format!(
            "preferred repo {repo} did not provide label {}",
            candidate.name
        )));
    }

    let all_same = candidate
        .remote_variants
        .iter()
        .all(|variant| variant.description == first.description && variant.color == first.color);
    if all_same {
        return Ok(first);
    }

    Err(Error::Other(format!(
        "label {} has conflicting remote metadata; re-run with --prefer-repo",
        candidate.name
    )))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_and_read_import_session_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let session = LabelImportSession {
            id: "20260403T120000Z".to_string(),
            fetched_at: "2026-04-03T12:00:00Z".to_string(),
            repos: vec!["owner/repo".to_string()],
            candidates: vec![LabelImportCandidate {
                name: "bug".to_string(),
                status: CandidateStatus::New,
                local: None,
                remote_variants: vec![RemoteLabelVariant {
                    repo: "owner/repo".to_string(),
                    description: "Broken behavior".to_string(),
                    color: Some("D73A4A".to_string()),
                }],
            }],
        };

        write_import_session(tmp.path(), &session).unwrap();
        let loaded = read_import_session(tmp.path(), &session.id).unwrap();

        assert_eq!(loaded.candidates.len(), 1);
        assert_eq!(loaded.candidates[0].name, "bug");
    }

    #[test]
    fn build_import_session_marks_new_unchanged_and_metadata_drift() {
        let local = LabelsFile {
            labels: vec![
                LabelDef {
                    name: "bug".to_string(),
                    description: "Broken behavior".to_string(),
                    color: Some("D73A4A".to_string()),
                    repos: vec![],
                    pinned: false,
                },
                LabelDef {
                    name: "area:infra".to_string(),
                    description: "Infrastructure work".to_string(),
                    color: Some("0052CC".to_string()),
                    repos: vec![],
                    pinned: false,
                },
            ],
        };

        let remote = vec![RepoLabels {
            repo: "owner/repo".to_string(),
            labels: vec![
                RemoteFetchedLabel {
                    name: "bug".to_string(),
                    description: "Broken behavior".to_string(),
                    color: Some("D73A4A".to_string()),
                },
                RemoteFetchedLabel {
                    name: "area:infra".to_string(),
                    description: "Infra".to_string(),
                    color: Some("0052CC".to_string()),
                },
                RemoteFetchedLabel {
                    name: "priority:high".to_string(),
                    description: "Needs prompt attention".to_string(),
                    color: Some("B60205".to_string()),
                },
            ],
        }];

        let session = build_import_session("session-1", "2026-04-03T12:00:00Z", &local, remote);

        assert_eq!(
            session
                .candidates
                .iter()
                .find(|c| c.name == "bug")
                .unwrap()
                .status,
            CandidateStatus::Unchanged
        );
        assert_eq!(
            session
                .candidates
                .iter()
                .find(|c| c.name == "area:infra")
                .unwrap()
                .status,
            CandidateStatus::MetadataDrift
        );
        assert_eq!(
            session
                .candidates
                .iter()
                .find(|c| c.name == "priority:high")
                .unwrap()
                .status,
            CandidateStatus::New
        );
    }

    #[test]
    fn build_import_session_collapses_duplicate_remote_names() {
        let local = LabelsFile::default();
        let remote = vec![
            RepoLabels {
                repo: "owner/app".to_string(),
                labels: vec![RemoteFetchedLabel {
                    name: "bug".to_string(),
                    description: "Broken".to_string(),
                    color: Some("D73A4A".to_string()),
                }],
            },
            RepoLabels {
                repo: "owner/api".to_string(),
                labels: vec![RemoteFetchedLabel {
                    name: "bug".to_string(),
                    description: "Broken".to_string(),
                    color: Some("D73A4A".to_string()),
                }],
            },
        ];

        let session = build_import_session("session-1", "2026-04-03T12:00:00Z", &local, remote);

        assert_eq!(session.candidates.len(), 1);
        assert_eq!(
            session.candidates[0].status,
            CandidateStatus::DuplicateRemote
        );
        assert_eq!(session.candidates[0].remote_variants.len(), 2);
    }

    #[test]
    fn merge_selected_candidates_adds_new_labels_and_updates_drifted() {
        let mut local = LabelsFile {
            labels: vec![LabelDef {
                name: "bug".to_string(),
                description: "Old".to_string(),
                color: Some("AAAAAA".to_string()),
                repos: vec![],
                pinned: false,
            }],
        };
        let session = LabelImportSession {
            id: "session-1".to_string(),
            fetched_at: "2026-04-03T12:00:00Z".to_string(),
            repos: vec!["owner/repo".to_string()],
            candidates: vec![
                LabelImportCandidate {
                    name: "bug".to_string(),
                    status: CandidateStatus::MetadataDrift,
                    local: Some(LabelDef {
                        name: "bug".to_string(),
                        description: "Old".to_string(),
                        color: Some("AAAAAA".to_string()),
                        repos: vec![],
                        pinned: false,
                    }),
                    remote_variants: vec![RemoteLabelVariant {
                        repo: "owner/repo".to_string(),
                        description: "New".to_string(),
                        color: Some("D73A4A".to_string()),
                    }],
                },
                LabelImportCandidate {
                    name: "priority:high".to_string(),
                    status: CandidateStatus::New,
                    local: None,
                    remote_variants: vec![RemoteLabelVariant {
                        repo: "owner/repo".to_string(),
                        description: "Needs prompt attention".to_string(),
                        color: Some("B60205".to_string()),
                    }],
                },
            ],
        };

        merge_selected_candidates(
            &mut local,
            &session,
            &MergeSelection {
                selected_names: ["bug".to_string(), "priority:high".to_string()]
                    .into_iter()
                    .collect(),
                prefer_repo: None,
            },
        )
        .unwrap();

        assert_eq!(local.labels.len(), 2);
        assert_eq!(
            local
                .labels
                .iter()
                .find(|l| l.name == "bug")
                .unwrap()
                .description,
            "New"
        );
    }

    #[test]
    fn merge_requires_prefer_repo_for_conflicting_duplicate_remote_metadata() {
        let mut local = LabelsFile::default();
        let session = LabelImportSession {
            id: "session-1".to_string(),
            fetched_at: "2026-04-03T12:00:00Z".to_string(),
            repos: vec!["owner/app".to_string(), "owner/api".to_string()],
            candidates: vec![LabelImportCandidate {
                name: "bug".to_string(),
                status: CandidateStatus::DuplicateRemote,
                local: None,
                remote_variants: vec![
                    RemoteLabelVariant {
                        repo: "owner/app".to_string(),
                        description: "Broken app".to_string(),
                        color: Some("D73A4A".to_string()),
                    },
                    RemoteLabelVariant {
                        repo: "owner/api".to_string(),
                        description: "Broken api".to_string(),
                        color: Some("E11D21".to_string()),
                    },
                ],
            }],
        };

        let err = merge_selected_candidates(
            &mut local,
            &session,
            &MergeSelection {
                selected_names: ["bug".to_string()].into_iter().collect(),
                prefer_repo: None,
            },
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("prefer-repo"));
        assert!(err.contains("bug"));
    }

    #[test]
    fn default_interactive_selection_prefers_new_labels_only() {
        let session = LabelImportSession {
            id: "session-1".to_string(),
            fetched_at: "2026-04-03T12:00:00Z".to_string(),
            repos: vec!["owner/repo".to_string()],
            candidates: vec![
                LabelImportCandidate {
                    name: "priority:high".to_string(),
                    status: CandidateStatus::New,
                    local: None,
                    remote_variants: vec![RemoteLabelVariant {
                        repo: "owner/repo".to_string(),
                        description: "Needs prompt attention".to_string(),
                        color: Some("B60205".to_string()),
                    }],
                },
                LabelImportCandidate {
                    name: "bug".to_string(),
                    status: CandidateStatus::MetadataDrift,
                    local: Some(LabelDef {
                        name: "bug".to_string(),
                        description: "Broken".to_string(),
                        color: Some("AAAAAA".to_string()),
                        repos: vec![],
                        pinned: false,
                    }),
                    remote_variants: vec![RemoteLabelVariant {
                        repo: "owner/repo".to_string(),
                        description: "Broken".to_string(),
                        color: Some("D73A4A".to_string()),
                    }],
                },
                LabelImportCandidate {
                    name: "area:infra".to_string(),
                    status: CandidateStatus::Unchanged,
                    local: Some(LabelDef {
                        name: "area:infra".to_string(),
                        description: "Infrastructure work".to_string(),
                        color: Some("0052CC".to_string()),
                        repos: vec![],
                        pinned: false,
                    }),
                    remote_variants: vec![RemoteLabelVariant {
                        repo: "owner/repo".to_string(),
                        description: "Infrastructure work".to_string(),
                        color: Some("0052CC".to_string()),
                    }],
                },
            ],
        };

        let defaults = default_interactive_selection(&session);

        assert!(defaults.contains("priority:high"));
        assert!(!defaults.contains("bug"));
        assert!(!defaults.contains("area:infra"));
    }

    #[test]
    fn labels_for_repo_includes_universal_and_scoped() {
        let labels = LabelsFile {
            labels: vec![
                LabelDef {
                    name: "bug".to_string(),
                    description: "Broken".to_string(),
                    color: None,
                    repos: vec![], // universal
                    pinned: false,
                },
                LabelDef {
                    name: "area:frontend".to_string(),
                    description: "Frontend issues".to_string(),
                    color: None,
                    repos: vec!["owner/web-app".to_string()],
                    pinned: false,
                },
                LabelDef {
                    name: "area:sdk".to_string(),
                    description: "SDK issues".to_string(),
                    color: None,
                    repos: vec!["owner/other-repo".to_string()],
                    pinned: false,
                },
            ],
        };

        let for_web = labels_for_repo(&labels, "owner/web-app");
        assert_eq!(for_web.len(), 2);
        assert!(for_web.iter().any(|l| l.name == "bug"));
        assert!(for_web.iter().any(|l| l.name == "area:frontend"));

        let for_other = labels_for_repo(&labels, "owner/unrelated");
        assert_eq!(for_other.len(), 1);
        assert!(for_other.iter().any(|l| l.name == "bug"));
    }

    #[test]
    fn labels_for_repo_matches_across_at_qualifier() {
        let labels = LabelsFile {
            labels: vec![LabelDef {
                name: "area:ir".to_string(),
                description: "IR issues".to_string(),
                color: None,
                repos: vec!["owner/atlas".to_string()],
                pinned: false,
            }],
        };

        // Query with @qualifier should still match
        let matched = labels_for_repo(&labels, "owner/atlas@rust");
        assert_eq!(matched.len(), 1);

        // And the label's repos field having an @qualifier should also match
        let labels2 = LabelsFile {
            labels: vec![LabelDef {
                name: "area:ir".to_string(),
                description: "IR issues".to_string(),
                color: None,
                repos: vec!["owner/atlas@rust".to_string()],
                pinned: false,
            }],
        };
        let matched2 = labels_for_repo(&labels2, "owner/atlas");
        assert_eq!(matched2.len(), 1);
    }
}
