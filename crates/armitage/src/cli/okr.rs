use std::collections::HashMap;

use armitage_core::goal::{GoalsFile, node_in_goal};
use armitage_core::node::NodeStatus;
use armitage_core::period::Period;
use armitage_core::team::TeamFile;
use armitage_core::tree::{NodeEntry, find_org_root, walk_nodes};
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
    /// Sub-issues of this issue, if any.
    pub sub_issues: Vec<SubKeyResult>,
}

/// A sub-issue nested under a key result.
#[derive(Debug, Serialize)]
pub struct SubKeyResult {
    pub issue_ref: String,
    pub title: String,
    pub state: String,
    pub assignees: Vec<String>,
    pub target_date: Option<String>,
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
// Issue loading with track-field overrides
// ---------------------------------------------------------------------------

/// Parse "owner/repo#N" into (repo, number).
fn parse_track_ref(s: &str) -> Option<(String, u64)> {
    let (repo, num_s) = s.rsplit_once('#')?;
    let number = num_s.parse::<u64>().ok()?;
    Some((repo.to_string(), number))
}

/// Load all classified issues from the DB, then apply `track`-field overrides:
///
/// 1. For any issue already present whose (repo, number) matches a node's
///    `track` field, update `node_path` to that node — making the `track`
///    field authoritative over the triage suggestion.
/// 2. For tracking issues that were never classified (no triage suggestion),
///    fetch them directly from the DB and assign them to their node.
///
/// This ensures milestone tracking issues always appear as key results for
/// their node, even when the triage pipeline has not yet processed them.
fn load_issues_with_track_overrides(
    org_root: &std::path::Path,
    all_nodes: &[NodeEntry],
) -> Vec<armitage_triage::db::IssueWithProjectData> {
    let conn_opt = armitage_triage::db::open_db(org_root).ok();

    let mut all_issues = conn_opt
        .as_ref()
        .and_then(|c| armitage_triage::db::get_all_issues_with_project_data(c).ok())
        .unwrap_or_default();

    // Build track_map: (repo, number) -> node_path
    let track_map: HashMap<(String, u64), String> = all_nodes
        .iter()
        .filter_map(|e| {
            e.node
                .track
                .as_deref()
                .and_then(parse_track_ref)
                .map(|(repo, num)| ((repo, num), e.path.clone()))
        })
        .collect();

    if track_map.is_empty() {
        return all_issues;
    }

    // 1. Override node_path for tracking issues already in all_issues.
    let mut seen: HashMap<(String, u64), ()> = HashMap::new();
    for issue in &mut all_issues {
        let key = (issue.issue.repo.clone(), issue.issue.number);
        if let Some(node_path) = track_map.get(&key) {
            issue.node_path = Some(node_path.clone());
            seen.insert(key, ());
        }
    }

    // 2. Fetch tracking issues not yet in all_issues (unclassified).
    if let Some(conn) = &conn_opt {
        for ((repo, number), node_path) in &track_map {
            if seen.contains_key(&(repo.clone(), *number)) {
                continue;
            }
            if let Ok(Some(mut issue)) =
                armitage_triage::db::get_issue_with_project_data_by_ref(conn, repo, *number)
            {
                issue.node_path = Some(node_path.clone());
                all_issues.push(issue);
            }
        }
    }

    all_issues
}

// ---------------------------------------------------------------------------
// okr show
// ---------------------------------------------------------------------------

pub fn run_show(
    period_str: String,
    goal_slug: Option<String>,
    person: Option<String>,
    team: Option<String>,
    depth: usize,
    format: String,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    // If --goal is given, resolve the goal. When period is "current" and the goal
    // has a deadline, use the goal's deadline year as the period so all relevant
    // work in that year is included.
    let goal_filter = if let Some(ref slug) = goal_slug {
        let goals = GoalsFile::read(&org_root)?;
        let g = goals
            .find(slug)
            .ok_or_else(|| crate::error::Error::Other(format!("goal '{slug}' not found")))?
            .clone();
        Some(g)
    } else {
        None
    };

    let effective_period_str = if period_str == "current" {
        if let Some(ref g) = goal_filter
            && let Some(deadline) = g.deadline
        {
            deadline.format("%Y").to_string()
        } else {
            period_str.clone()
        }
    } else {
        period_str.clone()
    };
    let period = Period::parse(&effective_period_str)?;
    let today = chrono::Local::now().date_naive();

    let all_nodes = walk_nodes(&org_root)?;
    let team_file = TeamFile::read(&org_root).unwrap_or_default();

    // Load all classified issues, with track-field overrides applied.
    let all_issues = load_issues_with_track_overrides(&org_root, &all_nodes);

    // Load sub-issue relationships for nesting in key results.
    let sub_issue_map: std::collections::HashMap<String, Vec<String>> =
        armitage_triage::db::open_db(&org_root)
            .ok()
            .and_then(|c| armitage_triage::db::get_all_sub_issue_relationships(&c).ok())
            .unwrap_or_default();

    // Group issues by their effective node path.
    let mut issues_by_node: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, issue) in all_issues.iter().enumerate() {
        if let Some(ref path) = issue.node_path {
            issues_by_node.entry(path.clone()).or_default().push(idx);
        }
    }

