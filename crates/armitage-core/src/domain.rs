use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::error::Result;
use crate::org::{Org, OrgInfo};

/// Plugin mechanism for domain crates.
///
/// Each domain declares a unique name, config key, and the files it owns at
/// both node and org-root level.
pub trait Domain {
    /// Short identifier (e.g. "core", "sync", "triage").
    const NAME: &'static str;

    /// Key in `armitage.toml` holding this domain's configuration section.
    const CONFIG_KEY: &'static str;

    /// The type deserialized from that config section.
    type Config: DeserializeOwned + Default;

    /// Files this domain owns inside each node directory.
    const NODE_FILES: &'static [&'static str] = &[];

    /// Files this domain owns at the org root.
    const ROOT_FILES: &'static [&'static str] = &[];

    /// Return (and lazily create) the per-machine data directory for this
    /// domain inside `.armitage/`.
    fn data_dir(org: &Org) -> Result<PathBuf> {
        let dir = org.root().join(".armitage").join(Self::NAME);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// The built-in core domain that owns `node.toml`, `issue.md`, and
/// `armitage.toml`.
pub struct CoreDomain;

impl Domain for CoreDomain {
    const NAME: &'static str = "core";
    const CONFIG_KEY: &'static str = "org";
    type Config = OrgInfo;
    const NODE_FILES: &'static [&'static str] = &["node.toml", "issue.md"];
    const ROOT_FILES: &'static [&'static str] = &["armitage.toml"];
}
