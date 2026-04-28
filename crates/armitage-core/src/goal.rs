use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional hard deadline. Absent means the goal has no fixed date yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
    /// Optional tracking issue in `owner/repo#N` format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    /// Roadmap node paths that contribute to this goal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalsFile {
    #[serde(default)]
    pub goals: Vec<Goal>,
}

const GOALS_FILE: &str = "goals.toml";

impl GoalsFile {
    pub fn path(org_root: &Path) -> PathBuf {
        org_root.join(GOALS_FILE)
    }

    pub fn read(org_root: &Path) -> Result<Self> {
        let path = Self::path(org_root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })
    }

    pub fn write(&self, org_root: &Path) -> Result<()> {
        let path = Self::path(org_root);
        let content = toml::to_string_pretty(self).map_err(|e| Error::Other(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn find(&self, slug: &str) -> Option<&Goal> {
        self.goals.iter().find(|g| g.slug == slug)
    }

    pub fn find_mut(&mut self, slug: &str) -> Option<&mut Goal> {
        self.goals.iter_mut().find(|g| g.slug == slug)
    }
}

/// True if `node_path` is one of `goal_nodes` or a child of one.
pub fn node_in_goal(node_path: &str, goal_nodes: &[String]) -> bool {
    goal_nodes
        .iter()
        .any(|g| node_path == g || node_path.starts_with(&format!("{g}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_in_goal_exact() {
        let nodes = vec!["abc/widget".to_string()];
        assert!(node_in_goal("abc/widget", &nodes));
        assert!(!node_in_goal("abc/gadget", &nodes));
    }

    #[test]
    fn node_in_goal_child() {
        let nodes = vec!["abc/widget".to_string()];
        assert!(node_in_goal("abc/widget/sub", &nodes));
        assert!(!node_in_goal("abc/widgetx", &nodes));
    }

    #[test]
    fn round_trip_goals_file() {
        let goal = Goal {
            slug: "demo".to_string(),
            name: "Demo Goal".to_string(),
            description: Some("A test goal".to_string()),
            deadline: NaiveDate::from_ymd_opt(2026, 12, 31),
            owners: vec!["alice".to_string()],
            track: None,
            nodes: vec!["abc/widget".to_string()],
        };
        let file = GoalsFile { goals: vec![goal] };
        let serialized = toml::to_string_pretty(&file).unwrap();
        let deserialized: GoalsFile = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.goals.len(), 1);
        assert_eq!(deserialized.goals[0].slug, "demo");
        assert_eq!(
            deserialized.goals[0].deadline,
            NaiveDate::from_ymd_opt(2026, 12, 31)
        );
    }
}
