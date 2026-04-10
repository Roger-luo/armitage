use std::collections::HashMap;
use std::path::Path;

use chrono::NaiveDate;
use serde::Serialize;

use armitage_core::tree::NodeEntry;
use armitage_milestones::milestone::{MilestoneFile, MilestoneType};

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
    pub children: Vec<ChartNode>,
    pub milestones: Vec<ChartMilestone>,
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

fn date_to_str(d: &NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
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
            milestone_type: match m.milestone_type {
                MilestoneType::Checkpoint => "checkpoint".to_string(),
                MilestoneType::Okr => "okr".to_string(),
            },
        })
        .collect()
}

/// Build a `ChartNode` tree recursively from a parent-to-children map.
fn build_node(entry: &NodeEntry, children_map: &HashMap<String, Vec<&NodeEntry>>) -> ChartNode {
    // Recursively build children first (bottom-up for effective timeline).
    let children: Vec<ChartNode> = children_map
        .get(&entry.path)
        .map(|kids| {
            kids.iter()
                .map(|kid| build_node(kid, children_map))
                .collect()
        })
        .unwrap_or_default();

    let has_timeline = entry.node.timeline.is_some();
    let start = entry.node.timeline.as_ref().map(|t| date_to_str(&t.start));
    let end = entry.node.timeline.as_ref().map(|t| date_to_str(&t.end));

    // Compute effective timeline: own timeline, or min/max of children's effective timelines.
    let (eff_start, eff_end) = if has_timeline {
        (start.clone(), end.clone())
    } else {
        let child_starts: Vec<&str> = children
            .iter()
            .filter_map(|c| c.eff_start.as_deref())
            .collect();
        let child_ends: Vec<&str> = children
            .iter()
            .filter_map(|c| c.eff_end.as_deref())
            .collect();
        let eff_s = child_starts.into_iter().min().map(String::from);
        let eff_e = child_ends.into_iter().max().map(String::from);
        (eff_s, eff_e)
    };

    let milestones = read_milestones(&entry.dir);

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
    }
}

/// Build chart data from the org's node entries.
///
/// Walks the flat `NodeEntry` list, reconstructs the tree hierarchy, reads
/// milestones, computes effective timelines, and returns the full `ChartData`
/// for embedding in the HTML template.
pub fn build_chart_data(
    entries: &[NodeEntry],
    org_root: &Path,
    org_name: &str,
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

    let _ = org_root; // reserved for future use (e.g. reading additional config)

    // Build the tree from root entries.
    let nodes: Vec<ChartNode> = root_entries
        .iter()
        .map(|entry| build_node(entry, &children_map))
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
        let data = build_chart_data(&entries, root, "test").unwrap();

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
        let data = build_chart_data(&entries, root, "test").unwrap();

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
        let data = build_chart_data(&entries, root, "test").unwrap();

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
        let data = build_chart_data(&entries, root, "test").unwrap();

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
        let data = build_chart_data(&entries, root, "test").unwrap();

        let lonely = &data.nodes[0];
        assert!(!lonely.has_timeline);
        assert!(lonely.eff_start.is_none());
        assert!(lonely.eff_end.is_none());
    }
}
