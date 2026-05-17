use std::str::FromStr;

use armitage_core::goal::{Checkpoint, CheckpointStatus, Goal, GoalsFile, validate_quarter};
use armitage_core::team::TeamFile;
use serde::Serialize;

use crate::cli::util::{self, parse_csv, parse_date, truncate};
use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// goal list
// ---------------------------------------------------------------------------

pub fn run_list(format: String) -> Result<()> {
    let org_root = util::org_root()?;
    let file = GoalsFile::read(&org_root)?;

    if file.goals.is_empty() {
        println!("No goals defined. Use `armitage goal add` to create one.");
        return Ok(());
    }

    if util::maybe_print_json(&format, &file.goals)? {
        return Ok(());
    }

    println!("{:<20}  {:<35}  {:<12}  NODES", "SLUG", "NAME", "DEADLINE");
    println!("{}", "-".repeat(85));
    for g in &file.goals {
        let deadline = g
            .deadline
            .map(|d| d.to_string())
            .unwrap_or_else(|| "TBD".to_string());
        println!(
            "{:<20}  {:<35}  {:<12}  {}",
            g.slug,
            truncate(&g.name, 35),
            deadline,
            g.nodes.len()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// goal show
// ---------------------------------------------------------------------------

pub fn run_show(slug: String, format: String) -> Result<()> {
    let org_root = util::org_root()?;
    let file = GoalsFile::read(&org_root)?;
    let team_file = TeamFile::read(&org_root).unwrap_or_default();

    let goal = file
        .find(&slug)
        .ok_or_else(|| Error::other(format!("goal '{slug}' not found")))?;

    if util::maybe_print_json(&format, goal)? {
        return Ok(());
    }

    println!("name:     {}", goal.name);
    println!("slug:     {}", goal.slug);
    if let Some(ref desc) = goal.description {
        println!("desc:     {desc}");
    }
    println!(
        "deadline: {}",
        goal.deadline
            .map(|d| d.to_string())
            .unwrap_or_else(|| "TBD".to_string())
    );
    if !goal.owners.is_empty() {
        let owner_names: Vec<String> = goal
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
        println!("owners:   {}", owner_names.join(", "));
    }
    if let Some(ref t) = goal.track {
        println!("track:    {t}");
    }
    if !goal.nodes.is_empty() {
        println!("nodes:");
        for n in &goal.nodes {
            println!("  - {n}");
        }
    }
    if !goal.checkpoints.is_empty() {
        println!("checkpoints:");
        for d in &goal.checkpoints {
            println!(
                "  - {slug:<22}  {quarter:<8}  {status:<12}  {name}",
                slug = d.slug,
                quarter = d.target_quarter,
                status = d.status.to_string(),
                name = d.name,
            );
            if !d.owners.is_empty() {
                println!("    owners: {}", d.owners.join(", "));
            }
            if !d.nodes.is_empty() {
                println!("    nodes: {}", d.nodes.join(", "));
            }
            if !d.issues.is_empty() {
                println!("    issues: {}", d.issues.join(", "));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// goal checkpoint add/set/list/rm
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn run_checkpoint_add(
    goal_slug: String,
    slug: String,
    name: String,
    quarter: String,
    description: Option<String>,
    status: Option<String>,
    owners: Option<String>,
    nodes: Option<String>,
    issues: Option<String>,
) -> Result<()> {
    let org_root = util::org_root()?;
    let mut file = GoalsFile::read(&org_root)?;

    validate_quarter(&quarter)?;
    let status = match status {
        Some(s) => CheckpointStatus::from_str(&s)?,
        None => CheckpointStatus::default(),
    };

    let goal = file
        .find_mut(&goal_slug)
        .ok_or_else(|| Error::other(format!("goal '{goal_slug}' not found")))?;
    if goal.find_checkpoint(&slug).is_some() {
        return Err(Error::other(format!(
            "checkpoint '{slug}' already exists in goal '{goal_slug}'"
        )));
    }
    goal.checkpoints.push(Checkpoint {
        slug: slug.clone(),
        name,
        description,
        target_quarter: quarter,
        status,
        owners: parse_csv(owners),
        nodes: parse_csv(nodes),
        issues: parse_csv(issues),
    });
    file.write(&org_root)?;
    println!("Checkpoint '{slug}' added to goal '{goal_slug}'.");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_checkpoint_set(
    goal_slug: String,
    slug: String,
    name: Option<String>,
    description: Option<String>,
    quarter: Option<String>,
    status: Option<String>,
    owners: Option<String>,
    nodes: Option<String>,
    issues: Option<String>,
) -> Result<()> {
    let org_root = util::org_root()?;
    let mut file = GoalsFile::read(&org_root)?;

    let goal = file
        .find_mut(&goal_slug)
        .ok_or_else(|| Error::other(format!("goal '{goal_slug}' not found")))?;
    let d = goal.find_checkpoint_mut(&slug).ok_or_else(|| {
        Error::other(format!(
            "checkpoint '{slug}' not found in goal '{goal_slug}'"
        ))
    })?;

    if let Some(n) = name {
        d.name = n;
    }
    if let Some(desc) = description {
        d.description = Some(desc);
    }
    if let Some(q) = quarter {
        validate_quarter(&q)?;
        d.target_quarter = q;
    }
    if let Some(s) = status {
        d.status = CheckpointStatus::from_str(&s)?;
    }
    if let Some(o) = owners {
        d.owners = parse_csv(Some(o));
    }
    if let Some(n) = nodes {
        d.nodes = parse_csv(Some(n));
    }
    if let Some(i) = issues {
        d.issues = parse_csv(Some(i));
    }
    file.write(&org_root)?;
    println!("Checkpoint '{goal_slug}/{slug}' updated.");
    Ok(())
}

pub fn run_checkpoint_list(
    goal: Option<String>,
    quarter: Option<String>,
    status: Option<String>,
    format: String,
) -> Result<()> {
    let org_root = util::org_root()?;
    let file = GoalsFile::read(&org_root)?;

    let status_filter = status
        .as_deref()
        .map(CheckpointStatus::from_str)
        .transpose()?;

    #[derive(serde::Serialize)]
    struct Row<'a> {
        goal: &'a str,
        slug: &'a str,
        name: &'a str,
        quarter: &'a str,
        status: String,
        owners: &'a [String],
        nodes: &'a [String],
        issues: &'a [String],
    }

    let mut rows: Vec<Row> = Vec::new();
    for g in &file.goals {
        if let Some(ref gs) = goal
            && &g.slug != gs
        {
            continue;
        }
        for d in &g.checkpoints {
            if let Some(ref q) = quarter
                && &d.target_quarter != q
            {
                continue;
            }
            if let Some(ref s) = status_filter
                && &d.status != s
            {
                continue;
            }
            rows.push(Row {
                goal: &g.slug,
                slug: &d.slug,
                name: &d.name,
                quarter: &d.target_quarter,
                status: d.status.to_string(),
                owners: &d.owners,
                nodes: &d.nodes,
                issues: &d.issues,
            });
        }
    }

    if util::maybe_print_json(&format, &rows)? {
        return Ok(());
    }

    if rows.is_empty() {
        println!("No checkpoints match the given filters.");
        return Ok(());
    }
    println!(
        "{:<18}  {:<22}  {:<8}  {:<12}  NAME",
        "GOAL", "SLUG", "QUARTER", "STATUS"
    );
    println!("{}", "-".repeat(90));
    for r in &rows {
        println!(
            "{:<18}  {:<22}  {:<8}  {:<12}  {}",
            r.goal,
            r.slug,
            r.quarter,
            r.status,
            truncate(r.name, 40),
        );
    }
    Ok(())
}

pub fn run_checkpoint_remove(goal_slug: String, slug: String, yes: bool) -> Result<()> {
    let org_root = util::org_root()?;
    let mut file = GoalsFile::read(&org_root)?;

    let goal = file
        .find_mut(&goal_slug)
        .ok_or_else(|| Error::other(format!("goal '{goal_slug}' not found")))?;
    if goal.find_checkpoint(&slug).is_none() {
        return Err(Error::other(format!(
            "checkpoint '{slug}' not found in goal '{goal_slug}'"
        )));
    }
    if !util::confirm(&format!("Remove checkpoint '{goal_slug}/{slug}'?"), yes)? {
        return Ok(());
    }
    goal.checkpoints.retain(|d| d.slug != slug);
    file.write(&org_root)?;
    println!("Checkpoint '{goal_slug}/{slug}' removed.");
    Ok(())
}

// ---------------------------------------------------------------------------
// goal add
// ---------------------------------------------------------------------------

pub fn run_add(
    slug: String,
    name: String,
    description: Option<String>,
    deadline: Option<String>,
    owners: Option<String>,
    track: Option<String>,
    nodes: Option<String>,
) -> Result<()> {
    let org_root = util::org_root()?;
    let mut file = GoalsFile::read(&org_root)?;

    if file.find(&slug).is_some() {
        return Err(Error::other(format!("goal '{slug}' already exists")));
    }

    let deadline = deadline
        .map(|d| parse_date(&d).ok_or_else(|| Error::other(format!("invalid date '{d}'"))))
        .transpose()?;

    let goal = Goal {
        slug: slug.clone(),
        name,
        description,
        deadline,
        owners: parse_csv(owners),
        track,
        nodes: parse_csv(nodes),
        checkpoints: vec![],
    };

    file.goals.push(goal);
    file.write(&org_root)?;
    println!("Goal '{slug}' added.");
    Ok(())
}

// ---------------------------------------------------------------------------
// goal set
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn run_set(
    slug: String,
    name: Option<String>,
    description: Option<String>,
    deadline: Option<String>,
    owners: Option<String>,
    track: Option<String>,
    nodes: Option<String>,
    add_nodes: Option<String>,
    remove_nodes: Option<String>,
) -> Result<()> {
    let org_root = util::org_root()?;
    let mut file = GoalsFile::read(&org_root)?;

    let goal = file
        .find_mut(&slug)
        .ok_or_else(|| Error::other(format!("goal '{slug}' not found")))?;

    if let Some(n) = name {
        goal.name = n;
    }
    if let Some(d) = description {
        goal.description = Some(d);
    }
    if let Some(d) = deadline {
        goal.deadline =
            Some(parse_date(&d).ok_or_else(|| Error::other(format!("invalid date '{d}'")))?);
    }
    if let Some(o) = owners {
        goal.owners = parse_csv(Some(o));
    }
    if let Some(t) = track {
        goal.track = Some(t);
    }
    if let Some(n) = nodes {
        goal.nodes = parse_csv(Some(n));
    }
    if let Some(n) = add_nodes {
        let to_add = parse_csv(Some(n));
        for node in to_add {
            if !goal.nodes.contains(&node) {
                goal.nodes.push(node);
            }
        }
    }
    if let Some(n) = remove_nodes {
        let to_remove = parse_csv(Some(n));
        goal.nodes.retain(|n| !to_remove.contains(n));
    }

    file.write(&org_root)?;
    println!("Goal '{slug}' updated.");
    Ok(())
}

// ---------------------------------------------------------------------------
// goal remove
// ---------------------------------------------------------------------------

pub fn run_remove(slug: String, yes: bool) -> Result<()> {
    let org_root = util::org_root()?;
    let mut file = GoalsFile::read(&org_root)?;

    if file.find(&slug).is_none() {
        return Err(Error::other(format!("goal '{slug}' not found")));
    }

    if !util::confirm(&format!("Remove goal '{slug}'?"), yes)? {
        return Ok(());
    }

    file.goals.retain(|g| g.slug != slug);
    file.write(&org_root)?;
    println!("Goal '{slug}' removed.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct GoalSummary {
    pub slug: String,
    pub name: String,
    pub deadline: Option<String>,
    pub owners: Vec<String>,
    pub track: Option<String>,
    pub nodes: Vec<String>,
}
