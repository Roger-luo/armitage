use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::Domain;
use crate::error::{Error, Result};
use crate::secrets;
use crate::tree::{self, NodeEntry};

/// Basic identity information for an org (the `[org]` section of
/// `armitage.toml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrgInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub github_orgs: Vec<String>,
    /// Default repo for issues (e.g. "owner/repo"). Always included in triage
    /// fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_repo: Option<String>,
}

/// A loaded org — bundles the root path, raw TOML table, and parsed
/// `[org]` section.
#[derive(Debug, Clone)]
pub struct Org {
    root: PathBuf,
    raw: toml::Table,
    info: OrgInfo,
}

impl Org {
    /// Walk up from `start` to find `armitage.toml`, then open the org.
    pub fn discover_from(start: &Path) -> Result<Self> {
        let root = tree::find_org_root(start)?;
        Self::open(&root)
    }

    /// Open an org whose root directory is already known.
    pub fn open(root: &Path) -> Result<Self> {
        let path = root.join("armitage.toml");
        let content = std::fs::read_to_string(&path).map_err(|_| Error::NotInOrg)?;
        let raw: toml::Table = toml::from_str(&content).map_err(|source| Error::TomlParse {
            path: path.clone(),
            source,
        })?;

        let info: OrgInfo = raw
            .get("org")
            .map(|v| {
                v.clone()
                    .try_into()
                    .map_err(|source| Error::TomlParse { path, source })
            })
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            root: root.to_path_buf(),
            raw,
            info,
        })
    }

    /// The org root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Parsed `[org]` section.
    pub fn info(&self) -> &OrgInfo {
        &self.info
    }

    /// The full raw TOML table from `armitage.toml`.
    pub fn raw_config(&self) -> &toml::Table {
        &self.raw
    }

    /// Extract a domain's config section from the raw table.
    ///
    /// Returns `D::Config::default()` when the section is absent.
    pub fn domain_config<D: Domain>(&self) -> Result<D::Config> {
        self.raw.get(D::CONFIG_KEY).map_or_else(
            || Ok(D::Config::default()),
            |v| {
                v.clone().try_into().map_err(|source| Error::TomlParse {
                    path: self.root.join("armitage.toml"),
                    source,
                })
            },
        )
    }

    // ------------------------------------------------------------------
    // Convenience delegations to `tree`
    // ------------------------------------------------------------------

    /// Recursively walk the org and return all nodes.
    pub fn walk_nodes(&self) -> Result<Vec<NodeEntry>> {
        tree::walk_nodes(&self.root)
    }

    /// Read a single node at a path relative to the org root.
    pub fn read_node(&self, node_path: &str) -> Result<NodeEntry> {
        tree::read_node(&self.root, node_path)
    }

    /// List direct children of a node (or top-level if `parent_path` is
    /// empty).
    pub fn list_children(&self, parent_path: &str) -> Result<Vec<NodeEntry>> {
        tree::list_children(&self.root, parent_path)
    }

    // ------------------------------------------------------------------
    // Convenience delegations to `secrets`
    // ------------------------------------------------------------------

    /// Read a secret by key from `.armitage/secrets.toml`.
    pub fn read_secret(&self, key: &str) -> Result<Option<String>> {
        secrets::read_secret(&self.root, key)
    }

    /// Write a secret (ensures `.armitage/` is gitignored first).
    pub fn write_secret(&self, key: &str, value: &str) -> Result<()> {
        secrets::write_secret(&self.root, key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CoreDomain, Domain};
    use tempfile::TempDir;

    /// Helper: write a minimal armitage.toml with optional extra sections.
    fn write_config(dir: &Path, extra: &str) {
        let content = format!("[org]\nname = \"test-org\"\ngithub_orgs = [\"acme\"]\n{extra}");
        std::fs::write(dir.join("armitage.toml"), content).unwrap();
    }

    #[test]
    fn org_open_reads_info() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "");
        let org = Org::open(tmp.path()).unwrap();
        assert_eq!(org.info().name, "test-org");
        assert_eq!(org.info().github_orgs, vec!["acme"]);
        assert!(org.info().default_repo.is_none());
    }

    #[test]
    fn org_discover_from_subdir() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "");
        let sub = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        let org = Org::discover_from(&sub).unwrap();
        assert_eq!(org.info().name, "test-org");
        assert_eq!(
            org.root().canonicalize().unwrap(),
            tmp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn domain_config_returns_default_when_missing() {
        // A made-up domain whose key is absent from the config.
        struct FakeDomain;
        impl Domain for FakeDomain {
            const NAME: &'static str = "fake";
            const CONFIG_KEY: &'static str = "fake_section";
            type Config = OrgInfo;
        }

        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "");
        let org = Org::open(tmp.path()).unwrap();

        let cfg = org.domain_config::<FakeDomain>().unwrap();
        assert_eq!(cfg.name, ""); // OrgInfo::default().name is empty
    }

    #[test]
    fn domain_config_parses_section() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "");
        let org = Org::open(tmp.path()).unwrap();

        // CoreDomain's CONFIG_KEY is "org", which is present.
        let cfg = org.domain_config::<CoreDomain>().unwrap();
        assert_eq!(cfg.name, "test-org");
        assert_eq!(cfg.github_orgs, vec!["acme"]);
    }

    #[test]
    fn find_org_root_not_found() {
        let tmp = TempDir::new().unwrap();
        let result = Org::discover_from(tmp.path());
        assert!(matches!(result, Err(Error::NotInOrg)));
    }
}