    // Full subtree lookup — used only for the initial in-scope determination.
    let issues_for_subtree = |node_path: &str| -> Vec<&armitage_triage::db::IssueWithProjectData> {
        issues_by_node
            .iter()
            .filter(|(p, _)| *p == node_path || p.starts_with(&format!("{node_path}/")))
            .flat_map(|(_, idxs)| idxs.iter().map(|&i| &all_issues[i]))
            .collect()
    };

    // Predicate shared by both the in-scope pass and the build pass.
    let node_passes_filters = |e: &&NodeEntry| -> bool {
        let node_depth = e.path.matches('/').count() + 1;
        if node_depth > depth {
            return false;
        }
        if e.node.status == NodeStatus::Cancelled {
            return false;
        }
        if let Some(ref g) = goal_filter
            && !node_in_goal(&e.path, &g.nodes)
        {
            return false;
        }
        if let Some(ref t) = team
            && e.node.team.as_deref() != Some(t.as_str())
        {
            return false;
        }
        if let Some(ref p) = person {
            let is_owner = e.node.owners.contains(p);
            let has_assigned_issue = issues_for_subtree(&e.path)
                .iter()
                .any(|i| i.issue.assignees.contains(p));
            if !is_owner && !has_assigned_issue {
                return false;
            }
        }
        true
    };

    // Pass 1 — collect in-scope paths using full subtree rollup for the date check.
    let in_scope_paths: std::collections::HashSet<String> = all_nodes
        .iter()
        .filter(|e| {
            if !node_passes_filters(e) {
                return false;
            }
            let timeline_overlap = e
                .node
                .timeline
                .as_ref()
                .is_some_and(|tl| period.overlaps_timeline(tl));
            let has_dated_issues = issues_for_subtree(&e.path).iter().any(|i| {
                i.target_date
                    .as_deref()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .is_some_and(|d| period.contains_date(d))
            });
            timeline_overlap || has_dated_issues
        })
        .map(|e| e.path.clone())
        .collect();

    // Smart issue lookup — for a given node, return its direct issues plus any
    // subtree issues that are NOT already claimed by an in-scope child node.
    // This prevents parent nodes from duplicating KRs already shown under a child.
    let issues_for_node = |node_path: &str| -> Vec<&armitage_triage::db::IssueWithProjectData> {
        issues_by_node
            .iter()
            .filter(|(p, _)| {
                let p = p.as_str();
                if p != node_path && !p.starts_with(&format!("{node_path}/")) {
                    return false;
                }
                if p == node_path {
                    return true;
                }
                // Subtree path: include only if no in-scope child of node_path covers it.
                !in_scope_paths.iter().any(|scope| {
                    scope.as_str() != node_path
                        && scope.starts_with(&format!("{node_path}/"))
                        && (p == scope.as_str() || p.starts_with(&format!("{scope}/")))
                })
            })
            .flat_map(|(_, idxs)| idxs.iter().map(|&i| &all_issues[i]))
            .collect()
    };

