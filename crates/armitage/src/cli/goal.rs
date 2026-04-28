use armitage_core::goal::{Goal, GoalsFile};
use armitage_core::team::TeamFile;
use armitage_core::tree::find_org_root;
use chrono::NaiveDate;
use serde::Serialize;

use crate::error::Result;

// ---------------------------------------------------------------------------
// goal list
// ---------------------------------------------------------------------------

pub fn run_list(format: String) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let file = GoalsFile::read(&org_root)?;

    if file.goals.is_empty() {
        println!("No goals defined. Use `armitage goal add` to create one.");
        return Ok(());
    }

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&file.goals)
                .map_err(|e| crate::error::Error::Other(e.to_string()))?
        );
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
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let file = GoalsFile::read(&org_root)?;
    let team_file = TeamFile::read(&org_root).unwrap_or_default();

    let goal = file
        .find(&slug)
        .ok_or_else(|| crate::error::Error::Other(format!("goal '{slug}' not found")))?;

    if format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(goal)
                .map_err(|e| crate::error::Error::Other(e.to_string()))?
        );
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
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let mut file = GoalsFile::read(&org_root)?;

    if file.find(&slug).is_some() {
        return Err(crate::error::Error::Other(format!(
            "goal '{slug}' already exists"
        )));
    }

    let deadline = deadline
        .map(|d| {
            NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                .map_err(|_| crate::error::Error::Other(format!("invalid date '{d}'")))
        })
        .transpose()?;

    let goal = Goal {
        slug: slug.clone(),
        name,
        description,
        deadline,
        owners: parse_csv(owners),
        track,
        nodes: parse_csv(nodes),
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
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let mut file = GoalsFile::read(&org_root)?;

    let goal = file
        .find_mut(&slug)
        .ok_or_else(|| crate::error::Error::Other(format!("goal '{slug}' not found")))?;

    if let Some(n) = name {
        goal.name = n;
    }
    if let Some(d) = description {
        goal.description = Some(d);
    }
    if let Some(d) = deadline {
        goal.deadline = Some(
            NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                .map_err(|_| crate::error::Error::Other(format!("invalid date '{d}'")))?,
        );
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
    let cwd = std::env::current_dir()?;
    let org_root = find_org_root(&cwd)?;
    let mut file = GoalsFile::read(&org_root)?;

    if file.find(&slug).is_none() {
        return Err(crate::error::Error::Other(format!(
            "goal '{slug}' not found"
        )));
    }

    if !yes {
        eprint!("Remove goal '{slug}'? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
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

fn parse_csv(s: Option<String>) -> Vec<String> {
    s.map(|v| {
        v.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
