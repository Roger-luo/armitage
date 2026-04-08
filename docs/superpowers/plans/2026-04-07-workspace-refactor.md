# Workspace Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the single-crate armitage codebase into a 7-crate Cargo workspace with domain-driven boundaries.

**Architecture:** Bottom-up extraction — build `armitage-core` first (Org, Domain trait, Node types, tree walking), then layer-1 crates in parallel (labels, milestones, github), then layer-2 in parallel (sync, triage), then the CLI binary last. Each crate owns its types, files, config section, and error type.

**Tech Stack:** Rust 2024 edition, serde, chrono, toml, thiserror, ionem, rusqlite, sha2, hex, clap, rustyline, console, termimad, indicatif, ureq, dialoguer

**Spec:** `docs/superpowers/specs/2026-04-07-workspace-refactor-design.md`

---

## Parallelism Map

```
Phase 0: Task 1 (scaffold) → Task 2 (armitage-core)        [sequential]
Phase 1: Task 3 (labels) | Task 4 (milestones) | Task 5 (github)  [parallel]
Phase 2: Task 6 (sync) | Task 7 (triage)                   [parallel]
Phase 3: Task 8 (CLI binary)                                [sequential]
Phase 4: Task 9 (integration tests + cleanup)               [sequential]
```

Each task from Phase 1 onward can run in an isolated worktree. Phase 1 tasks start from the commit after Task 2. Phase 2 tasks start from the merge of Phase 1 results.

---

### Task 1: Workspace Scaffold

**Files:**
- Create: `crates/` directory structure
- Modify: `Cargo.toml` (convert to workspace manifest)

- [ ] **Step 1: Create crate directories**

```bash
mkdir -p crates/armitage-core/src
mkdir -p crates/armitage-labels/src
mkdir -p crates/armitage-milestones/src
mkdir -p crates/armitage-github/src
mkdir -p crates/armitage-sync/src
mkdir -p crates/armitage-triage/src
mkdir -p crates/armitage/src/cli
```

- [ ] **Step 2: Write workspace Cargo.toml**

Replace the root `Cargo.toml` with:

```toml
[workspace]
resolver = "2"
members = [
    "crates/armitage-core",
    "crates/armitage-labels",
    "crates/armitage-milestones",
    "crates/armitage-github",
    "crates/armitage-sync",
    "crates/armitage-triage",
    "crates/armitage",
]

[workspace.package]
edition = "2024"

[workspace.dependencies]
# Internal crates
armitage-core = { path = "crates/armitage-core" }
armitage-labels = { path = "crates/armitage-labels" }
armitage-milestones = { path = "crates/armitage-milestones" }
armitage-github = { path = "crates/armitage-github" }
armitage-sync = { path = "crates/armitage-sync" }
armitage-triage = { path = "crates/armitage-triage" }

# External crates
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
ionem = { version = "0.2.0", features = ["gh", "git", "self-update"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.11"
hex = "0.4"
thiserror = "2"
toml = "1.1"
rusqlite = { version = "0.39", features = ["bundled"] }
rustyline = "18"
console = "0.16"
dialoguer = "0.12"
indicatif = "0.18"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
ureq = "3"
tempfile = "3"
termimad = "0.34.1"
insta = "1"
```

- [ ] **Step 3: Write stub Cargo.toml for each crate**

Each crate needs a minimal `Cargo.toml` and `src/lib.rs` (or `src/main.rs` for the binary) so the workspace compiles. Create stub files:

`crates/armitage-core/Cargo.toml`:
```toml
[package]
name = "armitage-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
chrono = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

`crates/armitage-core/src/lib.rs`:
```rust
pub fn placeholder() {}
```

Repeat for each crate with the correct dependency list (see Task 2–7 for full Cargo.toml contents). For now, each `src/lib.rs` is just `pub fn placeholder() {}`.

`crates/armitage/Cargo.toml`:
```toml
[package]
name = "armitage"
version = "0.1.0"
edition.workspace = true

[dependencies]
armitage-core = { workspace = true }
```

`crates/armitage/src/main.rs`:
```rust
fn main() {
    println!("placeholder");
}
```

- [ ] **Step 4: Verify workspace compiles**

Run: `cargo check`
Expected: compiles with no errors (all crates are stubs).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: scaffold workspace with 7 crate directories"
```

---

### Task 2: armitage-core

Extract the `Org` abstraction, `Domain` trait, core node types, and filesystem operations into `armitage-core`.

**Source files to move/adapt:**
- `src/model/node.rs` → `crates/armitage-core/src/node.rs`
- `src/model/org.rs` → partially: only `OrgInfo` stays. `LabelSchema`, `SyncConfig`, `TriageConfig` move to their domain crates later
- `src/fs/tree.rs` → `crates/armitage-core/src/tree.rs`
- `src/fs/secrets.rs` → `crates/armitage-core/src/secrets.rs`

**New files to create:**
- `crates/armitage-core/src/lib.rs`
- `crates/armitage-core/src/error.rs`
- `crates/armitage-core/src/domain.rs`
- `crates/armitage-core/src/org.rs`

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage-core/Cargo.toml`:
```toml
[package]
name = "armitage-core"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
chrono = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Write error.rs**

`crates/armitage-core/src/error.rs`:
```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("not an org directory (no armitage.toml found)")]
    NotInOrg,

    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("parent node not found: {0}")]
    ParentNotFound(String),

    #[error("node already exists: {0}")]
    NodeExists(String),

    #[error("invalid issue reference: {0} (expected owner/repo#number)")]
    InvalidIssueRef(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Write domain.rs (Domain trait)**

`crates/armitage-core/src/domain.rs`:
```rust
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use crate::error::Result;
use crate::org::Org;

/// Plugin trait for domain crates. Each domain declares its config section,
/// node files, and gitignored data directory.
pub trait Domain {
    /// Unique domain identifier (e.g., "sync", "triage").
    const NAME: &'static str;

    /// Config section key in armitage.toml (e.g., "sync", "labels").
    const CONFIG_KEY: &'static str;