    // Pass 2 — build objectives using the smart issue lookup.
    let mut objectives: Vec<OkrObjective> = all_nodes
        .iter()
        .filter(|e| in_scope_paths.contains(&e.path))
        .map(|e| {
            let subtree_issues = issues_for_node(&e.path);

            // Collect issues relevant to this period.
            // Open issues always appear — a project can span multiple OKR periods and
            // all open work is relevant context regardless of when it is due.
            // Closed issues appear only if they completed within this period.
            let period_issues: Vec<&&armitage_triage::db::IssueWithProjectData> = subtree_issues
                .iter()
                .filter(|i| {
                    // Person filter on assignee when --person is given.
                    if let Some(ref p) = person
                        && !i.issue.assignees.contains(p)
                    {
                        return false;
                    }
                    let is_open = i.issue.state.eq_ignore_ascii_case("open");
                    if is_open {
                        return true;
                    }
                    // Closed: only include if it completed within this period.
                    i.target_date
                        .as_deref()
                        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                        .is_some_and(|d| period.contains_date(d))
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
                    let issue_ref = format!("{}#{}", i.issue.repo, i.issue.number);
                    let sub_issues = sub_issue_map
                        .get(&issue_ref)
                        .map(|children| {
                            children
                                .iter()
                                .filter_map(|child_ref| {
                                    all_issues.iter().find(|ci| {
                                        &format!("{}#{}", ci.issue.repo, ci.issue.number)
                                            == child_ref
                                    })
                                })
                                .map(|ci| {
                                    let sub_overdue = ci.issue.state.eq_ignore_ascii_case("open")
                                        && ci
                                            .target_date
                                            .as_deref()
                                            .and_then(|d| {
                                                NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
                                            })
                                            .is_some_and(|d| d < today);
                                    SubKeyResult {
                                        issue_ref: format!("{}#{}", ci.issue.repo, ci.issue.number),
                                        title: ci.issue.title.clone(),
                                        state: ci.issue.state.clone(),
                                        assignees: ci.issue.assignees.clone(),
                                        target_date: ci.target_date.clone(),
                                        overdue: sub_overdue,
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    KeyResult {
                        issue_ref,
                        title: i.issue.title.clone(),
                        state: i.issue.state.clone(),
                        assignees: i.issue.assignees.clone(),
                        target_date: i.target_date.clone(),
                        overdue,
                        sub_issues,
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
    goal_slug: Option<String>,
    person: Option<String>,
    team: Option<String>,
    format: String,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;

    let goal_filter = if let Some(ref slug) = goal_slug {
        let goals = GoalsFile::read(&org_root)?;
        let g = goals
            .find(slug)
            .ok_or_else(|| crate::error::Error::Other(format!("goal '{slug}' not found")))?
            .clone();
        Some(g)
    } else {
        None
    };

    let effective_period_str = if period_str == "current" {
        if let Some(ref g) = goal_filter
            && let Some(deadline) = g.deadline
        {
            deadline.format("%Y").to_string()
        } else {
            period_str.clone()
        }
    } else {
        period_str.clone()
    };
    let period = Period::parse(&effective_period_str)?;
    let today = chrono::Local::now().date_naive();

    let all_nodes = walk_nodes(&org_root)?;

    let all_issues = load_issues_with_track_overrides(&org_root, &all_nodes);

    let mut issues_by_node: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, issue) in all_issues.iter().enumerate() {
        if let Some(ref path) = issue.node_path {
            issues_by_node.entry(path.clone()).or_default().push(idx);
        }
    }

    // Collect timeline-active nodes that pass all filters.
    let in_scope_paths: std::collections::HashSet<String> = all_nodes
        .iter()
        .filter(|e| {
            if e.node.status == NodeStatus::Cancelled {
                return false;
            }
            if !e
                .node
                .timeline
                .as_ref()
                .is_some_and(|tl| period.overlaps_timeline(tl))
            {
                return false;
            }
            if let Some(ref g) = goal_filter
                && !node_in_goal(&e.path, &g.nodes)
            {
                return false;
            }
            if let Some(ref t) = team
                && e.node.team.as_deref() != Some(t.as_str())
            {
                return false;
            }
            if let Some(ref p) = person {
                let is_owner = e.node.owners.contains(p);
                let has_assigned_issue = issues_by_node
                    .iter()
                    .filter(|(path, _)| {
                        path.as_str() == e.path || path.starts_with(&format!("{}/", e.path))
                    })
                    .flat_map(|(_, idxs)| idxs.iter().map(|&i| &all_issues[i]))
                    .any(|i| i.issue.assignees.contains(p));
                if !is_owner && !has_assigned_issue {
                    return false;
                }
            }
            true
        })
        .map(|e| e.path.clone())
        .collect();

    // Smart issue lookup — same exclusion logic as run_show.
    let issues_for_node = |node_path: &str| -> Vec<&armitage_triage::db::IssueWithProjectData> {
        issues_by_node
            .iter()
            .filter(|(p, _)| {
                let p = p.as_str();
                if p != node_path && !p.starts_with(&format!("{node_path}/")) {
                    return false;
                }
                if p == node_path {
                    return true;
                }
                !in_scope_paths.iter().any(|scope| {
                    scope.as_str() != node_path
                        && scope.starts_with(&format!("{node_path}/"))
                        && (p == scope.as_str() || p.starts_with(&format!("{scope}/")))
                })
            })
            .flat_map(|(_, idxs)| idxs.iter().map(|&i| &all_issues[i]))
            .collect()
    };

    let mut problems: Vec<CheckProblem> = Vec::new();

    for e in &all_nodes {
        if !in_scope_paths.contains(&e.path) {
            continue;
        }

        // Skip parent nodes whose in-scope children already cover all the work —
        // they have no direct key results to check.
        let has_in_scope_child = in_scope_paths
            .iter()
            .any(|p| p != &e.path && p.starts_with(&format!("{}/", e.path)));
        let has_direct_dated = issues_by_node.get(&e.path).is_some_and(|idxs| {
            idxs.iter().any(|&i| {
                all_issues[i]
                    .target_date
                    .as_deref()
                    .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .is_some_and(|d| period.contains_date(d))
            })
        });
        if has_in_scope_child && !has_direct_dated {
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

        let subtree_issues = issues_for_node(&e.path);
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
            for sub in &kr.sub_issues {
                let sub_status = if sub.state.eq_ignore_ascii_case("closed") {
                    "✅ CLOSED"
                } else if sub.overdue {
                    "⚠️ OVERDUE"
                } else {
                    "🔄 OPEN"
                };
                let sub_target = sub.target_date.as_deref().unwrap_or("—");
                let sub_assignees = if sub.assignees.is_empty() {
                    "—".to_string()
                } else {
                    sub.assignees
                        .iter()
                        .map(|a| format!("@{a}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                println!(
                    "| ↳ {} | {} | {} | {} | {} |",
                    sub.issue_ref, sub.title, sub_status, sub_target, sub_assignees
                );
            }
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
