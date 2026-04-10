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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Timeline>,
    #[serde(default)]
    pub status: NodeStatus,
}

/// Max single-line string length before converting to multi-line TOML.
const MULTILINE_THRESHOLD: usize = 80;

impl Node {
    /// Serialize to TOML, using multi-line strings for long values.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        let raw = toml::to_string(self)?;
        Ok(to_multiline_toml(&raw))
    }
}

/// Post-process serialized TOML to convert long string values to multi-line (`"""`)
/// with word-wrapped content.
fn to_multiline_toml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        if let Some((key, val)) = line.split_once(" = ") {
            // Only convert basic strings (starting with `"`, not arrays or inline tables)
            let trimmed = val.trim();
            if trimmed.starts_with('"')
                && trimmed.ends_with('"')
                && !trimmed.starts_with("\"\"\"")
                && trimmed.len() > MULTILINE_THRESHOLD
            {
                // Strip outer quotes to get the raw escaped content
                let inner = &trimmed[1..trimmed.len() - 1];
                let lines = wrap_str(inner, MULTILINE_THRESHOLD);
                out.push_str(key);
                out.push_str(" = \"\"\"\n");
                for (i, wrapped) in lines.iter().enumerate() {
                    out.push_str(wrapped);
                    if i + 1 < lines.len() {
                        // Trailing backslash joins lines in TOML multi-line strings
                        out.push_str(" \\\n");
                    } else {
                        out.push_str("\"\"\"\n");
                    }
                }
                if lines.is_empty() {
                    out.push_str("\"\"\"\n");
                }
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Word-wrap a string at `width`, breaking on spaces.
fn wrap_str(text: &str, width: usize) -> Vec<&str> {
    if width == 0 || text.is_empty() {
        return vec![text];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut last_space = 0;
    let mut col = 0;
    for (i, ch) in text.char_indices() {
        if ch == ' ' {
            last_space = i;
        }
        col += 1;
        if col > width {
            if last_space > start {
                lines.push(&text[start..last_space]);
                start = last_space + 1;
            } else {
                // No space found — hard break
                lines.push(&text[start..i]);
                start = i;
            }
            col = text[start..=i].chars().count();
            last_space = start;
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
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
            owners = ["alice", "bob"]
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
        assert_eq!(node.owners, vec!["alice", "bob"]);
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
        assert!(node.owners.is_empty());
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
            owners: vec!["alice".to_string()],
            team: Some("circuit".to_string()),
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
        assert_eq!(deserialized.owners, original.owners);
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

    #[test]
    fn to_toml_uses_multiline_with_wrapping_for_long_description() {
        let desc = "Quantum circuit development: the bloqade-circuit Python SDK for neutral atom quantum computing. Core IR (squin), QEC, and more.";
        let node = Node {
            name: "test".to_string(),
            description: desc.to_string(),
            github_issue: None,
            labels: vec![],
            repos: vec![],
            owners: vec![],
            team: None,
            timeline: None,
            status: NodeStatus::Active,
        };
        let toml_str = node.to_toml().expect("serialize");
        assert!(
            toml_str.contains("\"\"\""),
            "long description should use multi-line string"
        );
        // The content inside """ should have line breaks from word-wrapping
        let inside_quotes: &str = toml_str.split("\"\"\"").nth(1).expect("multi-line content");
        let lines: Vec<&str> = inside_quotes.lines().filter(|l| !l.is_empty()).collect();
        assert!(
            lines.len() > 1,
            "multi-line content should be word-wrapped into multiple lines, got: {lines:?}"
        );
        // Each line should be <= threshold (allow some slack for words)
        for line in &lines {
            assert!(
                line.len() <= 120,
                "line too long ({} chars): {line}",
                line.len()
            );
        }
        // Verify it roundtrips correctly
        let parsed: Node = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.description, desc);
    }

    #[test]
    fn to_toml_keeps_short_description_inline() {
        let node = Node {
            name: "test".to_string(),
            description: "Short".to_string(),
            github_issue: None,
            labels: vec![],
            repos: vec![],
            owners: vec![],
            team: None,
            timeline: None,
            status: NodeStatus::Active,
        };
        let toml_str = node.to_toml().expect("serialize");
        assert!(
            !toml_str.contains("\"\"\""),
            "short description should stay inline"
        );
    }
}