    /// Deserialized config type. Default is used when the section is absent.
    type Config: DeserializeOwned + Default;

    /// Git-tracked files this domain creates in node directories.
    const NODE_FILES: &'static [&'static str] = &[];

    /// Git-tracked files this domain creates at the org root.
    const ROOT_FILES: &'static [&'static str] = &[];

    /// Gitignored data directory: `.armitage/<NAME>/`.
    fn data_dir(org: &Org) -> Result<PathBuf> {
        let dir = org.root().join(".armitage").join(Self::NAME);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}
```

- [ ] **Step 4: Move node.rs**

Copy `src/model/node.rs` to `crates/armitage-core/src/node.rs`.

Replace the import:
```rust
// OLD
use crate::error::Error;
// NEW — same, the crate is now armitage-core
use crate::error::Error;
```

No other changes needed — the file's `use crate::error::Error` already works within armitage-core.

- [ ] **Step 5: Write org.rs (OrgInfo + Org struct)**

`crates/armitage-core/src/org.rs`:

This file contains `OrgInfo` (moved from `model/org.rs`) and the new `Org` struct.

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::Domain;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub github_orgs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_repo: Option<String>,
}

/// Root abstraction for an armitage organization.
///
/// Created once at the CLI layer and passed to domain crates.
pub struct Org {
    root: PathBuf,
    raw: toml::Table,
    info: OrgInfo,
}

impl Org {
    /// Walk up from `start` looking for `armitage.toml`. Returns an `Org`.
    pub fn discover_from(start: &Path) -> Result<Self> {
        let root = find_org_root(start)?;
        Self::open(&root)
    }

    /// Open an org at a known path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().to_path_buf();
        let config_path = root.join("armitage.toml");
        let content = std::fs::read_to_string(&config_path)?;
        let raw: toml::Table =
            toml::from_str(&content).map_err(|source| Error::TomlParse {
                path: config_path,
                source,
            })?;
        let info: OrgInfo = raw
            .get("org")
            .cloned()
            .ok_or_else(|| Error::Other("missing [org] section in armitage.toml".to_string()))?
            .try_into()
            .map_err(|e: toml::de::Error| Error::TomlParse {
                path: root.join("armitage.toml"),
                source: e,
            })?;
        Ok(Self { root, raw, info })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn info(&self) -> &OrgInfo {
        &self.info
    }

    /// Access the full raw TOML table (for domain crates to extract their sections).
    pub fn raw_config(&self) -> &toml::Table {
        &self.raw
    }

    /// Extract a domain's config section, returning Default if absent.
    pub fn domain_config<D: Domain>(&self) -> Result<D::Config> {
        match self.raw.get(D::CONFIG_KEY) {
            Some(section) => {
                let config: D::Config = section.clone().try_into().map_err(|e: toml::de::Error| {
                    Error::TomlParse {
                        path: self.root.join("armitage.toml"),
                        source: e,
                    }
                })?;
                Ok(config)
            }
            None => Ok(D::Config::default()),
        }
    }
}

/// Walk up from `start` looking for `armitage.toml`. Returns the org root directory.
pub fn find_org_root(start: &Path) -> Result<PathBuf> {
    let start = start.canonicalize()?;
    let mut current = start.as_path();
    loop {
        if current.join("armitage.toml").exists() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Err(Error::NotInOrg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_armitage_toml(dir: &Path) {
        let content = "[org]\nname = \"test\"\ngithub_orgs = [\"test\"]\n";
        std::fs::write(dir.join("armitage.toml"), content).unwrap();
    }

    #[test]
    fn org_open_reads_info() {
        let tmp = TempDir::new().unwrap();
        write_armitage_toml(tmp.path());
        let org = Org::open(tmp.path()).unwrap();
        assert_eq!(org.info().name, "test");
        assert_eq!(org.info().github_orgs, vec!["test"]);
    }

    #[test]
    fn org_discover_from_subdir() {
        let tmp = TempDir::new().unwrap();
        write_armitage_toml(tmp.path());
        let nested = tmp.path().join("foo").join("bar");
        std::fs::create_dir_all(&nested).unwrap();
        let org = Org::discover_from(&nested).unwrap();
        assert_eq!(org.info().name, "test");
    }

    #[test]
    fn domain_config_returns_default_when_missing() {
        let tmp = TempDir::new().unwrap();
        write_armitage_toml(tmp.path());
        let org = Org::open(tmp.path()).unwrap();

        #[derive(Debug, Default, serde::Deserialize)]
        struct TestConfig {
            #[serde(default)]
            value: String,
        }
        struct TestDomain;
        impl Domain for TestDomain {
            const NAME: &'static str = "test";
            const CONFIG_KEY: &'static str = "test_section";
            type Config = TestConfig;
        }
        let config = org.domain_config::<TestDomain>().unwrap();
        assert_eq!(config.value, "");
    }

    #[test]
    fn domain_config_parses_section() {
        let tmp = TempDir::new().unwrap();
        let content = "[org]\nname = \"test\"\n\n[my_domain]\nvalue = \"hello\"\n";
        std::fs::write(tmp.path().join("armitage.toml"), content).unwrap();
        let org = Org::open(tmp.path()).unwrap();

        #[derive(Debug, Default, serde::Deserialize)]
        struct MyConfig {
            value: String,
        }
        struct MyDomain;
        impl Domain for MyDomain {
            const NAME: &'static str = "my";
            const CONFIG_KEY: &'static str = "my_domain";
            type Config = MyConfig;
        }
        let config = org.domain_config::<MyDomain>().unwrap();
        assert_eq!(config.value, "hello");
    }

    #[test]
    fn find_org_root_not_found() {
        let tmp = TempDir::new().unwrap();
        let result = find_org_root(tmp.path());
        assert!(result.is_err());
    }
}
```

- [ ] **Step 6: Move tree.rs**

Copy `src/fs/tree.rs` to `crates/armitage-core/src/tree.rs`.

Replace imports:
```rust
// OLD
use crate::error::{Error, Result};
use crate::model::node::Node;
use crate::model::org::OrgConfig;

// NEW
use crate::error::{Error, Result};
use crate::node::Node;
```

