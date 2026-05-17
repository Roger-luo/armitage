use std::path::PathBuf;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::toml_file::TomlFile;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointStatus {
    #[default]
    Planned,
    InProgress,
    Done,
    Dropped,
}

impl std::fmt::Display for CheckpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CheckpointStatus::Planned => "planned",
            CheckpointStatus::InProgress => "in-progress",
            CheckpointStatus::Done => "done",
            CheckpointStatus::Dropped => "dropped",
        };
        f.write_str(s)
    }
}

impl std::str::FromStr for CheckpointStatus {
    type Err = Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "planned" => Ok(Self::Planned),
            "in-progress" | "inprogress" | "in_progress" => Ok(Self::InProgress),
            "done" => Ok(Self::Done),
            "dropped" => Ok(Self::Dropped),
            other => Err(Error::Other(format!(
                "invalid checkpoint status '{other}': expected planned, in-progress, done, dropped"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub slug: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// e.g. "2026-Q2"
    pub target_quarter: String,
    #[serde(default)]
    pub status: CheckpointStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    /// Issue refs in `owner/repo#N` form.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

impl Checkpoint {
    pub fn effective_owners<'a>(&'a self, goal: &'a Goal) -> &'a [String] {
        if self.owners.is_empty() {
            &goal.owners
        } else {
            &self.owners
        }
    }

    pub fn effective_nodes<'a>(&'a self, goal: &'a Goal) -> &'a [String] {
        if self.nodes.is_empty() {
            &goal.nodes
        } else {
            &self.nodes
        }
    }
}

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
    /// Quarterly checkpoints that slice this long-running goal into OKRs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<Checkpoint>,
}

impl Goal {
    /// Checkpoints targeting `quarter` exactly.
    pub fn checkpoints_for_quarter(&self, quarter: &str) -> Vec<&Checkpoint> {
        self.checkpoints
            .iter()
            .filter(|d| d.target_quarter == quarter)
            .collect()
    }

    /// Checkpoints whose `target_quarter` is lexicographically before `quarter`
    /// AND whose status is Planned or InProgress (i.e. carry-over candidates).
    pub fn carried_over_into(&self, quarter: &str) -> Vec<&Checkpoint> {
        self.checkpoints
            .iter()
            .filter(|d| {
                d.target_quarter.as_str() < quarter
                    && matches!(
                        d.status,
                        CheckpointStatus::Planned | CheckpointStatus::InProgress
                    )
            })
            .collect()
    }

    pub fn find_checkpoint(&self, slug: &str) -> Option<&Checkpoint> {
        self.checkpoints.iter().find(|d| d.slug == slug)
    }

    pub fn find_checkpoint_mut(&mut self, slug: &str) -> Option<&mut Checkpoint> {
        self.checkpoints.iter_mut().find(|d| d.slug == slug)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalsFile {
    #[serde(default)]
    pub goals: Vec<Goal>,
}

impl TomlFile for GoalsFile {
    type Error = Error;

    fn file_name() -> &'static str {
        "goals.toml"
    }

    fn parse_error(path: PathBuf, source: toml::de::Error) -> Self::Error {
        Error::toml_parse(path, source)
    }

    fn default_when_missing() -> Option<Self> {
        Some(Self::default())
    }
}

impl GoalsFile {
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

/// Validate that a quarter string matches `^\d{4}-Q[1-4]$`.
pub fn validate_quarter(s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    let ok = bytes.len() == 7
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[4] == b'-'
        && bytes[5] == b'Q'
        && (b'1'..=b'4').contains(&bytes[6]);
    if ok {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid quarter '{s}': expected YYYY-Q[1-4] (e.g. 2026-Q2)"
        )))
    }
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
            checkpoints: vec![],
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

    #[test]
    fn round_trip_goals_with_checkpoints() {
        let d = Checkpoint {
            slug: "alpha".to_string(),
            name: "Alpha checkpoint".to_string(),
            description: Some("First slice".to_string()),
            target_quarter: "2026-Q2".to_string(),
            status: CheckpointStatus::InProgress,
            owners: vec!["bob".to_string()],
            nodes: vec!["abc/widget".to_string()],
            issues: vec!["acme/repo#1".to_string()],
        };
        let goal = Goal {
            slug: "demo".to_string(),
            name: "Demo Goal".to_string(),
            description: None,
            deadline: None,
            owners: vec!["alice".to_string()],
            track: None,
            nodes: vec!["abc/widget".to_string()],
            checkpoints: vec![d],
        };
        let file = GoalsFile { goals: vec![goal] };
        let serialized = toml::to_string_pretty(&file).unwrap();
        let deserialized: GoalsFile = toml::from_str(&serialized).unwrap();
        let g = &deserialized.goals[0];
        assert_eq!(g.checkpoints.len(), 1);
        let dl = &g.checkpoints[0];
        assert_eq!(dl.slug, "alpha");
        assert_eq!(dl.target_quarter, "2026-Q2");
        assert_eq!(dl.status, CheckpointStatus::InProgress);
        assert_eq!(dl.issues, vec!["acme/repo#1".to_string()]);
    }

    #[test]
    fn carried_over_lex_logic() {
        let mk = |slug: &str, q: &str, status: CheckpointStatus| Checkpoint {
            slug: slug.to_string(),
            name: slug.to_string(),
            description: None,
            target_quarter: q.to_string(),
            status,
            owners: vec![],
            nodes: vec![],
            issues: vec![],
        };
        let goal = Goal {
            slug: "g".into(),
            name: "g".into(),
            description: None,
            deadline: None,
            owners: vec![],
            track: None,
            nodes: vec![],
            checkpoints: vec![
                mk("a", "2026-Q1", CheckpointStatus::InProgress),
                mk("b", "2026-Q1", CheckpointStatus::Done),
                mk("c", "2025-Q4", CheckpointStatus::Planned),
                mk("d", "2026-Q2", CheckpointStatus::InProgress),
                mk("e", "2026-Q3", CheckpointStatus::InProgress),
                mk("f", "2026-Q1", CheckpointStatus::Dropped),
            ],
        };
        let carried: Vec<&str> = goal
            .carried_over_into("2026-Q2")
            .iter()
            .map(|d| d.slug.as_str())
            .collect();
        assert_eq!(carried, vec!["a", "c"]);

        let now: Vec<&str> = goal
            .checkpoints_for_quarter("2026-Q2")
            .iter()
            .map(|d| d.slug.as_str())
            .collect();
        assert_eq!(now, vec!["d"]);
    }

    #[test]
    fn effective_fallback_to_goal() {
        let goal = Goal {
            slug: "g".into(),
            name: "g".into(),
            description: None,
            deadline: None,
            owners: vec!["alice".into()],
            track: None,
            nodes: vec!["abc".into()],
            checkpoints: vec![],
        };
        let d_inherits = Checkpoint {
            slug: "x".into(),
            name: "x".into(),
            description: None,
            target_quarter: "2026-Q2".into(),
            status: CheckpointStatus::Planned,
            owners: vec![],
            nodes: vec![],
            issues: vec![],
        };
        assert_eq!(
            d_inherits.effective_owners(&goal),
            &["alice".to_string()][..]
        );
        assert_eq!(d_inherits.effective_nodes(&goal), &["abc".to_string()][..]);

        let d_overrides = Checkpoint {
            slug: "y".into(),
            name: "y".into(),
            description: None,
            target_quarter: "2026-Q2".into(),
            status: CheckpointStatus::Planned,
            owners: vec!["bob".into()],
            nodes: vec!["xyz".into()],
            issues: vec![],
        };
        assert_eq!(
            d_overrides.effective_owners(&goal),
            &["bob".to_string()][..]
        );
        assert_eq!(d_overrides.effective_nodes(&goal), &["xyz".to_string()][..]);
    }

    #[test]
    fn validate_quarter_ok() {
        assert!(validate_quarter("2026-Q2").is_ok());
        assert!(validate_quarter("2025-Q1").is_ok());
        assert!(validate_quarter("2030-Q4").is_ok());
    }

    #[test]
    fn validate_quarter_bad() {
        assert!(validate_quarter("2026-Q5").is_err());
        assert!(validate_quarter("26-Q2").is_err());
        assert!(validate_quarter("2026-q2").is_err());
        assert!(validate_quarter("2026Q2").is_err());
    }
}
