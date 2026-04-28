use std::collections::HashMap;

use armitage_core::node::NodeStatus;
use armitage_core::period::Period;
use armitage_core::team::TeamFile;
use armitage_core::tree::{find_org_root, walk_nodes};
use chrono::NaiveDate;
use serde::Serialize;

use crate::error::Result;

// ---------------------------------------------------------------------------
// Output types (JSON-serialisable so agents can consume them directly)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct KeyResult {
    pub issue_ref: String,
    pub title: String,
    pub state: String,
    pub assignees: Vec<String>,
    pub target_date: Option<String>,
    /// True when target_date has passed and the issue is still open.
    pub overdue: bool,
}

#[derive(Debug, Serialize)]
pub struct OkrObjective {
    pub node_path: String,
    pub name: String,
    pub team: Option<String>,
    pub owners: Vec<String>,
    /// Node timeline end (ISO date), if set.
    pub node_end: Option<String>,
    pub node_status: String,
    pub total_issues: usize,
    pub closed_issues: usize,
    /// Fraction of issues closed (0.0 – 1.0).  0.0 when no issues.
    pub progress: f64,
    pub key_results: Vec<KeyResult>,
    /// Refs of open issues whose target date is already past.
    pub at_risk: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckProblem {
    pub kind: String,
    pub node_path: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// okr show
// ---------------------------------------------------------------------------

pub fn run_show(
    period_str: String,
    person: Option<String>,
    team: Option<String>,
    depth: usize,
    format: String,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let period = Period::parse(&period_str)?;
    let today = chrono::Local::now().date_naive();

    let all_nodes = walk_nodes(&org_root)?;
    let team_file = TeamFile::read(&org_root).unwrap_or_default();

    // Load all classified issues with project data from the triage DB.
    // Gracefully degrade when no DB exists (e.g. fresh org).
    let all_issues = armitage_triage::db::open_db(&org_root)
        .ok()
        .and_then(|conn| armitage_triage::db::get_all_issues_with_project_data(&conn).ok())
        .unwrap_or_default();

    // Group issues by their effective node path.
    let mut issues_by_node: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, issue) in all_issues.iter().enumerate() {
        if let Some(ref path) = issue.node_path {
            issues_by_node.entry(path.clone()).or_default().push(idx);
        }
    }

    // Collect all issues for a node path including its subtree.
    let issues_for_subtree = |node_path: &str| -> Vec<&armitage_triage::db::IssueWithProjectData> {
        issues_by_node
            .iter()
            .filter(|(p, _)| *p == node_path || p.starts_with(&format!("{node_path}/")))
            .flat_map(|(_, idxs)| idxs.iter().map(|&i| &all_issues[i]))
            .collect()
    };

    let mut objectives: Vec<OkrObjective> = all_nodes
        .iter()
        .filter(|e| {
            // Depth filter (1-based).
            let node_depth = e.path.matches('/').count() + 1;
            if node_depth > depth {
                return false;
            }
            // Skip cancelled nodes.
            if e.node.status == NodeStatus::Cancelled {
                return false;
            }
            // Team filter.
            if let Some(ref t) = team
                && e.node.team.as_deref() != Some(t.as_str())
            {
                return false;
            }
            // Person filter (must be listed as an owner of this node).
            if let Some(ref p) = person
                && !e.node.owners.contains(p)
            {
                return false;
            }
            // Include if the node's timeline overlaps this period …
            let timeline_overlap = e
                .node
                .timeline
                .as_ref()
                .is_some_and(|tl| period.overlaps_timeline(tl));
            // … or if it has at least one issue with a target_date in this period.
            let has_dated_issues = issues_for_subtree(&e.path).iter().any(|i| {
                i.target_date
                    .as_deref()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .is_some_and(|d| period.contains_date(d))
            });
            timeline_overlap || has_dated_issues
        })
        .map(|e| {
            let subtree_issues = issues_for_subtree(&e.path);

            // Collect issues relevant to this period.
            // An issue is included when its target_date falls in the period.
            // Issues without a target_date are included when they are still open
            // (they represent ongoing work with no scheduled deadline).
            let period_issues: Vec<&&armitage_triage::db::IssueWithProjectData> = subtree_issues
                .iter()
                .filter(|i| {
                    // Person filter on assignee when --person is given.
                    if let Some(ref p) = person
                        && !i.issue.assignees.contains(p)
                    {
                        return false;
                    }
                    match i.target_date.as_deref() {
                        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
                            .ok()
                            .is_some_and(|d| period.contains_date(d)),
                        None => i.issue.state.eq_ignore_ascii_case("open"),
                    }
                })
                .collect();

            let closed = period_issues
                .iter()
                .filter(|i| i.issue.state.eq_ignore_ascii_case("closed"))
                .count();
            let total = period_issues.len();
            let progress = if total > 0 {
                closed as f64 / total as f64
            } else {
                0.0
            };

            let at_risk: Vec<String> = period_issues
                .iter()
                .filter(|i| {
                    i.issue.state.eq_ignore_ascii_case("open")
                        && i.target_date
                            .as_deref()
                            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                            .is_some_and(|d| d < today)
                })
                .map(|i| format!("{}#{}", i.issue.repo, i.issue.number))
                .collect();

            let key_results: Vec<KeyResult> = period_issues
                .iter()
                .map(|i| {
                    let overdue = i.issue.state.eq_ignore_ascii_case("open")
                        && i.target_date
                            .as_deref()
                            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                            .is_some_and(|d| d < today);
                    KeyResult {
                        issue_ref: format!("{}#{}", i.issue.repo, i.issue.number),
                        title: i.issue.title.clone(),
                        state: i.issue.state.clone(),
                        assignees: i.issue.assignees.clone(),
                        target_date: i.target_date.clone(),
                        overdue,
                    }
                })
                .collect();

            OkrObjective {
                node_path: e.path.clone(),
                name: e.node.name.clone(),
                team: e.node.team.clone(),
                owners: e.node.owners.clone(),
                node_end: e.node.timeline.as_ref().map(|tl| tl.end.to_string()),
                node_status: e.node.status.to_string(),
                total_issues: total,
                closed_issues: closed,
                progress,
                key_results,
                at_risk,
            }
        })
        // Drop nodes that have no relevant issues this period.
        .filter(|o| o.total_issues > 0)
        .collect();

    // Sort: soonest deadline first; within same deadline, lowest progress first.
    objectives.sort_by(|a, b| {
        let ae = a.node_end.as_deref().unwrap_or("9999-12-31");
        let be = b.node_end.as_deref().unwrap_or("9999-12-31");
        ae.cmp(be).then(
            a.progress
                .partial_cmp(&b.progress)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    match format.as_str() {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&objectives)
                .map_err(|e| crate::error::Error::Other(e.to_string()))?
        ),
        "markdown" => print_markdown(&period, &objectives, &team_file),
        _ => print_table(&period, &objectives, today),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// okr check
// ---------------------------------------------------------------------------

pub fn run_check(
    period_str: String,
    person: Option<String>,
    team: Option<String>,
    format: String,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let period = Period::parse(&period_str)?;
    let today = chrono::Local::now().date_naive();

    let all_nodes = walk_nodes(&org_root)?;

    let all_issues = armitage_triage::db::open_db(&org_root)
        .ok()
        .and_then(|conn| armitage_triage::db::get_all_issues_with_project_data(&conn).ok())
        .unwrap_or_default();

    let mut issues_by_node: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, issue) in all_issues.iter().enumerate() {
        if let Some(ref path) = issue.node_path {
            issues_by_node.entry(path.clone()).or_default().push(idx);
        }
    }

    let issues_for_subtree = |node_path: &str| -> Vec<&armitage_triage::db::IssueWithProjectData> {
        issues_by_node
            .iter()
            .filter(|(p, _)| *p == node_path || p.starts_with(&format!("{node_path}/")))
            .flat_map(|(_, idxs)| idxs.iter().map(|&i| &all_issues[i]))
            .collect()
    };

    let mut problems: Vec<CheckProblem> = Vec::new();

    for e in &all_nodes {
        if e.node.status == NodeStatus::Cancelled {
            continue;
        }
        let timeline_active = e
            .node
            .timeline
            .as_ref()
            .is_some_and(|tl| period.overlaps_timeline(tl));
        if !timeline_active {
            continue;
        }
        if let Some(ref t) = team
            && e.node.team.as_deref() != Some(t.as_str())
        {
            continue;
        }
        if let Some(ref p) = person
            && !e.node.owners.contains(p)
        {
            continue;
        }

        // Unowned node.
        if e.node.owners.is_empty() {
            problems.push(CheckProblem {
                kind: "unowned".to_string(),
                node_path: e.path.clone(),
                detail: format!("'{}' has no owners set", e.node.name),
            });
        }

        let subtree_issues = issues_for_subtree(&e.path);
        let dated_issues: Vec<_> = subtree_issues
            .iter()
            .filter(|i| {
                i.target_date
                    .as_deref()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .is_some_and(|d| period.contains_date(d))
            })
            .collect();

        // No key results scheduled.
        if dated_issues.is_empty() && subtree_issues.is_empty() {
            problems.push(CheckProblem {
                kind: "no-key-results".to_string(),
                node_path: e.path.clone(),
                detail: format!(
                    "'{}' is active this period but has no issues with target dates",
                    e.node.name
                ),
            });
        }

        // Overdue open issues.
        for i in &subtree_issues {
            if !i.issue.state.eq_ignore_ascii_case("open") {
                continue;
            }
            if let Some(d) = i
                .target_date
                .as_deref()
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                && d < today
            {
                problems.push(CheckProblem {
                    kind: "overdue".to_string(),
                    node_path: e.path.clone(),
                    detail: format!(
                        "{}#{} '{}' was due {} but is still open",
                        i.issue.repo, i.issue.number, i.issue.title, d
                    ),
                });
            }
        }

        // Unassigned open issues with a target date this period.
        for i in &dated_issues {
            if i.issue.state.eq_ignore_ascii_case("open") && i.issue.assignees.is_empty() {
                problems.push(CheckProblem {
                    kind: "unassigned".to_string(),
                    node_path: e.path.clone(),
                    detail: format!(
                        "{}#{} '{}' has no assignee",
                        i.issue.repo, i.issue.number, i.issue.title
                    ),
                });
            }
        }
    }

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&problems)
                .map_err(|e| crate::error::Error::Other(e.to_string()))?
        );
        return Ok(());
    }

