use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueEntry {
    /// Issue reference in `owner/repo#number` format.
    #[serde(rename = "ref")]
    pub issue_ref: String,
    /// Issue title for human readability (not authoritative — GitHub is the source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IssuesFile {
    #[serde(default)]
    pub issues: Vec<IssueEntry>,
}

const ISSUES_FILE: &str = "issues.toml";

impl IssuesFile {
    pub fn read(node_dir: &Path) -> Result<Self> {
        let path = node_dir.join(ISSUES_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let parsed: Self =
            toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })?;
        Ok(parsed)
    }

    pub fn write(&self, node_dir: &Path) -> Result<()> {
        let path = node_dir.join(ISSUES_FILE);
        if self.issues.is_empty() {
            // Don't write empty files; remove if exists
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            return Ok(());
        }
        let content = toml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn has(&self, issue_ref: &str) -> bool {
        self.issues.iter().any(|e| e.issue_ref == issue_ref)
    }

    /// Add an issue if not already present. Returns true if added.
    pub fn add(&mut self, issue_ref: String, title: Option<String>) -> bool {
        if self.has(&issue_ref) {
            return false;
        }
        self.issues.push(IssueEntry { issue_ref, title });
        true
    }

    /// Remove an issue by ref. Returns true if removed.
    pub fn remove(&mut self, issue_ref: &str) -> bool {
        let before = self.issues.len();
        self.issues.retain(|e| e.issue_ref != issue_ref);
        self.issues.len() < before
    }

    pub fn len(&self) -> usize {
        self.issues.len()
    }

    pub fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_issues_file() {
        let toml = r#"
            [[issues]]
            ref = "acme/widget#42"
            title = "Fix login"

            [[issues]]
            ref = "acme/widget#57"
        "#;
        let f: IssuesFile = toml::from_str(toml).expect("deserialize");
        assert_eq!(f.issues.len(), 2);
        assert_eq!(f.issues[0].issue_ref, "acme/widget#42");
        assert_eq!(f.issues[0].title.as_deref(), Some("Fix login"));
        assert_eq!(f.issues[1].issue_ref, "acme/widget#57");
        assert!(f.issues[1].title.is_none());
    }

    #[test]
    fn empty_file_returns_default() {
        let f: IssuesFile = toml::from_str("").expect("deserialize empty");
        assert!(f.issues.is_empty());
    }

    #[test]
    fn add_deduplicates() {
        let mut f = IssuesFile::default();
        assert!(f.add("a/b#1".to_string(), Some("First".to_string())));
        assert!(!f.add("a/b#1".to_string(), Some("Duplicate".to_string())));
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn remove_works() {
        let mut f = IssuesFile::default();
        f.add("a/b#1".to_string(), None);
        f.add("a/b#2".to_string(), None);
        assert!(f.remove("a/b#1"));
        assert_eq!(f.len(), 1);
        assert!(!f.remove("a/b#1"));
    }

    #[test]
    fn roundtrip() {
        let f = IssuesFile {
            issues: vec![
                IssueEntry {
                    issue_ref: "acme/widget#1".to_string(),
                    title: Some("Bug".to_string()),
                },
                IssueEntry {
                    issue_ref: "acme/widget#2".to_string(),
                    title: None,
                },
            ],
        };
        let s = toml::to_string(&f).expect("serialize");
        let parsed: IssuesFile = toml::from_str(&s).expect("deserialize");
        assert_eq!(parsed.issues, f.issues);
    }
}
