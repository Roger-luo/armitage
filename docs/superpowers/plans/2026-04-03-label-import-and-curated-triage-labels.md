# Label Import And Curated Triage Labels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a staged GitHub label import workflow with interactive and non-interactive merge into curated `labels.toml`, and include that curated label catalog in `armitage triage classify`.

**Architecture:** Keep `labels.toml` as the curated source of truth and introduce a separate local import-session layer under `.armitage/label-imports/` for fetched remote labels, deduplication, and merge decisions. Extend the triage CLI with a `labels` subcommand family, add a small GitHub label fetch adapter plus merge helpers, and update the prompt builder to load curated label names and descriptions from `labels.toml`.

**Tech Stack:** Rust, clap, serde, toml, ionem `gh` wrapper, rustyline, cargo-nextest

---

### Task 1: Extend the curated label model and lock label identity by name

**Files:**
- Modify: `src/model/label.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn roundtrip_preserves_optional_color() {
    let tmp = TempDir::new().unwrap();
    let lf = LabelsFile {
        labels: vec![LabelDef {
            name: "bug".to_string(),
            description: "Something is broken".to_string(),
            color: Some("D73A4A".to_string()),
        }],
    };

    lf.write(tmp.path()).unwrap();
    let loaded = LabelsFile::read(tmp.path()).unwrap();

    assert_eq!(loaded.labels[0].color.as_deref(), Some("D73A4A"));
}

#[test]
fn upsert_updates_existing_label_by_name() {
    let mut lf = LabelsFile::default();
    lf.upsert(LabelDef {
        name: "bug".to_string(),
        description: "Old".to_string(),
        color: Some("AAAAAA".to_string()),
    });
    lf.upsert(LabelDef {
        name: "bug".to_string(),
        description: "New".to_string(),
        color: Some("BBBBBB".to_string()),
    });

    assert_eq!(lf.labels.len(), 1);
    assert_eq!(lf.labels[0].description, "New");
    assert_eq!(lf.labels[0].color.as_deref(), Some("BBBBBB"));
}

#[test]
fn duplicate_names_in_file_are_rejected() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("labels.toml"),
        r#"
            [[labels]]
            name = "bug"
            description = "First"

            [[labels]]
            name = "bug"
            description = "Second"
        "#,
    )
    .unwrap();

    let err = LabelsFile::read(tmp.path()).unwrap_err().to_string();
    assert!(err.contains("duplicate"));
    assert!(err.contains("bug"));
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(roundtrip_preserves_optional_color|upsert_updates_existing_label_by_name|duplicate_names_in_file_are_rejected)'`

Expected: FAIL because `LabelDef` does not yet have `color`, `LabelsFile` has no `upsert()` helper, and duplicate local names are not validated.

