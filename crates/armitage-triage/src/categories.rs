use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DismissedCategories {
    #[serde(default)]
    pub dismissed: Vec<String>,
}

fn categories_path(org_root: &Path) -> PathBuf {
    org_root
        .join(".armitage")
        .join("triage")
        .join("dismissed-categories.toml")
}

pub fn read_dismissed(org_root: &Path) -> Result<DismissedCategories> {
    let path = categories_path(org_root);
    if !path.exists() {
        return Ok(DismissedCategories::default());
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|e| Error::Other(format!("parse dismissed-categories: {e}")))
}

pub fn write_dismissed(org_root: &Path, dc: &DismissedCategories) -> Result<()> {
    let path = categories_path(org_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string(dc).map_err(|e| Error::Other(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn is_dismissed(dc: &DismissedCategories, category: &str) -> bool {
    dc.dismissed.iter().any(|d| d == category)
}

pub fn dismiss(org_root: &Path, category: &str) -> Result<bool> {
    let mut dc = read_dismissed(org_root)?;
    if is_dismissed(&dc, category) {
        return Ok(false); // already dismissed
    }
    dc.dismissed.push(category.to_string());
    dc.dismissed.sort();
    write_dismissed(org_root, &dc)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_write_dismissed_categories() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        std::fs::create_dir_all(org.join(".armitage").join("triage")).unwrap();

        // Empty initially
        let dc = read_dismissed(org).unwrap();
        assert!(dc.dismissed.is_empty());

        // Dismiss one
        let was_new = dismiss(org, "circuit/emulator").unwrap();
        assert!(was_new);

        // Read back
        let dc = read_dismissed(org).unwrap();
        assert_eq!(dc.dismissed, vec!["circuit/emulator"]);

        // Dismiss same one again -- no-op
        let was_new = dismiss(org, "circuit/emulator").unwrap();
        assert!(!was_new);

        // Dismiss another
        dismiss(org, "docs/tutorials").unwrap();
        let dc = read_dismissed(org).unwrap();
        assert_eq!(dc.dismissed.len(), 2);
        assert!(is_dismissed(&dc, "circuit/emulator"));
        assert!(is_dismissed(&dc, "docs/tutorials"));
        assert!(!is_dismissed(&dc, "other"));
    }
}
