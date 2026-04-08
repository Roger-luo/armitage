use crate::error::Error;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_issue: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Timeline>,
    #[serde(default)]
    pub status: NodeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl Timeline {
    pub fn contains(&self, other: &Timeline) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    #[default]
    Active,
    Completed,
    Paused,
    Cancelled,
}

impl fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeStatus::Active => write!(f, "active"),
            NodeStatus::Completed => write!(f, "completed"),
            NodeStatus::Paused => write!(f, "paused"),
            NodeStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

impl IssueRef {
    pub fn parse(s: &str) -> Result<Self, Error> {
        let Some((owner_repo, num_str)) = s.split_once('#') else {
            return Err(Error::InvalidIssueRef(s.to_string()));
        };
        let Some((owner, repo)) = owner_repo.split_once('/') else {
            return Err(Error::InvalidIssueRef(s.to_string()));
        };
        let number: u64 = num_str
            .parse()
            .map_err(|_| Error::InvalidIssueRef(s.to_string()))?;
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
            number,
        })
    }

    pub fn repo_full(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

impl fmt::Display for IssueRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}#{}", self.owner, self.repo, self.number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_node() {
        let toml = r#"
            name = "my-node"
            description = "A full node"
            github_issue = "owner/repo#42"
            labels = ["team:alpha", "priority:high"]
            repos = ["owner/repo1", "owner/repo2"]
            status = "completed"

            [timeline]
            start = "2025-01-01"
            end = "2025-06-30"
        "#;
        let node: Node = toml::from_str(toml).expect("deserialize full node");
        assert_eq!(node.name, "my-node");
        assert_eq!(node.description, "A full node");
        assert_eq!(node.github_issue.as_deref(), Some("owner/repo#42"));
        assert_eq!(node.labels, vec!["team:alpha", "priority:high"]);
        assert_eq!(node.repos, vec!["owner/repo1", "owner/repo2"]);
        assert_eq!(node.status, NodeStatus::Completed);
        let tl = node.timeline.expect("timeline present");
        assert_eq!(tl.start, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        assert_eq!(tl.end, NaiveDate::from_ymd_opt(2025, 6, 30).unwrap());
    }

    #[test]
    fn deserialize_minimal_node() {
        let toml = r#"
            name = "bare"
            description = "Minimal node"
        "#;
        let node: Node = toml::from_str(toml).expect("deserialize minimal node");
        assert_eq!(node.name, "bare");
        assert_eq!(node.description, "Minimal node");
        assert!(node.github_issue.is_none());
        assert!(node.labels.is_empty());
        assert!(node.repos.is_empty());
        assert!(node.timeline.is_none());
        assert_eq!(node.status, NodeStatus::Active);
    }

    #[test]
    fn roundtrip_node() {
        let original = Node {
            name: "round-trip".to_string(),
            description: "Testing roundtrip".to_string(),
            github_issue: Some("acme/widget#7".to_string()),
            labels: vec!["area:core".to_string()],
            repos: vec!["acme/widget".to_string()],
            timeline: Some(Timeline {
                start: NaiveDate::from_ymd_opt(2025, 3, 1).unwrap(),
                end: NaiveDate::from_ymd_opt(2025, 9, 30).unwrap(),
            }),
            status: NodeStatus::Paused,
        };
        let serialized = toml::to_string(&original).expect("serialize node");
        let deserialized: Node = toml::from_str(&serialized).expect("deserialize node");
        assert_eq!(deserialized.name, original.name);
        assert_eq!(deserialized.description, original.description);
        assert_eq!(deserialized.github_issue, original.github_issue);
        assert_eq!(deserialized.labels, original.labels);
        assert_eq!(deserialized.repos, original.repos);
        assert_eq!(deserialized.status, original.status);
        let tl = deserialized
            .timeline
            .expect("timeline present after roundtrip");
        assert_eq!(tl.start, NaiveDate::from_ymd_opt(2025, 3, 1).unwrap());
        assert_eq!(tl.end, NaiveDate::from_ymd_opt(2025, 9, 30).unwrap());
    }

    #[test]
    fn parse_issue_ref_valid() {
        let r = IssueRef::parse("anthropic/gemini#123").expect("valid ref");
        assert_eq!(r.owner, "anthropic");
        assert_eq!(r.repo, "gemini");
        assert_eq!(r.number, 123);
        assert_eq!(r.repo_full(), "anthropic/gemini");
    }

    #[test]
    fn parse_issue_ref_invalid() {
        // Missing '#'
        assert!(IssueRef::parse("anthropic/gemini").is_err());
        // Missing '/'
        assert!(IssueRef::parse("anthropic#123").is_err());
        // Non-numeric issue number
        assert!(IssueRef::parse("anthropic/gemini#abc").is_err());
        // Completely empty
        assert!(IssueRef::parse("").is_err());
        // Just a hash
        assert!(IssueRef::parse("#123").is_err());
    }

    #[test]
    fn issue_ref_display() {
        let r = IssueRef {
            owner: "anthropic".to_string(),
            repo: "gemini".to_string(),
            number: 42,
        };
        assert_eq!(r.to_string(), "anthropic/gemini#42");
    }

    #[test]
    fn timeline_contains() {
        let parent = Timeline {
            start: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
        };
        let child = Timeline {
            start: NaiveDate::from_ymd_opt(2025, 3, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2025, 6, 30).unwrap(),
        };
        assert!(parent.contains(&child), "parent should contain child");

        let outside = Timeline {
            start: NaiveDate::from_ymd_opt(2024, 11, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2025, 2, 28).unwrap(),
        };
        assert!(
            !parent.contains(&outside),
            "parent should not contain outside"
        );

        // Exact same range -- a timeline contains itself
        assert!(parent.contains(&parent));
    }
}
