use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelDef {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// When set, this label is only pushed to these repos. When empty/absent, pushed everywhere.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
    /// When true, this label is kept as-is: excluded from LLM reconciliation and renaming.
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelsFile {
    #[serde(default)]
    pub labels: Vec<LabelDef>,
}

const LABELS_FILE: &str = "labels.toml";

impl LabelsFile {
    pub fn read(org_root: &Path) -> Result<Self> {
        let path = org_root.join(LABELS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let parsed: Self =
            toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })?;
        parsed.validate_unique_names()?;
        Ok(parsed)
    }

    pub fn write(&self, org_root: &Path) -> Result<()> {
        self.validate_unique_names()?;
        let path = org_root.join(LABELS_FILE);
        let content = toml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn has(&self, name: &str) -> bool {
        self.labels.iter().any(|l| l.name == name)
    }

    pub fn names(&self) -> Vec<String> {
        self.labels.iter().map(|l| l.name.clone()).collect()
    }

    pub fn add(&mut self, name: String, description: String) {
        if !self.has(&name) {
            self.labels.push(LabelDef {
                name,
                description,
                color: None,
                repos: vec![],
                pinned: false,
            });
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.labels.retain(|l| l.name != name);
    }

    pub fn upsert(&mut self, label: LabelDef) {
        if let Some(existing) = self
            .labels
            .iter_mut()
            .find(|existing| existing.name == label.name)
        {
            *existing = label;
        } else {
            self.labels.push(label);
        }
    }

    fn validate_unique_names(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for label in &self.labels {
            if !seen.insert(label.name.clone()) {
                return Err(Error::Other(format!(
                    "duplicate label name in labels.toml: {}",
                    label.name
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn roundtrip_preserves_optional_color() {
        let tmp = TempDir::new().unwrap();
        let lf = LabelsFile {
            labels: vec![LabelDef {
                name: "bug".to_string(),
                description: "Something is broken".to_string(),
                color: Some("D73A4A".to_string()),
                repos: vec![],
                pinned: false,
            }],
        };

        lf.write(tmp.path()).unwrap();
        let loaded = LabelsFile::read(tmp.path()).unwrap();

        assert_eq!(loaded.labels[0].color.as_deref(), Some("D73A4A"));
    }

    #[test]
    fn roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut lf = LabelsFile::default();
        lf.add("bug".to_string(), "Something is broken".to_string());
        lf.add("team:alpha".to_string(), "Alpha team".to_string());
        lf.write(tmp.path()).unwrap();

        let loaded = LabelsFile::read(tmp.path()).unwrap();
        assert_eq!(loaded.labels.len(), 2);
        assert!(loaded.has("bug"));
        assert!(loaded.has("team:alpha"));
        assert!(!loaded.has("nonexistent"));
    }

    #[test]
    fn missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let lf = LabelsFile::read(tmp.path()).unwrap();
        assert!(lf.labels.is_empty());
    }

    #[test]
    fn add_is_idempotent() {
        let mut lf = LabelsFile::default();
        lf.add("bug".to_string(), "desc".to_string());
        lf.add("bug".to_string(), "different desc".to_string());
        assert_eq!(lf.labels.len(), 1);
    }

    #[test]
    fn upsert_updates_existing_label_by_name() {
        let mut lf = LabelsFile::default();
        lf.upsert(LabelDef {
            name: "bug".to_string(),
            description: "Old".to_string(),
            color: Some("AAAAAA".to_string()),
            repos: vec![],
            pinned: false,
        });
        lf.upsert(LabelDef {
            name: "bug".to_string(),
            description: "New".to_string(),
            color: Some("BBBBBB".to_string()),
            repos: vec![],
            pinned: false,
        });

        assert_eq!(lf.labels.len(), 1);
        assert_eq!(lf.labels[0].description, "New");
        assert_eq!(lf.labels[0].color.as_deref(), Some("BBBBBB"));
    }

    #[test]
    fn duplicate_names_in_file_are_rejected() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("labels.toml"),
            r#"
                [[labels]]
                name = "bug"
                description = "First"

                [[labels]]
                name = "bug"
                description = "Second"
            "#,
        )
        .unwrap();

        let err = LabelsFile::read(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("duplicate"));
        assert!(err.contains("bug"));
    }

    #[test]
    fn names_returns_sorted_list() {
        let mut lf = LabelsFile::default();
        lf.add("c-label".to_string(), String::new());
        lf.add("a-label".to_string(), String::new());
        let names = lf.names();
        assert_eq!(names, vec!["c-label", "a-label"]);
    }
}