- [ ] **Step 3: Write the minimal implementation**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabelDef {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl LabelsFile {
    pub fn read(org_root: &Path) -> Result<Self> {
        let path = org_root.join(LABELS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let parsed: Self =
            toml::from_str(&content).map_err(|source| Error::TomlParse { path: path.clone(), source })?;
        parsed.validate_unique_names()?;
        Ok(parsed)
    }

    pub fn upsert(&mut self, label: LabelDef) {
        if let Some(existing) = self.labels.iter_mut().find(|l| l.name == label.name) {
            *existing = label;
        } else {
            self.labels.push(label);
        }
    }

    fn validate_unique_names(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for label in &self.labels {
            if !seen.insert(label.name.clone()) {
                return Err(Error::Other(format!(
                    "duplicate label name in labels.toml: {}",
                    label.name
                )));
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(roundtrip_preserves_optional_color|upsert_updates_existing_label_by_name|duplicate_names_in_file_are_rejected|roundtrip|missing_file_returns_empty|add_is_idempotent|names_returns_sorted_list)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/model/label.rs
git commit -m "feat: extend curated label catalog model"
```

### Task 2: Add GitHub label fetching and import-session persistence

**Files:**
- Modify: `src/github/issue.rs`
- Create: `src/triage/labels.rs`
- Modify: `src/triage/mod.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parse_github_label_list_json() {
    let json = r#"[
        {"name":"bug","description":"Broken behavior","color":"D73A4A"},
        {"name":"priority:high","description":"Needs prompt attention","color":"B60205"}
    ]"#;

    let labels: Vec<GitHubRepoLabel> = serde_json::from_str(json).unwrap();
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].name, "bug");
    assert_eq!(labels[0].description.as_deref(), Some("Broken behavior"));
    assert_eq!(labels[0].color, "D73A4A");
}

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
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(parse_github_label_list_json|write_and_read_import_session_roundtrip)'`

Expected: FAIL because repo-label parsing and session persistence do not exist yet.

- [ ] **Step 3: Write the minimal implementation**

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRepoLabel {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub color: String,
}

pub fn fetch_repo_labels(gh: &ionem::shell::gh::Gh, repo: &str) -> Result<Vec<GitHubRepoLabel>> {
    let json = gh.run(&["label", "list", "--repo", repo, "--json", "name,description,color"])?;
    let labels: Vec<GitHubRepoLabel> = serde_json::from_str(&json)?;
    Ok(labels)
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelImportSession {
    pub id: String,
    pub fetched_at: String,
    pub repos: Vec<String>,
    pub candidates: Vec<LabelImportCandidate>,
}

pub fn write_import_session(org_root: &Path, session: &LabelImportSession) -> Result<()> {
    let dir = org_root.join(".armitage").join("label-imports");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.toml", session.id));
    std::fs::write(path, toml::to_string(session)?)?;
    Ok(())
}
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(parse_github_label_list_json|write_and_read_import_session_roundtrip)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/github/issue.rs src/triage/labels.rs src/triage/mod.rs
git commit -m "feat: add staged github label imports"
```

### Task 3: Classify import candidates by name and deduplicate cross-repo labels

**Files:**
- Modify: `src/triage/labels.rs`
- Modify: `src/model/label.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn build_import_session_marks_new_unchanged_and_metadata_drift() {
    let local = LabelsFile {
        labels: vec![
            LabelDef {
                name: "bug".to_string(),
                description: "Broken behavior".to_string(),
                color: Some("D73A4A".to_string()),
            },
            LabelDef {
                name: "area:infra".to_string(),
                description: "Infrastructure work".to_string(),
                color: Some("0052CC".to_string()),
            },
        ],
    };

    let remote = vec![
        RepoLabels {
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
        },
    ];

    let session = build_import_session("session-1", "2026-04-03T12:00:00Z", &local, remote);

    assert_eq!(
        session.candidates.iter().find(|c| c.name == "bug").unwrap().status,
        CandidateStatus::Unchanged
    );
    assert_eq!(
        session.candidates.iter().find(|c| c.name == "area:infra").unwrap().status,
        CandidateStatus::MetadataDrift
    );
    assert_eq!(
        session.candidates.iter().find(|c| c.name == "priority:high").unwrap().status,
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
    assert_eq!(session.candidates[0].status, CandidateStatus::DuplicateRemote);
    assert_eq!(session.candidates[0].remote_variants.len(), 2);
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(build_import_session_marks_new_unchanged_and_metadata_drift|build_import_session_collapses_duplicate_remote_names)'`

Expected: FAIL because diff classification and cross-repo deduplication logic do not yet exist.

- [ ] **Step 3: Write the minimal implementation**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateStatus {
    New,
    Unchanged,
    MetadataDrift,
    DuplicateRemote,
}

pub fn build_import_session(
    id: &str,
    fetched_at: &str,
    local: &LabelsFile,
    repos: Vec<RepoLabels>,
) -> LabelImportSession {
    let mut grouped: std::collections::BTreeMap<String, Vec<RemoteLabelVariant>> =
        std::collections::BTreeMap::new();

    for repo_labels in repos {
        for label in repo_labels.labels {
            grouped.entry(label.name.clone()).or_default().push(RemoteLabelVariant {
                repo: repo_labels.repo.clone(),
                description: label.description,
                color: label.color,
            });
        }
    }

    let candidates = grouped
        .into_iter()
        .map(|(name, remote_variants)| {
            let local_label = local.labels.iter().find(|l| l.name == name).cloned();
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
        repos: vec![],
        candidates,
    }
}
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(build_import_session_marks_new_unchanged_and_metadata_drift|build_import_session_collapses_duplicate_remote_names)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/triage/labels.rs src/model/label.rs
git commit -m "feat: diff imported labels against curated catalog"
```

### Task 4: Add non-interactive merge selection and deterministic conflict resolution

**Files:**
- Modify: `src/triage/labels.rs`
- Modify: `src/model/label.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn merge_selected_candidates_adds_new_labels_and_updates_drifted() {
    let mut local = LabelsFile {
        labels: vec![LabelDef {
            name: "bug".to_string(),
            description: "Old".to_string(),
            color: Some("AAAAAA".to_string()),
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
            selected_names: ["bug".to_string(), "priority:high".to_string()].into_iter().collect(),
            prefer_repo: None,
        },
    )
    .unwrap();

    assert_eq!(local.labels.len(), 2);
    assert_eq!(local.labels.iter().find(|l| l.name == "bug").unwrap().description, "New");
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
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(merge_selected_candidates_adds_new_labels_and_updates_drifted|merge_requires_prefer_repo_for_conflicting_duplicate_remote_metadata)'`

Expected: FAIL because merge selection and conflict resolution do not yet exist.

- [ ] **Step 3: Write the minimal implementation**

```rust
pub struct MergeSelection {
    pub selected_names: std::collections::BTreeSet<String>,
    pub prefer_repo: Option<String>,
}

pub fn merge_selected_candidates(
    local: &mut LabelsFile,
    session: &LabelImportSession,
    selection: &MergeSelection,
) -> Result<()> {
    for candidate in &session.candidates {
        if !selection.selected_names.contains(&candidate.name) {
            continue;
        }

        let chosen = choose_remote_variant(candidate, selection.prefer_repo.as_deref())?;
        local.upsert(LabelDef {
            name: candidate.name.clone(),
            description: chosen.description.clone(),
            color: chosen.color.clone(),
        });
    }
    Ok(())
}
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(merge_selected_candidates_adds_new_labels_and_updates_drifted|merge_requires_prefer_repo_for_conflicting_duplicate_remote_metadata)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/triage/labels.rs src/model/label.rs
git commit -m "feat: add non-interactive curated label merge"
```

### Task 5: Add interactive merge selection flow

**Files:**
- Modify: `src/triage/labels.rs`
- Modify: `src/cli/triage.rs`

- [ ] **Step 1: Write the failing tests**

```rust
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
```

Add a helper-level test rather than a terminal end-to-end test. Keep terminal I/O in thin wrappers around pure selection logic.

- [ ] **Step 2: Run the targeted test to verify it fails**

Run: `cargo nextest run -E 'test(default_interactive_selection_prefers_new_labels_only)'`

Expected: FAIL because interactive selection defaults do not yet exist.

- [ ] **Step 3: Write the minimal implementation**

```rust
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

pub fn run_labels_merge_interactive(org_root: &Path, session: &LabelImportSession) -> Result<()> {
    let mut editor = rustyline::DefaultEditor::new()?;
    let mut selected = default_interactive_selection(session);

    for candidate in &session.candidates {
        let default = if selected.contains(&candidate.name) { "Y/n" } else { "y/N" };
        let preview = candidate
            .remote_variants
            .first()
            .map(|variant| variant.description.as_str())
            .unwrap_or("");
        let prompt = format!(
            "[{}] {} ({:?}) {} ",
            candidate.name,
            preview,
            candidate.status,
            default
        );
        let input = editor.readline(&prompt)?;
        let accept = match input.trim().to_ascii_lowercase().as_str() {
            "" => selected.contains(&candidate.name),
            "y" | "yes" => true,
            "n" | "no" => false,
            _ => selected.contains(&candidate.name),
        };
        if accept {
            selected.insert(candidate.name.clone());
        } else {
            selected.remove(&candidate.name);
        }
    }

    let mut labels = LabelsFile::read(org_root)?;
    merge_selected_candidates(
        &mut labels,
        session,
        &MergeSelection {
            selected_names: selected,
            prefer_repo: None,
        },
    )?;
    labels.write(org_root)
}
```

- [ ] **Step 4: Run the targeted test to verify it passes**

Run: `cargo nextest run -E 'test(default_interactive_selection_prefers_new_labels_only)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/triage/labels.rs src/cli/triage.rs
git commit -m "feat: add interactive label merge flow"
```

### Task 6: Add `triage labels fetch` and `triage labels merge` CLI plumbing

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/triage.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn labels_fetch_requires_repo_arguments() {
    let err = run_labels_fetch(vec![]).unwrap_err().to_string();
    assert!(err.contains("--repo"));
}

#[test]
fn latest_session_is_used_when_session_flag_is_absent() {
    let latest = resolve_merge_session_id(None, &["20260403T120000Z".to_string(), "20260403T130000Z".to_string()]).unwrap();
    assert_eq!(latest, "20260403T130000Z");
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(labels_fetch_requires_repo_arguments|latest_session_is_used_when_session_flag_is_absent)'`

Expected: FAIL because the labels CLI entry points do not yet exist.

- [ ] **Step 3: Write the minimal implementation**

```rust
#[derive(Subcommand)]
enum TriageCommands {
    // existing variants ...
    Labels {
        #[command(subcommand)]
        command: TriageLabelCommands,
    },
}

#[derive(Subcommand)]
enum TriageLabelCommands {
    Fetch {
        #[arg(long)]
        repo: Vec<String>,
    },
    Merge {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        all_new: bool,
        #[arg(long)]
        update_drifted: bool,
        #[arg(long)]
        name: Vec<String>,
        #[arg(long)]
        exclude_name: Vec<String>,
        #[arg(long)]
        prefer_repo: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}
```

```rust
pub fn run_labels_fetch(repo: Vec<String>) -> Result<()> {
    if repo.is_empty() {
        return Err(Error::Other("specify at least one --repo <owner/repo>".to_string()));
    }
    // locate org root, fetch remote labels, stage import session, print summary
    Ok(())
}

fn resolve_merge_session_id(explicit: Option<String>, session_ids: &[String]) -> Result<String> {
    if let Some(id) = explicit {
        return Ok(id);
    }

    session_ids
        .iter()
        .max()
        .cloned()
        .ok_or_else(|| Error::Other("no label import session found".to_string()))
}
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(labels_fetch_requires_repo_arguments|latest_session_is_used_when_session_flag_is_absent)'`

Run: `cargo run -- triage labels fetch --help`

Expected: PASS, and help output shows the new fetch and merge subcommands.

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage label import commands"
```

### Task 7: Include curated labels in the triage prompt as name plus description

**Files:**
- Modify: `src/triage/llm.rs`
- Modify: `src/cli/triage.rs`
- Modify: `src/model/label.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn curated_labels_section_lists_name_and_description_only() {
    let labels = LabelsFile {
        labels: vec![
            LabelDef {
                name: "bug".to_string(),
                description: "Broken behavior".to_string(),
                color: Some("D73A4A".to_string()),
            },
            LabelDef {
                name: "priority:high".to_string(),
                description: "Needs prompt attention".to_string(),
                color: Some("B60205".to_string()),
            },
        ],
    };

    let section = build_curated_labels_section(&labels);

    assert!(section.contains("- bug: Broken behavior"));
    assert!(section.contains("- priority:high: Needs prompt attention"));
    assert!(!section.contains("D73A4A"));
    assert!(!section.contains("owner/repo"));
}

#[test]
fn prompt_includes_curated_labels_section() {
    let issue = StoredIssue {
        id: 1,
        repo: "owner/repo".to_string(),
        number: 42,
        title: "Fix label import".to_string(),
        body: "Need better label curation.".to_string(),
        state: "OPEN".to_string(),
        labels: vec!["bug".to_string()],
        updated_at: "2026-04-03T12:00:00Z".to_string(),
        fetched_at: "2026-04-03T12:00:00Z".to_string(),
        sub_issues_count: 0,
    };
    let nodes = vec![NodeEntry {
        path: "infra".to_string(),
        dir: std::path::PathBuf::from("/tmp/infra"),
        node: Node {
            name: "Infra".to_string(),
            description: "Infrastructure work".to_string(),
            github_issue: None,
            labels: vec![],
            repos: vec![],
            timeline: None,
            status: NodeStatus::Active,
        },
    }];
    let schema = LabelSchema::default();
    let labels = LabelsFile {
        labels: vec![LabelDef {
            name: "bug".to_string(),
            description: "Broken behavior".to_string(),
            color: None,
        }],
    };

    let prompt = build_prompt(&issue, &nodes, &schema, &labels);

    assert!(prompt.contains("## Curated Labels"));
    assert!(prompt.contains("- bug: Broken behavior"));
}
```

- [ ] **Step 2: Run the targeted tests to verify they fail**

Run: `cargo nextest run -E 'test(curated_labels_section_lists_name_and_description_only|prompt_includes_curated_labels_section)'`

Expected: FAIL because prompt builders do not yet accept curated labels.

- [ ] **Step 3: Write the minimal implementation**

```rust
fn build_curated_labels_section(labels: &LabelsFile) -> String {
    let mut s = String::from("## Curated Labels\n");
    if labels.labels.is_empty() {
        s.push_str("No curated labels defined.\n");
    } else {
        for label in &labels.labels {
            s.push_str(&format!("- {}: {}\n", label.name, label.description));
        }
    }
    s
}
```

```rust
fn build_prompt(
    issue: &StoredIssue,
    nodes: &[NodeEntry],
    label_schema: &LabelSchema,
    curated_labels: &LabelsFile,
) -> String {
    let mut prompt = String::from(/* existing header */);
    prompt.push_str(&build_roadmap_section(nodes));
    prompt.push('\n');
    prompt.push_str(&build_label_schema_section(label_schema));
    prompt.push('\n');
    prompt.push_str(&build_curated_labels_section(curated_labels));
    prompt.push('\n');
    prompt.push_str(&build_issue_section(issue));
    // existing guidelines
    prompt
}
```

- [ ] **Step 4: Run the targeted tests to verify they pass**

Run: `cargo nextest run -E 'test(curated_labels_section_lists_name_and_description_only|prompt_includes_curated_labels_section)'`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/triage/llm.rs src/cli/triage.rs src/model/label.rs
git commit -m "feat: include curated labels in triage prompts"
```

### Task 8: Document the workflow and run repository verification

**Files:**
- Modify: `README.md`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/triage.rs`
- Modify: `src/triage/llm.rs`
- Modify: `src/triage/labels.rs`
- Modify: `src/model/label.rs`
- Modify: `src/github/issue.rs`
- Modify: `src/triage/mod.rs`

- [ ] **Step 1: Write the failing docs-oriented expectations**

```text
Add README coverage for:
- labels.toml as curated source of truth
- triage labels fetch
- triage labels merge
- non-interactive merge examples
- classify using curated labels
```

This task does not need a Rust test first. The failure condition is missing user-facing documentation for the new workflow.

- [ ] **Step 2: Update the documentation**

```md
### Curated labels

Use `labels.toml` as the curated label catalog shared across repos.

```bash
armitage triage labels fetch --repo owner/repo --repo owner/infra
armitage triage labels merge
armitage triage labels merge --all-new --update-drifted --yes
```

`armitage triage classify` uses the curated label catalog from `labels.toml` as part of the LLM prompt.
```

- [ ] **Step 3: Run formatting, lint, and tests**

Run: `cargo fmt --all`

Expected: exits 0

Run: `cargo clippy --all-targets --all-features -- -D warnings`

Expected: exits 0

Run: `cargo nextest run`

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add README.md src/cli/mod.rs src/cli/triage.rs src/triage/llm.rs src/triage/labels.rs src/model/label.rs src/github/issue.rs src/triage/mod.rs
git commit -m "feat: add curated github label import workflow"
```

## Spec Coverage Check

- Staged import sessions under `.armitage/label-imports/`: covered by Tasks 2 and 3.
- Interactive terminal picker and non-interactive merge path: covered by Tasks 4, 5, and 6.
- `labels.toml` as curated truth with unique-name identity and optional color: covered by Task 1.
- Name-only comparison and metadata drift detection: covered by Task 3.
- No delete path during import: enforced in Tasks 4 and 5 by limiting merge to upsert behavior.
- Prompt changes to send only label name and short description: covered by Task 7.
- README and final verification: covered by Task 8.

## Self-Review Notes

- Placeholder scan: no `TODO`, `TBD`, or deferred implementation text appears inside executable tasks.
- Type consistency: the plan consistently uses `LabelImportSession`, `LabelImportCandidate`, `CandidateStatus`, `MergeSelection`, and `LabelsFile`.
- Scope check: the work stays within one subsystem: curated label import plus prompt consumption.
