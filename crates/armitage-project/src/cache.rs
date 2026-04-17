use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldCache {
    pub project_id: String,
    pub cached_at: String,
    #[serde(default)]
    pub fields: HashMap<String, CachedField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CachedField {
    Date {
        id: String,
    },
    SingleSelect {
        id: String,
        /// Maps option display name → option ID.
        options: HashMap<String, String>,
    },
}

fn cache_path(org_root: &Path) -> PathBuf {
    org_root
        .join(".armitage")
        .join("project")
        .join("field-cache.toml")
}

pub fn read_field_cache(org_root: &Path) -> Result<Option<FieldCache>> {
    let path = cache_path(org_root);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    let cache: FieldCache =
        toml::from_str(&content).map_err(|source| Error::TomlParse { path, source })?;
    Ok(Some(cache))
}

pub fn write_field_cache(org_root: &Path, cache: &FieldCache) -> Result<()> {
    let path = cache_path(org_root);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let content = toml::to_string(cache)?;
    std::fs::write(path, content)?;
    Ok(())
}