Remove the `read_org_config()` function entirely — `Org::open()` replaces it.

Add a method block to make tree functions available on `Org`:

At the bottom of `crates/armitage-core/src/org.rs`, add (or create a new `crates/armitage-core/src/tree_ext.rs`):
```rust
use crate::tree::{NodeEntry, walk_nodes, read_node, list_children};

impl Org {
    pub fn walk_nodes(&self) -> Result<Vec<NodeEntry>> {
        walk_nodes(&self.root)
    }

    pub fn read_node(&self, rel_path: &str) -> Result<NodeEntry> {
        read_node(&self.root, rel_path)
    }

    pub fn list_children(&self, rel_path: &str) -> Result<Vec<NodeEntry>> {
        list_children(&self.root, rel_path)
    }
}
```

- [ ] **Step 7: Move secrets.rs**

Copy `src/fs/secrets.rs` to `crates/armitage-core/src/secrets.rs`.

Replace imports:
```rust
// OLD
use crate::error::{Error, Result};
// NEW — same path, works within armitage-core
use crate::error::{Error, Result};
```

Add methods on `Org` in `org.rs`:
```rust
use crate::secrets;

impl Org {
    pub fn read_secret(&self, key: &str) -> Result<Option<String>> {
        secrets::read_secret(&self.root, key)
    }

    pub fn write_secret(&self, key: &str, value: &str) -> Result<()> {
        secrets::write_secret(&self.root, key, value)
    }
}
```

- [ ] **Step 8: Write lib.rs**

`crates/armitage-core/src/lib.rs`:
```rust
pub mod domain;
pub mod error;
pub mod node;
pub mod org;
pub mod secrets;
pub mod tree;
```

- [ ] **Step 9: Write CoreDomain implementation**

Add to `crates/armitage-core/src/domain.rs`:
```rust
use crate::org::OrgInfo;

pub struct CoreDomain;

impl Domain for CoreDomain {
    const NAME: &'static str = "core";
    const CONFIG_KEY: &'static str = "org";
    type Config = OrgInfo;
    const NODE_FILES: &'static [&'static str] = &["node.toml", "issue.md"];
    const ROOT_FILES: &'static [&'static str] = &["armitage.toml"];
}
```

- [ ] **Step 10: Run tests**

Run: `cargo nextest run -p armitage-core`
Expected: All node, tree, secrets, org, and domain tests pass.

- [ ] **Step 11: Run clippy**

Run: `cargo clippy -p armitage-core --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 12: Commit**

```bash
git add crates/armitage-core/
git commit -m "feat: extract armitage-core with Org, Domain trait, node types, tree walking"
```

---

### Task 3: armitage-labels (parallelizable with Tasks 4, 5)

Extract label types, schema config, and the rename ledger.

**Source files to move/adapt:**
- `src/model/label.rs` → `crates/armitage-labels/src/def.rs` (LabelDef, LabelsFile)
- `src/model/org.rs` → `crates/armitage-labels/src/schema.rs` (LabelSchema, LabelPrefix, LabelStyle, LabelStyleExample)
- `src/triage/labels.rs` → `crates/armitage-labels/src/rename.rs` (LabelRenameLedger, LabelRename, translate_labels, record_renames, etc.)

**New files to create:**
- `crates/armitage-labels/src/lib.rs`
- `crates/armitage-labels/src/error.rs`

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage-labels/Cargo.toml`:
```toml
[package]
name = "armitage-labels"
version = "0.1.0"
edition.workspace = true

[dependencies]
armitage-core = { workspace = true }
serde = { workspace = true }
chrono = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Write error.rs**

`crates/armitage-labels/src/error.rs`:
```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Write schema.rs**

Move `LabelSchema`, `LabelPrefix`, `LabelStyle`, `LabelStyleExample` from `src/model/org.rs` into `crates/armitage-labels/src/schema.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelSchema {
    #[serde(default)]
    pub prefixes: Vec<LabelPrefix>,
    #[serde(default)]
    pub style: Option<LabelStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPrefix {
    pub prefix: String,
    pub category: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyle {
    pub convention: String,
    #[serde(default)]
    pub examples: Vec<LabelStyleExample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyleExample {
    pub name: String,
    pub description: String,
}
```

- [ ] **Step 4: Write def.rs**

Copy `src/model/label.rs` to `crates/armitage-labels/src/def.rs`.

Replace imports:
```rust
// OLD
use crate::error::{Error, Result};
// NEW
use crate::error::{Error, Result};
```

The file's internal structure stays the same. `LabelsFile::read()` and `write()` take `&Path` (a node directory) — this is unchanged.

- [ ] **Step 5: Write rename.rs**

Extract the rename-ledger portion of `src/triage/labels.rs` into `crates/armitage-labels/src/rename.rs`. This includes:
- `LabelRename`, `LabelRenameLedger`
- `read_rename_ledger()`, `write_rename_ledger()`, `record_renames()`, `mark_rename_synced()`, `pending_renames_for_repo()`, `dedup_rename_ledger()`, `prune_fully_synced()`
- `translate_labels()`

Replace imports:
```rust
// OLD
use crate::error::{Error, Result};
use crate::model::label::LabelDef;
// NEW
use crate::error::{Error, Result};
use crate::def::LabelDef;
```

**Key change:** Update `renames_path()` to use the Domain data directory:
```rust
fn renames_path(org: &armitage_core::org::Org) -> std::path::PathBuf {
    // New: .armitage/labels/renames.toml
    org.root().join(".armitage").join("labels").join("renames.toml")
}
```

Update all functions that took `org_root: &Path` to take `org: &armitage_core::org::Org`:
```rust
pub fn read_rename_ledger(org: &armitage_core::org::Org) -> Result<LabelRenameLedger> {
    let path = renames_path(org);
    // ... rest same
}
```

- [ ] **Step 6: Write Domain implementation**

