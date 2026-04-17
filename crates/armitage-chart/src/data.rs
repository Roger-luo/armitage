use std::collections::HashMap;
use std::path::Path;

use chrono::NaiveDate;
use serde::Serialize;

use armitage_core::issues::IssuesFile;
use armitage_core::tree::NodeEntry;
use armitage_milestones::milestone::MilestoneFile;

use crate::error::Result;

/// A single node in the chart hierarchy, serialized to JSON for the browser.
#[derive(Debug, Clone, Serialize)]
pub struct ChartNode {
    pub path: String,
    pub name: String,
    pub description: String,
    pub status: String,
    /// ISO date from the node's own timeline (None if no timeline set).
    pub start: Option<String>,
    pub end: Option<String>,
    /// Effective timeline: own timeline, or derived from children.
    pub eff_start: Option<String>,
    pub eff_end: Option<String>,
    pub has_timeline: bool,
    pub owners: Vec<String>,
    pub team: Option<String>,
    pub children: Vec<Self>,
    pub milestones: Vec<ChartMilestone>,
    pub issues: Vec<ChartIssue>,
    pub overflow_start: Option<String>,
    pub overflow_end: Option<String>,
    pub issue_start: Option<String>,
    pub issue_end: Option<String>,
}

/// An issue reference for the chart panel.
#[derive(Debug, Clone, Serialize)]
pub struct ChartIssue {
    pub issue_ref: String,
    pub title: Option<String>,
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    /// "OPEN" or "CLOSED"
    pub state: Option<String>,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub author: Option<String>,
    pub assignees: Vec<String>,
    pub is_pr: bool,
}

/// A milestone or OKR marker on the timeline.
#[derive(Debug, Clone, Serialize)]
pub struct ChartMilestone {
    pub name: String,
    pub date: String,
    pub description: String,
    pub milestone_type: String,
}

/// Top-level chart data embedded in the HTML page.
#[derive(Debug, Clone, Serialize)]
pub struct ChartData {
    pub nodes: Vec<ChartNode>,
    pub org_name: String,
    pub global_start: Option<String>,
    pub global_end: Option<String>,
}

/// Project metadata for a single issue, passed into chart data builder.
#[derive(Debug, Clone, Default)]
pub struct IssueDates {
    pub start_date: Option<String>,
    pub target_date: Option<String>,
    pub state: Option<String>,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub author: Option<String>,
    pub assignees: Vec<String>,
    pub is_pr: bool,
}

