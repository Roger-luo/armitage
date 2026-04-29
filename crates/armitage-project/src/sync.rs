use std::collections::HashMap;

use armitage_core::node::IssueRef;
use armitage_core::org::Org;
use armitage_github::Gh;
use chrono::{Datelike, NaiveDate};
use tracing::warn;

use crate::cache::{CachedField, FieldCache, read_field_cache, write_field_cache};
use crate::config::{GitHubProjectConfig, ProjectDomain, StatusValues};
use crate::error::{Error, Result};
use crate::graphql::{
    ProjectItem, add_item_to_project, fetch_field_cache, fetch_issue_node_id, fetch_project_items,
    update_date_field, update_single_select_field,
};

pub struct SyncStats {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: usize,
}

pub fn sync(gh: &Gh, org: &Org, dry_run: bool, node_path: Option<&str>) -> Result<SyncStats> {
    let cfg = org.domain_config::<ProjectDomain>()?;

    if cfg.org.is_empty() || cfg.number == 0 {
        return Err(Error::Other(
            "github_project.org and github_project.number must be set in armitage.toml".into(),
        ));
    }

    let cache = load_or_fetch_cache(gh, org, &cfg)?;
    tracing::debug!(project_id = %cache.project_id, "project metadata loaded");

    // Fetch all items currently on the board so we can do no-op detection.
    println!("Fetching current project items…");
    let existing: HashMap<String, ProjectItem> = fetch_project_items(gh, &cfg.org, cfg.number)?;

    let all_nodes = org.walk_nodes()?;
    let nodes: Vec<_> = match node_path {
        Some(path) => all_nodes
            .into_iter()
            .filter(|e| e.path == path || e.path.starts_with(&format!("{path}/")))
            .collect(),
        None => all_nodes,
    };

    let mut stats = SyncStats {
        added: 0,
        updated: 0,
        skipped: 0,
        errors: 0,
    };

    for entry in &nodes {
        let node = &entry.node;
        let (Some(issue_str), Some(timeline)) = (&node.track, &node.timeline) else {
            continue;
        };

        let issue_ref = match IssueRef::parse(issue_str) {
            Ok(r) => r,
            Err(e) => {
                warn!(node = %entry.path, "invalid track: {e}");
                stats.errors += 1;
                continue;
            }
        };

        let canonical = format!(
            "{}/{}#{}",
            issue_ref.owner,
            strip_qualifier(&issue_ref.repo),
            issue_ref.number
        );

        let result = sync_one(
            gh,
            &cfg,
            &cache,
            &existing,
            &canonical,
            &issue_ref,
            timeline.start,
            timeline.end,
            dry_run,
            &entry.path,
        );

        match result {
            Ok(SyncOneResult::Added) => {
                stats.added += 1;
            }
            Ok(SyncOneResult::Updated) => {
                stats.updated += 1;
            }
            Ok(SyncOneResult::Skipped) => {
                stats.skipped += 1;
            }
            Err(e) => {
                eprintln!("  error syncing {canonical}: {e}");
                stats.errors += 1;
            }
        }
    }

    Ok(stats)
}

enum SyncOneResult {
    Added,
    Updated,
    Skipped,
}