Add to `crates/armitage-labels/src/lib.rs`:
```rust
pub mod def;
pub mod error;
pub mod rename;
pub mod schema;

use armitage_core::domain::Domain;
use armitage_core::org::Org;

pub struct LabelsDomain;

impl Domain for LabelsDomain {
    const NAME: &'static str = "labels";
    const CONFIG_KEY: &'static str = "label_schema";
    type Config = schema::LabelSchema;
    const NODE_FILES: &'static [&'static str] = &["labels.toml"];
}
```

Note: `CONFIG_KEY` is `"label_schema"` to match the current `armitage.toml` field name.

- [ ] **Step 7: Run tests**

Run: `cargo nextest run -p armitage-labels`
Expected: All label def, rename, and schema tests pass.

- [ ] **Step 8: Run clippy**

Run: `cargo clippy -p armitage-labels --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 9: Commit**

```bash
git add crates/armitage-labels/
git commit -m "feat: extract armitage-labels with label defs, schema, and rename ledger"
```

---

### Task 4: armitage-milestones (parallelizable with Tasks 3, 5)

Extract milestone types.

**Source files to move/adapt:**
- `src/model/milestone.rs` → `crates/armitage-milestones/src/milestone.rs`

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage-milestones/Cargo.toml`:
```toml
[package]
name = "armitage-milestones"
version = "0.1.0"
edition.workspace = true

[dependencies]
armitage-core = { workspace = true }
serde = { workspace = true }
chrono = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Write error.rs**

`crates/armitage-milestones/src/error.rs`:
```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Move milestone.rs**

Copy `src/model/milestone.rs` to `crates/armitage-milestones/src/milestone.rs`.

The file has no `use crate::` imports at all — it only uses `chrono`, `serde`, and `std::fmt`. No changes needed to the source.

- [ ] **Step 4: Write Domain implementation and lib.rs**

`crates/armitage-milestones/src/lib.rs`:
```rust
pub mod error;
pub mod milestone;

use armitage_core::domain::Domain;

#[derive(Debug, Default, serde::Deserialize)]
pub struct MilestonesConfig {}

pub struct MilestonesDomain;

impl Domain for MilestonesDomain {
    const NAME: &'static str = "milestones";
    const CONFIG_KEY: &'static str = "milestones";
    type Config = MilestonesConfig;
    const NODE_FILES: &'static [&'static str] = &["milestones.toml"];
}
```

- [ ] **Step 5: Run tests**