    if problems.is_empty() {
        println!("No OKR gaps found for {}.", period.label);
        return Ok(());
    }
    println!(
        "{} problem(s) found for {}:\n",
        problems.len(),
        period.label
    );
    for p in &problems {
        let icon = match p.kind.as_str() {
            "overdue" => "⚠",
            "unowned" => "👤",
            "unassigned" => "—",
            _ => "?",
        };
        println!(
            "  {icon} [{kind}] {path}",
            kind = p.kind,
            path = p.node_path
        );
        println!("     {}", p.detail);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn print_table(period: &Period, objectives: &[OkrObjective], today: NaiveDate) {
    println!("OKRs — {} ({})\n", period.label, period.display_range());
    for obj in objectives {
        let pct = (obj.progress * 100.0).round() as u32;
        let bar = progress_bar(obj.progress, 10);
        let deadline = obj
            .node_end
            .as_deref()
            .map(|d| format!("  ends {d}"))
            .unwrap_or_default();
        println!(
            "{path:<35}  {name:<30}  {bar} {closed}/{total} ({pct}%){deadline}",
            path = obj.node_path,
            name = truncate(&obj.name, 30),
            closed = obj.closed_issues,
            total = obj.total_issues,
        );
        if !obj.owners.is_empty() {
            println!("  owners: {}", obj.owners.join(", "));
        }
        for kr in &obj.key_results {
            let state_icon = if kr.state.eq_ignore_ascii_case("closed") {
                "✓"
            } else if kr.overdue {
                "⚠"
            } else {
                "→"
            };
            let due = kr
                .target_date
                .as_deref()
                .map(|d| {
                    if kr.state.eq_ignore_ascii_case("open") {
                        let date = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok();
                        let overdue = date.is_some_and(|dt| dt < today);
                        if overdue {
                            format!("  {d} OVERDUE")
                        } else {
                            format!("  {d}")
                        }
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();
            let assignee = if kr.assignees.is_empty() {
                String::new()
            } else {
                format!("  @{}", kr.assignees.join(", @"))
            };
            println!(
                "  {state_icon} {iref:<45}  {title}{due}{assignee}",
                iref = kr.issue_ref,
                title = truncate(&kr.title, 55),
            );
        }
        println!();
    }
}

fn print_markdown(period: &Period, objectives: &[OkrObjective], team_file: &TeamFile) {
    println!("# OKRs — {} ({})\n", period.label, period.display_range());
    for obj in objectives {
        let pct = (obj.progress * 100.0).round() as u32;
        let owners: Vec<String> = obj
            .owners
            .iter()
            .map(|u| {
                team_file
                    .members
                    .iter()
                    .find(|m| m.github == *u)
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| u.clone())
            })
            .collect();
        let owner_str = if owners.is_empty() {
            "—".to_string()
        } else {
            owners.join(", ")
        };
        let deadline = obj
            .node_end
            .as_deref()
            .map(|d| format!(" | **Due:** {d}"))
            .unwrap_or_default();
        println!(
            "## {} — {} ({}%)\n**Owners:** {}{}\n",
            obj.node_path, obj.name, pct, owner_str, deadline
        );
        println!("| Issue | Title | Status | Target | Assignees |");
        println!("|---|---|---|---|---|");
        for kr in &obj.key_results {
            let status = if kr.state.eq_ignore_ascii_case("closed") {
                "✅ CLOSED"
            } else if kr.overdue {
                "⚠️ OVERDUE"
            } else {
                "🔄 OPEN"
            };
            let target = kr.target_date.as_deref().unwrap_or("—");
            let assignees = if kr.assignees.is_empty() {
                "—".to_string()
            } else {
                kr.assignees
                    .iter()
                    .map(|a| format!("@{a}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            println!(
                "| {} | {} | {} | {} | {} |",
                kr.issue_ref, kr.title, status, target, assignees
            );
        }
        println!();
    }
}

fn progress_bar(progress: f64, width: usize) -> String {
    let filled = (progress * width as f64).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width + 2);
    s.push('[');
    for i in 0..width {
        if i < filled {
            s.push('█');
        } else {
            s.push('░');
        }
    }
    s.push(']');
    s
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
