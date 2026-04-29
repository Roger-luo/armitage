use armitage_core::org::Org;
use armitage_project::ProjectDomain;
use chrono::NaiveDate;

use crate::error::{Error, Result};

/// Run `armitage issue create`.
#[allow(clippy::too_many_arguments)]
pub fn run_create(
    title: String,
    body: String,
    node_path: Option<String>,
    repo: Option<String>,
    assignees: Vec<String>,
    extra_labels: Vec<String>,
    start_date: Option<String>,
    target_date: Option<String>,
    no_project: bool,
    dry_run: bool,
) -> Result<()> {
    // --- Resolve org ---
    let org = Org::discover_from(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;

    // --- Parse dates ---
    let parse_date = |s: &str| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| Error::Other(format!("invalid date '{s}', expected YYYY-MM-DD")))
    };

    // --- Resolve node if given ---
    let node_entry = node_path.as_deref().map(|p| org.read_node(p)).transpose()?;

    // --- Resolve repo ---
    let effective_repo: String = if let Some(r) = repo {
        r
    } else if let Some(ref entry) = node_entry {
        let raw = entry.node.repos.first().ok_or_else(|| {
            Error::Other(format!(
                "node '{}' has no repos configured; use --repo to specify one",
                entry.path
            ))
        })?;
        // Strip @branch qualifier
        raw.split_once('@')
            .map(|(r, _)| r)
            .unwrap_or(raw)
            .to_string()
    } else {
        return Err(Error::Other(
            "either --node or --repo is required".to_string(),
        ));
    };

    // --- Resolve labels (node labels + extra) ---
    let mut labels: Vec<String> = node_entry
        .as_ref()
        .map(|e| e.node.labels.clone())
        .unwrap_or_default();
    labels.extend(extra_labels);
    // Deduplicate while preserving order
    labels.dedup();

    // --- Resolve dates ---
    let effective_start: Option<NaiveDate> = if let Some(ref s) = start_date {
        Some(parse_date(s)?)
    } else {
        node_entry
            .as_ref()
            .and_then(|e| e.node.timeline.as_ref())
            .map(|t| t.start)
    };

    let effective_target: Option<NaiveDate> = if let Some(ref s) = target_date {
        Some(parse_date(s)?)
    } else {
        node_entry
            .as_ref()
            .and_then(|e| e.node.timeline.as_ref())
            .map(|t| t.end)
    };

    // --- Determine if we should set up the project board ---
    let project_cfg = org.domain_config::<ProjectDomain>().ok();
    let has_project = project_cfg
        .as_ref()
        .map(|c| !c.org.is_empty() && c.number != 0)
        .unwrap_or(false);

    let do_project = !no_project && has_project && effective_target.is_some();

    // --- Compute desired status label for display ---
    let status_label: Option<String> = if do_project {
        let cfg = project_cfg.as_ref().unwrap();
        let today = chrono::Local::now().date_naive();
        Some(
            armitage_project::desired_status(effective_target.unwrap(), today, &cfg.status_values)
                .to_string(),
        )
    } else {
        None
    };

    // --- Dry-run output ---
    if dry_run {
        println!("[dry-run] Would create issue in {effective_repo}:");
        println!("  title:  {title}");
        if !body.is_empty() {
            println!("  body:   ({}  chars)", body.len());
        }
        if !labels.is_empty() {
            println!("  labels: {}", labels.join(", "));
        }
        if !assignees.is_empty() {
            println!("  assignees: {}", assignees.join(", "));
        }
        if let Some(d) = effective_start {
            println!("  start:  {}", d.format("%Y-%m-%d"));
        }
        if let Some(d) = effective_target {
            println!("  target: {}", d.format("%Y-%m-%d"));
        }
        if let Some(ref s) = status_label {
            println!("  status: {s}");
        }
        if !do_project {
            println!("  (project board: skipped)");
        }
        return Ok(());
    }

    // --- Create the issue ---
    let created = armitage_github::issue::create_issue(
        &gh,
        &effective_repo,
        &title,
        &body,
        &labels,
        &assignees,
    )?;

    println!("Created: {}", created.url);
    if !labels.is_empty() {
        println!("  labels: {}", labels.join(", "));
    }
    if let Some(d) = effective_start {
        println!("  start:  {}", d.format("%Y-%m-%d"));
    }
    if let Some(d) = effective_target {
        println!("  target: {}", d.format("%Y-%m-%d"));
    }
    if let Some(ref s) = status_label {
        println!("  status: {s}");
    }

    // --- Add to project board ---
    if do_project {
        let issue_str = format!("{}#{}", effective_repo, created.number);
        if let Err(e) = armitage_project::set_issue(
            &gh,
            &org,
            &issue_str,
            effective_start,
            effective_target,
            false,
        ) {
            eprintln!("warning: failed to add issue to project board: {e}");
        }
    }

    Ok(())
}
