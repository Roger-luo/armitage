use std::collections::BTreeSet;
use std::path::Path;

use chrono::Utc;
use rustyline::{DefaultEditor, Editor, error::ReadlineError};

use crate::cli::complete::{CommaCompleteHelper, NodePathHelper};
use crate::error::{Error, Result};
use armitage_core::node::IssueRef;
use armitage_core::org::Org;
use armitage_core::tree::{NodeEntry, find_org_root, walk_nodes};
use armitage_github::issue;
use armitage_labels::LabelsDomain;
use armitage_labels::def::{LabelDef, LabelsFile};
use armitage_labels::rename;
use armitage_labels::schema::LabelSchema;
use armitage_triage::config::TriageConfig;
use armitage_triage::label_import;
use armitage_triage::{TriageDomain, apply, cache, categories, db, examples, fetch, llm, review};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Summary,
    Refs,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            "summary" => Ok(Self::Summary),
            "refs" => Ok(Self::Refs),
            other => Err(Error::Other(format!(
                "unknown format '{other}', expected 'table', 'json', 'summary', or 'refs'"
            ))),
        }
    }
}

pub fn run_fetch(repo: Vec<String>, since: Option<String>) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;
    let conn = db::open_db(&org_root)?;
    let org = Org::open(&org_root)?;

    let count = fetch::fetch_all(
        &gh,
        &conn,
        &org_root,
        &repo,
        org.info().default_repo.as_deref(),
        since.as_deref(),
    )?;
    println!("Total: {count} issues fetched");

    let repos_cached = cache::refresh_all(&conn, &org_root)?;
    println!("Issue cache refreshed ({repos_cached} repos)");
    Ok(())
}