Run: `cargo nextest run -p armitage-milestones`
Expected: All milestone tests pass.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p armitage-milestones --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/armitage-milestones/
git commit -m "feat: extract armitage-milestones with milestone types"
```

---

### Task 5: armitage-github (parallelizable with Tasks 3, 4)

Extract GitHub API operations.

**Source files to move/adapt:**
- `src/github/issue.rs` → `crates/armitage-github/src/issue.rs`

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage-github/Cargo.toml`:
```toml
[package]
name = "armitage-github"
version = "0.1.0"
edition.workspace = true

[dependencies]
armitage-core = { workspace = true }
ionem = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 2: Write error.rs**

`crates/armitage-github/src/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("GitHub CLI error: {0}")]
    Cli(#[from] ionem::shell::CliError),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Move issue.rs**

Copy `src/github/issue.rs` to `crates/armitage-github/src/issue.rs`.

Replace imports:
```rust
// OLD
use crate::error::Result;
use crate::model::node::IssueRef;
// NEW
use crate::error::Result;
use armitage_core::node::IssueRef;
```

- [ ] **Step 4: Write lib.rs**

`crates/armitage-github/src/lib.rs`:
```rust
pub mod error;
pub mod issue;

// Re-export ionem types that downstream crates need
pub use ionem::shell::gh::Gh;
pub fn require_gh() -> std::result::Result<Gh, ionem::shell::CliError> {
    ionem::shell::gh::require()
}
```

- [ ] **Step 5: Run tests**

Run: `cargo nextest run -p armitage-github`
Expected: All JSON deserialization tests pass. (Functions that call `gh.run()` aren't unit-tested.)

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p armitage-github --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/armitage-github/
git commit -m "feat: extract armitage-github with GitHub API operations"
```

---

### Task 6: armitage-sync (parallelizable with Task 7; depends on Tasks 2, 5)

Extract sync engine. Key change: remove `translate_labels` call from `pull_node`.

**Source files to move/adapt:**
- `src/sync/hash.rs` → `crates/armitage-sync/src/hash.rs`
- `src/sync/state.rs` → `crates/armitage-sync/src/state.rs`
- `src/sync/conflict.rs` → `crates/armitage-sync/src/conflict.rs`
- `src/sync/merge.rs` → `crates/armitage-sync/src/merge.rs`
- `src/sync/pull.rs` → `crates/armitage-sync/src/pull.rs`
- `src/sync/push.rs` → `crates/armitage-sync/src/push.rs`

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage-sync/Cargo.toml`:
```toml
[package]
name = "armitage-sync"
version = "0.1.0"
edition.workspace = true

[dependencies]
armitage-core = { workspace = true }
armitage-github = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
chrono = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Write error.rs**

`crates/armitage-sync/src/error.rs`:
```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("unresolved conflicts exist — run `armitage resolve` first")]
    UnresolvedConflicts,

    #[error("remote has changed since last pull — run `armitage pull` first")]
    StalePush,

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error(transparent)]
    Github(#[from] armitage_github::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Write SyncConfig**

`crates/armitage-sync/src/config.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default)]
    pub conflict_strategy: ConflictStrategy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictStrategy {
    #[default]
    Detect,
    GithubWins,
    LocalWins,
}
```

- [ ] **Step 4: Move hash.rs, state.rs, conflict.rs, merge.rs**

Copy each file. Update imports in each:

**hash.rs** — replace `use crate::error::Result;` → same (now refers to armitage-sync's error).

**state.rs** — replace `use crate::error::{Error, Result};` → same. Update `state_path()` to use the new namespaced path:
```rust
fn state_path(org_root: &Path) -> std::path::PathBuf {
    // NEW: .armitage/sync/state.toml
    org_root.join(".armitage").join("sync").join("state.toml")
}
```

**conflict.rs** — replace:
```rust
// OLD
use crate::error::{Error, Result};
use crate::sync::merge::FieldConflict;
// NEW
use crate::error::{Error, Result};
use crate::merge::FieldConflict;
```

Update `conflicts_dir()`:
```rust
fn conflicts_dir(org_root: &Path) -> std::path::PathBuf {
    // NEW: .armitage/sync/conflicts/
    org_root.join(".armitage").join("sync").join("conflicts")
}
```

**merge.rs** — replace:
```rust
// OLD
use crate::model::node::{Node, NodeStatus, Timeline};
// NEW
use armitage_core::node::{Node, NodeStatus, Timeline};
```

- [ ] **Step 5: Move pull.rs — remove translate_labels**

Copy `src/sync/pull.rs` to `crates/armitage-sync/src/pull.rs`.

Replace imports:
```rust
// OLD
use crate::error::{Error, Result};
use crate::fs::tree::{NodeEntry, walk_nodes};
use crate::github::issue::{GitHubIssue, fetch_issue};
use crate::model::node::{IssueRef, Node, NodeStatus};
use crate::sync::conflict::write_conflict;
use crate::sync::hash::compute_node_hash;
use crate::sync::merge::{BodyMergeResult, MergeResult, merge_issue_body, merge_nodes};
use crate::sync::state::{NodeSyncEntry, read_sync_state, write_sync_state};
use crate::triage::labels::{LabelRenameLedger, translate_labels};

// NEW
use crate::error::{Error, Result};
use crate::conflict::write_conflict;
use crate::hash::compute_node_hash;
use crate::merge::{BodyMergeResult, MergeResult, merge_issue_body, merge_nodes};
use crate::state::{NodeSyncEntry, read_sync_state, write_sync_state};
use armitage_core::node::{IssueRef, Node, NodeStatus};
use armitage_core::tree::{NodeEntry, walk_nodes};
use armitage_github::issue::{GitHubIssue, fetch_issue};
use armitage_github::Gh;
```

**Critical change:** Remove all `translate_labels` calls. In `apply_remote_to_local()`:
```rust
// OLD
node.labels = translate_labels(&raw_labels, rename_ledger);
// NEW — use raw labels directly; CLI orchestrates translation
node.labels = issue.labels.iter().map(|l| l.name.clone()).collect();
```

Remove `rename_ledger` parameter from `apply_remote_to_local()`, `pull_node()`, and `pull_all()`. In `pull_all()`, remove the `read_rename_ledger()` call.

Update `pull_node` signature:
```rust
pub fn pull_node(
    gh: &Gh,
    org_root: &Path,
    entry: &NodeEntry,
) -> Result<PullNodeResult> {
```

Update `pull_all` signature:
```rust
pub fn pull_all(
    gh: &Gh,
    org_root: &Path,
    scope: Option<&str>,
    dry_run: bool,
) -> Result<()> {
```

Also update all `&ionem::shell::gh::Gh` to `&armitage_github::Gh` throughout.

- [ ] **Step 6: Move push.rs**

Copy `src/sync/push.rs`. Replace imports:
```rust
// OLD
use crate::error::{Error, Result};
use crate::fs::tree::{NodeEntry, walk_nodes};
use crate::github::issue::{create_issue, fetch_issue, set_issue_state, update_issue};
use crate::model::node::{IssueRef, NodeStatus};
use crate::sync::conflict::has_conflicts;
use crate::sync::hash::compute_node_hash;
use crate::sync::state::{NodeSyncEntry, read_sync_state, write_sync_state};

// NEW
use crate::error::{Error, Result};
use crate::conflict::has_conflicts;
use crate::hash::compute_node_hash;
use crate::state::{NodeSyncEntry, read_sync_state, write_sync_state};
use armitage_core::node::{IssueRef, NodeStatus};
use armitage_core::tree::{NodeEntry, walk_nodes};
use armitage_github::issue::{create_issue, fetch_issue, set_issue_state, update_issue};
use armitage_github::Gh;
```

Replace `&ionem::shell::gh::Gh` with `&Gh` throughout.

- [ ] **Step 7: Write lib.rs and Domain implementation**

`crates/armitage-sync/src/lib.rs`:
```rust
pub mod config;
pub mod conflict;
pub mod error;
pub mod hash;
pub mod merge;
pub mod pull;
pub mod push;
pub mod state;

use armitage_core::domain::Domain;

pub struct SyncDomain;

impl Domain for SyncDomain {
    const NAME: &'static str = "sync";
    const CONFIG_KEY: &'static str = "sync";
    type Config = config::SyncConfig;
}
```

- [ ] **Step 8: Run tests**

Run: `cargo nextest run -p armitage-sync`
Expected: All hash, state, conflict, merge tests pass. Pull/push tests that require GitHub are skipped.

- [ ] **Step 9: Run clippy**

Run: `cargo clippy -p armitage-sync --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 10: Commit**

```bash
git add crates/armitage-sync/
git commit -m "feat: extract armitage-sync with sync engine, remove translate_labels dependency"
```

---

### Task 7: armitage-triage (parallelizable with Task 6; depends on Tasks 2, 3, 5)

Extract triage pipeline. Key change: remove interactive UI (rustyline, console, termimad).

**Source files to move/adapt:**
- `src/triage/db.rs` → `crates/armitage-triage/src/db.rs`
- `src/triage/fetch.rs` → `crates/armitage-triage/src/fetch.rs`
- `src/triage/llm.rs` → `crates/armitage-triage/src/llm.rs`
- `src/triage/apply.rs` → `crates/armitage-triage/src/apply.rs`
- `src/triage/cache.rs` → `crates/armitage-triage/src/cache.rs`
- `src/triage/examples.rs` → `crates/armitage-triage/src/examples.rs`
- `src/triage/categories.rs` → `crates/armitage-triage/src/categories.rs`
- `src/triage/review.rs` → `crates/armitage-triage/src/review.rs` (logic only, strip interactive UI)
- `src/triage/labels.rs` → partially: only the label-import types (`LabelSuggestion`, `MergeGroup`, `LabelImportSession`, etc.) stay here. Rename-ledger moves to armitage-labels.

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage-triage/Cargo.toml`:
```toml
[package]
name = "armitage-triage"
version = "0.1.0"
edition.workspace = true

[dependencies]
armitage-core = { workspace = true }
armitage-labels = { workspace = true }
armitage-github = { workspace = true }
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }
ureq = { workspace = true }
indicatif = { workspace = true }
tracing = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
insta = { workspace = true }
```

- [ ] **Step 2: Write error.rs**

`crates/armitage-triage/src/error.rs`:
```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("LLM invocation failed: {0}")]
    LlmInvocation(String),

    #[error("LLM output parse error: {0}")]
    LlmParse(String),

    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error(transparent)]
    Github(#[from] armitage_github::error::Error),

    #[error(transparent)]
    Labels(#[from] armitage_labels::error::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Write TriageConfig**

`crates/armitage-triage/src/config.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageConfig {
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub thinking_budget: Option<i64>,
    #[serde(default)]
    pub labels: Option<TriageLlmOverride>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriageLlmOverride {
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub thinking_budget: Option<i64>,
}
```

- [ ] **Step 4: Move db.rs, fetch.rs, cache.rs, examples.rs, categories.rs, apply.rs**

For each file, copy to the new location and update imports. The general pattern:

```rust
// OLD
use crate::error::{Error, Result};
use crate::fs::tree::{...};
use crate::model::org::OrgConfig;
use crate::model::label::LabelsFile;
use crate::triage::db::{...};

// NEW
use crate::error::{Error, Result};
use crate::db::{...};
use armitage_core::tree::{...};
use armitage_labels::def::LabelsFile;
```

**db.rs** — update `open_db()` to use the namespaced path:
```rust
pub fn open_db(org_root: &Path) -> Result<Connection> {
    // NEW: .armitage/triage/triage.db
    let dir = org_root.join(".armitage").join("triage");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("triage.db");
    open_db_from_path(&path)
}
```

**examples.rs** — update path:
```rust
fn examples_path(org_root: &Path) -> PathBuf {
    // NEW: .armitage/triage/examples.toml
    org_root.join(".armitage").join("triage").join("examples.toml")
}
```

**categories.rs** — update path:
```rust
fn categories_path(org_root: &Path) -> PathBuf {
    // NEW: .armitage/triage/dismissed-categories.toml
    org_root.join(".armitage").join("triage").join("dismissed-categories.toml")
}
```

**cache.rs** — update path:
```rust
fn cache_dir(org_root: &Path) -> PathBuf {
    // NEW: .armitage/triage/repo-cache/
    org_root.join(".armitage").join("triage").join("repo-cache")
}
```

**apply.rs** — replace `&ionem::shell::gh::Gh` with `&armitage_github::Gh`.

**fetch.rs** — replace `&ionem::shell::gh::Gh` with `&armitage_github::Gh`.

- [ ] **Step 5: Move llm.rs**

Copy `src/triage/llm.rs`. Update imports to reference `armitage_core`, `armitage_labels`, and `crate::` modules. Replace `use crate::model::org::*` with `use crate::config::TriageConfig` and `use armitage_labels::schema::LabelSchema`.

- [ ] **Step 6: Split labels.rs**

The label-import types (`LabelSuggestion`, `MergeGroup`, `ReconcileResponse`, `CandidateStatus`, `LabelImportCandidate`, `LabelImportSession`, `MergeSelection`, `RemoteFetchedLabel`, `RepoLabels`, `RemoteLabelVariant`) and their associated functions (`write_import_session`, `read_import_session`, `list_import_session_ids`, `build_import_session`, `merge_selected_candidates`, `labels_for_repo`, `default_interactive_selection`, `choose_remote_variant`) stay in triage as `crates/armitage-triage/src/label_import.rs`.

Update imports to use `armitage_labels::def::LabelDef` instead of `crate::model::label::LabelDef`, and `armitage_labels::rename::*` for rename-ledger operations.

- [ ] **Step 7: Refactor review.rs — extract logic from UI**

Copy `src/triage/review.rs`. Remove:
- `use crate::cli::complete::{CommaCompleteHelper, NodePathHelper};`
- `use rustyline::Editor;`
- `use rustyline::error::ReadlineError;`
- `use console::{Style, Term};`

Keep `review_auto_approve()` as-is (it's pure logic). For `review_interactive()` and `review_list()`:
- Extract the data-fetching and decision-recording logic into non-interactive functions
- Move the interactive prompting code to the CLI crate (Task 8)

The review module should expose:
```rust
pub fn review_auto_approve(conn: &Connection, min_confidence: f64) -> Result<ReviewStats>;
pub fn get_reviewable_suggestions(conn: &Connection, filters: &SuggestionFilters) -> Result<Vec<(StoredIssue, TriageSuggestion)>>;
pub fn record_review_decision(conn: &Connection, decision: &ReviewDecision) -> Result<()>;
// ... other logic-only functions
```

- [ ] **Step 8: Write lib.rs and Domain implementation**

`crates/armitage-triage/src/lib.rs`:
```rust
pub mod apply;
pub mod cache;
pub mod categories;
pub mod config;
pub mod db;
pub mod error;
pub mod examples;
pub mod fetch;
pub mod label_import;
pub mod llm;
pub mod review;

use armitage_core::domain::Domain;

pub struct TriageDomain;

impl Domain for TriageDomain {
    const NAME: &'static str = "triage";
    const CONFIG_KEY: &'static str = "triage";
    type Config = config::TriageConfig;
}
```

- [ ] **Step 9: Run tests**

Run: `cargo nextest run -p armitage-triage`
Expected: All DB, cache, examples, categories, label-import, review-auto-approve tests pass.

- [ ] **Step 10: Run clippy**

Run: `cargo clippy -p armitage-triage --all-targets -- -D warnings`
Expected: No warnings.

- [ ] **Step 11: Commit**

```bash
git add crates/armitage-triage/
git commit -m "feat: extract armitage-triage with LLM pipeline, strip interactive UI"
```

---

### Task 8: armitage CLI binary

Wire up the CLI binary crate using all domain crates. This is the only crate that changes the existing `src/cli/` code.

**Source files to move/adapt:**
- `src/main.rs` → `crates/armitage/src/main.rs`
- `src/cli/mod.rs` → `crates/armitage/src/cli/mod.rs`
- `src/cli/node.rs` → `crates/armitage/src/cli/node.rs`
- `src/cli/triage.rs` → `crates/armitage/src/cli/triage.rs` (+ interactive review UI from old triage/review.rs)
- `src/cli/pull.rs` → `crates/armitage/src/cli/pull.rs` (+ label translation orchestration)
- `src/cli/push.rs` → `crates/armitage/src/cli/push.rs`
- `src/cli/init.rs` → `crates/armitage/src/cli/init.rs`
- `src/cli/config.rs` → `crates/armitage/src/cli/config.rs`
- `src/cli/status.rs` → `crates/armitage/src/cli/status.rs`
- `src/cli/resolve.rs` → `crates/armitage/src/cli/resolve.rs`
- `src/cli/milestone.rs` → `crates/armitage/src/cli/milestone.rs`
- `src/cli/complete.rs` → `crates/armitage/src/cli/complete.rs`
- `build.rs` → `crates/armitage/build.rs`
- `SKILL.md` stays at workspace root; build.rs copies it from there

- [ ] **Step 1: Write Cargo.toml**

`crates/armitage/Cargo.toml`:
```toml
[package]
name = "armitage"
version = "0.1.0"
edition.workspace = true
description = "CLI for project management across GitHub repositories"

[dependencies]
armitage-core = { workspace = true }
armitage-labels = { workspace = true }
armitage-milestones = { workspace = true }
armitage-github = { workspace = true }
armitage-sync = { workspace = true }
armitage-triage = { workspace = true }
chrono = { workspace = true }
clap = { workspace = true }
ionem = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
thiserror = { workspace = true }
rustyline = { workspace = true }
console = { workspace = true }
dialoguer = { workspace = true }
termimad = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

[build-dependencies]
ionem = { version = "0.2.0" }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 2: Write top-level error type**

`crates/armitage/src/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] armitage_core::error::Error),

    #[error(transparent)]
    Labels(#[from] armitage_labels::error::Error),

    #[error(transparent)]
    Milestones(#[from] armitage_milestones::error::Error),

    #[error(transparent)]
    Github(#[from] armitage_github::error::Error),

    #[error(transparent)]
    Sync(#[from] armitage_sync::error::Error),

    #[error(transparent)]
    Triage(#[from] armitage_triage::error::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 3: Move all CLI source files**

Copy each file from `src/cli/` to `crates/armitage/src/cli/`.

For each file, replace all `use crate::` imports:
```rust
// OLD patterns → NEW patterns
use crate::error::{Error, Result}       → use crate::error::{Error, Result}
use crate::fs::tree::*                  → use armitage_core::tree::*
use crate::model::node::*              → use armitage_core::node::*
use crate::model::org::*              → use armitage_core::org::* (for OrgInfo)
                                         use armitage_labels::schema::* (for LabelSchema)
                                         use armitage_sync::config::* (for SyncConfig)
                                         use armitage_triage::config::* (for TriageConfig)
use crate::model::label::*            → use armitage_labels::def::*
use crate::model::milestone::*        → use armitage_milestones::milestone::*
use crate::github::issue::*           → use armitage_github::issue::*
use crate::sync::*                    → use armitage_sync::*
use crate::triage::*                  → use armitage_triage::*
use crate::triage::labels::*          → use armitage_labels::rename::* (for ledger)
                                         use armitage_triage::label_import::* (for import session)
```

- [ ] **Step 4: Update cli/pull.rs — add label translation orchestration**

After `armitage_sync::pull::pull_all()` returns, call label translation:

```rust
pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    let org = armitage_core::org::Org::discover_from(&std::env::current_dir()?)?;
    let gh = armitage_github::require_gh()?;

    armitage_sync::pull::pull_all(&gh, org.root(), path.as_deref(), dry_run)?;

    // After pull, translate labels using the rename ledger
    if !dry_run {
        let ledger = armitage_labels::rename::read_rename_ledger(&org)?;
        if !ledger.renames.is_empty() {
            // Walk nodes and translate labels in each node.toml
            let nodes = org.walk_nodes()?;
            for entry in &nodes {
                let translated = armitage_labels::rename::translate_labels(&entry.node.labels, &ledger);
                if translated != entry.node.labels {
                    let mut node = entry.node.clone();
                    node.labels = translated;
                    let content = toml::to_string(&node)?;
                    std::fs::write(entry.dir.join("node.toml"), content)?;
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 5: Update cli/triage.rs — move interactive review UI here**

Add the interactive review functions that were stripped from `triage/review.rs`. These use `rustyline`, `console`, `termimad`, and `cli/complete.rs` helpers. They call into `armitage_triage::review` for data operations and `armitage_triage::db` for queries.

- [ ] **Step 6: Update cli/init.rs — use Org and domain crate types**

The init function currently constructs an `OrgConfig` with `LabelSchema`, `SyncConfig`, `TriageConfig`. Update to use types from domain crates:

```rust
use armitage_labels::schema::{LabelSchema, LabelStyle, LabelStyleExample};
use armitage_core::org::OrgInfo;
```

The init function needs to construct the full `armitage.toml` by assembling sections from each domain. Build a `toml::Table` manually:

```rust
let mut config = toml::Table::new();
config.insert("org".to_string(), toml::Value::try_from(&org_info)?);
config.insert("label_schema".to_string(), toml::Value::try_from(&label_schema)?);
config.insert("sync".to_string(), toml::Value::try_from(&armitage_sync::config::SyncConfig::default())?);
config.insert("triage".to_string(), toml::Value::try_from(&armitage_triage::config::TriageConfig::default())?);
let toml_content = toml::to_string(&config)?;
```

- [ ] **Step 7: Move build.rs**

`crates/armitage/build.rs`:
```rust
fn main() {
    ionem::build::emit_target();
    ionem::build::copy_skill_md();
}
```

If `copy_skill_md()` expects `SKILL.md` in the crate directory, create a symlink or copy `SKILL.md` from the workspace root into `crates/armitage/`. Alternatively, if ionem supports a path parameter, use that. Check ionem docs during implementation.

- [ ] **Step 8: Write main.rs and lib.rs**

`crates/armitage/src/main.rs`:
```rust
fn main() {
    if let Err(e) = armitage::cli::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
```

`crates/armitage/src/lib.rs`:
```rust
pub mod cli;
pub mod error;
```

- [ ] **Step 9: Run full test suite**

Run: `cargo nextest run`
Expected: All tests across all crates pass.

- [ ] **Step 10: Run clippy and fmt**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: No warnings, code formatted.

- [ ] **Step 11: Commit**

```bash
git add crates/armitage/
git commit -m "feat: wire up CLI binary crate with all domain crate dependencies"
```

---

### Task 9: Integration Tests + Cleanup

Move integration tests to workspace root, delete old `src/` directory, verify everything works end-to-end.

- [ ] **Step 1: Move integration tests**

The existing `tests/integration.rs` calls `armitage::cli::init::init_at()`, `armitage::cli::node::create_node()`, `armitage::fs::tree::walk_nodes()`, etc.

Update imports:
```rust
// OLD
armitage::cli::init::init_at(...)
armitage::cli::node::create_node(...)
armitage::fs::tree::walk_nodes(...)
armitage::fs::tree::list_children(...)
armitage::cli::milestone::add_milestone(...)
armitage::cli::milestone::read_milestones(...)
armitage::fs::tree::read_node(...)
armitage::cli::node::move_node(...)

// NEW — use domain crates directly where possible
armitage::cli::init::init_at(...)          // stays (CLI crate)
armitage::cli::node::create_node(...)      // stays (CLI crate)
armitage_core::tree::walk_nodes(...)       // from core
armitage_core::tree::list_children(...)    // from core
armitage::cli::milestone::add_milestone(...)  // stays (CLI crate)
armitage::cli::milestone::read_milestones(...)  // stays (CLI crate)
armitage_core::tree::read_node(...)        // from core
armitage::cli::node::move_node(...)        // stays (CLI crate)
```

Or, if tree functions are accessed through `Org`:
```rust
let org = armitage_core::org::Org::open(&org_dir).unwrap();
let nodes = org.walk_nodes().unwrap();
```

- [ ] **Step 2: Delete old src/ directory**

```bash
rm -rf src/
```

- [ ] **Step 3: .armitage/ migration function**

Add a migration function to the CLI that moves files from the old flat layout to the new namespaced layout. Call it during `Org::discover_from()` or at CLI startup:

```rust
pub fn migrate_dotarmitage(org_root: &Path) -> std::io::Result<()> {
    let old_sync_state = org_root.join(".armitage/sync-state.toml");
    let new_sync_dir = org_root.join(".armitage/sync");
    if old_sync_state.exists() && !new_sync_dir.join("state.toml").exists() {
        std::fs::create_dir_all(&new_sync_dir)?;
        std::fs::rename(&old_sync_state, new_sync_dir.join("state.toml"))?;
    }

    let old_conflicts = org_root.join(".armitage/conflicts");
    let new_conflicts = org_root.join(".armitage/sync/conflicts");
    if old_conflicts.exists() && !new_conflicts.exists() {
        std::fs::create_dir_all(new_conflicts.parent().unwrap())?;
        std::fs::rename(&old_conflicts, &new_conflicts)?;
    }

    let old_db = org_root.join(".armitage/triage.db");
    let new_triage_dir = org_root.join(".armitage/triage");
    if old_db.exists() && !new_triage_dir.join("triage.db").exists() {
        std::fs::create_dir_all(&new_triage_dir)?;
        std::fs::rename(&old_db, new_triage_dir.join("triage.db"))?;
    }

    let old_renames = org_root.join(".armitage/label-renames.toml");
    let new_labels_dir = org_root.join(".armitage/labels");
    if old_renames.exists() && !new_labels_dir.join("renames.toml").exists() {
        std::fs::create_dir_all(&new_labels_dir)?;
        std::fs::rename(&old_renames, new_labels_dir.join("renames.toml"))?;
    }

    let old_examples = org_root.join(".armitage/triage-examples.toml");
    if old_examples.exists() && !new_triage_dir.join("examples.toml").exists() {
        std::fs::rename(&old_examples, new_triage_dir.join("examples.toml"))?;
    }

    let old_dismissed = org_root.join(".armitage/dismissed-categories.toml");
    if old_dismissed.exists() && !new_triage_dir.join("dismissed-categories.toml").exists() {
        std::fs::rename(&old_dismissed, new_triage_dir.join("dismissed-categories.toml"))?;
    }

    let old_cache = org_root.join(".armitage/repo-cache");
    if old_cache.exists() && !new_triage_dir.join("repo-cache").exists() {
        std::fs::rename(&old_cache, new_triage_dir.join("repo-cache"))?;
    }

    Ok(())
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo nextest run`
Expected: All tests pass.

- [ ] **Step 5: Run pre-commit checklist**

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
```

Expected: All three pass cleanly.

- [ ] **Step 6: Verify against test org**

```bash
cargo build
```

Then `cd` into the test org directory and run:
```bash
cargo run -- node tree
cargo run -- triage status
```

Verify commands work correctly against the live org data.

- [ ] **Step 7: Update CLAUDE.md**

Update the Architecture section to reflect the new workspace structure. Update the "Module layout" section to describe crates instead of modules. Update build/test commands if workspace-level invocations change.

- [ ] **Step 8: Update SKILL.md if needed**

If any command names, flags, or behaviors changed, update `SKILL.md`.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "refactor: complete workspace migration, delete old src/, add .armitage migration"
```
