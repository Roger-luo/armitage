use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub github: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamFile {
    #[serde(default)]
    pub members: Vec<TeamMember>,
}

const TEAM_FILE: &str = "team.toml";

impl TeamFile {
    pub fn read(org_root: &Path) -> Result<Self> {
        let path = org_root.join(TEAM_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let parsed: Self =
            toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })?;
        parsed.validate_unique_github()?;
        Ok(parsed)
    }

    pub fn write(&self, org_root: &Path) -> Result<()> {
        self.validate_unique_github()?;
        let path = org_root.join(TEAM_FILE);
        let content = toml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn has(&self, github: &str) -> bool {
        self.members.iter().any(|m| m.github == github)
    }

    pub fn github_usernames(&self) -> Vec<String> {
        self.members.iter().map(|m| m.github.clone()).collect()
    }

    fn validate_unique_github(&self) -> Result<()> {
        let mut seen = std::collections::HashSet::new();
        for m in &self.members {
            if !seen.insert(&m.github) {
                return Err(Error::Other(format!(
                    "duplicate github username in team.toml: {}",
                    m.github
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_team_file() {
        let toml = r#"
            [[members]]
            github = "alice"
            name = "Alice Smith"
            role = "Engineer"

            [[members]]
            github = "bob"
            name = "Bob Jones"
        "#;
        let tf: TeamFile = toml::from_str(toml).expect("deserialize");
        assert_eq!(tf.members.len(), 2);
        assert_eq!(tf.members[0].github, "alice");
        assert_eq!(tf.members[0].role.as_deref(), Some("Engineer"));
        assert_eq!(tf.members[1].github, "bob");
        assert!(tf.members[1].role.is_none());
    }

    #[test]
    fn empty_file_returns_default() {
        let tf: TeamFile = toml::from_str("").expect("deserialize empty");
        assert!(tf.members.is_empty());
    }

    #[test]
    fn roundtrip() {
        let tf = TeamFile {
            members: vec![
                TeamMember {
                    github: "alice".to_string(),
                    name: "Alice".to_string(),
                    role: Some("Lead".to_string()),
                },
                TeamMember {
                    github: "bob".to_string(),
                    name: "Bob".to_string(),
                    role: None,
                },
            ],
        };
        let s = toml::to_string(&tf).expect("serialize");
        let parsed: TeamFile = toml::from_str(&s).expect("deserialize");
        assert_eq!(parsed.members, tf.members);
    }

    #[test]
    fn has_checks_github() {
        let tf = TeamFile {
            members: vec![TeamMember {
                github: "alice".to_string(),
                name: "Alice".to_string(),
                role: None,
            }],
        };
        assert!(tf.has("alice"));
        assert!(!tf.has("bob"));
    }

    #[test]
    fn duplicate_github_rejected() {
        let tf = TeamFile {
            members: vec![
                TeamMember {
                    github: "alice".to_string(),
                    name: "Alice".to_string(),
                    role: None,
                },
                TeamMember {
                    github: "alice".to_string(),
                    name: "Alice 2".to_string(),
                    role: None,
                },
            ],
        };
        assert!(tf.validate_unique_github().is_err());
    }
}