#[allow(clippy::too_many_arguments)]
fn sync_one(
    gh: &Gh,
    cfg: &GitHubProjectConfig,
    cache: &FieldCache,
    existing: &HashMap<String, ProjectItem>,
    canonical: &str,
    issue_ref: &IssueRef,
    start: NaiveDate,
    end: NaiveDate,
    dry_run: bool,
    node_path: &str,
) -> Result<SyncOneResult> {
    let today = chrono::Local::now().date_naive();

    // --- Determine desired status (never "in_progress" automatically) ---
    let desired_status = desired_status(end, today, &cfg.status_values);

    // --- Check if already on the board ---
    let (item_id, needs_add, current_start, current_end, current_status) =
        if let Some(item) = existing.get(canonical) {
            let cur_start = item
                .field_values
                .get(cfg.start_date_field.as_deref().unwrap_or(""))
                .cloned();
            let cur_end = item
                .field_values
                .get(cfg.target_date_field.as_deref().unwrap_or(""))
                .cloned();
            let cur_status = item
                .field_values
                .get(cfg.status_field.as_deref().unwrap_or(""))
                .cloned();
            (item.item_id.clone(), false, cur_start, cur_end, cur_status)
        } else {
            (String::new(), true, None, None, None)
        };

    // --- Determine what needs updating ---
    let start_str = start.format("%Y-%m-%d").to_string();
    let end_str = end.format("%Y-%m-%d").to_string();

    let need_start = cfg.start_date_field.is_some() && current_start.as_deref() != Some(&start_str);
    let need_end = cfg.target_date_field.is_some() && current_end.as_deref() != Some(&end_str);

    // Only auto-set status if the current status is NOT "in_progress".
    let in_progress_name = &cfg.status_values.in_progress;
    let currently_in_progress = current_status
        .as_deref()
        .map(|s| s == in_progress_name)
        .unwrap_or(false);

    let need_status = cfg.status_field.is_some()
        && !currently_in_progress
        && current_status.as_deref() != Some(desired_status);

    // No-op check (already on board and all values match).
    if !needs_add && !need_start && !need_end && !need_status {
        println!("  skip   {node_path} ({canonical}) — already up to date");
        return Ok(SyncOneResult::Skipped);
    }

    if dry_run {
        if needs_add {
            println!("  [dry]  {node_path}: would add {canonical} to project");
        }
        if need_start {
            println!(
                "  [dry]  {node_path}: would set {} = {start_str}",
                cfg.start_date_field.as_deref().unwrap_or("")
            );
        }
        if need_end {
            println!(
                "  [dry]  {node_path}: would set {} = {end_str}",
                cfg.target_date_field.as_deref().unwrap_or("")
            );
        }
        if need_status {
            println!(
                "  [dry]  {node_path}: would set {} = {desired_status}",
                cfg.status_field.as_deref().unwrap_or("")
            );
        }
        return Ok(if needs_add {
            SyncOneResult::Added
        } else {
            SyncOneResult::Updated
        });
    }

    // --- Add to project if needed ---
    let item_id = if needs_add {
        let owner = &issue_ref.owner;
        let repo = strip_qualifier(&issue_ref.repo);

        println!("  add    {node_path} ({canonical})");
        let content_id = fetch_issue_node_id(gh, owner, &repo, issue_ref.number)?;
        add_item_to_project(gh, &cache.project_id, &content_id)?
    } else {
        println!("  update {node_path} ({canonical})");
        item_id
    };

    // --- Set start date ---
    if need_start && let Some(ref field_name) = cfg.start_date_field {
        match cache.fields.get(field_name) {
            Some(CachedField::Date { id }) => {
                update_date_field(gh, &cache.project_id, &item_id, id, &start_str)?;
            }
            _ => {
                warn!("start_date_field '{field_name}' not found on board, skipping");
            }
        }
    }

    // --- Set target date ---
    if need_end && let Some(ref field_name) = cfg.target_date_field {
        match cache.fields.get(field_name) {
            Some(CachedField::Date { id }) => {
                update_date_field(gh, &cache.project_id, &item_id, id, &end_str)?;
            }
            _ => {
                warn!("target_date_field '{field_name}' not found on board, skipping");
            }
        }
    }

    // --- Set status ---
    if need_status && let Some(ref field_name) = cfg.status_field {
        match cache.fields.get(field_name) {
            Some(CachedField::SingleSelect { id, options }) => match options.get(desired_status) {
                Some(option_id) => {
                    update_single_select_field(gh, &cache.project_id, &item_id, id, option_id)?;
                }
                None => {
                    warn!(
                        "status option '{desired_status}' not found on board field '{field_name}', skipping"
                    );
                }
            },
            _ => {
                warn!("status_field '{field_name}' not found or wrong type, skipping");
            }
        }
    }

    Ok(if needs_add {
        SyncOneResult::Added
    } else {
        SyncOneResult::Updated
    })
}

/// Return the option display name to auto-assign based on how soon `target` is.
///
/// Never returns "in_progress" — that requires explicit user action.
pub fn desired_status(target: NaiveDate, today: NaiveDate, values: &StatusValues) -> &str {
    let two_weeks = today + chrono::Duration::weeks(2);
    let quarter_end = end_of_quarter(today);
    if target <= two_weeks {
        &values.sprint_todo
    } else if target <= quarter_end {
        &values.todo
    } else {
        &values.backlog
    }
}

fn strip_qualifier(repo: &str) -> String {
    repo.split_once('@')
        .map(|(r, _)| r)
        .unwrap_or(repo)
        .to_string()
}

fn end_of_quarter(date: NaiveDate) -> NaiveDate {
    let month = date.month();
    let quarter_end_month = ((month - 1) / 3 + 1) * 3;
    let year = date.year();
    let next_month_first = if quarter_end_month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, quarter_end_month + 1, 1).unwrap()
    };
    next_month_first.pred_opt().unwrap()
}