fn date_to_str(d: &NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn read_issues(node_dir: &Path, dates_map: &HashMap<String, IssueDates>) -> Vec<ChartIssue> {
    let Ok(file) = IssuesFile::read(node_dir) else {
        return vec![];
    };
    file.issues
        .into_iter()
        .filter_map(|e| {
            let dates = dates_map.get(&e.issue_ref);
            let state = dates.and_then(|d| d.state.clone());
            // Skip closed issues
            if state.as_deref() == Some("CLOSED") {
                return None;
            }
            Some(ChartIssue {
                issue_ref: e.issue_ref,
                title: e.title,
                start_date: dates.and_then(|d| d.start_date.clone()),
                target_date: dates.and_then(|d| d.target_date.clone()),
                state,
                description: dates.and_then(|d| d.description.clone()),
                labels: dates.map(|d| d.labels.clone()).unwrap_or_default(),
                author: dates.and_then(|d| d.author.clone()),
                assignees: dates.map(|d| d.assignees.clone()).unwrap_or_default(),
                is_pr: dates.map(|d| d.is_pr).unwrap_or(false),
            })
        })
        .collect()
}

fn read_milestones(node_dir: &Path) -> Vec<ChartMilestone> {
    let path = node_dir.join("milestones.toml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    let Ok(mf) = toml::from_str::<MilestoneFile>(&content) else {
        return vec![];
    };
    mf.milestones
        .into_iter()
        .map(|m| ChartMilestone {
            name: m.name,
            date: date_to_str(&m.date),
            description: m.description,
            milestone_type: m.milestone_type.to_string(),
        })
        .collect()
}

/// Build a `ChartNode` tree recursively from a parent-to-children map.
fn build_node(
    entry: &NodeEntry,
    children_map: &HashMap<String, Vec<&NodeEntry>>,
    dates_map: &HashMap<String, IssueDates>,
) -> ChartNode {
    let children: Vec<ChartNode> = children_map
        .get(&entry.path)
        .map(|kids| {
            kids.iter()
                .map(|kid| build_node(kid, children_map, dates_map))
                .collect()
        })
        .unwrap_or_default();

    let has_timeline = entry.node.timeline.is_some();
    let start = entry.node.timeline.as_ref().map(|t| date_to_str(&t.start));
    let end = entry.node.timeline.as_ref().map(|t| date_to_str(&t.end));

    let (eff_start, eff_end) = if has_timeline {
        (start.clone(), end.clone())
    } else {
        let eff_s = children
            .iter()
            .filter_map(|c| c.eff_start.as_deref())
            .min()
            .map(String::from);
        let eff_e = children
            .iter()
            .filter_map(|c| c.eff_end.as_deref())
            .max()
            .map(String::from);
        (eff_s, eff_e)
    };

    let milestones = read_milestones(&entry.dir);
    let issues = read_issues(&entry.dir, dates_map);

    let own_issue_start = issues.iter().filter_map(|i| i.start_date.as_deref()).min();
    let child_issue_start = children.iter().filter_map(|c| c.issue_start.as_deref());
    let issue_start = own_issue_start
        .into_iter()
        .chain(child_issue_start)
        .min()
        .map(String::from);

    let own_issue_end = issues.iter().filter_map(|i| i.target_date.as_deref()).max();
    let child_issue_end = children.iter().filter_map(|c| c.issue_end.as_deref());
    let issue_end = own_issue_end
        .into_iter()
        .chain(child_issue_end)
        .max()
        .map(String::from);
    // Compute overflow: max of own issues overshooting + children's overflows
    let own_overflow = end.as_deref().and_then(|node_end| {
        issues
            .iter()
            .filter_map(|i| i.target_date.as_deref())
            .filter(|t| *t > node_end)
            .max()
            .map(String::from)
    });
    let child_overflow = children
        .iter()
        .filter_map(|c| c.overflow_end.as_deref())
        .max()
        .map(String::from);
    let overflow_end = [own_overflow.as_deref(), child_overflow.as_deref()]
        .into_iter()
        .flatten()
        .max()
        .map(String::from);

    // overflow_start = earliest deadline that was violated (where blue ends, red begins).
    // For own overflow: the node's end date. For children: their overflow_start.
    let own_overflow_start = if own_overflow.is_some() {
        end.as_deref().map(String::from)
    } else {
        None
    };
    let child_overflow_start = children
        .iter()
        .filter_map(|c| c.overflow_start.as_deref())
        .min()
        .map(String::from);
    let overflow_start = [
        own_overflow_start.as_deref(),
        child_overflow_start.as_deref(),
    ]
    .into_iter()
    .flatten()
    .min()
    .map(String::from);

    ChartNode {
        path: entry.path.clone(),
        name: entry.node.name.clone(),
        description: entry.node.description.clone(),
        status: entry.node.status.to_string(),
        start,
        end,
        eff_start,
        eff_end,
        has_timeline,
        owners: entry.node.owners.clone(),
        team: entry.node.team.clone(),
        children,
        milestones,
        issues,
        overflow_start,
        overflow_end,
        issue_start,
        issue_end,
    }
}

/// Build chart data from the org's node entries.
///
/// Walks the flat `NodeEntry` list, reconstructs the tree hierarchy, reads
/// milestones, computes effective timelines, and returns the full `ChartData`
/// for embedding in the HTML template.
pub fn build_chart_data(
    entries: &[NodeEntry],
    org_name: &str,
    issue_dates: &HashMap<String, IssueDates>,
) -> Result<ChartData> {
    // Group entries by parent path.
    let mut children_map: HashMap<String, Vec<&NodeEntry>> = HashMap::new();
    let mut root_entries: Vec<&NodeEntry> = Vec::new();

    for entry in entries {
        if let Some(slash) = entry.path.rfind('/') {
            let parent = &entry.path[..slash];
            children_map
                .entry(parent.to_string())
                .or_default()
                .push(entry);
        } else {
            root_entries.push(entry);
        }
    }

    // Build the tree from root entries.
    let nodes: Vec<ChartNode> = root_entries
        .iter()
        .map(|entry| build_node(entry, &children_map, issue_dates))
        .collect();

    // Compute global date range across all effective timelines.
    let global_start = nodes
        .iter()
        .filter_map(|n| n.eff_start.as_deref())
        .min()
        .map(String::from);
    let global_end = nodes
        .iter()
        .filter_map(|n| n.eff_end.as_deref())
        .max()
        .map(String::from);

    Ok(ChartData {
        nodes,
        org_name: org_name.to_string(),
        global_start,
        global_end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use armitage_core::node::{Node, NodeStatus, Timeline};
    use chrono::NaiveDate;
    use std::fs;
    use tempfile::TempDir;

    fn make_node(name: &str, timeline: Option<(NaiveDate, NaiveDate)>) -> Node {
        Node {
            name: name.to_string(),
            description: format!("{name} description"),
            github_issue: None,
            labels: vec![],
            repos: vec![],
            owners: vec![],
            team: None,
            timeline: timeline.map(|(start, end)| Timeline { start, end }),
            status: NodeStatus::Active,
            triage_hint: None,
        }
    }

    fn d(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn write_node(dir: &Path, node: &Node) {
        fs::create_dir_all(dir).unwrap();
        let toml = node.to_toml().unwrap();
        fs::write(dir.join("node.toml"), toml).unwrap();
    }

    #[test]
    fn builds_hierarchy_from_flat_entries() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create org config
        fs::write(
            root.join("armitage.toml"),
            "[org]\nname = \"test\"\ngithub_orgs = []\n",
        )
        .unwrap();

        // initiative (no timeline)
        let init_node = make_node("Init", None);
        write_node(&root.join("init"), &init_node);

        // project under initiative (has timeline)
        let proj_node = make_node("Proj", Some((d(2026, 1, 1), d(2026, 6, 30))));
        write_node(&root.join("init/proj"), &proj_node);

        let entries = armitage_core::tree::walk_nodes(root).unwrap();
        let data = build_chart_data(&entries, "test", &HashMap::new()).unwrap();

        assert_eq!(data.nodes.len(), 1);
        let init = &data.nodes[0];
        assert_eq!(init.name, "Init");
        assert!(!init.has_timeline);
        assert_eq!(init.eff_start.as_deref(), Some("2026-01-01"));
        assert_eq!(init.eff_end.as_deref(), Some("2026-06-30"));

        assert_eq!(init.children.len(), 1);
        let proj = &init.children[0];
        assert_eq!(proj.name, "Proj");
        assert!(proj.has_timeline);
        assert_eq!(proj.start.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn effective_timeline_from_multiple_children() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("armitage.toml"),
            "[org]\nname = \"test\"\ngithub_orgs = []\n",
        )
        .unwrap();

        let parent = make_node("Parent", None);
        write_node(&root.join("parent"), &parent);

        let child_a = make_node("A", Some((d(2026, 3, 1), d(2026, 6, 30))));
        write_node(&root.join("parent/a"), &child_a);

        let child_b = make_node("B", Some((d(2026, 1, 15), d(2026, 9, 30))));
        write_node(&root.join("parent/b"), &child_b);

        let entries = armitage_core::tree::walk_nodes(root).unwrap();
        let data = build_chart_data(&entries, "test", &HashMap::new()).unwrap();

        let parent = &data.nodes[0];
        assert_eq!(parent.eff_start.as_deref(), Some("2026-01-15"));
        assert_eq!(parent.eff_end.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn global_date_range() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("armitage.toml"),
            "[org]\nname = \"test\"\ngithub_orgs = []\n",
        )
        .unwrap();

        let a = make_node("A", Some((d(2026, 1, 1), d(2026, 6, 30))));
        write_node(&root.join("a"), &a);

        let b = make_node("B", Some((d(2026, 4, 1), d(2026, 12, 31))));
        write_node(&root.join("b"), &b);

        let entries = armitage_core::tree::walk_nodes(root).unwrap();
        let data = build_chart_data(&entries, "test", &HashMap::new()).unwrap();

        assert_eq!(data.global_start.as_deref(), Some("2026-01-01"));
        assert_eq!(data.global_end.as_deref(), Some("2026-12-31"));
    }

    #[test]
    fn reads_milestones() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("armitage.toml"),
            "[org]\nname = \"test\"\ngithub_orgs = []\n",
        )
        .unwrap();

        let node = make_node("Proj", Some((d(2026, 1, 1), d(2026, 12, 31))));
        write_node(&root.join("proj"), &node);

        fs::write(
            root.join("proj/milestones.toml"),
            r#"[[milestone]]
name = "Alpha"
date = "2026-03-31"
description = "Alpha release"
type = "checkpoint"

[[milestone]]
name = "OKR Q2"
date = "2026-06-30"
description = "Q2 target"
type = "okr"
"#,
        )
        .unwrap();

        let entries = armitage_core::tree::walk_nodes(root).unwrap();
        let data = build_chart_data(&entries, "test", &HashMap::new()).unwrap();

        let proj = &data.nodes[0];
        assert_eq!(proj.milestones.len(), 2);
        assert_eq!(proj.milestones[0].name, "Alpha");
        assert_eq!(proj.milestones[0].milestone_type, "checkpoint");
        assert_eq!(proj.milestones[1].name, "OKR Q2");
        assert_eq!(proj.milestones[1].milestone_type, "okr");
    }

    #[test]
    fn no_timeline_no_children_yields_no_effective() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("armitage.toml"),
            "[org]\nname = \"test\"\ngithub_orgs = []\n",
        )
        .unwrap();

        let node = make_node("Lonely", None);
        write_node(&root.join("lonely"), &node);

        let entries = armitage_core::tree::walk_nodes(root).unwrap();
        let data = build_chart_data(&entries, "test", &HashMap::new()).unwrap();

        let lonely = &data.nodes[0];
        assert!(!lonely.has_timeline);
        assert!(lonely.eff_start.is_none());
        assert!(lonely.eff_end.is_none());
    }
}