pub fn run_labels_fetch(repo: Vec<String>, org_flag: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let org = Org::open(&org_root)?;
    let gh = armitage_github::require_gh()?;

    let mut repos: BTreeSet<String> = if !repo.is_empty() {
        repo.into_iter().collect()
    } else if org_flag {
        // Fetch from all repos in configured github_orgs
        let mut all = BTreeSet::new();
        for org_name in &org.info().github_orgs {
            println!("Listing repos in {org_name}...");
            match issue::list_org_repos(&gh, org_name) {
                Ok(org_repos) => {
                    println!("  Found {} repo(s)", org_repos.len());
                    all.extend(org_repos);
                }
                Err(e) => {
                    eprintln!("  Error listing {org_name}: {e}");
                }
            }
        }
        all
    } else {
        fetch::collect_repos_from_nodes(&org_root)?
            .into_iter()
            .collect()
    };

    if let Some(dr) = &org.info().default_repo {
        repos.insert(dr.clone());
    }

    if repos.is_empty() {
        println!(
            "No repos found. Specify --repo, --org, add repos to node.toml files, or set org.default_repo in armitage.toml."
        );
        return Ok(());
    }

    let repos: Vec<String> = repos.into_iter().collect();
    let local = LabelsFile::read(&org_root)?;

    let mut remote = Vec::new();
    for repo_name in &repos {
        let fetched = issue::fetch_repo_labels(&gh, repo_name)?;
        remote.push(label_import::RepoLabels {
            repo: repo_name.clone(),
            labels: fetched
                .into_iter()
                .map(|label| label_import::RemoteFetchedLabel {
                    name: label.name,
                    description: label.description.unwrap_or_default(),
                    color: Some(label.color),
                })
                .collect(),
        });
    }

    let now = Utc::now();
    let session_id = now.format("%Y%m%dT%H%M%SZ").to_string();
    let session =
        label_import::build_import_session(&session_id, &now.to_rfc3339(), &local, remote);
    label_import::write_import_session(&org_root, &session)?;

    let new = session
        .candidates
        .iter()
        .filter(|candidate| candidate.status == label_import::CandidateStatus::New)
        .count();
    let drifted = session
        .candidates
        .iter()
        .filter(|candidate| candidate.status == label_import::CandidateStatus::MetadataDrift)
        .count();
    let unchanged = session
        .candidates
        .iter()
        .filter(|candidate| candidate.status == label_import::CandidateStatus::Unchanged)
        .count();
    let duplicate_remote = session
        .candidates
        .iter()
        .filter(|candidate| candidate.status == label_import::CandidateStatus::DuplicateRemote)
        .count();

    println!(
        "Fetched {} labels across {} repo(s)",
        session.candidates.len(),
        repos.len()
    );
    println!("  New: {new}");
    println!("  Drifted: {drifted}");
    println!("  Unchanged: {unchanged}");
    println!("  Duplicate remote: {duplicate_remote}");
    println!("  Session: {}", session.id);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_labels_merge(
    session: Option<String>,
    all_new: bool,
    update_drifted: bool,
    name: Vec<String>,
    exclude_name: Vec<String>,
    prefer_repo: Option<String>,
    yes: bool,
    no_llm: bool,
    auto_accept: bool,
    backend: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let org = Org::open(&org_root)?;
    let triage_config: TriageConfig = org.domain_config::<TriageDomain>()?;
    let label_schema: LabelSchema = org.domain_config::<LabelsDomain>()?;

    let session_ids = label_import::list_import_session_ids(&org_root)?;
    let session_id = resolve_merge_session_id(session, &session_ids)?;
    let import_session = label_import::read_import_session(&org_root, &session_id)?;

    // LLM reconciliation is on by default; --no-llm disables it
    if no_llm {
        tracing::debug!("LLM reconciliation disabled via --no-llm");
    } else {
        match resolve_labels_llm_config(backend, model, effort, &triage_config) {
            Ok(config) => {
                let local = LabelsFile::read(&org_root)?;
                run_llm_reconcile(
                    &org_root,
                    &local,
                    &import_session,
                    &config,
                    &label_schema,
                    auto_accept,
                )?;
            }
            Err(e) => {
                tracing::debug!(reason = %e, "skipping LLM reconciliation (no backend configured)");
            }
        }
    }

    let has_noninteractive_selection =
        all_new || update_drifted || !name.is_empty() || !exclude_name.is_empty();
    if !has_noninteractive_selection {
        return run_labels_merge_interactive(&org_root, &import_session, prefer_repo);
    }

    let mut local = LabelsFile::read(&org_root)?;
    let selected_names = build_noninteractive_selection(
        &org_root,
        &import_session,
        &local,
        all_new,
        update_drifted,
        &name,
        &exclude_name,
    );

    println!(
        "Applying {} selected label(s) from session {}",
        selected_names.len(),
        import_session.id
    );
    if !yes && !confirm("Apply selected label changes? [y/N] ")? {
        println!("Aborted.");
        return Ok(());
    }

    label_import::merge_selected_candidates(
        &mut local,
        &import_session,
        &label_import::MergeSelection {
            selected_names,
            prefer_repo,
        },
    )?;
    local.write(&org_root)?;
    println!("Updated labels.toml");
    Ok(())
}

fn run_llm_reconcile(
    org_root: &Path,
    local: &LabelsFile,
    session: &label_import::LabelImportSession,
    config: &llm::LlmConfig,
    schema: &LabelSchema,
    auto_accept: bool,
) -> Result<()> {
    let response = llm::reconcile_labels(local, session, config, schema)?;

    if response.merge_groups.is_empty() {
        println!("No similar labels found — nothing to reconcile.\n");
        return Ok(());
    }

    println!("Found {} merge group(s)\n", response.merge_groups.len());

    let mut local = local.clone();
    let pinned_names: BTreeSet<String> = local
        .labels
        .iter()
        .filter(|l| l.pinned)
        .map(|l| l.name.clone())
        .collect();
    let mut applied = 0usize;
    let mut rename_mappings: Vec<(String, String)> = Vec::new();
    for (i, group) in response.merge_groups.iter().enumerate() {
        // Skip groups that contain any pinned label
        if group.labels.iter().any(|l| pinned_names.contains(l)) {
            tracing::debug!(
                labels = ?group.labels,
                "skipping reconciliation group containing pinned label(s)"
            );
            continue;
        }

        let is_reformat = group.labels.len() == 1;
        let action = if is_reformat { "Reformat" } else { "Merge" };

        println!("--- {action} {}/{} ---", i + 1, response.merge_groups.len());
        if is_reformat {
            let name = &group.labels[0];
            let desc = find_label_description(name, &local, session);
            println!("Label: {name} — {desc}");
        } else {
            println!("Labels:");
            for label_name in &group.labels {
                let desc = find_label_description(label_name, &local, session);
                println!("  - {label_name} — {desc}");
            }
        }
        println!("Reason: {}", group.reason);
        println!();

        // Step 1: for multi-label groups, select which labels to merge
        // For single-label (reformat), skip straight to pick
        let selected_labels: Vec<&String> = if is_reformat {
            if auto_accept {
                // Auto-accept: always reformat
                group.labels.iter().collect()
            } else {
                // Reformat: confirm or skip
                let choice =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Reformat this label?")
                        .items(["Yes", "No (keep as-is)"])
                        .default(0)
                        .interact()
                        .map_err(|e| Error::Other(e.to_string()))?;
                if choice != 0 {
                    println!("  Skipped.\n");
                    continue;
                }
                group.labels.iter().collect()
            }
        } else if auto_accept {
            // Auto-accept: select all labels
            group.labels.iter().collect()
        } else {
            let label_items: Vec<String> = group
                .labels
                .iter()
                .map(|name| {
                    let desc = find_label_description(name, &local, session);
                    format!("{name} — {desc}")
                })
                .collect();
            let defaults: Vec<bool> = vec![true; label_items.len()];

            let selected_indices =
                dialoguer::MultiSelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Select labels to merge (space to toggle, enter to confirm)")
                    .items(&label_items)
                    .defaults(&defaults)
                    .max_length(10)
                    .interact()
                    .map_err(|e| Error::Other(e.to_string()))?;

            let sel: Vec<&String> = selected_indices
                .iter()
                .map(|&idx| &group.labels[idx])
                .collect();

            if sel.len() < 2 {
                println!("  Fewer than 2 labels selected, skipping.\n");
                continue;
            }
            sel
        };

        // Step 2: pick target — loop allows refining via LLM
        let suggestions = group.suggestions.clone();
        let recommended = group.recommended.clone();
        let chosen = if auto_accept {
            // Auto-accept: pick the recommended option
            pick_recommended(
                &selected_labels,
                &suggestions,
                &recommended,
                &local,
                session,
            )
        } else {
            pick_interactive(
                &selected_labels,
                suggestions,
                recommended,
                &local,
                session,
                config,
                schema,
            )?
        };

        // Apply: remove selected labels, upsert chosen
        for label_name in &selected_labels {
            local.remove(label_name);
            if **label_name != chosen.name {
                rename_mappings.push(((*label_name).clone(), chosen.name.clone()));
            }
        }
        local.upsert(chosen.clone());
        applied += 1;

        if is_reformat {
            println!(
                "  Reformatted to: {} — {}\n",
                chosen.name, chosen.description
            );
        } else {
            println!("  Merged into: {} — {}\n", chosen.name, chosen.description);
        }
    }

    if applied > 0 {
        local.write(org_root)?;
        println!("Updated labels.toml ({applied} group(s) reconciled)");
        if !rename_mappings.is_empty() {
            rename::record_renames(org_root, &rename_mappings)?;
            println!(
                "Recorded {} rename(s) — run `armitage triage labels sync` to push to GitHub\n",
                rename_mappings.len()
            );
        }
    } else {
        println!("No reconciliation changes made.\n");
    }
    Ok(())
}

/// Auto-accept: pick the recommended label, falling back to the first suggestion or first selected label.
fn pick_recommended(
    selected_labels: &[&String],
    suggestions: &[label_import::LabelSuggestion],
    recommended: &Option<String>,
    local: &LabelsFile,
    session: &label_import::LabelImportSession,
) -> LabelDef {
    // Try to find the recommended option among suggestions
    if let Some(rec) = recommended {
        if let Some(s) = suggestions.iter().find(|s| s.name == *rec) {
            return LabelDef {
                name: s.name.clone(),
                description: s.description.clone(),
                color: None,
                repos: vec![],
                pinned: false,
            };
        }
        // Recommended might be an existing label
        if let Some(label_name) = selected_labels.iter().find(|l| ***l == *rec) {
            let desc = find_label_description(label_name, local, session);
            return LabelDef {
                name: (*label_name).clone(),
                description: desc,
                color: find_label_color(label_name, local),
                repos: vec![],
                pinned: false,
            };
        }
    }
    // Fallback: first suggestion, then first selected label
    if let Some(s) = suggestions.first() {
        LabelDef {
            name: s.name.clone(),
            description: s.description.clone(),
            color: None,
            repos: vec![],
            pinned: false,
        }
    } else {
        let name = selected_labels[0];
        let desc = find_label_description(name, local, session);
        LabelDef {
            name: (*name).clone(),
            description: desc,
            color: find_label_color(name, local),
            repos: vec![],
            pinned: false,
        }
    }
}

/// Interactive label selection with LLM refinement loop.
#[allow(clippy::too_many_arguments)]
fn pick_interactive(
    selected_labels: &[&String],
    mut suggestions: Vec<label_import::LabelSuggestion>,
    mut recommended: Option<String>,
    local: &LabelsFile,
    session: &label_import::LabelImportSession,
    config: &llm::LlmConfig,
    schema: &LabelSchema,
) -> Result<LabelDef> {
    loop {
        let mut options: Vec<String> = Vec::new();
        let mut option_labels: Vec<LabelDef> = Vec::new();
        let mut recommended_idx: usize = 0;

        // New suggestions from LLM
        for suggestion in &suggestions {
            let marker = if recommended.as_deref() == Some(&suggestion.name) {
                " ★"
            } else {
                ""
            };
            options.push(format!(
                "[new] {} — {}{marker}",
                suggestion.name, suggestion.description
            ));
            if recommended.as_deref() == Some(&suggestion.name) {
                recommended_idx = option_labels.len();
            }
            option_labels.push(LabelDef {
                name: suggestion.name.clone(),
                description: suggestion.description.clone(),
                color: None,
                repos: vec![],
                pinned: false,
            });
        }

        // Existing labels
        for label_name in selected_labels {
            let desc = find_label_description(label_name, local, session);
            let marker = if recommended.as_deref() == Some(label_name.as_str()) {
                " ★"
            } else {
                ""
            };
            options.push(format!("{label_name} — {desc}{marker}"));
            if recommended.as_deref() == Some(label_name.as_str()) {
                recommended_idx = option_labels.len();
            }
            option_labels.push(LabelDef {
                name: (*label_name).clone(),
                description: desc,
                color: find_label_color(label_name, local),
                repos: vec![],
                pinned: false,
            });
        }

        // Refine option at the end
        let refine_idx = options.len();
        options.push("[refine] Ask LLM for different suggestions...".to_string());

        let pick = dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Pick the label to use (★ = recommended)")
            .items(&options)
            .default(recommended_idx)
            .max_length(10)
            .interact()
            .map_err(|e| Error::Other(e.to_string()))?;

        if pick != refine_idx {
            return Ok(option_labels.into_iter().nth(pick).unwrap());
        }

        // User wants to refine — get feedback and call LLM
        let feedback: String =
            dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                .with_prompt("What should the LLM change?")
                .interact_text()
                .map_err(|e| Error::Other(e.to_string()))?;

        let label_names: Vec<String> = selected_labels.iter().map(|s| (*s).clone()).collect();
        match llm::refine_label_suggestions(config, schema, &label_names, &suggestions, &feedback) {
            Ok(new_suggestions) => {
                recommended = new_suggestions.first().map(|s| s.name.clone());
                suggestions = new_suggestions;
                println!();
            }
            Err(e) => {
                println!("  LLM refinement failed: {e}\n  Showing previous options.\n");
            }
        }
    }
}

fn find_label_description(
    name: &str,
    local: &LabelsFile,
    session: &label_import::LabelImportSession,
) -> String {
    if let Some(label) = local.labels.iter().find(|l| l.name == name) {
        return label.description.clone();
    }
    if let Some(candidate) = session.candidates.iter().find(|c| c.name == name)
        && let Some(v) = candidate.remote_variants.first()
    {
        return v.description.clone();
    }
    String::new()
}

fn find_label_color(name: &str, local: &LabelsFile) -> Option<String> {
    local
        .labels
        .iter()
        .find(|l| l.name == name)
        .and_then(|l| l.color.clone())
}

/// Collect target repos for label sync/push operations.
fn collect_label_target_repos(
    repo: &[String],
    org_flag: bool,
    org: &Org,
    org_root: &Path,
    gh: &armitage_github::Gh,
) -> Result<Vec<String>> {
    let mut repos: BTreeSet<String> = if !repo.is_empty() {
        repo.iter().cloned().collect()
    } else if org_flag {
        let mut all = BTreeSet::new();
        for org_name in &org.info().github_orgs {
            println!("Listing repos in {org_name}...");
            match issue::list_org_repos(gh, org_name) {
                Ok(org_repos) => {
                    println!("  Found {} repo(s)", org_repos.len());
                    all.extend(org_repos);
                }
                Err(e) => {
                    eprintln!("  Error listing {org_name}: {e}");
                }
            }
        }
        all
    } else {
        fetch::collect_repos_from_nodes(org_root)?
            .into_iter()
            .collect()
    };
    if let Some(dr) = &org.info().default_repo {
        repos.insert(dr.clone());
    }
    Ok(repos.into_iter().collect())
}

pub fn run_labels_sync(
    repo: Vec<String>,
    org_flag: bool,
    dry_run: bool,
    prune: bool,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let org = Org::open(&org_root)?;
    let gh = armitage_github::require_gh()?;

    let repos = collect_label_target_repos(&repo, org_flag, &org, &org_root, &gh)?;
    if repos.is_empty() {
        println!(
            "No repos found. Specify --repo, --org, add repos to node.toml files, or set org.default_repo in armitage.toml."
        );
        return Ok(());
    }

    let local = LabelsFile::read(&org_root)?;
    let pinned_names: BTreeSet<String> = local
        .labels
        .iter()
        .filter(|l| l.pinned)
        .map(|l| l.name.clone())
        .collect();

    let mut ledger = rename::read_rename_ledger(&org_root)?;

    // Deduplicate conflicting renames
    let before_dedup = ledger.renames.len();
    rename::dedup_rename_ledger(&mut ledger);
    let deduped = before_dedup - ledger.renames.len();
    if deduped > 0 {
        println!("Cleaned up {deduped} conflicting rename(s) from ledger");
        rename::write_rename_ledger(&org_root, &ledger)?;
    }

    // Remove renames where old_name is a pinned label
    let before = ledger.renames.len();
    ledger
        .renames
        .retain(|r| !pinned_names.contains(&r.old_name));
    let dropped = before - ledger.renames.len();
    if dropped > 0 {
        tracing::debug!(dropped = dropped, "dropped renames for pinned labels");
        rename::write_rename_ledger(&org_root, &ledger)?;
    }

    if ledger.renames.is_empty() {
        println!("No pending renames.");
        return Ok(());
    }

    for repo_name in &repos {
        let pending = rename::pending_renames_for_repo(&ledger, repo_name);
        if pending.is_empty() {
            tracing::debug!(repo = repo_name.as_str(), "no pending renames");
            continue;
        }
        println!("{repo_name}: {} pending rename(s)", pending.len());

        let remote_labels = issue::fetch_repo_labels(&gh, repo_name)?;
        let mut remote_names: BTreeSet<String> =
            remote_labels.iter().map(|l| l.name.clone()).collect();

        for r in &pending {
            let old = &r.old_name;
            let new = &r.new_name;
            let old_exists = remote_names.contains(old.as_str());
            let new_exists = remote_names.contains(new.as_str());

            if !old_exists {
                if dry_run {
                    let issues = issue::list_issues_with_label(&gh, repo_name, old)?;
                    if issues.is_empty() {
                        println!("  \"{old}\" → \"{new}\" — old label not present, skipping");
                    } else {
                        println!(
                            "  \"{old}\" → \"{new}\" — would relabel {} issue(s) (ghost label)",
                            issues.len()
                        );
                    }
                    continue;
                }
                let issues = issue::list_issues_with_label(&gh, repo_name, old)?;
                if issues.is_empty() {
                    println!("  \"{old}\" → \"{new}\" — old label not present, skipping");
                    rename::mark_rename_synced(&mut ledger, old, new, repo_name);
                    continue;
                }
                for number in &issues {
                    let number_str = number.to_string();
                    gh.run(&[
                        "issue",
                        "edit",
                        &number_str,
                        "--add-label",
                        new,
                        "--remove-label",
                        old,
                        "--repo",
                        repo_name,
                    ])?;
                }
                println!(
                    "  \"{old}\" → \"{new}\" — relabeled {} issue(s) (ghost label)",
                    issues.len()
                );
                rename::mark_rename_synced(&mut ledger, old, new, repo_name);
                continue;
            }

            if dry_run {
                if new_exists {
                    println!("  \"{old}\" → \"{new}\" — would relabel issues and delete \"{old}\"");
                } else {
                    println!("  \"{old}\" → \"{new}\" — would rename label");
                }
                continue;
            }

            match apply_single_rename(&gh, repo_name, old, new, new_exists) {
                Ok(msg) => println!("  \"{old}\" → \"{new}\" — {msg}"),
                Err(e) if is_not_found_error(&e) => {
                    println!("  \"{old}\" → \"{new}\" — skipped (no write access to {repo_name})");
                    rename::mark_rename_synced(&mut ledger, old, new, repo_name);
                    continue;
                }
                Err(e) => return Err(e),
            }
            remote_names.insert(new.clone());
            remote_names.remove(old.as_str());
            rename::mark_rename_synced(&mut ledger, old, new, repo_name);
        }
    }

    if !dry_run {
        if prune {
            rename::prune_fully_synced(&mut ledger, &repos);
        }
        rename::write_rename_ledger(&org_root, &ledger)?;
    }
    Ok(())
}

fn is_not_found_error(e: &Error) -> bool {
    let msg = e.to_string();
    msg.contains("404") || msg.contains("Not Found")
}

/// Apply a single label rename on a repo. Returns a description of what happened.
fn apply_single_rename(
    gh: &armitage_github::Gh,
    repo: &str,
    old: &str,
    new: &str,
    new_exists: bool,
) -> Result<String> {
    if !new_exists {
        match issue::rename_label(gh, repo, old, new) {
            Ok(()) => return Ok("renamed".to_string()),
            Err(e) if e.to_string().contains("already exists") => {
                tracing::debug!(
                    old = old,
                    new = new,
                    "rename failed (already exists), falling back to relabel+delete"
                );
                // fall through to relabel
            }
            Err(e) => return Err(e.into()),
        }
    }

    let issues = issue::list_issues_with_label(gh, repo, old)?;
    for number in &issues {
        let number_str = number.to_string();
        gh.run(&[
            "issue",
            "edit",
            &number_str,
            "--add-label",
            new,
            "--remove-label",
            old,
            "--repo",
            repo,
        ])?;
    }
    issue::delete_label(gh, repo, old)?;
    Ok(format!(
        "relabeled {} issue(s), deleted old label",
        issues.len()
    ))
}

pub fn run_labels_push(
    repo: Vec<String>,
    org_flag: bool,
    dry_run: bool,
    delete_extra: bool,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let org = Org::open(&org_root)?;
    let local = LabelsFile::read(&org_root)?;
    let gh = armitage_github::require_gh()?;

    let repos = collect_label_target_repos(&repo, org_flag, &org, &org_root, &gh)?;
    if repos.is_empty() {
        println!(
            "No repos found. Specify --repo, --org, add repos to node.toml files, or set org.default_repo in armitage.toml."
        );
        return Ok(());
    }

    for repo_name in &repos {
        let applicable = label_import::labels_for_repo(&local, repo_name);
        let applicable_names: BTreeSet<&str> = applicable.iter().map(|l| l.name.as_str()).collect();

        let remote_labels = issue::fetch_repo_labels(&gh, repo_name)?;
        let remote_by_name: std::collections::BTreeMap<&str, &issue::GitHubRepoLabel> =
            remote_labels.iter().map(|l| (l.name.as_str(), l)).collect();

        let mut created = 0usize;
        let mut updated = 0usize;
        let mut deleted = 0usize;

        // Create or update labels that should exist on this repo
        for label in &applicable {
            match remote_by_name.get(label.name.as_str()) {
                None => {
                    if dry_run {
                        println!("  {repo_name}: would create \"{}\"", label.name);
                    } else {
                        issue::create_label(
                            &gh,
                            repo_name,
                            &label.name,
                            &label.description,
                            label.color.as_deref(),
                        )?;
                    }
                    created += 1;
                }
                Some(remote) => {
                    let remote_desc = remote.description.as_deref().unwrap_or("");
                    let desc_differs = remote_desc != label.description;
                    let color_differs = label
                        .color
                        .as_ref()
                        .is_some_and(|c| !c.eq_ignore_ascii_case(&remote.color));
                    if desc_differs || color_differs {
                        if dry_run {
                            println!("  {repo_name}: would update \"{}\"", label.name);
                        } else {
                            issue::update_label_metadata(
                                &gh,
                                repo_name,
                                &label.name,
                                &label.description,
                                label.color.as_deref(),
                            )?;
                        }
                        updated += 1;
                    }
                }
            }
        }

        // Delete labels that exist on remote but not in local catalog
        if delete_extra {
            for remote in &remote_labels {
                if !applicable_names.contains(remote.name.as_str()) {
                    if dry_run {
                        println!("  {repo_name}: would delete \"{}\"", remote.name);
                    } else {
                        issue::delete_label(&gh, repo_name, &remote.name)?;
                    }
                    deleted += 1;
                }
            }
        }

        let action = if dry_run { "would apply" } else { "applied" };
        println!(
            "{repo_name}: {action} {created} create(s), {updated} update(s), {deleted} delete(s)"
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_classify(
    backend: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    batch_size: usize,
    parallel: usize,
    limit: Option<usize>,
    repo: Option<String>,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;
    let nodes = walk_nodes(&org_root)?;
    let org = Org::open(&org_root)?;
    let triage_config: TriageConfig = org.domain_config::<TriageDomain>()?;
    let label_schema: LabelSchema = org.domain_config::<LabelsDomain>()?;
    let curated_labels = LabelsFile::read(&org_root)?;

    let config = resolve_classify_config(backend, model, effort, &triage_config)?;
    let triage_examples = examples::load_examples(&org_root)?;
    if !triage_examples.is_empty() {
        println!("Loaded {} classification example(s)", triage_examples.len());
    }
    let count = llm::triage_issues(
        &conn,
        &nodes,
        llm::PromptCatalog {
            label_schema: &label_schema,
            curated_labels: &curated_labels,
        },
        &triage_examples,
        &config,
        batch_size,
        parallel,
        limit,
        repo.as_deref(),
    )?;

    let repos_cached = cache::refresh_all(&conn, &org_root)?;

    if fmt == OutputFormat::Json {
        let dist = db::get_confidence_distribution(&conn, repo.as_deref())?;
        let nodes_breakdown = db::get_node_breakdown(&conn, repo.as_deref())?;
        let votes = db::get_new_category_votes(&conn, repo.as_deref())?;
        let null_count = nodes_breakdown
            .iter()
            .find(|n| n.node.is_none())
            .map(|n| n.count)
            .unwrap_or(0);

        let json = serde_json::json!({
            "classified": count,
            "confidence_distribution": dist,
            "top_nodes": nodes_breakdown.iter().filter(|n| n.node.is_some()).take(20).collect::<Vec<_>>(),
            "null_node_count": null_count,
            "suggested_new_categories": votes,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json).map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        println!("Classified {count} issues");
        println!("Issue cache refreshed ({repos_cached} repos)");
    }
    Ok(())
}

/// Resolve LLM config for `triage labels merge`.
fn resolve_labels_llm_config(
    backend: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    triage: &TriageConfig,
) -> Result<llm::LlmConfig> {
    let labels_cfg = triage.labels.as_ref();
    let backend_str = backend
        .or_else(|| labels_cfg.and_then(|l| l.backend.clone()))
        .or_else(|| triage.backend.clone())
        .ok_or_else(|| {
            Error::Other(
                "LLM backend not configured. Set [triage.labels].backend, \
                 [triage].backend, or pass --backend"
                    .to_string(),
            )
        })?;
    let model = model
        .or_else(|| labels_cfg.and_then(|l| l.model.clone()))
        .or_else(|| triage.model.clone());
    let effort = effort
        .or_else(|| labels_cfg.and_then(|l| l.effort.clone()))
        .or_else(|| triage.effort.clone());
    let backend = llm::LlmBackend::parse(&backend_str)?;

    if matches!(backend, llm::LlmBackend::Gemini) && model.is_none() {
        return Err(Error::Other(
            "model must be set when backend is gemini".to_string(),
        ));
    }

    tracing::debug!(
        backend = backend.name(),
        model = model.as_deref().unwrap_or("default"),
        effort = effort.as_deref().unwrap_or("default"),
        "resolved labels LLM config"
    );
    let api_key_env = labels_cfg
        .and_then(|l| l.api_key_env.clone())
        .or_else(|| triage.api_key_env.clone());
    let thinking_budget = labels_cfg
        .and_then(|l| l.thinking_budget)
        .or(triage.thinking_budget);

    Ok(llm::LlmConfig {
        backend,
        model,
        effort,
        api_key_env,
        thinking_budget,
    })
}

fn resolve_classify_config(
    backend: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    triage: &TriageConfig,
) -> Result<llm::LlmConfig> {
    let backend_str = backend.or_else(|| triage.backend.clone()).ok_or_else(|| {
        Error::Other("triage backend must be set via --backend or [triage].backend".to_string())
    })?;
    let model = model.or_else(|| triage.model.clone());
    let effort = effort.or_else(|| triage.effort.clone());
    let backend = llm::LlmBackend::parse(&backend_str)?;

    if matches!(backend, llm::LlmBackend::Gemini) && model.is_none() {
        return Err(Error::Other(
            "triage model must be set via --model or [triage].model when backend is gemini"
                .to_string(),
        ));
    }

    Ok(llm::LlmConfig {
        backend,
        model,
        effort,
        api_key_env: triage.api_key_env.clone(),
        thinking_budget: triage.thinking_budget,
    })
}

pub fn run_review(
    interactive: bool,
    list: bool,
    auto_approve: Option<f64>,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    if list || (!interactive && auto_approve.is_none()) {
        if fmt == OutputFormat::Json {
            let suggestions =
                db::get_pending_suggestions_filtered(&conn, min_confidence, max_confidence)?;
            let json_rows: Vec<serde_json::Value> = suggestions
                .iter()
                .map(|(issue, sug)| {
                    serde_json::json!({
                        "issue_ref": format!("{}#{}", issue.repo, issue.number),
                        "title": issue.title,
                        "suggested_node": sug.suggested_node,
                        "suggested_labels": sug.suggested_labels,
                        "confidence": sug.confidence,
                        "reasoning": sug.reasoning,
                        "is_tracking_issue": sug.is_tracking_issue,
                        "suggested_new_categories": sug.suggested_new_categories,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json_rows)
                    .map_err(|e| Error::Other(e.to_string()))?
            );
        } else {
            review_list(&conn, min_confidence, max_confidence)?;
        }
    } else if let Some(threshold) = auto_approve {
        let stats = review::review_auto_approve(&conn, threshold)?;
        println!("Approved: {}, Skipped: {}", stats.approved, stats.skipped);
    } else if interactive {
        let stats = review_interactive(&conn, &org_root, min_confidence, max_confidence)?;
        println!(
            "Approved: {}, Rejected: {}, Modified: {}, Stale: {}, Inquired: {}, Skipped: {}",
            stats.approved,
            stats.rejected,
            stats.modified,
            stats.stale,
            stats.inquired,
            stats.skipped
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive review (was in triage/review.rs, moved here for CLI access)
// ---------------------------------------------------------------------------

/// What the user decided for a single item — tracked so `[b]ack` can undo it.
#[derive(Debug, Clone)]
enum SessionAction {
    Decided {
        suggestion_id: i64,
        decision: String,
        saved_example: bool,
        issue_ref: String,
    },
    Skipped,
}

fn review_interactive(
    conn: &rusqlite::Connection,
    org_root: &Path,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
) -> Result<review::ReviewStats> {
    let pending = db::get_pending_suggestions_filtered(conn, min_confidence, max_confidence)?;
    if pending.is_empty() {
        println!("No pending suggestions to review.");
        return Ok(review::ReviewStats::default());
    }

    let node_entries = walk_nodes(org_root)?;
    let label_names: Vec<String> = LabelsFile::read(org_root)?
        .labels
        .into_iter()
        .map(|l| l.name)
        .collect();

    let total = pending.len();
    let mut stats = review::ReviewStats::default();
    let now = chrono::Utc::now().to_rfc3339();
    let term = console::Term::stderr();

    let mut history: Vec<SessionAction> = Vec::new();
    let mut i = 0;

    while i < total {
        let (issue_row, suggestion) = &pending[i];
        print_suggestion(i + 1, total, issue_row, suggestion);

        loop {
            eprint!(
                "  {}a{}pprove  {}r{}eject  {}m{}odify  s{}t{}ale  {}i{}nquire  {}s{}kip  {}b{}ack  {}q{}uit: ",
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
                console::Style::new().bold().apply_to("["),
                console::Style::new().bold().apply_to("]"),
            );

            let ch = term.read_char()?;
            eprintln!("{ch}");

            let issue_ref = format!("{}#{}", issue_row.repo, issue_row.number);

            match ch.to_ascii_lowercase() {
                'a' => {
                    let merged =
                        review::merge_labels(&issue_row.labels, &suggestion.suggested_labels);
                    db::insert_decision(
                        conn,
                        &db::ReviewDecision {
                            id: 0,
                            suggestion_id: suggestion.id,
                            decision: "approved".to_string(),
                            final_node: suggestion.suggested_node.clone(),
                            final_labels: merged,
                            decided_at: now.clone(),
                            applied_at: None,
                            question: String::new(),
                        },
                    )?;
                    history.push(SessionAction::Decided {
                        suggestion_id: suggestion.id,
                        decision: "approved".to_string(),
                        saved_example: false,
                        issue_ref,
                    });
                    stats.approved += 1;
                    eprintln!("  -> Approved");
                    break;
                }
                'r' => {
                    db::insert_decision(
                        conn,
                        &db::ReviewDecision {
                            id: 0,
                            suggestion_id: suggestion.id,
                            decision: "rejected".to_string(),
                            final_node: None,
                            final_labels: vec![],
                            decided_at: now.clone(),
                            applied_at: None,
                            question: String::new(),
                        },
                    )?;
                    let note = prompt_note(&term)?;
                    save_review_example(org_root, issue_row, suggestion, None, &[], false, &note);
                    history.push(SessionAction::Decided {
                        suggestion_id: suggestion.id,
                        decision: "rejected".to_string(),
                        saved_example: true,
                        issue_ref,
                    });
                    stats.rejected += 1;
                    eprintln!("  -> Rejected");
                    break;
                }
                'm' => {
                    let (node, labels) =
                        prompt_modification(issue_row, suggestion, &node_entries, &label_names)?;
                    db::insert_decision(
                        conn,
                        &db::ReviewDecision {
                            id: 0,
                            suggestion_id: suggestion.id,
                            decision: "modified".to_string(),
                            final_node: node.clone(),
                            final_labels: labels.clone(),
                            decided_at: now.clone(),
                            applied_at: None,
                            question: String::new(),
                        },
                    )?;
                    let note = prompt_note(&term)?;
                    save_review_example(
                        org_root,
                        issue_row,
                        suggestion,
                        node.as_deref(),
                        &labels,
                        suggestion.is_stale,
                        &note,
                    );
                    history.push(SessionAction::Decided {
                        suggestion_id: suggestion.id,
                        decision: "modified".to_string(),
                        saved_example: true,
                        issue_ref,
                    });
                    stats.modified += 1;
                    eprintln!("  -> Modified and approved");
                    break;
                }
                't' => {
                    let note = prompt_note(&term)?;
                    let stale_question =
                        prompt_stale_inquiry(&term, org_root, issue_row, &suggestion.reasoning)?;
                    db::insert_decision(
                        conn,
                        &db::ReviewDecision {
                            id: 0,
                            suggestion_id: suggestion.id,
                            decision: "stale".to_string(),
                            final_node: None,
                            final_labels: vec![],
                            decided_at: now.clone(),
                            applied_at: None,
                            question: stale_question.clone(),
                        },
                    )?;
                    save_review_example(org_root, issue_row, suggestion, None, &[], true, &note);
                    history.push(SessionAction::Decided {
                        suggestion_id: suggestion.id,
                        decision: "stale".to_string(),
                        saved_example: true,
                        issue_ref,
                    });
                    stats.stale += 1;
                    if stale_question.is_empty() {
                        eprintln!("  -> Marked as stale");
                    } else {
                        eprintln!("  -> Marked as stale (inquiry will be posted on apply)");
                    }
                    break;
                }
                'i' => {
                    let question = generate_inquire_question(org_root, issue_row, &node_entries)?;
                    let final_question = prompt_question(&question)?;
                    if final_question.is_empty() {
                        eprintln!("  Empty question — skipping inquire.");
                        continue;
                    }
                    db::insert_decision(
                        conn,
                        &db::ReviewDecision {
                            id: 0,
                            suggestion_id: suggestion.id,
                            decision: "inquired".to_string(),
                            final_node: None,
                            final_labels: vec![],
                            decided_at: now.clone(),
                            applied_at: None,
                            question: final_question,
                        },
                    )?;
                    history.push(SessionAction::Decided {
                        suggestion_id: suggestion.id,
                        decision: "inquired".to_string(),
                        saved_example: false,
                        issue_ref,
                    });
                    stats.inquired += 1;
                    eprintln!("  -> Inquire (question will be posted on apply)");
                    break;
                }
                's' => {
                    history.push(SessionAction::Skipped);
                    stats.skipped += 1;
                    break;
                }
                'b' => {
                    if let Some(prev) = history.pop() {
                        undo_action(conn, org_root, &prev, &mut stats)?;
                        i -= 1;
                        eprintln!();
                        break;
                    } else {
                        eprintln!("  Already at the first item.");
                    }
                }
                'q' => {
                    return Ok(stats);
                }
                _ => {
                    eprintln!("  Invalid choice. Press a, r, m, t, i, s, b, or q.");
                }
            }
        }
        if history.len() == i + 1 {
            i += 1;
            eprintln!();
        }
    }

    Ok(stats)
}

fn undo_action(
    conn: &rusqlite::Connection,
    org_root: &Path,
    action: &SessionAction,
    stats: &mut review::ReviewStats,
) -> Result<()> {
    match action {
        SessionAction::Decided {
            suggestion_id,
            decision,
            saved_example,
            issue_ref,
        } => {
            db::delete_decision_by_suggestion_id(conn, *suggestion_id)?;
            if *saved_example && let Err(e) = examples::remove_example(org_root, issue_ref) {
                eprintln!("  (warning: failed to remove example: {e})");
            }
            match decision.as_str() {
                "approved" => stats.approved = stats.approved.saturating_sub(1),
                "rejected" => stats.rejected = stats.rejected.saturating_sub(1),
                "modified" => stats.modified = stats.modified.saturating_sub(1),
                "stale" => stats.stale = stats.stale.saturating_sub(1),
                "inquired" => stats.inquired = stats.inquired.saturating_sub(1),
                _ => {}
            }
            eprintln!("  -> Undid previous decision, going back");
        }
        SessionAction::Skipped => {
            stats.skipped = stats.skipped.saturating_sub(1);
            eprintln!("  -> Going back to previous item");
        }
    }
    Ok(())
}

fn review_list(
    conn: &rusqlite::Connection,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
) -> Result<()> {
    let pending = db::get_pending_suggestions_filtered(conn, min_confidence, max_confidence)?;
    if pending.is_empty() {
        println!("No pending suggestions to review.");
        return Ok(());
    }

    println!("{} pending suggestions:\n", pending.len());
    for (i, (issue_row, suggestion)) in pending.iter().enumerate() {
        print_suggestion(i + 1, pending.len(), issue_row, suggestion);
        println!();
    }
    Ok(())
}

/// Maximum number of rendered terminal lines for issue body display.
const BODY_MAX_LINES: usize = 12;

/// Build a dim `MadSkin` for rendering issue body markdown.
fn body_skin() -> termimad::MadSkin {
    use termimad::crossterm::style::Color;
    let mut skin = termimad::MadSkin::default();
    skin.paragraph.set_fg(Color::DarkGrey);
    skin.bold.set_fg(Color::Grey);
    skin.italic.set_fg(Color::DarkGrey);
    skin.inline_code.set_fg(Color::Grey);
    skin.code_block.set_fg(Color::Grey);
    skin
}

fn print_suggestion(
    index: usize,
    total: usize,
    issue: &db::StoredIssue,
    suggestion: &db::TriageSuggestion,
) {
    let current_labels = if issue.labels.is_empty() {
        "none".to_string()
    } else {
        issue.labels.join(", ")
    };
    let confidence = suggestion
        .confidence
        .map(|c| format!("{:.0}%", c * 100.0))
        .unwrap_or_else(|| "?".to_string());

    let dim = console::Style::new().dim();
    let green = console::Style::new().green();

    let issue_ref = format!("{}#{}", issue.repo, issue.number);
    let issue_url = format!("https://github.com/{}/issues/{}", issue.repo, issue.number);
    let issue_link = osc8_link(&issue_url, &issue_ref);

    println!("[{index}/{total}] {issue_link}: {}", issue.title);

    if !issue.body.is_empty() {
        let term_width = console::Term::stdout().size().1 as usize;
        let width = term_width.saturating_sub(2).max(40);

        let skin = body_skin();
        let rendered = termimad::FmtText::from(&skin, &issue.body, Some(width));
        let rendered = format!("{rendered}");
        let rendered = linkify_markdown_links(&rendered);

        println!("  {}", dim.apply_to("---"));
        for (i, line) in rendered.lines().enumerate() {
            if i >= BODY_MAX_LINES {
                println!("  {}", dim.apply_to("... (truncated)"));
                break;
            }
            println!("  {line}");
        }
        println!("  {}", dim.apply_to("---"));
    }

    println!("  Existing labels:  {current_labels}");
    println!(
        "  Suggested node:   {} (confidence: {confidence})",
        suggestion.suggested_node.as_deref().unwrap_or("none")
    );

    let existing: BTreeSet<&str> = issue.labels.iter().map(|s| s.as_str()).collect();
    let new_labels: Vec<&str> = suggestion
        .suggested_labels
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !existing.contains(s))
        .collect();
    if new_labels.is_empty() {
        println!("  Labels to add:    {}", dim.apply_to("(none)"));
    } else {
        println!(
            "  Labels to add:    {}",
            green.apply_to(new_labels.join(", "))
        );
    }
    if suggestion.is_tracking_issue {
        println!("  Tracking issue:   yes");
    }
    if suggestion.is_stale {
        println!("  Stale:            yes");
    }
    if issue.sub_issues_count > 0 {
        println!("  Sub-issues:       {}", issue.sub_issues_count);
    }
    if !suggestion.suggested_new_categories.is_empty() {
        println!(
            "  New categories:   {}",
            suggestion.suggested_new_categories.join(", ")
        );
    }
    if !suggestion.reasoning.is_empty() {
        println!("  Reasoning: {}", suggestion.reasoning);
    }
}

fn osc8_link(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

fn linkify_markdown_links(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find('[') {
        result.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find(']') {
            let text = &after_open[..close];
            let after_close = &after_open[close + 1..];
            if after_close.starts_with('(')
                && let Some(paren_close) = after_close.find(')')
            {
                let url = &after_close[1..paren_close];
                if url.starts_with("http://") || url.starts_with("https://") {
                    result.push_str(&osc8_link(url, text));
                    rest = &after_close[paren_close + 1..];
                    continue;
                }
            }
            result.push('[');
            rest = after_open;
        } else {
            result.push('[');
            rest = after_open;
        }
    }
    result.push_str(rest);
    linkify_bare_urls(&result)
}

fn linkify_bare_urls(input: &str) -> String {
    if !input.contains("https://") {
        return input.to_string();
    }
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        if rest.starts_with("\x1b]8;")
            && let Some(close_pos) = rest.find("\x1b]8;;\x1b\\")
        {
            let full_end = close_pos + "\x1b]8;;\x1b\\".len();
            result.push_str(&rest[..full_end]);
            rest = &rest[full_end..];
            continue;
        }
        if rest.starts_with("https://") {
            let url_end = rest
                .find(|c: char| c.is_whitespace() || c == ')' || c == '>' || c == ']')
                .unwrap_or(rest.len());
            let url = &rest[..url_end];
            result.push_str(&osc8_link(url, url));
            rest = &rest[url_end..];
        } else {
            let ch = rest.chars().next().unwrap();
            result.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    result
}

fn prompt_note(_term: &console::Term) -> Result<String> {
    let mut editor = Editor::<(), rustyline::history::DefaultHistory>::new()
        .map_err(|e| Error::Other(format!("readline error: {e}")))?;
    match editor.readline("  Note (why? press Enter to skip): ") {
        Ok(line) => Ok(line.trim().to_string()),
        Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(String::new()),
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

fn generate_inquire_question(
    org_root: &Path,
    issue: &db::StoredIssue,
    node_entries: &[NodeEntry],
) -> Result<String> {
    let org = Org::open(org_root)?;
    let triage_config: TriageConfig = org.domain_config::<TriageDomain>()?;
    let label_schema: LabelSchema = org.domain_config::<LabelsDomain>()?;

    let backend_str = triage_config.backend.as_deref().ok_or_else(|| {
        Error::Other("[triage].backend must be set to generate questions".to_string())
    })?;
    let backend = llm::LlmBackend::parse(backend_str)?;
    let config = llm::LlmConfig {
        backend,
        model: triage_config.model.clone(),
        effort: triage_config.effort.clone(),
        api_key_env: triage_config.api_key_env.clone(),
        thinking_budget: triage_config.thinking_budget,
    };
    let curated_labels = LabelsFile::read(org_root).unwrap_or_default();
    let catalog = llm::PromptCatalog {
        label_schema: &label_schema,
        curated_labels: &curated_labels,
    };
    llm::generate_question(issue, node_entries, catalog, &config).map_err(|e| e.into())
}

fn prompt_question(generated: &str) -> Result<String> {
    eprintln!("  Generated question:");
    for line in generated.lines() {
        eprintln!("    {line}");
    }
    let mut editor = Editor::<(), rustyline::history::DefaultHistory>::new()
        .map_err(|e| Error::Other(format!("readline error: {e}")))?;
    match editor.readline("  Edit (Enter to accept, or type replacement): ") {
        Ok(line) => {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                Ok(generated.to_string())
            } else {
                Ok(trimmed.to_string())
            }
        }
        Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(String::new()),
        Err(e) => Err(Error::Other(format!("readline error: {e}"))),
    }
}

fn prompt_stale_inquiry(
    term: &console::Term,
    org_root: &Path,
    issue: &db::StoredIssue,
    reasoning: &str,
) -> Result<String> {
    eprint!("  Post staleness inquiry on the issue? [y/N]: ");
    let ch = term.read_char()?;
    eprintln!("{ch}");
    if !ch.eq_ignore_ascii_case(&'y') {
        return Ok(String::new());
    }
    let org = Org::open(org_root)?;
    let triage_config: TriageConfig = org.domain_config::<TriageDomain>()?;
    let backend_str = triage_config.backend.as_deref().ok_or_else(|| {
        Error::Other("[triage].backend must be set to generate questions".to_string())
    })?;
    let backend = llm::LlmBackend::parse(backend_str)?;
    let config = llm::LlmConfig {
        backend,
        model: triage_config.model.clone(),
        effort: triage_config.effort.clone(),
        api_key_env: triage_config.api_key_env.clone(),
        thinking_budget: triage_config.thinking_budget,
    };
    let generated = llm::generate_stale_question(issue, reasoning, &config)?;
    prompt_question(&generated)
}

fn save_review_example(
    org_root: &Path,
    issue: &db::StoredIssue,
    suggestion: &db::TriageSuggestion,
    final_node: Option<&str>,
    final_labels: &[String],
    is_stale: bool,
    note: &str,
) {
    let body_excerpt = if issue.body.len() > 300 {
        let mut end = 300;
        while !issue.body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &issue.body[..end])
    } else {
        issue.body.clone()
    };

    let example = examples::TriageExample {
        issue_ref: format!("{}#{}", issue.repo, issue.number),
        title: issue.title.clone(),
        body_excerpt,
        original_node: suggestion.suggested_node.clone(),
        node: final_node.map(String::from),
        labels: final_labels.to_vec(),
        is_tracking_issue: suggestion.is_tracking_issue,
        is_stale,
        note: note.to_string(),
    };

    if let Err(e) = examples::append_example(org_root, example) {
        eprintln!("  (warning: failed to save example: {e})");
    }
}

fn prompt_modification(
    issue: &db::StoredIssue,
    suggestion: &db::TriageSuggestion,
    node_entries: &[NodeEntry],
    label_names: &[String],
) -> Result<(Option<String>, Vec<String>)> {
    let default_node = suggestion.suggested_node.as_deref().unwrap_or("");
    let merged = review::merge_labels(&issue.labels, &suggestion.suggested_labels);
    let default_labels = merged.join(", ");

    let node_helper = NodePathHelper::from_entries(node_entries);
    let mut node_editor =
        Editor::new().map_err(|e| Error::Other(format!("readline error: {e}")))?;
    node_editor.set_helper(Some(node_helper));

    let prompt = format!("  Node [{default_node}]: ");
    let node_input = match node_editor.readline(&prompt) {
        Ok(line) => line,
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            return Ok((suggestion.suggested_node.clone(), merged));
        }
        Err(e) => return Err(Error::Other(format!("readline error: {e}"))),
    };
    let node_input = node_input.trim();
    let node = if node_input.is_empty() {
        suggestion.suggested_node.clone()
    } else if node_input == "null" || node_input == "none" {
        None
    } else {
        Some(node_input.to_string())
    };

    let labels_helper = CommaCompleteHelper {
        items: label_names.to_vec(),
    };
    let mut labels_editor =
        Editor::new().map_err(|e| Error::Other(format!("readline error: {e}")))?;
    labels_editor.set_helper(Some(labels_helper));

    let prompt = format!("  Labels [{default_labels}]: ");
    let labels_input = match labels_editor.readline(&prompt) {
        Ok(line) => line,
        Err(
            rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
        ) => {
            return Ok((node, merged));
        }
        Err(e) => return Err(Error::Other(format!("readline error: {e}"))),
    };
    let labels_input = labels_input.trim();
    let labels = if labels_input.is_empty() {
        merged
    } else {
        labels_input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    Ok((node, labels))
}

// ---------------------------------------------------------------------------
// End of interactive review
// ---------------------------------------------------------------------------

pub fn run_reset(
    below: Option<f64>,
    node: Option<String>,
    issue: Option<String>,
    all: bool,
    unreviewed: bool,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    if let Some(threshold) = below {
        let deleted = db::delete_suggestions_below_confidence(&conn, threshold)?;
        println!(
            "Reset {deleted} suggestion(s) with confidence below {:.0}%",
            threshold * 100.0
        );
        if deleted > 0 {
            println!("Those issues are now untriaged and will be re-classified on the next run.");
        }
    } else if let Some(prefix) = node {
        let deleted = db::delete_suggestions_by_node_prefix(&conn, &prefix)?;
        println!("Reset {deleted} suggestion(s) under node \"{prefix}\"");
        if deleted > 0 {
            println!("Those issues are now untriaged and will be re-classified on the next run.");
        }
    } else if let Some(ref issue_ref_str) = issue {
        let issue_ref = IssueRef::parse(issue_ref_str)?;
        let deleted =
            db::delete_suggestion_by_issue(&conn, &issue_ref.repo_full(), issue_ref.number)?;
        if deleted > 0 {
            println!("Reset suggestion for {issue_ref_str}");
            println!("The issue is now untriaged and will be re-classified on the next run.");
        } else {
            println!("No suggestion found for {issue_ref_str}");
        }
    } else if all {
        let deleted = db::delete_all_suggestions(&conn)?;
        println!("Reset all {deleted} suggestion(s)");
        if deleted > 0 {
            println!("All issues are now untriaged and will be re-classified on the next run.");
        }
    } else if unreviewed {
        let deleted = db::delete_unreviewed_suggestions(&conn)?;
        println!("Reset {deleted} unreviewed/rejected suggestion(s)");
        if deleted > 0 {
            println!("Those issues are now untriaged and will be re-classified on the next run.");
            println!("Approved/modified suggestions have been preserved.");
        }
    } else {
        return Err(Error::Other(
            "specify one of --below <threshold>, --node <path>, --issue <ref>, --all, or --unreviewed".to_string(),
        ));
    }

    cache::refresh_all(&conn, &org_root)?;
    Ok(())
}

pub fn run_apply(dry_run: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;
    let conn = db::open_db(&org_root)?;

    apply::apply_all(&gh, &conn, &org_root, dry_run)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_decide(
    issue_ref_strs: Vec<String>,
    decision: String,
    all_pending: bool,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
    node: Option<String>,
    labels: Option<String>,
    note: Option<String>,
    question: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let decision_type = match decision.as_str() {
        "approve" | "reject" | "modify" | "stale" | "inquire" => decision.as_str(),
        other => {
            return Err(Error::Other(format!(
                "unknown decision '{other}', expected: approve, reject, modify, stale, inquire"
            )));
        }
    };

    if !all_pending && issue_ref_strs.is_empty() {
        return Err(Error::Other(
            "provide issue references or use --all-pending".to_string(),
        ));
    }

    if !all_pending && (min_confidence.is_some() || max_confidence.is_some()) {
        return Err(Error::Other(
            "--min-confidence and --max-confidence can only be used with --all-pending".to_string(),
        ));
    }

    if decision_type != "modify" && (node.is_some() || labels.is_some()) {
        return Err(Error::Other(
            "--node and --labels can only be used with --decision modify".to_string(),
        ));
    }

    if !matches!(decision_type, "inquire" | "stale") && question.is_some() {
        return Err(Error::Other(
            "--question can only be used with --decision inquire or --decision stale".to_string(),
        ));
    }
    if decision_type == "inquire" && question.is_none() {
        return Err(Error::Other(
            "--question is required with --decision inquire".to_string(),
        ));
    }

    // Resolve the list of issue refs to process
    let resolved_refs: Vec<String> = if all_pending {
        let pending = db::get_pending_suggestions_filtered(&conn, min_confidence, max_confidence)?;
        if pending.is_empty() {
            println!("No pending suggestions found");
            return Ok(());
        }
        pending
            .iter()
            .map(|(issue, _)| format!("{}#{}", issue.repo, issue.number))
            .collect()
    } else {
        issue_ref_strs
    };

    let note = note.unwrap_or_default();
    let parsed_labels: Option<Vec<String>> = labels.as_ref().map(|l| {
        l.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    let mut errors: Vec<String> = Vec::new();

    for issue_ref_str in &resolved_refs {
        if let Err(e) = decide_one(
            &conn,
            &org_root,
            issue_ref_str,
            decision_type,
            &node,
            &parsed_labels,
            &note,
            &question,
        ) {
            eprintln!("Error on {issue_ref_str}: {e}");
            errors.push(format!("{issue_ref_str}: {e}"));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "Failed on {} of {} issues:\n  {}",
            errors.len(),
            resolved_refs.len(),
            errors.join("\n  ")
        )))
    }
}

#[allow(clippy::too_many_arguments)]
fn decide_one(
    conn: &rusqlite::Connection,
    org_root: &Path,
    issue_ref_str: &str,
    decision_type: &str,
    node: &Option<String>,
    parsed_labels: &Option<Vec<String>>,
    note: &str,
    question: &Option<String>,
) -> Result<()> {
    let issue_ref = IssueRef::parse(issue_ref_str)?;
    let (issue, suggestion, existing_decision) =
        db::get_suggestion_by_issue(conn, &issue_ref.repo_full(), issue_ref.number)?
            .ok_or_else(|| Error::Other(format!("No suggestion found for {issue_ref_str}")))?;

    if let Some(ref d) = existing_decision
        && d.applied_at.is_some()
    {
        return Err(Error::Other(format!(
            "Decision for {issue_ref_str} has already been applied to GitHub. \
             Use `triage reset --issue {issue_ref_str}` to clear it first."
        )));
    }

    let now = Utc::now().to_rfc3339();

    match decision_type {
        "approve" => {
            let merged = {
                let mut seen = std::collections::BTreeSet::new();
                let mut out = Vec::new();
                for label in issue
                    .labels
                    .iter()
                    .chain(suggestion.suggested_labels.iter())
                {
                    if seen.insert(label.as_str()) {
                        out.push(label.clone());
                    }
                }
                out
            };
            db::insert_decision(
                conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "approved".to_string(),
                    final_node: suggestion.suggested_node.clone(),
                    final_labels: merged,
                    decided_at: now,
                    applied_at: None,
                    question: String::new(),
                },
            )?;
            println!("Approved {issue_ref_str}");
        }
        "reject" => {
            db::insert_decision(
                conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "rejected".to_string(),
                    final_node: None,
                    final_labels: vec![],
                    decided_at: now,
                    applied_at: None,
                    question: String::new(),
                },
            )?;
            save_decide_example(org_root, &issue, &suggestion, None, &[], false, note);
            println!("Rejected {issue_ref_str}");
        }
        "modify" => {
            let final_node = node.clone().or_else(|| suggestion.suggested_node.clone());
            let final_labels = parsed_labels
                .clone()
                .unwrap_or_else(|| suggestion.suggested_labels.clone());
            db::insert_decision(
                conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "modified".to_string(),
                    final_node: final_node.clone(),
                    final_labels: final_labels.clone(),
                    decided_at: now,
                    applied_at: None,
                    question: String::new(),
                },
            )?;
            save_decide_example(
                org_root,
                &issue,
                &suggestion,
                final_node.as_deref(),
                &final_labels,
                suggestion.is_stale,
                note,
            );
            println!("Modified {issue_ref_str}");
        }
        "stale" => {
            let q = question.as_deref().unwrap_or("").to_string();
            db::insert_decision(
                conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "stale".to_string(),
                    final_node: None,
                    final_labels: vec![],
                    decided_at: now,
                    applied_at: None,
                    question: q.clone(),
                },
            )?;
            save_decide_example(org_root, &issue, &suggestion, None, &[], true, note);
            if q.is_empty() {
                println!("Marked {issue_ref_str} as stale");
            } else {
                println!("Marked {issue_ref_str} as stale (inquiry will be posted on apply)");
            }
        }
        "inquire" => {
            let q = question.as_deref().unwrap_or("").to_string();
            db::insert_decision(
                conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "inquired".to_string(),
                    final_node: None,
                    final_labels: vec![],
                    decided_at: now,
                    applied_at: None,
                    question: q,
                },
            )?;
            println!("Inquired {issue_ref_str} (question will be posted on apply)");
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn save_decide_example(
    org_root: &Path,
    issue: &db::StoredIssue,
    suggestion: &db::TriageSuggestion,
    final_node: Option<&str>,
    final_labels: &[String],
    is_stale: bool,
    note: &str,
) {
    let body_excerpt = if issue.body.len() > 300 {
        let mut end = 300;
        while !issue.body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &issue.body[..end])
    } else {
        issue.body.clone()
    };

    let example = examples::TriageExample {
        issue_ref: format!("{}#{}", issue.repo, issue.number),
        title: issue.title.clone(),
        body_excerpt,
        original_node: suggestion.suggested_node.clone(),
        node: final_node.map(String::from),
        labels: final_labels.to_vec(),
        is_tracking_issue: suggestion.is_tracking_issue,
        is_stale,
        note: note.to_string(),
    };

    if let Err(e) = examples::append_example(org_root, example) {
        eprintln!("(warning: failed to save example: {e})");
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_suggestions(
    issues: Vec<i64>,
    node: Option<String>,
    repo: Option<String>,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
    status: Option<String>,
    tracking_only: bool,
    unclassified: bool,
    stale_only: bool,
    sort: String,
    limit: usize,
    format: String,
    body_max: usize,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let status_filter = status
        .as_deref()
        .map(|s| match s {
            "pending" => Ok(db::SuggestionStatus::Pending),
            "approved" => Ok(db::SuggestionStatus::Approved),
            "rejected" => Ok(db::SuggestionStatus::Rejected),
            "applied" => Ok(db::SuggestionStatus::Applied),
            other => Err(Error::Other(format!(
                "unknown status '{other}', expected: pending, approved, rejected, applied"
            ))),
        })
        .transpose()?;

    let sort_field = match sort.as_str() {
        "confidence" => db::SuggestionSort::Confidence,
        "node" => db::SuggestionSort::Node,
        "repo" => db::SuggestionSort::Repo,
        other => {
            return Err(Error::Other(format!(
                "unknown sort '{other}', expected: confidence, node, repo"
            )));
        }
    };

    let filters = db::SuggestionFilters {
        issue_numbers: issues,
        node_prefix: node,
        repo,
        min_confidence,
        max_confidence,
        status: status_filter,
        tracking_only,
        unclassified,
        stale_only,
        sort: sort_field,
        limit,
    };

    let results = db::get_suggestions_filtered(&conn, &filters)?;

    match fmt {
        OutputFormat::Json => {
            let json_rows: Vec<serde_json::Value> = results
                .iter()
                .map(|(issue, sug)| {
                    let body = if body_max > 0 && issue.body.len() > body_max {
                        let mut end = body_max;
                        while !issue.body.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}...", &issue.body[..end])
                    } else {
                        issue.body.clone()
                    };
                    serde_json::json!({
                        "suggestion_id": sug.id,
                        "issue_ref": format!("{}#{}", issue.repo, issue.number),
                        "title": issue.title,
                        "repo": issue.repo,
                        "number": issue.number,
                        "body": body,
                        "current_labels": issue.labels,
                        "sub_issues_count": issue.sub_issues_count,
                        "suggested_node": sug.suggested_node,
                        "suggested_labels": sug.suggested_labels,
                        "confidence": sug.confidence,
                        "reasoning": sug.reasoning,
                        "is_tracking_issue": sug.is_tracking_issue,
                        "is_stale": sug.is_stale,
                        "suggested_new_categories": sug.suggested_new_categories,
                        "llm_backend": sug.llm_backend,
                        "created_at": sug.created_at,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json_rows)
                    .map_err(|e| Error::Other(e.to_string()))?
            );
        }
        OutputFormat::Summary => {
            if results.is_empty() {
                println!("No suggestions match the given filters.");
                return Ok(());
            }

            const CONFIDENCE_THRESHOLD: f64 = 0.80;

            let (auto, uncertain): (Vec<_>, Vec<_>) = results.iter().partition(|(_, sug)| {
                sug.confidence.unwrap_or(0.0) >= CONFIDENCE_THRESHOLD
                    && sug.suggested_node.is_some()
            });

            println!("=== AUTO-APPROVE ({}) ===", auto.len());
            for (issue, sug) in &auto {
                let issue_ref = format!("{}#{}", issue.repo, issue.number);
                let conf = sug.confidence.unwrap_or(0.0);
                let node = sug.suggested_node.as_deref().unwrap_or("(none)");
                let title: String = issue.title.chars().take(70).collect();
                println!("  {issue_ref} ({conf:.2}) -> {node} | {title}");
            }

            println!("\n=== NEEDS REVIEW ({}) ===", uncertain.len());
            for (issue, sug) in &uncertain {
                let issue_ref = format!("{}#{}", issue.repo, issue.number);
                let conf = sug.confidence.unwrap_or(0.0);
                let node = sug.suggested_node.as_deref().unwrap_or("(none)");
                let stale = if sug.is_stale { " [STALE]" } else { "" };
                let title: String = issue.title.chars().take(70).collect();
                println!("  {issue_ref} ({conf:.2}) -> {node}{stale} | {title}");
                if !sug.suggested_new_categories.is_empty() {
                    println!(
                        "    new categories: {}",
                        sug.suggested_new_categories.join(", ")
                    );
                }
                let reasoning: String = sug.reasoning.chars().take(150).collect();
                println!("    reasoning: {reasoning}");
            }

            println!(
                "\n{} suggestion(s): {} auto-approve, {} needs review",
                results.len(),
                auto.len(),
                uncertain.len()
            );
        }
        OutputFormat::Table => {
            if results.is_empty() {
                println!("No suggestions match the given filters.");
                return Ok(());
            }
            println!(
                "{:<30} {:<55} {:<25} {:>6}",
                "ISSUE", "TITLE", "NODE", "CONF"
            );
            println!("{}", "-".repeat(120));
            for (issue, sug) in &results {
                let issue_ref = format!("{}#{}", issue.repo, issue.number);
                let title: String = issue.title.chars().take(53).collect();
                let node = sug.suggested_node.as_deref().unwrap_or("(unclassified)");
                let conf = sug
                    .confidence
                    .map(|c| format!("{:.0}%", c * 100.0))
                    .unwrap_or_else(|| "\u{2014}".to_string());
                println!("{:<30} {:<55} {:<25} {:>6}", issue_ref, title, node, conf);
            }
            println!("\n{} suggestion(s)", results.len());
        }
        OutputFormat::Refs => {
            for (issue, _) in &results {
                println!("{}#{}", issue.repo, issue.number);
            }
        }
    }
    Ok(())
}

pub fn run_decisions(
    status: Option<String>,
    unapplied: bool,
    node: Option<String>,
    repo: Option<String>,
    limit: usize,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let filters = db::DecisionFilters {
        status,
        unapplied,
        node_prefix: node,
        repo,
        limit,
    };

    let results = db::get_decisions_filtered(&conn, &filters)?;

    if fmt == OutputFormat::Json {
        let json_rows: Vec<serde_json::Value> = results
            .iter()
            .map(|(issue, dec)| {
                let mut obj = serde_json::json!({
                    "issue_ref": format!("{}#{}", issue.repo, issue.number),
                    "title": issue.title,
                    "repo": issue.repo,
                    "number": issue.number,
                    "decision": dec.decision,
                    "final_node": dec.final_node,
                    "final_labels": dec.final_labels,
                    "decided_at": dec.decided_at,
                    "applied_at": dec.applied_at,
                });
                if !dec.question.is_empty() {
                    obj["question"] = serde_json::json!(dec.question);
                }
                obj
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json_rows).map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        if results.is_empty() {
            println!("No decisions match the given filters.");
            return Ok(());
        }
        let applied_hdr = "APPLIED";
        println!(
            "{:<30} {:<40} {:<10} {:<25} {}",
            "ISSUE", "TITLE", "DECISION", "NODE", applied_hdr
        );
        println!("{}", "-".repeat(115));
        for (issue, dec) in &results {
            let issue_ref = format!("{}#{}", issue.repo, issue.number);
            let title: String = issue.title.chars().take(38).collect();
            let node = dec.final_node.as_deref().unwrap_or("\u{2014}");
            let applied = dec.applied_at.as_deref().map(|_| "yes").unwrap_or("no");
            println!(
                "{:<30} {:<40} {:<10} {:<25} {}",
                issue_ref, title, dec.decision, node, applied
            );
        }
        println!("\n{} decision(s)", results.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Examples subcommands
// ---------------------------------------------------------------------------

pub fn run_examples_list() -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let exs = examples::load_examples(&org_root)?;
    if exs.is_empty() {
        println!("No classification examples found.");
        println!(
            "Examples are auto-saved when you reject or modify suggestions during `triage review -i`."
        );
        return Ok(());
    }
    println!("{} classification example(s):\n", exs.len());
    for (i, ex) in exs.iter().enumerate() {
        println!("  {}. {} — {}", i + 1, ex.issue_ref, ex.title,);
        if let Some(orig) = &ex.original_node {
            let final_node = ex.node.as_deref().unwrap_or("(none)");
            println!("     LLM suggested: {orig}  ->  corrected to: {final_node}");
        } else {
            let node = ex.node.as_deref().unwrap_or("(none)");
            println!("     Node: {node}");
        }
        if !ex.labels.is_empty() {
            println!("     Labels: {}", ex.labels.join(", "));
        }
        if !ex.note.is_empty() {
            println!("     Note: {}", ex.note);
        }
        println!();
    }
    Ok(())
}

pub fn run_examples_export(status: Option<String>, limit: Option<usize>) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let statuses: Vec<&str> = match status.as_deref() {
        Some(s) => s.split(',').collect(),
        None => vec!["rejected", "modified"],
    };
    let limit = limit.unwrap_or(50);

    let rows = db::get_decisions_with_original(&conn, &statuses, limit)?;
    if rows.is_empty() {
        println!("No decisions with status {} found.", statuses.join("/"));
        return Ok(());
    }

    let mut existing = examples::load_examples(&org_root)?;
    let existing_refs: std::collections::HashSet<String> =
        existing.iter().map(|e| e.issue_ref.clone()).collect();

    let mut added = 0usize;
    for (issue, suggestion, decision) in &rows {
        let issue_ref = format!("{}#{}", issue.repo, issue.number);
        if existing_refs.contains(&issue_ref) {
            continue;
        }

        let body_excerpt = if issue.body.len() > 300 {
            let mut end = 300;
            while !issue.body.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...", &issue.body[..end])
        } else {
            issue.body.clone()
        };

        existing.push(examples::TriageExample {
            issue_ref,
            title: issue.title.clone(),
            body_excerpt,
            original_node: suggestion.suggested_node.clone(),
            node: decision.final_node.clone(),
            labels: decision.final_labels.clone(),
            is_tracking_issue: suggestion.is_tracking_issue,
            is_stale: suggestion.is_stale,
            note: String::new(),
        });
        added += 1;
    }

    if added == 0 {
        println!("All matching decisions are already in triage-examples.toml.");
    } else {
        examples::save_examples(&org_root, &existing)?;
        println!(
            "Exported {added} example(s) to triage-examples.toml ({} total).",
            existing.len()
        );
    }
    Ok(())
}

pub fn run_examples_remove(issue_ref: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let mut exs = examples::load_examples(&org_root)?;
    let before = exs.len();
    exs.retain(|e| e.issue_ref != issue_ref);
    if exs.len() == before {
        println!("No example found for {issue_ref}.");
    } else {
        examples::save_examples(&org_root, &exs)?;
        println!("Removed example for {issue_ref} ({} remaining).", exs.len());
    }
    Ok(())
}

pub fn run_summary(repo: Option<String>, format: String) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let repo_ref = repo.as_deref();
    let distribution = db::get_confidence_distribution(&conn, repo_ref)?;
    let nodes = db::get_node_breakdown(&conn, repo_ref)?;
    let votes = db::get_new_category_votes(&conn, repo_ref)?;

    if fmt == OutputFormat::Json {
        let obj = serde_json::json!({
            "confidence_distribution": distribution,
            "node_breakdown": nodes,
            "suggested_categories": votes,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&obj).map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        println!("Confidence distribution:");
        println!("  {:<10} {:>5}  {:>6}", "BAND", "COUNT", "%");
        println!("  {}", "-".repeat(25));
        for band in &distribution {
            println!(
                "  {:<10} {:>5}  {:>5.1}%",
                band.label, band.count, band.percentage
            );
        }

        println!("\nNode breakdown:");
        println!(
            "  {:<30} {:>5}  {:>5}  {:>5}  {:>5}",
            "NODE", "COUNT", "AVG", "MIN", "MAX"
        );
        println!("  {}", "-".repeat(55));
        for node in &nodes {
            let name = node.node.as_deref().unwrap_or("(unclassified)");
            println!(
                "  {:<30} {:>5}  {:>4.0}%  {:>4.0}%  {:>4.0}%",
                name,
                node.count,
                node.avg_confidence * 100.0,
                node.min_confidence * 100.0,
                node.max_confidence * 100.0
            );
        }

        if !votes.is_empty() {
            println!("\nSuggested new categories:");
            let issues_hdr = "ISSUES";
            println!("  {:<30} {:>5}  {}", "CATEGORY", "VOTES", issues_hdr);
            println!("  {}", "-".repeat(70));
            for vote in &votes {
                let refs = vote.issue_refs.join(", ");
                println!("  {:<30} {:>5}  {}", vote.category, vote.vote_count, refs);
            }
        }
    }
    Ok(())
}

pub fn run_categories_list(min_votes: usize, format: String) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let dismissed = categories::read_dismissed(&org_root)?;
    let mut votes = db::get_new_category_votes(&conn, None)?;
    votes.retain(|v| {
        !categories::is_dismissed(&dismissed, &v.category) && v.vote_count >= min_votes
    });

    if fmt == OutputFormat::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&votes).map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        if votes.is_empty() {
            println!("No suggested new categories.");
            return Ok(());
        }
        println!("Suggested categories:");
        for vote in &votes {
            let refs: Vec<&str> = vote.issue_refs.iter().take(5).map(|s| s.as_str()).collect();
            println!(
                "  {:<30} {} vote(s)  {}",
                vote.category,
                vote.vote_count,
                refs.join(", ")
            );
        }
    }
    Ok(())
}

pub fn run_categories_dismiss(path: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let was_new = categories::dismiss(&org_root, &path)?;
    if was_new {
        println!("Dismissed category '{path}'");
    } else {
        println!("Category '{path}' was already dismissed");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_categories_apply(
    path: String,
    name: String,
    description: String,
    reclassify: bool,
    reclassify_backend: Option<String>,
    reclassify_model: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    ensure_ancestors_exist(&org_root, &path, false)?;

    crate::cli::node::create_node_full(
        &org_root,
        &path,
        Some(&name),
        Some(&description),
        None,
        None,
        &[],
        &[],
        "active",
        None,
    )?;

    let deleted = db::delete_suggestions_for_reclassify(&conn, &path)?;
    println!("Created node '{path}'. Reset {deleted} suggestion(s).");

    if reclassify && deleted > 0 {
        let nodes = walk_nodes(&org_root)?;
        let org = Org::open(&org_root)?;
        let triage_config: TriageConfig = org.domain_config::<TriageDomain>()?;
        let label_schema: LabelSchema = org.domain_config::<LabelsDomain>()?;
        let curated_labels = LabelsFile::read(&org_root)?;
        let config =
            resolve_classify_config(reclassify_backend, reclassify_model, None, &triage_config)?;
        let triage_examples = examples::load_examples(&org_root)?;
        let count = llm::triage_issues(
            &conn,
            &nodes,
            llm::PromptCatalog {
                label_schema: &label_schema,
                curated_labels: &curated_labels,
            },
            &triage_examples,
            &config,
            10,
            1,
            None,
            None,
        )?;
        println!("Reclassified {count} issues.");
        cache::refresh_all(&conn, &org_root)?;
    } else if deleted > 0 {
        println!("Run 'armitage triage classify' to reclassify affected issues.");
        cache::refresh_all(&conn, &org_root)?;
    }
    Ok(())
}

pub fn run_categories_refine(
    backend: Option<String>,
    model: Option<String>,
    auto_accept: bool,
    min_votes: usize,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;
    let nodes = walk_nodes(&org_root)?;
    let org = Org::open(&org_root)?;
    let triage_config: TriageConfig = org.domain_config::<TriageDomain>()?;

    let dismissed = categories::read_dismissed(&org_root)?;
    let mut votes = db::get_new_category_votes(&conn, None)?;
    votes.retain(|v| {
        !categories::is_dismissed(&dismissed, &v.category) && v.vote_count >= min_votes
    });

    if votes.is_empty() {
        println!("No category suggestions with >= {min_votes} votes.");
        return Ok(());
    }

    println!("Found {} category suggestions to refine.", votes.len());

    let config = resolve_classify_config(backend, model, None, &triage_config)?;
    let response = llm::refine_categories(&nodes, &votes, &config)?;

    if response.groups.is_empty() {
        println!("LLM found no groups to consolidate.");
        return Ok(());
    }

    println!("LLM proposed {} group(s).\n", response.groups.len());

    let vote_map: std::collections::HashMap<&str, usize> = votes
        .iter()
        .map(|v| (v.category.as_str(), v.vote_count))
        .collect();

    let mut applied = 0usize;
    let mut dismissed_count = 0usize;
    let mut skipped = 0usize;
    let total = response.groups.len();

    for (i, group) in response.groups.iter().enumerate() {
        let mut group = group.clone();

        loop {
            let raw_display: Vec<String> = group
                .raw_suggestions
                .iter()
                .map(|s| {
                    let count = vote_map.get(s.as_str()).copied().unwrap_or(0);
                    format!("{s} ({count} votes)")
                })
                .collect();

            println!("--- Group {}/{total} ---", i + 1);
            println!("  Raw suggestions: {}", raw_display.join(", "));

            if let Some(ref covered) = group.covered_by {
                println!("  LLM says: Covered by \"{covered}\"");
                println!("  Reason:   {}", group.reason);
                println!();

                if auto_accept {
                    for cat in &group.raw_suggestions {
                        categories::dismiss(&org_root, cat)?;
                    }
                    dismissed_count += 1;
                    break;
                }

                let choice =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Action")
                        .items(["Dismiss suggestions", "Skip", "Quit"])
                        .default(0)
                        .interact()
                        .map_err(|e| Error::Other(e.to_string()))?;

                match choice {
                    0 => {
                        for cat in &group.raw_suggestions {
                            categories::dismiss(&org_root, cat)?;
                        }
                        dismissed_count += 1;
                        println!("  Dismissed.\n");
                        break;
                    }
                    1 => {
                        skipped += 1;
                        println!("  Skipped.\n");
                        break;
                    }
                    _ => {
                        println!("  Quit.\n");
                        print_refine_summary(applied, dismissed_count, skipped);
                        cache::refresh_all(&conn, &org_root)?;
                        return Ok(());
                    }
                }
            } else {
                let path = group.proposed_path.as_deref().unwrap_or("???");
                let name = group.proposed_name.as_deref().unwrap_or("???");
                let desc = group.proposed_description.as_deref().unwrap_or("");
                println!("  LLM says: Create new node");
                println!("  Path:        {path}");
                println!("  Name:        {name}");
                println!("  Description: {desc}");
                println!("  Reason:      {}", group.reason);
                println!();

                if auto_accept {
                    if let (Some(p), Some(n), Some(d)) = (
                        &group.proposed_path,
                        &group.proposed_name,
                        &group.proposed_description,
                    ) {
                        apply_refined_group(
                            &org_root,
                            &conn,
                            p,
                            n,
                            d,
                            &group.raw_suggestions,
                            auto_accept,
                        )?;
                        applied += 1;
                    }
                    break;
                }

                let choice =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Action")
                        .items(["Apply", "Skip", "Refine", "Quit"])
                        .default(0)
                        .interact()
                        .map_err(|e| Error::Other(e.to_string()))?;

                match choice {
                    0 => {
                        if let (Some(p), Some(n), Some(d)) = (
                            &group.proposed_path,
                            &group.proposed_name,
                            &group.proposed_description,
                        ) {
                            apply_refined_group(
                                &org_root,
                                &conn,
                                p,
                                n,
                                d,
                                &group.raw_suggestions,
                                auto_accept,
                            )?;
                            applied += 1;
                        }
                        break;
                    }
                    1 => {
                        skipped += 1;
                        println!("  Skipped.\n");
                        break;
                    }
                    2 => {
                        let feedback: String = dialoguer::Input::with_theme(
                            &dialoguer::theme::ColorfulTheme::default(),
                        )
                        .with_prompt("What should change?")
                        .interact_text()
                        .map_err(|e| Error::Other(e.to_string()))?;
                        match llm::refine_category_group(&nodes, &group, &feedback, &config) {
                            Ok(updated) => {
                                group = updated;
                                println!();
                                continue;
                            }
                            Err(e) => {
                                eprintln!("  LLM refinement failed: {e}");
                                continue;
                            }
                        }
                    }
                    _ => {
                        println!("  Quit.\n");
                        print_refine_summary(applied, dismissed_count, skipped);
                        cache::refresh_all(&conn, &org_root)?;
                        return Ok(());
                    }
                }
            }
        }
    }

    print_refine_summary(applied, dismissed_count, skipped);
    cache::refresh_all(&conn, &org_root)?;
    Ok(())
}

fn apply_refined_group(
    org_root: &Path,
    conn: &rusqlite::Connection,
    path: &str,
    name: &str,
    description: &str,
    raw_suggestions: &[String],
    auto_accept: bool,
) -> Result<()> {
    ensure_ancestors_exist(org_root, path, auto_accept)?;
    crate::cli::node::create_node_full(
        org_root,
        path,
        Some(name),
        Some(description),
        None,
        None,
        &[],
        &[],
        "active",
        None,
    )?;

    let mut total_reset = 0;
    for cat in raw_suggestions {
        total_reset += db::delete_suggestions_for_reclassify(conn, cat)?;
    }
    println!("  Created node '{path}'. Reset {total_reset} suggestion(s).\n");
    Ok(())
}

fn ensure_ancestors_exist(org_root: &Path, node_path: &str, auto_accept: bool) -> Result<()> {
    let segments: Vec<&str> = node_path.split('/').collect();
    for depth in 1..segments.len() {
        let ancestor = segments[..depth].join("/");
        let ancestor_dir = org_root.join(&ancestor);
        if ancestor_dir.join("node.toml").exists() {
            continue;
        }

        let leaf = segments[depth - 1];
        println!("  Parent node '{ancestor}' does not exist and must be created.");

        let (name, description) = if auto_accept {
            let name = leaf
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string() + &leaf[1..])
                .unwrap_or_else(|| leaf.to_string());
            (name, String::new())
        } else {
            let default_name = leaf
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string() + &leaf[1..])
                .unwrap_or_else(|| leaf.to_string());

            let name: String =
                dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt(format!("  Name for '{ancestor}'"))
                    .default(default_name)
                    .interact_text()
                    .map_err(|e| Error::Other(e.to_string()))?;

            let description: String =
                dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt(format!("  Description for '{ancestor}'"))
                    .default(String::new())
                    .allow_empty(true)
                    .interact_text()
                    .map_err(|e| Error::Other(e.to_string()))?;

            (name, description)
        };

        crate::cli::node::create_node_full(
            org_root,
            &ancestor,
            Some(&name),
            Some(&description),
            None,
            None,
            &[],
            &[],
            "active",
            None,
        )?;
        println!("  Created parent node '{ancestor}'.");
    }
    Ok(())
}

fn print_refine_summary(applied: usize, dismissed: usize, skipped: usize) {
    println!(
        "Summary: applied {applied} node(s), dismissed {dismissed} group(s), skipped {skipped} group(s)."
    );
    if applied > 0 {
        println!("Run 'armitage triage classify' to reclassify affected issues.");
    }
}

pub fn run_status(format: String) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let counts = db::get_pipeline_counts(&conn)?;

    if fmt == OutputFormat::Json {
        println!(
            "{}",
            serde_json::to_string(&counts).map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        println!("Triage pipeline:");
        println!("  Fetched issues:       {}", counts.total_fetched);
        println!("  Untriaged:            {}", counts.untriaged);
        println!("  Pending review:       {}", counts.pending_review);
        println!("  Approved (unapplied): {}", counts.approved_unapplied);
        println!("  Applied:              {}", counts.applied);
        if counts.stale > 0 {
            println!("  Stale:                {}", counts.stale);
        }
    }
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

fn build_noninteractive_selection(
    org_root: &Path,
    session: &label_import::LabelImportSession,
    local: &LabelsFile,
    all_new: bool,
    update_drifted: bool,
    names: &[String],
    exclude_names: &[String],
) -> BTreeSet<String> {
    let mut selected = BTreeSet::new();

    let renamed_old_names: BTreeSet<String> = rename::read_rename_ledger(org_root)
        .map(|ledger| {
            let local_names: BTreeSet<&str> =
                local.labels.iter().map(|l| l.name.as_str()).collect();
            ledger
                .renames
                .into_iter()
                .filter(|r| local_names.contains(r.new_name.as_str()))
                .map(|r| r.old_name)
                .collect()
        })
        .unwrap_or_default();

    if all_new {
        selected.extend(
            session
                .candidates
                .iter()
                .filter(|candidate| candidate.status == label_import::CandidateStatus::New)
                .filter(|candidate| !renamed_old_names.contains(&candidate.name))
                .map(|candidate| candidate.name.clone()),
        );
    }

    if update_drifted {
        selected.extend(
            session
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.status == label_import::CandidateStatus::MetadataDrift
                })
                .map(|candidate| candidate.name.clone()),
        );
    }

    selected.extend(names.iter().cloned());

    for excluded in exclude_names {
        selected.remove(excluded);
    }

    if !renamed_old_names.is_empty() {
        let skipped = session
            .candidates
            .iter()
            .filter(|c| c.status == label_import::CandidateStatus::New)
            .filter(|c| renamed_old_names.contains(&c.name))
            .count();
        if skipped > 0 {
            tracing::debug!(
                skipped = skipped,
                "skipped new candidates with pending renames to existing labels"
            );
        }
    }

    selected
}

fn run_labels_merge_interactive(
    org_root: &Path,
    session: &label_import::LabelImportSession,
    global_prefer_repo: Option<String>,
) -> Result<()> {
    let mut editor = DefaultEditor::new()
        .map_err(|err| Error::Other(format!("interactive prompt failed: {err}")))?;
    let mut local = LabelsFile::read(org_root)?;
    let defaults = label_import::default_interactive_selection(session);
    let mut applied = 0usize;

    for candidate in &session.candidates {
        let default_selected = defaults.contains(&candidate.name);
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
            if default_selected { "Y/n" } else { "y/N" }
        );
        let input = read_line(&mut editor, &prompt)?;
        let accept = parse_yes_no(&input, default_selected);
        if !accept {
            continue;
        }

        let chosen_variant =
            match label_import::choose_remote_variant(candidate, global_prefer_repo.as_deref()) {
                Ok(variant) => variant.clone(),
                Err(err)
                    if candidate.status == label_import::CandidateStatus::DuplicateRemote
                        && err.to_string().contains("prefer-repo") =>
                {
                    let repo = prompt_repo_choice(&mut editor, candidate)?;
                    label_import::choose_remote_variant(candidate, Some(&repo))?.clone()
                }
                Err(err) => return Err(err.into()),
            };

        local.upsert(LabelDef {
            name: candidate.name.clone(),
            description: chosen_variant.description,
            color: chosen_variant.color,
            repos: vec![],
            pinned: false,
        });
        applied += 1;
    }

    local.write(org_root)?;
    println!("Updated labels.toml with {applied} label(s)");
    Ok(())
}

fn prompt_repo_choice(
    editor: &mut DefaultEditor,
    candidate: &label_import::LabelImportCandidate,
) -> Result<String> {
    let options = candidate
        .remote_variants
        .iter()
        .map(|variant| variant.repo.clone())
        .collect::<Vec<_>>();
    let prompt = format!(
        "Choose repo for {} [{}]: ",
        candidate.name,
        options.join(", ")
    );

    loop {
        let input = read_line(editor, &prompt)?;
        let trimmed = input.trim();
        if options.iter().any(|repo| repo == trimmed) {
            return Ok(trimmed.to_string());
        }
        println!("Enter one of: {}", options.join(", "));
    }
}

fn confirm(prompt: &str) -> Result<bool> {
    let mut editor = DefaultEditor::new()
        .map_err(|err| Error::Other(format!("interactive prompt failed: {err}")))?;
    let input = read_line(&mut editor, prompt)?;
    Ok(parse_yes_no(&input, false))
}

fn read_line(editor: &mut DefaultEditor, prompt: &str) -> Result<String> {
    editor.readline(prompt).map_err(|err| match err {
        ReadlineError::Interrupted | ReadlineError::Eof => {
            Error::Other("interactive label merge cancelled".to_string())
        }
        other => Error::Other(format!("interactive prompt failed: {other}")),
    })
}

fn parse_yes_no(input: &str, default: bool) -> bool {
    match input.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        "n" | "no" => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_requires_backend() {
        let err = resolve_classify_config(None, None, None, &TriageConfig::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("backend"));
    }

    #[test]
    fn resolve_config_requires_model_for_gemini() {
        let triage = TriageConfig {
            backend: Some("gemini".to_string()),
            model: None,
            effort: None,
            api_key_env: None,
            thinking_budget: None,
            labels: None,
        };
        let err = resolve_classify_config(None, None, None, &triage)
            .unwrap_err()
            .to_string();
        assert!(err.contains("model"));
        assert!(err.contains("gemini"));
    }

    #[test]
    fn resolve_config_uses_cli_overrides() {
        let triage = TriageConfig {
            backend: Some("claude".to_string()),
            model: Some("sonnet".to_string()),
            effort: Some("medium".to_string()),
            api_key_env: None,
            thinking_budget: None,
            labels: None,
        };
        let cfg = resolve_classify_config(
            Some("gemini".to_string()),
            Some("gemini-2.5-flash".to_string()),
            None,
            &triage,
        )
        .unwrap();

        assert_eq!(cfg.backend.name(), "gemini");
        assert_eq!(cfg.model.as_deref(), Some("gemini-2.5-flash"));
        assert_eq!(cfg.effort.as_deref(), Some("medium"));
    }

    #[test]
    fn latest_session_is_used_when_session_flag_is_absent() {
        let latest = resolve_merge_session_id(
            None,
            &[
                "20260403T120000Z".to_string(),
                "20260403T130000Z".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(latest, "20260403T130000Z");
    }
}