/// Set project board date fields for a single issue ref (`owner/repo#N`).
/// Adds the issue to the board if it is not already present.
/// At least one of `start_date` or `target_date` must be `Some`.
pub fn set_issue(
    gh: &Gh,
    org: &Org,
    issue_str: &str,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    dry_run: bool,
) -> Result<()> {
    let cfg = org.domain_config::<ProjectDomain>()?;

    if cfg.org.is_empty() || cfg.number == 0 {
        return Err(Error::Other(
            "github_project.org and github_project.number must be set in armitage.toml".into(),
        ));
    }

    let issue_ref = IssueRef::parse(issue_str)
        .map_err(|e| Error::Other(format!("invalid issue ref '{issue_str}': {e}")))?;
    let repo = strip_qualifier(&issue_ref.repo);
    let canonical = format!("{}/{}#{}", issue_ref.owner, repo, issue_ref.number);

    let cache = load_or_fetch_cache(gh, org, &cfg)?;

    println!("Fetching current project items…");
    let existing: HashMap<String, ProjectItem> = fetch_project_items(gh, &cfg.org, cfg.number)?;

    let (item_id, needs_add) = if let Some(item) = existing.get(&canonical) {
        (item.item_id.clone(), false)
    } else {
        (String::new(), true)
    };

    if dry_run {
        if needs_add {
            println!("  [dry]  would add {canonical} to project");
        }
        if let Some(d) = start_date {
            println!(
                "  [dry]  would set {} = {}",
                cfg.start_date_field.as_deref().unwrap_or("Start date"),
                d.format("%Y-%m-%d")
            );
        }
        if let Some(d) = target_date {
            println!(
                "  [dry]  would set {} = {}",
                cfg.target_date_field.as_deref().unwrap_or("Target date"),
                d.format("%Y-%m-%d")
            );
            if cfg.status_field.is_some() {
                let today = chrono::Local::now().date_naive();
                let status_value = desired_status(d, today, &cfg.status_values);
                println!(
                    "  [dry]  would set {} = {status_value}",
                    cfg.status_field.as_deref().unwrap_or("Status"),
                );
            }
        }
        return Ok(());
    }

    let item_id = if needs_add {
        println!("  add    {canonical}");
        let content_id = fetch_issue_node_id(gh, &issue_ref.owner, &repo, issue_ref.number)?;
        add_item_to_project(gh, &cache.project_id, &content_id)?
    } else {
        println!("  update {canonical}");
        item_id
    };

    if let Some(d) = start_date
        && let Some(ref field_name) = cfg.start_date_field
    {
        match cache.fields.get(field_name) {
            Some(CachedField::Date { id }) => {
                update_date_field(
                    gh,
                    &cache.project_id,
                    &item_id,
                    id,
                    &d.format("%Y-%m-%d").to_string(),
                )?;
                println!("  set    {field_name} = {}", d.format("%Y-%m-%d"));
            }
            _ => warn!("start_date_field '{field_name}' not found on board, skipping"),
        }
    }

    if let Some(d) = target_date
        && let Some(ref field_name) = cfg.target_date_field
    {
        match cache.fields.get(field_name) {
            Some(CachedField::Date { id }) => {
                update_date_field(
                    gh,
                    &cache.project_id,
                    &item_id,
                    id,
                    &d.format("%Y-%m-%d").to_string(),
                )?;
                println!("  set    {field_name} = {}", d.format("%Y-%m-%d"));
            }
            _ => warn!("target_date_field '{field_name}' not found on board, skipping"),
        }
    }

    // --- Set status based on target_date ---
    if let Some(d) = target_date
        && let Some(ref field_name) = cfg.status_field
    {
        let today = chrono::Local::now().date_naive();
        let status_value = desired_status(d, today, &cfg.status_values);
        match cache.fields.get(field_name) {
            Some(CachedField::SingleSelect { id, options }) => match options.get(status_value) {
                Some(option_id) => {
                    update_single_select_field(gh, &cache.project_id, &item_id, id, option_id)?;
                    println!("  set    {field_name} = {status_value}");
                }
                None => {
                    warn!(
                        "status option '{status_value}' not found on board field '{field_name}', skipping"
                    );
                }
            },
            _ => {
                warn!("status_field '{field_name}' not found or wrong type, skipping");
            }
        }
    }

    Ok(())
}

fn load_or_fetch_cache(gh: &Gh, org: &Org, cfg: &GitHubProjectConfig) -> Result<FieldCache> {
    if let Some(cache) = read_field_cache(org.root())? {
        // Re-use cached field IDs unless the project changed.
        if !cache.project_id.is_empty() {
            return Ok(cache);
        }
    }
    println!("Fetching project field metadata…");
    let cache = fetch_field_cache(gh, &cfg.org, cfg.number)?;
    write_field_cache(org.root(), &cache)?;
    Ok(cache)
}
