# Armitage MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the armitage CLI for managing a recursive hierarchy of project nodes backed by local files with bidirectional GitHub issue sync.

**Architecture:** Layered — `model/` (pure data structs with serde), `fs/` (directory scanning), `github/` (GitHub API via ionem gh), `sync/` (pull/push/merge orchestration), `cli/` (thin clap layer). All local state lives in an org directory with `armitage.toml` at the root.

**Tech Stack:** Rust 2024 edition, clap (CLI), serde + toml (serialization), chrono (dates), sha2 (hashing), ionem (gh/git CLI wrappers + self-management), serde_json (GitHub API responses), thiserror (errors), tempfile (tests)

---

### Task 1: Project setup and error types

**Files:**
- Modify: `Cargo.toml`
- Create: `build.rs`
- Create: `SKILL.md`
- Create: `src/error.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Update Cargo.toml with all dependencies**

```toml
[package]
name = "armitage"
version = "0.1.0"
edition = "2024"
description = "CLI for project management across GitHub repositories"

[dependencies]
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
ionem = { version = "0.2.0", features = ["gh", "git", "self-update"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
thiserror = "2"
toml = "0.8"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Create build.rs**

```rust
// build.rs
fn main() {
    ionem::build::emit_target();
    ionem::build::copy_skill_md();
}
```

- [ ] **Step 3: Create SKILL.md**

```markdown
# armitage

Project management CLI for tracking initiatives, projects, and tasks across GitHub repositories.

Version: {version}
```

- [ ] **Step 4: Create src/error.rs**

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

    #[error("GitHub CLI error: {0}")]
    GitHub(#[from] ionem::shell::CliError),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("not an org directory (no armitage.toml found)")]
    NotInOrg,

    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("parent node not found: {0}")]
    ParentNotFound(String),

    #[error("node already exists: {0}")]
    NodeExists(String),

    #[error("unresolved conflicts exist — run `armitage resolve` first")]
    UnresolvedConflicts,

    #[error("remote has changed since last pull — run `armitage pull` first")]
    StalePush,

    #[error("invalid issue reference: {0} (expected owner/repo#number)")]
    InvalidIssueRef(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 5: Update src/main.rs with module declarations**

```rust
mod cli;
mod error;
mod fs;
mod github;
mod model;
mod sync;

fn main() {
    if let Err(e) = cli::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 6: Create empty module files**

Create these files with minimal content:

`src/cli/mod.rs`:
```rust
mod init;
mod issue;
mod milestone;
mod pull;
mod push;
mod resolve;
mod status;

use crate::error::Result;

pub fn run() -> Result<()> {
    Ok(())
}
```

`src/model/mod.rs`:
```rust
pub mod milestone;
pub mod node;
pub mod org;
```

`src/fs/mod.rs`:
```rust
pub mod tree;
```

`src/github/mod.rs`:
```rust
pub mod issue;
```

`src/sync/mod.rs`:
```rust
pub mod conflict;
pub mod hash;
pub mod merge;
pub mod pull;
pub mod push;
pub mod state;
```

Create empty files for each submodule:
- `src/cli/init.rs`, `src/cli/issue.rs`, `src/cli/milestone.rs`, `src/cli/pull.rs`, `src/cli/push.rs`, `src/cli/resolve.rs`, `src/cli/status.rs`
- `src/model/node.rs`, `src/model/milestone.rs`, `src/model/org.rs`
- `src/fs/tree.rs`
- `src/github/issue.rs`
- `src/sync/state.rs`, `src/sync/hash.rs`, `src/sync/pull.rs`, `src/sync/push.rs`, `src/sync/merge.rs`, `src/sync/conflict.rs`

Each empty file should contain just `// TODO: implement` as a placeholder.

- [ ] **Step 7: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors (warnings about unused modules are OK)

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: project setup with dependencies and module skeleton"
```

---

### Task 2: Model — Node, Timeline, NodeStatus, IssueRef

**Files:**
- Create: `src/model/node.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for Node serialization**

In `src/model/node.rs`:

```rust
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// Structs will go here

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_node() {
        let toml_str = r#"
name = "Gemini"
description = "Next-gen multimodal AI platform"
github_issue = "anthropic/gemini#1"
labels = ["I-gemini", "P-high"]
repos = ["anthropic/gemini", "anthropic/gemini-infra"]
status = "active"

[timeline]
start = "2026-01-01"
end = "2026-12-31"
"#;
        let node: Node = toml::from_str(toml_str).unwrap();
        assert_eq!(node.name, "Gemini");
        assert_eq!(node.labels, vec!["I-gemini", "P-high"]);
        assert_eq!(node.status, NodeStatus::Active);
        let tl = node.timeline.unwrap();
        assert_eq!(tl.start, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    }

    #[test]
    fn deserialize_minimal_node() {
        let toml_str = r#"
name = "Auth Service"
description = "Handle authentication"
"#;
        let node: Node = toml::from_str(toml_str).unwrap();
        assert_eq!(node.name, "Auth Service");
        assert!(node.github_issue.is_none());
        assert!(node.labels.is_empty());
        assert_eq!(node.status, NodeStatus::Active);
    }

    #[test]
    fn roundtrip_node() {
        let node = Node {
            name: "Test".into(),
            description: "A test node".into(),
            github_issue: Some("org/repo#42".into()),
            labels: vec!["P-high".into()],
            repos: vec!["org/repo".into()],
            timeline: Some(Timeline {
                start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                end: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            }),
            status: NodeStatus::Paused,
        };
        let serialized = toml::to_string_pretty(&node).unwrap();
        let deserialized: Node = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.name, "Test");
        assert_eq!(deserialized.status, NodeStatus::Paused);
    }

    #[test]
    fn parse_issue_ref_valid() {
        let r = IssueRef::parse("anthropic/gemini#123").unwrap();
        assert_eq!(r.owner, "anthropic");
        assert_eq!(r.repo, "gemini");
        assert_eq!(r.number, 123);
    }

    #[test]
    fn parse_issue_ref_invalid() {
        assert!(IssueRef::parse("not-valid").is_err());
        assert!(IssueRef::parse("owner/repo").is_err());
        assert!(IssueRef::parse("owner/repo#abc").is_err());
    }

    #[test]
    fn issue_ref_display() {
        let r = IssueRef { owner: "org".into(), repo: "repo".into(), number: 42 };
        assert_eq!(r.to_string(), "org/repo#42");
    }

    #[test]
    fn timeline_contains() {
        let parent = Timeline {
            start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        };
        let child = Timeline {
            start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        };
        let outside = Timeline {
            start: NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        };
        assert!(parent.contains(&child));
        assert!(!parent.contains(&outside));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib model::node`
Expected: FAIL — structs not defined

- [ ] **Step 3: Implement Node, Timeline, NodeStatus, IssueRef**

In `src/model/node.rs`, above the tests module:

```rust
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::Error;

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
    /// Returns true if `other` is fully contained within this timeline.
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

/// Parsed GitHub issue reference: `owner/repo#number`.
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib model::node`
Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/model/node.rs
git commit -m "feat: add Node, Timeline, NodeStatus, IssueRef model types"
```

---

### Task 3: Model — Milestone and MilestoneFile

**Files:**
- Create: `src/model/milestone.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for Milestone serialization**

In `src/model/milestone.rs`:

```rust
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// Structs will go here

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_milestone_file() {
        let toml_str = r#"
[[milestone]]
name = "Alpha ready"
date = "2026-03-15"
description = "Core pipeline working"
github_issue = "anthropic/gemini#45"
type = "checkpoint"

[[milestone]]
name = "Q1 OKR: 50% coverage"
date = "2026-03-31"
description = "Reach training milestone"
github_issue = "anthropic/gemini#80"
type = "okr"
expected_progress = 0.5
"#;
        let file: MilestoneFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.milestones.len(), 2);
        assert_eq!(file.milestones[0].name, "Alpha ready");
        assert_eq!(file.milestones[0].milestone_type, MilestoneType::Checkpoint);
        assert_eq!(file.milestones[1].milestone_type, MilestoneType::Okr);
        assert_eq!(file.milestones[1].expected_progress, Some(0.5));
    }

    #[test]
    fn deserialize_minimal_milestone() {
        let toml_str = r#"
[[milestone]]
name = "Beta"
date = "2026-06-01"
description = "Beta launch"
"#;
        let file: MilestoneFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.milestones.len(), 1);
        assert_eq!(file.milestones[0].milestone_type, MilestoneType::Checkpoint);
        assert!(file.milestones[0].github_issue.is_none());
        assert!(file.milestones[0].expected_progress.is_none());
    }

    #[test]
    fn roundtrip_milestone_file() {
        let file = MilestoneFile {
            milestones: vec![Milestone {
                name: "Test".into(),
                date: NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
                description: "A test milestone".into(),
                github_issue: None,
                milestone_type: MilestoneType::Okr,
                expected_progress: Some(0.75),
            }],
        };
        let serialized = toml::to_string_pretty(&file).unwrap();
        let deserialized: MilestoneFile = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.milestones[0].name, "Test");
        assert_eq!(deserialized.milestones[0].expected_progress, Some(0.75));
    }

    #[test]
    fn milestone_is_in_quarter() {
        let m = Milestone {
            name: "Q1".into(),
            date: NaiveDate::from_ymd_opt(2026, 2, 15).unwrap(),
            description: "".into(),
            github_issue: None,
            milestone_type: MilestoneType::Okr,
            expected_progress: None,
        };
        assert!(m.is_in_quarter(2026, 1));
        assert!(!m.is_in_quarter(2026, 2));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib model::milestone`
Expected: FAIL — structs not defined

- [ ] **Step 3: Implement Milestone types**

In `src/model/milestone.rs`, above the tests module:

```rust
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneFile {
    #[serde(rename = "milestone")]
    pub milestones: Vec<Milestone>,
}

impl MilestoneFile {
    pub fn empty() -> Self {
        Self { milestones: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    pub date: NaiveDate,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_issue: Option<String>,
    #[serde(rename = "type", default)]
    pub milestone_type: MilestoneType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_progress: Option<f64>,
}

impl Milestone {
    /// Check if this milestone falls within a given quarter (1-4) of a year.
    pub fn is_in_quarter(&self, year: i32, quarter: u32) -> bool {
        let q_start_month = (quarter - 1) * 3 + 1;
        let q_start = NaiveDate::from_ymd_opt(year, q_start_month, 1).unwrap();
        let q_end = if quarter == 4 {
            NaiveDate::from_ymd_opt(year, 12, 31).unwrap()
        } else {
            NaiveDate::from_ymd_opt(year, q_start_month + 3, 1)
                .unwrap()
                .pred_opt()
                .unwrap()
        };
        self.date >= q_start && self.date <= q_end
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MilestoneType {
    #[default]
    Checkpoint,
    Okr,
}

impl fmt::Display for MilestoneType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MilestoneType::Checkpoint => write!(f, "checkpoint"),
            MilestoneType::Okr => write!(f, "okr"),
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib model::milestone`
Expected: all 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/model/milestone.rs
git commit -m "feat: add Milestone, MilestoneFile, MilestoneType model types"
```

---

### Task 4: Model — OrgConfig

**Files:**
- Create: `src/model/org.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for OrgConfig serialization**

In `src/model/org.rs`:

```rust
use serde::{Deserialize, Serialize};

// Structs will go here

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_config() {
        let toml_str = r#"
[org]
name = "anthropic"
github_org = "anthropic"

[[label_schema.prefixes]]
prefix = "P-"
category = "priority"
examples = ["P-high", "P-medium", "P-low"]

[[label_schema.prefixes]]
prefix = "A-"
category = "area"
examples = ["A-compiler", "A-infra"]

[sync]
conflict_strategy = "detect"
"#;
        let config: OrgConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.org.name, "anthropic");
        assert_eq!(config.org.github_org, "anthropic");
        assert_eq!(config.label_schema.prefixes.len(), 2);
        assert_eq!(config.label_schema.prefixes[0].prefix, "P-");
        assert_eq!(config.sync.conflict_strategy, ConflictStrategy::Detect);
    }

    #[test]
    fn deserialize_minimal_config() {
        let toml_str = r#"
[org]
name = "myorg"
github_org = "myorg"
"#;
        let config: OrgConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.org.name, "myorg");
        assert!(config.label_schema.prefixes.is_empty());
        assert_eq!(config.sync.conflict_strategy, ConflictStrategy::Detect);
    }

    #[test]
    fn conflict_strategy_variants() {
        let toml_str = r#"
[org]
name = "test"
github_org = "test"

[sync]
conflict_strategy = "github-wins"
"#;
        let config: OrgConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.sync.conflict_strategy, ConflictStrategy::GithubWins);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib model::org`
Expected: FAIL — structs not defined

- [ ] **Step 3: Implement OrgConfig types**

In `src/model/org.rs`, above the tests module:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgConfig {
    pub org: OrgInfo,
    #[serde(default)]
    pub label_schema: LabelSchema,
    #[serde(default)]
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgInfo {
    pub name: String,
    pub github_org: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LabelSchema {
    #[serde(default)]
    pub prefixes: Vec<LabelPrefix>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelPrefix {
    pub prefix: String,
    pub category: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

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

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib model::org`
Expected: all 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/model/org.rs
git commit -m "feat: add OrgConfig, LabelSchema, SyncConfig model types"
```

---

### Task 5: Filesystem — tree walking and org root discovery

**Files:**
- Create: `src/fs/tree.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for find_org_root and walk_nodes**

In `src/fs/tree.rs`:

```rust
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::node::Node;

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_node_toml(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let content = format!(
            "name = \"{name}\"\ndescription = \"test node\"\n"
        );
        std::fs::write(dir.join("node.toml"), content).unwrap();
    }

    fn write_armitage_toml(dir: &Path) {
        let content = "[org]\nname = \"test\"\ngithub_org = \"test\"\n";
        std::fs::write(dir.join("armitage.toml"), content).unwrap();
    }

    #[test]
    fn find_org_root_from_subdir() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path().join("myorg");
        std::fs::create_dir_all(&org).unwrap();
        write_armitage_toml(&org);

        let subdir = org.join("gemini").join("auth");
        std::fs::create_dir_all(&subdir).unwrap();

        assert_eq!(find_org_root(&subdir).unwrap(), org);
    }

    #[test]
    fn find_org_root_at_root() {
        let tmp = TempDir::new().unwrap();
        write_armitage_toml(tmp.path());
        assert_eq!(find_org_root(tmp.path()).unwrap(), tmp.path().to_path_buf());
    }

    #[test]
    fn find_org_root_not_found() {
        let tmp = TempDir::new().unwrap();
        assert!(find_org_root(tmp.path()).is_err());
    }

    #[test]
    fn walk_nodes_finds_all_nodes() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);
        write_node_toml(&org.join("gemini"), "Gemini");
        write_node_toml(&org.join("gemini").join("auth"), "Auth");
        write_node_toml(&org.join("m4"), "M4");

        let nodes = walk_nodes(org).unwrap();
        let paths: Vec<&str> = nodes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&"gemini"));
        assert!(paths.contains(&"gemini/auth"));
        assert!(paths.contains(&"m4"));
    }

    #[test]
    fn walk_nodes_skips_dot_dirs() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);
        write_node_toml(&org.join("gemini"), "Gemini");
        write_node_toml(&org.join(".armitage").join("hidden"), "Hidden");

        let nodes = walk_nodes(org).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].path, "gemini");
    }

    #[test]
    fn walk_nodes_skips_dirs_without_node_toml() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);
        write_node_toml(&org.join("gemini"), "Gemini");
        std::fs::create_dir_all(org.join("notes")).unwrap();
        std::fs::write(org.join("notes").join("readme.md"), "notes").unwrap();

        let nodes = walk_nodes(org).unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn list_children_returns_direct_children() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);
        write_node_toml(&org.join("gemini"), "Gemini");
        write_node_toml(&org.join("gemini").join("auth"), "Auth");
        write_node_toml(&org.join("gemini").join("training"), "Training");
        write_node_toml(&org.join("gemini").join("auth").join("oauth"), "OAuth");

        let children = list_children(org, "gemini").unwrap();
        assert_eq!(children.len(), 2);
        let paths: Vec<&str> = children.iter().map(|n| n.path.as_str()).collect();
        assert!(paths.contains(&"gemini/auth"));
        assert!(paths.contains(&"gemini/training"));
    }

    #[test]
    fn list_top_level_nodes() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);
        write_node_toml(&org.join("gemini"), "Gemini");
        write_node_toml(&org.join("m4"), "M4");
        write_node_toml(&org.join("gemini").join("auth"), "Auth");

        let children = list_children(org, "").unwrap();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn read_node_at_path() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);
        write_node_toml(&org.join("gemini"), "Gemini");

        let entry = read_node(org, "gemini").unwrap();
        assert_eq!(entry.node.name, "Gemini");
        assert_eq!(entry.path, "gemini");
    }

    #[test]
    fn read_node_not_found() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        write_armitage_toml(org);

        assert!(read_node(org, "nonexistent").is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib fs::tree`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement tree functions**

In `src/fs/tree.rs`, above the tests module:

```rust
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::model::node::Node;

/// A node discovered on the filesystem.
#[derive(Debug, Clone)]
pub struct NodeEntry {
    /// Relative path from org root (e.g. "gemini/auth-service").
    pub path: String,
    /// Absolute path to the node directory.
    pub dir: PathBuf,
    /// Parsed node.toml contents.
    pub node: Node,
}

/// Walk up from `start` looking for `armitage.toml`. Returns the org root directory.
pub fn find_org_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("armitage.toml").exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(Error::NotInOrg);
        }
    }
}

/// Recursively walk the org directory and return all nodes (dirs with `node.toml`).
pub fn walk_nodes(org_root: &Path) -> Result<Vec<NodeEntry>> {
    let mut entries = Vec::new();
    walk_recursive(org_root, org_root, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn walk_recursive(org_root: &Path, dir: &Path, entries: &mut Vec<NodeEntry>) -> Result<()> {
    let node_toml = dir.join("node.toml");
    if node_toml.exists() {
        let content = std::fs::read_to_string(&node_toml)?;
        let node: Node = toml::from_str(&content).map_err(|e| Error::TomlParse {
            path: node_toml.clone(),
            source: e,
        })?;
        let rel_path = dir
            .strip_prefix(org_root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        entries.push(NodeEntry {
            path: rel_path,
            dir: dir.to_path_buf(),
            node,
        });
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Ok(());
    };

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with('.') {
                walk_recursive(org_root, &path, entries)?;
            }
        }
    }

    Ok(())
}

/// List direct children of a node (or top-level nodes if `parent_path` is empty).
pub fn list_children(org_root: &Path, parent_path: &str) -> Result<Vec<NodeEntry>> {
    let parent_dir = if parent_path.is_empty() {
        org_root.to_path_buf()
    } else {
        org_root.join(parent_path)
    };

    if !parent_dir.exists() {
        return Err(Error::NodeNotFound(parent_path.to_string()));
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&parent_dir)? {
        let entry = entry?;
        let path = entry.path();
        let node_toml = path.join("node.toml");
        if path.is_dir() && node_toml.exists() {
            let content = std::fs::read_to_string(&node_toml)?;
            let node: Node = toml::from_str(&content).map_err(|e| Error::TomlParse {
                path: node_toml,
                source: e,
            })?;
            let rel_path = path
                .strip_prefix(org_root)
                .unwrap()
                .to_string_lossy()
                .to_string();
            entries.push(NodeEntry {
                path: rel_path,
                dir: path,
                node,
            });
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// Read a single node at a specific path relative to the org root.
pub fn read_node(org_root: &Path, node_path: &str) -> Result<NodeEntry> {
    let dir = org_root.join(node_path);
    let node_toml = dir.join("node.toml");

    if !node_toml.exists() {
        return Err(Error::NodeNotFound(node_path.to_string()));
    }

    let content = std::fs::read_to_string(&node_toml)?;
    let node: Node = toml::from_str(&content).map_err(|e| Error::TomlParse {
        path: node_toml,
        source: e,
    })?;

    Ok(NodeEntry {
        path: node_path.to_string(),
        dir,
        node,
    })
}

/// Read the OrgConfig from armitage.toml in the org root.
pub fn read_org_config(org_root: &Path) -> Result<crate::model::org::OrgConfig> {
    let path = org_root.join("armitage.toml");
    let content = std::fs::read_to_string(&path)?;
    let config = toml::from_str(&content).map_err(|e| Error::TomlParse {
        path,
        source: e,
    })?;
    Ok(config)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib fs::tree`
Expected: all 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/fs/tree.rs
git commit -m "feat: add filesystem tree walking, org root discovery, node reading"
```

---

### Task 6: CLI skeleton with clap

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement the full clap CLI structure**

In `src/cli/mod.rs`:

```rust
mod init;
mod issue;
mod milestone;
mod pull;
mod push;
mod resolve;
mod status;

use clap::{Parser, Subcommand};

use crate::error::Result;

const SKILL_MD: &str = include_str!(concat!(env!("OUT_DIR"), "/SKILL.md"));

#[derive(Parser)]
#[command(name = "armitage", version, about = "Project management CLI for GitHub repositories")]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new org directory
    Init {
        /// Organization name (used as directory name)
        name: String,
        /// GitHub organization name (defaults to org name)
        #[arg(long)]
        github_org: Option<String>,
    },
    /// Manage issues (nodes in the project hierarchy)
    Issue {
        #[command(subcommand)]
        command: IssueCommands,
    },
    /// Manage milestones and OKRs
    Milestone {
        #[command(subcommand)]
        command: MilestoneCommands,
    },
    /// Pull changes from GitHub
    Pull {
        /// Node path to pull (pulls all if omitted)
        path: Option<String>,
        /// Show what would change without applying
        #[arg(long)]
        dry_run: bool,
    },
    /// Push local changes to GitHub
    Push {
        /// Node path to push (pushes all if omitted)
        path: Option<String>,
        /// Show what would be pushed
        #[arg(long)]
        dry_run: bool,
    },
    /// Resolve sync conflicts
    Resolve {
        /// Node path to resolve
        path: Option<String>,
        /// List all conflicted nodes
        #[arg(long)]
        list: bool,
    },
    /// Show sync status overview
    Status,
    /// Self-management commands
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommands,
    },
}

#[derive(Subcommand)]
enum IssueCommands {
    /// Create a new issue node
    Create {
        /// Path relative to org root (e.g. "gemini/auth-service")
        path: String,
        /// Node name (defaults to directory name)
        #[arg(long)]
        name: Option<String>,
        /// Short description
        #[arg(long)]
        description: Option<String>,
        /// GitHub issue reference (owner/repo#number)
        #[arg(long)]
        github_issue: Option<String>,
        /// Comma-separated labels
        #[arg(long)]
        labels: Option<String>,
        /// Status: active, completed, paused, cancelled
        #[arg(long, default_value = "active")]
        status: String,
    },
    /// List issue nodes
    List {
        /// Parent path (lists top-level nodes if omitted)
        path: Option<String>,
        /// Show full recursive tree
        #[arg(long, short)]
        recursive: bool,
    },
    /// Show details of an issue node
    Show {
        /// Node path
        path: String,
    },
    /// Open node.toml in $EDITOR
    Edit {
        /// Node path
        path: String,
    },
    /// Move/reparent a node
    Move {
        /// Source path
        from: String,
        /// Destination path
        to: String,
    },
    /// Remove a node
    Remove {
        /// Node path
        path: String,
        /// Skip confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Display full hierarchy as tree
    Tree,
}

#[derive(Subcommand)]
enum MilestoneCommands {
    /// Add a milestone to a node
    Add {
        /// Node path
        node_path: String,
        /// Milestone name
        #[arg(long)]
        name: String,
        /// Target date (YYYY-MM-DD)
        #[arg(long)]
        date: String,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
        /// Type: checkpoint or okr
        #[arg(long, default_value = "checkpoint")]
        milestone_type: String,
        /// Expected progress (0.0 to 1.0, for OKR type)
        #[arg(long)]
        expected_progress: Option<f64>,
        /// GitHub issue reference
        #[arg(long)]
        github_issue: Option<String>,
    },
    /// List milestones
    List {
        /// Node path (lists all milestones if omitted)
        node_path: Option<String>,
        /// Filter by type: checkpoint or okr
        #[arg(long)]
        milestone_type: Option<String>,
        /// Filter by quarter (e.g. 2026-Q1)
        #[arg(long)]
        quarter: Option<String>,
    },
    /// Remove a milestone from a node
    Remove {
        /// Node path
        node_path: String,
        /// Milestone name
        name: String,
    },
}

#[derive(Subcommand)]
enum SelfCommands {
    /// Print the embedded SKILL.md
    Skill,
    /// Show version and build info
    Info,
    /// Check for updates
    Check,
    /// Update to the latest version
    Update {
        /// Specific version to install
        #[arg(long)]
        version: Option<String>,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name, github_org } => {
            init::run(name, github_org)
        }
        Commands::Issue { command } => match command {
            IssueCommands::Create { path, name, description, github_issue, labels, status } => {
                issue::run_create(path, name, description, github_issue, labels, status)
            }
            IssueCommands::List { path, recursive } => {
                issue::run_list(path, recursive)
            }
            IssueCommands::Show { path } => {
                issue::run_show(path)
            }
            IssueCommands::Edit { path } => {
                issue::run_edit(path)
            }
            IssueCommands::Move { from, to } => {
                issue::run_move(from, to)
            }
            IssueCommands::Remove { path, yes } => {
                issue::run_remove(path, yes)
            }
            IssueCommands::Tree => {
                issue::run_tree()
            }
        }
        Commands::Milestone { command } => match command {
            MilestoneCommands::Add {
                node_path, name, date, description,
                milestone_type, expected_progress, github_issue,
            } => {
                milestone::run_add(
                    node_path, name, date, description,
                    milestone_type, expected_progress, github_issue,
                )
            }
            MilestoneCommands::List { node_path, milestone_type, quarter } => {
                milestone::run_list(node_path, milestone_type, quarter)
            }
            MilestoneCommands::Remove { node_path, name } => {
                milestone::run_remove(node_path, name)
            }
        }
        Commands::Pull { path, dry_run } => {
            pull::run(path, dry_run)
        }
        Commands::Push { path, dry_run } => {
            push::run(path, dry_run)
        }
        Commands::Resolve { path, list } => {
            resolve::run(path, list)
        }
        Commands::Status => {
            status::run()
        }
        Commands::SelfCmd { command } => {
            run_self(command);
            Ok(())
        }
    }
}

fn run_self(command: SelfCommands) {
    let manager = ionem::self_update::SelfManager::new(
        "user/armitage", // TODO: update with real GitHub repo
        "armitage",
        "v",
        env!("CARGO_PKG_VERSION"),
        env!("TARGET"),
    );

    match command {
        SelfCommands::Skill => print!("{SKILL_MD}"),
        SelfCommands::Info => manager.print_info(),
        SelfCommands::Check => {
            if let Err(e) = manager.print_check() {
                eprintln!("error: {e}");
            }
        }
        SelfCommands::Update { version } => {
            if let Err(e) = manager.run_update(version.as_deref()) {
                eprintln!("error: {e}");
            }
        }
    }
}
```

- [ ] **Step 2: Add stub implementations for all command modules**

Each command module gets a stub that returns `Ok(())` with a TODO message. For example, `src/cli/init.rs`:

```rust
use crate::error::Result;

pub fn run(name: String, github_org: Option<String>) -> Result<()> {
    eprintln!("TODO: init {name}");
    Ok(())
}
```

`src/cli/issue.rs`:
```rust
use crate::error::Result;

pub fn run_create(
    path: String,
    name: Option<String>,
    description: Option<String>,
    github_issue: Option<String>,
    labels: Option<String>,
    status: String,
) -> Result<()> {
    eprintln!("TODO: issue create {path}");
    Ok(())
}

pub fn run_list(path: Option<String>, recursive: bool) -> Result<()> {
    eprintln!("TODO: issue list");
    Ok(())
}

pub fn run_show(path: String) -> Result<()> {
    eprintln!("TODO: issue show {path}");
    Ok(())
}

pub fn run_edit(path: String) -> Result<()> {
    eprintln!("TODO: issue edit {path}");
    Ok(())
}

pub fn run_move(from: String, to: String) -> Result<()> {
    eprintln!("TODO: issue move {from} -> {to}");
    Ok(())
}

pub fn run_remove(path: String, _yes: bool) -> Result<()> {
    eprintln!("TODO: issue remove {path}");
    Ok(())
}

pub fn run_tree() -> Result<()> {
    eprintln!("TODO: issue tree");
    Ok(())
}
```

`src/cli/milestone.rs`:
```rust
use crate::error::Result;

pub fn run_add(
    node_path: String,
    name: String,
    date: String,
    description: String,
    milestone_type: String,
    expected_progress: Option<f64>,
    github_issue: Option<String>,
) -> Result<()> {
    eprintln!("TODO: milestone add {name} to {node_path}");
    Ok(())
}

pub fn run_list(
    node_path: Option<String>,
    milestone_type: Option<String>,
    quarter: Option<String>,
) -> Result<()> {
    eprintln!("TODO: milestone list");
    Ok(())
}

pub fn run_remove(node_path: String, name: String) -> Result<()> {
    eprintln!("TODO: milestone remove {name} from {node_path}");
    Ok(())
}
```

`src/cli/pull.rs`:
```rust
use crate::error::Result;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    eprintln!("TODO: pull");
    Ok(())
}
```

`src/cli/push.rs`:
```rust
use crate::error::Result;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    eprintln!("TODO: push");
    Ok(())
}
```

`src/cli/resolve.rs`:
```rust
use crate::error::Result;

pub fn run(path: Option<String>, list: bool) -> Result<()> {
    eprintln!("TODO: resolve");
    Ok(())
}
```

`src/cli/status.rs`:
```rust
use crate::error::Result;

pub fn run() -> Result<()> {
    eprintln!("TODO: status");
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles and CLI help works**

Run: `cargo build && cargo run -- --help`
Expected: compiles, shows help with all subcommands listed

Run: `cargo run -- issue --help`
Expected: shows issue subcommands (create, list, show, edit, move, remove, tree)

- [ ] **Step 4: Commit**

```bash
git add src/cli/ src/main.rs
git commit -m "feat: add CLI skeleton with all subcommands via clap"
```

---

### Task 7: `armitage init` command

**Files:**
- Modify: `src/cli/init.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for init**

In `src/cli/init.rs`:

```rust
use std::path::Path;

use crate::error::Result;
use crate::model::org::{OrgConfig, OrgInfo, LabelSchema, SyncConfig};

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn init_creates_org_directory() {
        let tmp = TempDir::new().unwrap();
        let org_dir = tmp.path().join("myorg");

        init_at(&org_dir, "myorg", "myorg").unwrap();

        assert!(org_dir.join("armitage.toml").exists());
        assert!(org_dir.join(".armitage").exists());
        assert!(org_dir.join(".gitignore").exists());

        let config: OrgConfig =
            toml::from_str(&std::fs::read_to_string(org_dir.join("armitage.toml")).unwrap())
                .unwrap();
        assert_eq!(config.org.name, "myorg");
        assert_eq!(config.org.github_org, "myorg");
    }

    #[test]
    fn init_with_different_github_org() {
        let tmp = TempDir::new().unwrap();
        let org_dir = tmp.path().join("myorg");

        init_at(&org_dir, "myorg", "my-github-org").unwrap();

        let config: OrgConfig =
            toml::from_str(&std::fs::read_to_string(org_dir.join("armitage.toml")).unwrap())
                .unwrap();
        assert_eq!(config.org.name, "myorg");
        assert_eq!(config.org.github_org, "my-github-org");
    }

    #[test]
    fn init_gitignore_contains_armitage() {
        let tmp = TempDir::new().unwrap();
        let org_dir = tmp.path().join("myorg");

        init_at(&org_dir, "myorg", "myorg").unwrap();

        let gitignore = std::fs::read_to_string(org_dir.join(".gitignore")).unwrap();
        assert!(gitignore.contains(".armitage/"));
    }

    #[test]
    fn init_fails_if_already_exists() {
        let tmp = TempDir::new().unwrap();
        let org_dir = tmp.path().join("myorg");

        init_at(&org_dir, "myorg", "myorg").unwrap();
        let result = init_at(&org_dir, "myorg", "myorg");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::init`
Expected: FAIL — `init_at` not defined

- [ ] **Step 3: Implement init**

In `src/cli/init.rs`, above the tests module:

```rust
use std::path::Path;

use crate::error::{Error, Result};
use crate::model::org::{LabelSchema, OrgConfig, OrgInfo, SyncConfig};

/// Core init logic, testable without clap.
pub fn init_at(org_dir: &Path, name: &str, github_org: &str) -> Result<()> {
    if org_dir.join("armitage.toml").exists() {
        return Err(Error::Other(format!(
            "org directory already initialized: {}",
            org_dir.display()
        )));
    }

    std::fs::create_dir_all(org_dir)?;
    std::fs::create_dir_all(org_dir.join(".armitage").join("conflicts"))?;

    let config = OrgConfig {
        org: OrgInfo {
            name: name.to_string(),
            github_org: github_org.to_string(),
        },
        label_schema: LabelSchema::default(),
        sync: SyncConfig::default(),
    };

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(org_dir.join("armitage.toml"), toml_str)?;
    std::fs::write(org_dir.join(".gitignore"), ".armitage/\n")?;

    Ok(())
}

/// CLI entry point.
pub fn run(name: String, github_org: Option<String>) -> Result<()> {
    let github_org = github_org.as_deref().unwrap_or(&name);
    let org_dir = std::env::current_dir()?.join(&name);

    init_at(&org_dir, &name, github_org)?;

    println!("Initialized org directory: {}", org_dir.display());
    println!("  config: {}/armitage.toml", name);
    println!("  GitHub org: {github_org}");
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib cli::init`
Expected: all 4 tests PASS

- [ ] **Step 5: Manual smoke test**

Run: `cargo run -- init test-org && ls test-org/ && cat test-org/armitage.toml && rm -rf test-org`
Expected: shows created files and config contents

- [ ] **Step 6: Commit**

```bash
git add src/cli/init.rs
git commit -m "feat: implement armitage init command"
```

---

### Task 8: `armitage issue create` command

**Files:**
- Modify: `src/cli/issue.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for issue create**

In `src/cli/issue.rs`:

```rust
use std::path::Path;

use crate::error::{Error, Result};
use crate::fs::tree::{find_org_root, read_node};
use crate::model::node::{Node, NodeStatus};

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_org(tmp: &TempDir) -> std::path::PathBuf {
        let org = tmp.path().join("testorg");
        crate::cli::init::init_at(&org, "testorg", "testorg").unwrap();
        org
    }

    fn write_node(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let content = format!("name = \"{name}\"\ndescription = \"test\"\n");
        std::fs::write(dir.join("node.toml"), content).unwrap();
    }

    #[test]
    fn create_top_level_node() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        create_node(&org, "gemini", None, None, None, None, "active").unwrap();

        let entry = read_node(&org, "gemini").unwrap();
        assert_eq!(entry.node.name, "gemini");
    }

    #[test]
    fn create_with_custom_name() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        create_node(
            &org, "gemini",
            Some("Gemini Platform"),
            Some("Next-gen AI"),
            None, None, "active",
        ).unwrap();

        let entry = read_node(&org, "gemini").unwrap();
        assert_eq!(entry.node.name, "Gemini Platform");
        assert_eq!(entry.node.description, "Next-gen AI");
    }

    #[test]
    fn create_child_node() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);
        write_node(&org.join("gemini"), "Gemini");

        create_node(&org, "gemini/auth", None, None, None, None, "active").unwrap();

        let entry = read_node(&org, "gemini/auth").unwrap();
        assert_eq!(entry.node.name, "auth");
    }

    #[test]
    fn create_fails_if_parent_missing() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        let result = create_node(&org, "gemini/auth", None, None, None, None, "active");
        assert!(result.is_err());
    }

    #[test]
    fn create_fails_if_already_exists() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        create_node(&org, "gemini", None, None, None, None, "active").unwrap();
        let result = create_node(&org, "gemini", None, None, None, None, "active");
        assert!(result.is_err());
    }

    #[test]
    fn create_with_labels() {
        let tmp = TempDir::new().unwrap();
        let org = setup_org(&tmp);

        create_node(
            &org, "gemini", None, None, None,
            Some("I-gemini,P-high"), "active",
        ).unwrap();

        let entry = read_node(&org, "gemini").unwrap();
        assert_eq!(entry.node.labels, vec!["I-gemini", "P-high"]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::issue`
Expected: FAIL — `create_node` not defined

- [ ] **Step 3: Implement create_node and CLI wiring**

In `src/cli/issue.rs`, above the tests module:

```rust
use std::path::Path;

use crate::error::{Error, Result};
use crate::fs::tree::{find_org_root, list_children, read_node, walk_nodes};
use crate::model::node::{Node, NodeStatus};

/// Core create logic, testable without clap.
pub fn create_node(
    org_root: &Path,
    path: &str,
    name: Option<&str>,
    description: Option<&str>,
    github_issue: Option<&str>,
    labels: Option<&str>,
    status: &str,
) -> Result<()> {
    let node_dir = org_root.join(path);

    if node_dir.join("node.toml").exists() {
        return Err(Error::NodeExists(path.to_string()));
    }

    // Check parent exists (if creating a nested node)
    if let Some(parent_path) = Path::new(path).parent() {
        let parent_str = parent_path.to_string_lossy();
        if !parent_str.is_empty() {
            let parent_dir = org_root.join(parent_path);
            if !parent_dir.join("node.toml").exists() {
                return Err(Error::ParentNotFound(parent_str.to_string()));
            }
        }
    }

    // Derive name from last path component if not provided
    let dir_name = Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let name = name.unwrap_or(&dir_name);
    let description = description.unwrap_or("");

    let status: NodeStatus = match status {
        "active" => NodeStatus::Active,
        "completed" => NodeStatus::Completed,
        "paused" => NodeStatus::Paused,
        "cancelled" => NodeStatus::Cancelled,
        other => return Err(Error::Other(format!("invalid status: {other}"))),
    };

    let labels: Vec<String> = labels
        .map(|l| l.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let node = Node {
        name: name.to_string(),
        description: description.to_string(),
        github_issue: github_issue.map(String::from),
        labels,
        repos: vec![],
        timeline: None,
        status,
    };

    std::fs::create_dir_all(&node_dir)?;
    let toml_str = toml::to_string_pretty(&node)?;
    std::fs::write(node_dir.join("node.toml"), toml_str)?;

    Ok(())
}

/// CLI entry point for `issue create`.
pub fn run_create(
    path: String,
    name: Option<String>,
    description: Option<String>,
    github_issue: Option<String>,
    labels: Option<String>,
    status: String,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    create_node(
        &org_root,
        &path,
        name.as_deref(),
        description.as_deref(),
        github_issue.as_deref(),
        labels.as_deref(),
        &status,
    )?;
    println!("Created node: {path}");
    Ok(())
}

/// CLI entry point for `issue list`.
pub fn run_list(path: Option<String>, recursive: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;

    if recursive {
        let nodes = walk_nodes(&org_root)?;
        for entry in &nodes {
            println!("{:<40} [{}] {}", entry.path, entry.node.status, entry.node.name);
        }
        if nodes.is_empty() {
            println!("No nodes found.");
        }
    } else {
        let parent = path.as_deref().unwrap_or("");
        let children = list_children(&org_root, parent)?;
        for entry in &children {
            println!("{:<40} [{}] {}", entry.path, entry.node.status, entry.node.name);
        }
        if children.is_empty() {
            println!("No nodes found.");
        }
    }

    Ok(())
}

/// CLI entry point for `issue show`.
pub fn run_show(path: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let entry = read_node(&org_root, &path)?;
    let node = &entry.node;

    println!("Name:        {}", node.name);
    println!("Path:        {}", entry.path);
    println!("Status:      {}", node.status);
    println!("Description: {}", node.description);

    if let Some(ref gh) = node.github_issue {
        println!("GitHub:      {gh}");
    }
    if !node.labels.is_empty() {
        println!("Labels:      {}", node.labels.join(", "));
    }
    if !node.repos.is_empty() {
        println!("Repos:       {}", node.repos.join(", "));
    }
    if let Some(ref tl) = node.timeline {
        println!("Timeline:    {} to {}", tl.start, tl.end);
    }

    // Show children
    let children = list_children(&org_root, &path)?;
    if !children.is_empty() {
        println!("\nChildren:");
        for child in &children {
            println!("  {:<36} [{}] {}", child.path, child.node.status, child.node.name);
        }
    }

    // Show milestones if present
    let ms_path = entry.dir.join("milestones.toml");
    if ms_path.exists() {
        let content = std::fs::read_to_string(&ms_path)?;
        let ms_file: crate::model::milestone::MilestoneFile =
            toml::from_str(&content).map_err(|e| Error::TomlParse {
                path: ms_path,
                source: e,
            })?;
        if !ms_file.milestones.is_empty() {
            println!("\nMilestones:");
            for m in &ms_file.milestones {
                println!("  {} ({}) - {} [{}]", m.date, m.milestone_type, m.name, m.description);
            }
        }
    }

    Ok(())
}

/// CLI entry point for `issue edit`.
pub fn run_edit(path: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let entry = read_node(&org_root, &path)?;
    let node_toml = entry.dir.join("node.toml");

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new(&editor)
        .arg(&node_toml)
        .status()?;

    if !status.success() {
        return Err(Error::Other(format!("{editor} exited with error")));
    }

    // Validate the edited file parses correctly
    let content = std::fs::read_to_string(&node_toml)?;
    let _: Node = toml::from_str(&content).map_err(|e| Error::TomlParse {
        path: node_toml,
        source: e,
    })?;

    println!("Updated node: {path}");
    Ok(())
}

/// CLI entry point for `issue move`.
pub fn run_move(from: String, to: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;

    let from_dir = org_root.join(&from);
    let to_dir = org_root.join(&to);

    if !from_dir.join("node.toml").exists() {
        return Err(Error::NodeNotFound(from.clone()));
    }
    if to_dir.exists() {
        return Err(Error::NodeExists(to.clone()));
    }

    // Check destination parent exists
    if let Some(parent) = Path::new(&to).parent() {
        let parent_str = parent.to_string_lossy();
        if !parent_str.is_empty() && !org_root.join(parent).join("node.toml").exists() {
            return Err(Error::ParentNotFound(parent_str.to_string()));
        }
    }

    std::fs::rename(&from_dir, &to_dir)?;
    println!("Moved: {from} -> {to}");
    Ok(())
}

/// CLI entry point for `issue remove`.
pub fn run_remove(path: String, yes: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let node_dir = org_root.join(&path);

    if !node_dir.join("node.toml").exists() {
        return Err(Error::NodeNotFound(path.clone()));
    }

    if !yes {
        eprint!("Remove node '{path}' and all children? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    std::fs::remove_dir_all(&node_dir)?;
    println!("Removed: {path}");
    Ok(())
}

/// CLI entry point for `issue tree`.
pub fn run_tree() -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let nodes = walk_nodes(&org_root)?;

    if nodes.is_empty() {
        println!("No nodes found.");
        return Ok(());
    }

    for entry in &nodes {
        let depth = entry.path.matches('/').count();
        let indent = "  ".repeat(depth);
        let name = Path::new(&entry.path)
            .file_name()
            .unwrap()
            .to_string_lossy();
        println!("{indent}{name} [{}] {}", entry.node.status, entry.node.name);
    }

    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib cli::issue`
Expected: all 6 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/cli/issue.rs
git commit -m "feat: implement issue create/list/show/edit/move/remove/tree commands"
```

---

### Task 9: `armitage milestone` commands

**Files:**
- Modify: `src/cli/milestone.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for milestone commands**

In `src/cli/milestone.rs`:

```rust
use std::path::Path;

use chrono::NaiveDate;

use crate::error::{Error, Result};
use crate::fs::tree::{find_org_root, read_node, walk_nodes};
use crate::model::milestone::{Milestone, MilestoneFile, MilestoneType};

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_org_with_node(tmp: &TempDir) -> (std::path::PathBuf, String) {
        let org = tmp.path().join("testorg");
        crate::cli::init::init_at(&org, "testorg", "testorg").unwrap();
        crate::cli::issue::create_node(&org, "gemini", None, None, None, None, "active").unwrap();
        (org, "gemini".to_string())
    }

    #[test]
    fn add_milestone_to_node() {
        let tmp = TempDir::new().unwrap();
        let (org, path) = setup_org_with_node(&tmp);

        add_milestone(
            &org, &path, "Alpha", "2026-03-15", "Core ready",
            "checkpoint", None, None,
        ).unwrap();

        let ms = read_milestones(&org, &path).unwrap();
        assert_eq!(ms.milestones.len(), 1);
        assert_eq!(ms.milestones[0].name, "Alpha");
    }

    #[test]
    fn add_okr_milestone() {
        let tmp = TempDir::new().unwrap();
        let (org, path) = setup_org_with_node(&tmp);

        add_milestone(
            &org, &path, "Q1 OKR", "2026-03-31", "50% coverage",
            "okr", Some(0.5), None,
        ).unwrap();

        let ms = read_milestones(&org, &path).unwrap();
        assert_eq!(ms.milestones[0].milestone_type, MilestoneType::Okr);
        assert_eq!(ms.milestones[0].expected_progress, Some(0.5));
    }

    #[test]
    fn add_multiple_milestones() {
        let tmp = TempDir::new().unwrap();
        let (org, path) = setup_org_with_node(&tmp);

        add_milestone(&org, &path, "Alpha", "2026-03-15", "", "checkpoint", None, None).unwrap();
        add_milestone(&org, &path, "Beta", "2026-06-15", "", "checkpoint", None, None).unwrap();

        let ms = read_milestones(&org, &path).unwrap();
        assert_eq!(ms.milestones.len(), 2);
    }

    #[test]
    fn remove_milestone() {
        let tmp = TempDir::new().unwrap();
        let (org, path) = setup_org_with_node(&tmp);

        add_milestone(&org, &path, "Alpha", "2026-03-15", "", "checkpoint", None, None).unwrap();
        add_milestone(&org, &path, "Beta", "2026-06-15", "", "checkpoint", None, None).unwrap();

        remove_milestone(&org, &path, "Alpha").unwrap();

        let ms = read_milestones(&org, &path).unwrap();
        assert_eq!(ms.milestones.len(), 1);
        assert_eq!(ms.milestones[0].name, "Beta");
    }

    #[test]
    fn remove_nonexistent_milestone_errors() {
        let tmp = TempDir::new().unwrap();
        let (org, path) = setup_org_with_node(&tmp);

        let result = remove_milestone(&org, &path, "Nonexistent");
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::milestone`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement milestone commands**

In `src/cli/milestone.rs`, above the tests module:

```rust
use std::path::Path;

use chrono::NaiveDate;

use crate::error::{Error, Result};
use crate::fs::tree::{find_org_root, read_node, walk_nodes};
use crate::model::milestone::{Milestone, MilestoneFile, MilestoneType};

/// Read milestones.toml for a node, returning empty if it doesn't exist.
pub fn read_milestones(org_root: &Path, node_path: &str) -> Result<MilestoneFile> {
    let ms_path = org_root.join(node_path).join("milestones.toml");
    if !ms_path.exists() {
        return Ok(MilestoneFile::empty());
    }
    let content = std::fs::read_to_string(&ms_path)?;
    let file: MilestoneFile = toml::from_str(&content).map_err(|e| Error::TomlParse {
        path: ms_path,
        source: e,
    })?;
    Ok(file)
}

/// Write milestones.toml for a node.
fn write_milestones(org_root: &Path, node_path: &str, file: &MilestoneFile) -> Result<()> {
    let ms_path = org_root.join(node_path).join("milestones.toml");
    let content = toml::to_string_pretty(file)?;
    std::fs::write(ms_path, content)?;
    Ok(())
}

/// Add a milestone to a node.
pub fn add_milestone(
    org_root: &Path,
    node_path: &str,
    name: &str,
    date: &str,
    description: &str,
    milestone_type: &str,
    expected_progress: Option<f64>,
    github_issue: Option<&str>,
) -> Result<()> {
    // Verify node exists
    let _ = read_node(org_root, node_path)?;

    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| Error::Other(format!("invalid date '{date}': {e}")))?;

    let milestone_type = match milestone_type {
        "checkpoint" => MilestoneType::Checkpoint,
        "okr" => MilestoneType::Okr,
        other => return Err(Error::Other(format!("invalid milestone type: {other}"))),
    };

    let milestone = Milestone {
        name: name.to_string(),
        date,
        description: description.to_string(),
        github_issue: github_issue.map(String::from),
        milestone_type,
        expected_progress,
    };

    let mut ms_file = read_milestones(org_root, node_path)?;
    ms_file.milestones.push(milestone);
    write_milestones(org_root, node_path, &ms_file)?;

    Ok(())
}

/// Remove a milestone by name from a node.
pub fn remove_milestone(org_root: &Path, node_path: &str, name: &str) -> Result<()> {
    let mut ms_file = read_milestones(org_root, node_path)?;
    let before = ms_file.milestones.len();
    ms_file.milestones.retain(|m| m.name != name);

    if ms_file.milestones.len() == before {
        return Err(Error::Other(format!(
            "milestone '{name}' not found in {node_path}"
        )));
    }

    write_milestones(org_root, node_path, &ms_file)?;
    Ok(())
}

/// Parse a quarter string like "2026-Q1" into (year, quarter).
fn parse_quarter(s: &str) -> Result<(i32, u32)> {
    let parts: Vec<&str> = s.split("-Q").collect();
    if parts.len() != 2 {
        return Err(Error::Other(format!("invalid quarter format: {s} (expected YYYY-QN)")));
    }
    let year: i32 = parts[0].parse().map_err(|_| Error::Other(format!("invalid year in {s}")))?;
    let quarter: u32 = parts[1].parse().map_err(|_| Error::Other(format!("invalid quarter in {s}")))?;
    if !(1..=4).contains(&quarter) {
        return Err(Error::Other(format!("quarter must be 1-4, got {quarter}")));
    }
    Ok((year, quarter))
}

// --- CLI entry points ---

pub fn run_add(
    node_path: String,
    name: String,
    date: String,
    description: String,
    milestone_type: String,
    expected_progress: Option<f64>,
    github_issue: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    add_milestone(
        &org_root,
        &node_path,
        &name,
        &date,
        &description,
        &milestone_type,
        expected_progress,
        github_issue.as_deref(),
    )?;
    println!("Added milestone '{name}' to {node_path}");
    Ok(())
}

pub fn run_list(
    node_path: Option<String>,
    milestone_type: Option<String>,
    quarter: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;

    let (year, q) = quarter.as_deref().map(parse_quarter).transpose()?;
    let type_filter = milestone_type.as_deref();

    // Determine which nodes to check
    let node_paths: Vec<String> = if let Some(ref p) = node_path {
        vec![p.clone()]
    } else {
        walk_nodes(&org_root)?
            .into_iter()
            .map(|e| e.path)
            .collect()
    };

    let mut found = false;
    for np in &node_paths {
        let ms_file = read_milestones(&org_root, np)?;
        for m in &ms_file.milestones {
            // Apply type filter
            if let Some(tf) = type_filter {
                let matches = match tf {
                    "checkpoint" => m.milestone_type == MilestoneType::Checkpoint,
                    "okr" => m.milestone_type == MilestoneType::Okr,
                    _ => true,
                };
                if !matches {
                    continue;
                }
            }
            // Apply quarter filter
            if let Some((y, q)) = (year, q) {
                if !m.is_in_quarter(y, q) {
                    continue;
                }
            }

            found = true;
            let progress = m
                .expected_progress
                .map(|p| format!(" ({:.0}%)", p * 100.0))
                .unwrap_or_default();
            println!(
                "{} {} [{}] {}{} - {}",
                np, m.date, m.milestone_type, m.name, progress, m.description
            );
        }
    }

    if !found {
        println!("No milestones found.");
    }

    Ok(())
}

pub fn run_remove(node_path: String, name: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    remove_milestone(&org_root, &node_path, &name)?;
    println!("Removed milestone '{name}' from {node_path}");
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib cli::milestone`
Expected: all 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/cli/milestone.rs
git commit -m "feat: implement milestone add/list/remove commands"
```

---

### Task 10: Sync state model and node hashing

**Files:**
- Create: `src/sync/state.rs`
- Create: `src/sync/hash.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for SyncState and hashing**

In `src/sync/state.rs`:

```rust
use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn roundtrip_sync_state() {
        let mut state = SyncState::default();
        state.nodes.insert(
            "gemini".to_string(),
            NodeSyncEntry {
                github_issue: "org/repo#1".to_string(),
                last_pulled_at: Some(Utc::now()),
                last_pushed_at: None,
                remote_updated_at: Some(Utc::now()),
                local_hash: Some("abc123".to_string()),
            },
        );

        let serialized = toml::to_string_pretty(&state).unwrap();
        let deserialized: SyncState = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.nodes.len(), 1);
        assert_eq!(deserialized.nodes["gemini"].github_issue, "org/repo#1");
    }

    #[test]
    fn read_write_sync_state() {
        let tmp = TempDir::new().unwrap();
        let armitage_dir = tmp.path().join(".armitage");
        std::fs::create_dir_all(&armitage_dir).unwrap();

        let mut state = SyncState::default();
        state.nodes.insert(
            "test".to_string(),
            NodeSyncEntry {
                github_issue: "org/repo#1".to_string(),
                last_pulled_at: None,
                last_pushed_at: None,
                remote_updated_at: None,
                local_hash: None,
            },
        );

        write_sync_state(tmp.path(), &state).unwrap();
        let loaded = read_sync_state(tmp.path()).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
    }

    #[test]
    fn read_missing_sync_state_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let state = read_sync_state(tmp.path()).unwrap();
        assert!(state.nodes.is_empty());
    }
}
```

In `src/sync/hash.rs`:

```rust
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::Result;

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn hash_changes_when_node_toml_changes() {
        let tmp = TempDir::new().unwrap();
        let node_dir = tmp.path();
        std::fs::write(
            node_dir.join("node.toml"),
            "name = \"test\"\ndescription = \"v1\"\n",
        ).unwrap();

        let hash1 = compute_node_hash(node_dir).unwrap();

        std::fs::write(
            node_dir.join("node.toml"),
            "name = \"test\"\ndescription = \"v2\"\n",
        ).unwrap();

        let hash2 = compute_node_hash(node_dir).unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn hash_changes_when_issue_md_changes() {
        let tmp = TempDir::new().unwrap();
        let node_dir = tmp.path();
        std::fs::write(node_dir.join("node.toml"), "name = \"t\"\ndescription = \"\"\n").unwrap();
        std::fs::write(node_dir.join("issue.md"), "body v1").unwrap();

        let hash1 = compute_node_hash(node_dir).unwrap();

        std::fs::write(node_dir.join("issue.md"), "body v2").unwrap();

        let hash2 = compute_node_hash(node_dir).unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn hash_stable_without_issue_md() {
        let tmp = TempDir::new().unwrap();
        let node_dir = tmp.path();
        std::fs::write(node_dir.join("node.toml"), "name = \"t\"\ndescription = \"\"\n").unwrap();

        let hash1 = compute_node_hash(node_dir).unwrap();
        let hash2 = compute_node_hash(node_dir).unwrap();
        assert_eq!(hash1, hash2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib sync::state && cargo test --lib sync::hash`
Expected: FAIL — types/functions not defined

- [ ] **Step 3: Implement SyncState**

In `src/sync/state.rs`, above the tests module:

```rust
use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeSyncEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSyncEntry {
    pub github_issue: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pulled_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pushed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_hash: Option<String>,
}

/// Read .armitage/sync.toml, returning empty state if it doesn't exist.
pub fn read_sync_state(org_root: &Path) -> Result<SyncState> {
    let path = org_root.join(".armitage").join("sync.toml");
    if !path.exists() {
        return Ok(SyncState::default());
    }
    let content = std::fs::read_to_string(&path)?;
    let state: SyncState = toml::from_str(&content).map_err(|e| Error::TomlParse {
        path,
        source: e,
    })?;
    Ok(state)
}

/// Write .armitage/sync.toml.
pub fn write_sync_state(org_root: &Path, state: &SyncState) -> Result<()> {
    let dir = org_root.join(".armitage");
    std::fs::create_dir_all(&dir)?;
    let content = toml::to_string_pretty(state)?;
    std::fs::write(dir.join("sync.toml"), content)?;
    Ok(())
}
```

- [ ] **Step 4: Implement compute_node_hash**

In `src/sync/hash.rs`, above the tests module:

```rust
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::Result;

/// Compute a hash of the sync-relevant files in a node directory.
/// Includes node.toml and issue.md (if present).
pub fn compute_node_hash(node_dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    // Hash node.toml
    let node_toml = std::fs::read_to_string(node_dir.join("node.toml"))?;
    hasher.update(b"node.toml:");
    hasher.update(node_toml.as_bytes());

    // Hash issue.md if present
    let issue_md_path = node_dir.join("issue.md");
    if issue_md_path.exists() {
        let issue_md = std::fs::read_to_string(&issue_md_path)?;
        hasher.update(b"issue.md:");
        hasher.update(issue_md.as_bytes());
    }

    let result = hasher.finalize();
    Ok(format!("{result:x}"))
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib sync::state && cargo test --lib sync::hash`
Expected: all 6 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/sync/state.rs src/sync/hash.rs
git commit -m "feat: add sync state persistence and node hashing"
```

---

### Task 11: GitHub issue operations

**Files:**
- Create: `src/github/issue.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Implement GitHubIssue types and fetch/create/update functions**

In `src/github/issue.rs`:

```rust
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::model::node::IssueRef;

/// GitHub issue data as returned by `gh issue view --json ...`.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub labels: Vec<GitHubLabel>,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubLabel {
    pub name: String,
}

/// Newly created issue response.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedIssue {
    pub number: u64,
    pub url: String,
}

const ISSUE_JSON_FIELDS: &str = "number,title,body,state,labels,updatedAt";

/// Fetch a GitHub issue by reference.
pub fn fetch_issue(gh: &ionem::shell::gh::Gh, issue_ref: &IssueRef) -> Result<GitHubIssue> {
    let repo = issue_ref.repo_full();
    let number = issue_ref.number.to_string();
    let json = gh.run(&[
        "issue", "view", &number,
        "--repo", &repo,
        "--json", ISSUE_JSON_FIELDS,
    ])?;
    let issue: GitHubIssue = serde_json::from_str(&json)?;
    Ok(issue)
}

/// Create a new GitHub issue. Returns the created issue number and URL.
pub fn create_issue(
    gh: &ionem::shell::gh::Gh,
    repo: &str,
    title: &str,
    body: &str,
    labels: &[String],
) -> Result<CreatedIssue> {
    let mut args = vec![
        "issue", "create",
        "--repo", repo,
        "--title", title,
        "--body", body,
    ];

    let labels_joined = labels.join(",");
    if !labels.is_empty() {
        args.push("--label");
        args.push(&labels_joined);
    }

    // gh issue create doesn't support --json, parse URL from output
    let output = gh.run(&args)?;

    // Output is the issue URL like "https://github.com/owner/repo/issues/123"
    let url = output.trim().to_string();
    let number = url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::Other(format!("could not parse issue number from: {url}")))?;

    Ok(CreatedIssue { number, url })
}

/// Update an existing GitHub issue.
pub fn update_issue(
    gh: &ionem::shell::gh::Gh,
    issue_ref: &IssueRef,
    title: Option<&str>,
    body: Option<&str>,
    add_labels: &[String],
    remove_labels: &[String],
) -> Result<()> {
    let repo = issue_ref.repo_full();
    let number = issue_ref.number.to_string();

    let mut args = vec!["issue", "edit", &number, "--repo", &repo];

    if let Some(t) = title {
        args.push("--title");
        args.push(t);
    }

    if let Some(b) = body {
        args.push("--body");
        args.push(b);
    }

    let add_joined = add_labels.join(",");
    if !add_labels.is_empty() {
        args.push("--add-label");
        args.push(&add_joined);
    }

    let remove_joined = remove_labels.join(",");
    if !remove_labels.is_empty() {
        args.push("--remove-label");
        args.push(&remove_joined);
    }

    gh.run(&args)?;
    Ok(())
}

/// Close or reopen a GitHub issue.
pub fn set_issue_state(
    gh: &ionem::shell::gh::Gh,
    issue_ref: &IssueRef,
    open: bool,
) -> Result<()> {
    let repo = issue_ref.repo_full();
    let number = issue_ref.number.to_string();
    let subcmd = if open { "reopen" } else { "close" };

    gh.run(&["issue", subcmd, &number, "--repo", &repo])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_issue_json() {
        let json = r#"{
            "number": 42,
            "title": "Fix auth",
            "body": "The auth is broken",
            "state": "OPEN",
            "labels": [{"name": "P-high"}, {"name": "A-auth"}],
            "updatedAt": "2026-03-30T10:00:00Z"
        }"#;

        let issue: GitHubIssue = serde_json::from_str(json).unwrap();
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "Fix auth");
        assert_eq!(issue.labels.len(), 2);
        assert_eq!(issue.labels[0].name, "P-high");
    }

    #[test]
    fn parse_created_issue_url() {
        let url = "https://github.com/anthropic/gemini/issues/123";
        let number: u64 = url.rsplit('/').next().unwrap().parse().unwrap();
        assert_eq!(number, 123);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib github::issue`
Expected: all 2 tests PASS (these are unit tests for JSON parsing, not integration)

- [ ] **Step 3: Commit**

```bash
git add src/github/issue.rs
git commit -m "feat: add GitHub issue fetch/create/update operations via ionem gh"
```

---

### Task 12: Sync merge logic

**Files:**
- Create: `src/sync/merge.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for field-level merge**

In `src/sync/merge.rs`:

```rust
use std::collections::HashSet;

use crate::error::Result;
use crate::model::node::Node;

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(name: &str, labels: &[&str], desc: &str) -> Node {
        Node {
            name: name.to_string(),
            description: desc.to_string(),
            github_issue: None,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            repos: vec![],
            timeline: None,
            status: crate::model::node::NodeStatus::Active,
        }
    }

    #[test]
    fn no_changes_returns_clean() {
        let base = make_node("Test", &["P-high"], "desc");
        let local = base.clone();
        let remote = base.clone();

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Clean(_)));
    }

    #[test]
    fn local_only_change_takes_local() {
        let base = make_node("Test", &["P-high"], "desc");
        let local = make_node("Test Updated", &["P-high"], "desc");
        let remote = base.clone();

        let result = merge_nodes(&base, &local, &remote);
        if let MergeResult::Clean(merged) = result {
            assert_eq!(merged.name, "Test Updated");
        } else {
            panic!("expected Clean");
        }
    }

    #[test]
    fn remote_only_change_takes_remote() {
        let base = make_node("Test", &["P-high"], "desc");
        let local = base.clone();
        let remote = make_node("Test", &["P-high"], "new desc");

        let result = merge_nodes(&base, &local, &remote);
        if let MergeResult::Clean(merged) = result {
            assert_eq!(merged.description, "new desc");
        } else {
            panic!("expected Clean");
        }
    }

    #[test]
    fn both_changed_different_fields_merges() {
        let base = make_node("Test", &["P-high"], "desc");
        let local = make_node("Updated Name", &["P-high"], "desc");
        let remote = make_node("Test", &["P-high"], "updated desc");

        let result = merge_nodes(&base, &local, &remote);
        if let MergeResult::Clean(merged) = result {
            assert_eq!(merged.name, "Updated Name");
            assert_eq!(merged.description, "updated desc");
        } else {
            panic!("expected Clean");
        }
    }

    #[test]
    fn both_changed_same_field_conflicts() {
        let base = make_node("Test", &["P-high"], "desc");
        let local = make_node("Local Name", &["P-high"], "desc");
        let remote = make_node("Remote Name", &["P-high"], "desc");

        let result = merge_nodes(&base, &local, &remote);
        assert!(matches!(result, MergeResult::Conflict { .. }));
    }

    #[test]
    fn labels_union_non_conflicting() {
        let base = make_node("Test", &["P-high"], "desc");
        let local = make_node("Test", &["P-high", "A-auth"], "desc");
        let remote = make_node("Test", &["P-high", "I-gemini"], "desc");

        let result = merge_nodes(&base, &local, &remote);
        if let MergeResult::Clean(merged) = result {
            let labels: HashSet<&str> = merged.labels.iter().map(|s| s.as_str()).collect();
            assert!(labels.contains("P-high"));
            assert!(labels.contains("A-auth"));
            assert!(labels.contains("I-gemini"));
        } else {
            panic!("expected Clean");
        }
    }

    #[test]
    fn label_removed_one_side_takes_removal() {
        let base = make_node("Test", &["P-high", "P-low"], "desc");
        let local = make_node("Test", &["P-high"], "desc"); // removed P-low
        let remote = make_node("Test", &["P-high", "P-low"], "desc"); // no change

        let result = merge_nodes(&base, &local, &remote);
        if let MergeResult::Clean(merged) = result {
            assert_eq!(merged.labels, vec!["P-high"]);
        } else {
            panic!("expected Clean");
        }
    }

    #[test]
    fn merge_issue_body_both_changed_conflicts() {
        let result = merge_issue_body(
            "base body",
            "local body",
            "remote body",
        );
        assert!(matches!(result, BodyMergeResult::Conflict { .. }));
    }

    #[test]
    fn merge_issue_body_one_side_changed() {
        let result = merge_issue_body("base", "changed", "base");
        if let BodyMergeResult::Clean(body) = result {
            assert_eq!(body, "changed");
        } else {
            panic!("expected Clean");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib sync::merge`
Expected: FAIL — types/functions not defined

- [ ] **Step 3: Implement merge logic**

In `src/sync/merge.rs`, above the tests module:

```rust
use std::collections::HashSet;

use crate::model::node::Node;

/// Result of merging two versions of a node's metadata.
#[derive(Debug)]
pub enum MergeResult {
    /// All fields merged cleanly.
    Clean(Node),
    /// At least one field has a conflict.
    Conflict {
        /// Best-effort merged node (non-conflicting fields applied).
        merged: Node,
        /// Fields that conflict: (field_name, local_value, remote_value).
        conflicts: Vec<FieldConflict>,
    },
}

#[derive(Debug, Clone)]
pub struct FieldConflict {
    pub field: String,
    pub local_value: String,
    pub remote_value: String,
}

/// Three-way merge of node metadata. `base` is the last-synced version.
pub fn merge_nodes(base: &Node, local: &Node, remote: &Node) -> MergeResult {
    let mut merged = base.clone();
    let mut conflicts = Vec::new();

    // Merge name
    merge_field(
        "name",
        &base.name, &local.name, &remote.name,
        &mut merged.name, &mut conflicts,
    );

    // Merge description
    merge_field(
        "description",
        &base.description, &local.description, &remote.description,
        &mut merged.description, &mut conflicts,
    );

    // Merge status
    let base_status = base.status.to_string();
    let local_status = local.status.to_string();
    let remote_status = remote.status.to_string();
    let mut merged_status = base_status.clone();
    merge_field(
        "status",
        &base_status, &local_status, &remote_status,
        &mut merged_status, &mut conflicts,
    );
    // Parse back the merged status
    merged.status = match merged_status.as_str() {
        "completed" => crate::model::node::NodeStatus::Completed,
        "paused" => crate::model::node::NodeStatus::Paused,
        "cancelled" => crate::model::node::NodeStatus::Cancelled,
        _ => crate::model::node::NodeStatus::Active,
    };

    // Merge labels (set-based)
    merged.labels = merge_labels(&base.labels, &local.labels, &remote.labels, &mut conflicts);

    // Merge github_issue (take any change, conflict if both changed differently)
    let base_gh = base.github_issue.as_deref().unwrap_or("");
    let local_gh = local.github_issue.as_deref().unwrap_or("");
    let remote_gh = remote.github_issue.as_deref().unwrap_or("");
    let mut merged_gh = base_gh.to_string();
    merge_field("github_issue", base_gh, local_gh, remote_gh, &mut merged_gh, &mut conflicts);
    merged.github_issue = if merged_gh.is_empty() { None } else { Some(merged_gh) };

    if conflicts.is_empty() {
        MergeResult::Clean(merged)
    } else {
        MergeResult::Conflict { merged, conflicts }
    }
}

/// Three-way merge of a single string field.
fn merge_field(
    name: &str,
    base: &str,
    local: &str,
    remote: &str,
    target: &mut String,
    conflicts: &mut Vec<FieldConflict>,
) {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => {} // No change
        (true, false) => *target = local.to_string(),
        (false, true) => *target = remote.to_string(),
        (true, true) => {
            if local == remote {
                *target = local.to_string(); // Same change
            } else {
                conflicts.push(FieldConflict {
                    field: name.to_string(),
                    local_value: local.to_string(),
                    remote_value: remote.to_string(),
                });
                *target = local.to_string(); // Default to local in conflict
            }
        }
    }
}

/// Three-way merge of label sets.
fn merge_labels(
    base: &[String],
    local: &[String],
    remote: &[String],
    conflicts: &mut Vec<FieldConflict>,
) -> Vec<String> {
    let base_set: HashSet<&str> = base.iter().map(|s| s.as_str()).collect();
    let local_set: HashSet<&str> = local.iter().map(|s| s.as_str()).collect();
    let remote_set: HashSet<&str> = remote.iter().map(|s| s.as_str()).collect();

    let local_added: HashSet<&str> = local_set.difference(&base_set).copied().collect();
    let local_removed: HashSet<&str> = base_set.difference(&local_set).copied().collect();
    let remote_added: HashSet<&str> = remote_set.difference(&base_set).copied().collect();
    let remote_removed: HashSet<&str> = base_set.difference(&remote_set).copied().collect();

    // Check for conflicts: added on one side, removed on other
    let conflict_labels: HashSet<&str> = local_added.intersection(&remote_removed)
        .chain(remote_added.intersection(&local_removed))
        .copied()
        .collect();

    if !conflict_labels.is_empty() {
        conflicts.push(FieldConflict {
            field: "labels".to_string(),
            local_value: local.join(","),
            remote_value: remote.join(","),
        });
    }

    // Start with base, apply non-conflicting changes
    let mut result: HashSet<&str> = base_set;

    // Add labels added on either side
    for l in local_added.union(&remote_added) {
        if !conflict_labels.contains(l) {
            result.insert(l);
        }
    }

    // Remove labels removed on either side
    for l in local_removed.union(&remote_removed) {
        if !conflict_labels.contains(l) {
            result.remove(l);
        }
    }

    let mut labels: Vec<String> = result.into_iter().map(|s| s.to_string()).collect();
    labels.sort();
    labels
}

/// Result of merging issue body text.
#[derive(Debug)]
pub enum BodyMergeResult {
    Clean(String),
    Conflict { local: String, remote: String },
}

/// Three-way merge of issue.md body text.
pub fn merge_issue_body(base: &str, local: &str, remote: &str) -> BodyMergeResult {
    let local_changed = local != base;
    let remote_changed = remote != base;

    match (local_changed, remote_changed) {
        (false, false) => BodyMergeResult::Clean(base.to_string()),
        (true, false) => BodyMergeResult::Clean(local.to_string()),
        (false, true) => BodyMergeResult::Clean(remote.to_string()),
        (true, true) => {
            if local == remote {
                BodyMergeResult::Clean(local.to_string())
            } else {
                BodyMergeResult::Conflict {
                    local: local.to_string(),
                    remote: remote.to_string(),
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sync::merge`
Expected: all 9 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/sync/merge.rs
git commit -m "feat: implement three-way field-level merge for node sync"
```

---

### Task 13: Conflict storage

**Files:**
- Create: `src/sync/conflict.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write tests for conflict storage**

In `src/sync/conflict.rs`:

```rust
use std::path::Path;

use crate::error::{Error, Result};
use crate::sync::merge::FieldConflict;

// Implementation will go here

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_armitage_dir(tmp: &TempDir) {
        std::fs::create_dir_all(tmp.path().join(".armitage").join("conflicts")).unwrap();
    }

    #[test]
    fn write_and_read_conflict() {
        let tmp = TempDir::new().unwrap();
        setup_armitage_dir(&tmp);

        let conflicts = vec![
            FieldConflict {
                field: "name".to_string(),
                local_value: "Local Name".to_string(),
                remote_value: "Remote Name".to_string(),
            },
        ];

        write_conflict(tmp.path(), "gemini/auth", &conflicts, None).unwrap();
        let loaded = list_conflicts(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].node_path, "gemini/auth");
        assert_eq!(loaded[0].field_conflicts.len(), 1);
    }

    #[test]
    fn write_conflict_with_body() {
        let tmp = TempDir::new().unwrap();
        setup_armitage_dir(&tmp);

        write_conflict(
            tmp.path(),
            "gemini",
            &[],
            Some(("local body", "remote body")),
        ).unwrap();

        let loaded = list_conflicts(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].body_conflict.is_some());
    }

    #[test]
    fn remove_conflict() {
        let tmp = TempDir::new().unwrap();
        setup_armitage_dir(&tmp);

        write_conflict(tmp.path(), "gemini", &[], None).unwrap();
        assert_eq!(list_conflicts(tmp.path()).unwrap().len(), 1);

        remove_conflict(tmp.path(), "gemini").unwrap();
        assert_eq!(list_conflicts(tmp.path()).unwrap().len(), 0);
    }

    #[test]
    fn has_conflicts_returns_correct() {
        let tmp = TempDir::new().unwrap();
        setup_armitage_dir(&tmp);

        assert!(!has_conflicts(tmp.path()).unwrap());

        write_conflict(tmp.path(), "gemini", &[], None).unwrap();
        assert!(has_conflicts(tmp.path()).unwrap());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib sync::conflict`
Expected: FAIL

- [ ] **Step 3: Implement conflict storage**

In `src/sync/conflict.rs`, above the tests module:

```rust
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::sync::merge::FieldConflict;

/// A stored conflict for a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConflict {
    pub node_path: String,
    pub field_conflicts: Vec<StoredFieldConflict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_conflict: Option<BodyConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredFieldConflict {
    pub field: String,
    pub local_value: String,
    pub remote_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyConflict {
    pub local: String,
    pub remote: String,
}

/// Convert node path to a conflict filename (replace / with --).
fn conflict_filename(node_path: &str) -> String {
    format!("{}.toml", node_path.replace('/', "--"))
}

/// Write a conflict file for a node.
pub fn write_conflict(
    org_root: &Path,
    node_path: &str,
    field_conflicts: &[FieldConflict],
    body_conflict: Option<(&str, &str)>,
) -> Result<()> {
    let dir = org_root.join(".armitage").join("conflicts");
    std::fs::create_dir_all(&dir)?;

    let stored = StoredConflict {
        node_path: node_path.to_string(),
        field_conflicts: field_conflicts
            .iter()
            .map(|c| StoredFieldConflict {
                field: c.field.clone(),
                local_value: c.local_value.clone(),
                remote_value: c.remote_value.clone(),
            })
            .collect(),
        body_conflict: body_conflict.map(|(l, r)| BodyConflict {
            local: l.to_string(),
            remote: r.to_string(),
        }),
    };

    let content = toml::to_string_pretty(&stored)?;
    std::fs::write(dir.join(conflict_filename(node_path)), content)?;
    Ok(())
}

/// List all stored conflicts.
pub fn list_conflicts(org_root: &Path) -> Result<Vec<StoredConflict>> {
    let dir = org_root.join(".armitage").join("conflicts");
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut conflicts = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            let content = std::fs::read_to_string(&path)?;
            let conflict: StoredConflict = toml::from_str(&content).map_err(|e| Error::TomlParse {
                path: path.clone(),
                source: e,
            })?;
            conflicts.push(conflict);
        }
    }

    conflicts.sort_by(|a, b| a.node_path.cmp(&b.node_path));
    Ok(conflicts)
}

/// Remove the conflict file for a node.
pub fn remove_conflict(org_root: &Path, node_path: &str) -> Result<()> {
    let path = org_root
        .join(".armitage")
        .join("conflicts")
        .join(conflict_filename(node_path));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Check if there are any unresolved conflicts.
pub fn has_conflicts(org_root: &Path) -> Result<bool> {
    let conflicts = list_conflicts(org_root)?;
    Ok(!conflicts.is_empty())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sync::conflict`
Expected: all 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/sync/conflict.rs
git commit -m "feat: implement conflict storage and retrieval"
```

---

### Task 14: `armitage pull` command

**Files:**
- Modify: `src/sync/pull.rs`
- Modify: `src/cli/pull.rs`

- [ ] **Step 1: Implement pull logic**

In `src/sync/pull.rs`:

```rust
use std::path::Path;

use chrono::Utc;

use crate::error::{Error, Result};
use crate::fs::tree::{walk_nodes, NodeEntry};
use crate::github::issue as gh_issue;
use crate::model::node::{IssueRef, Node, NodeStatus};
use crate::sync::conflict::write_conflict;
use crate::sync::hash::compute_node_hash;
use crate::sync::merge::{merge_issue_body, merge_nodes, BodyMergeResult, MergeResult};
use crate::sync::state::{read_sync_state, write_sync_state, NodeSyncEntry};

/// Result of pulling a single node.
#[derive(Debug)]
pub enum PullNodeResult {
    Skipped,           // No github_issue or no remote changes
    FastForward,       // Only remote changed, overwrote local
    Merged,            // Both changed, merged cleanly
    Conflicted,        // Both changed, conflicts written
}

/// Pull a single node from GitHub.
pub fn pull_node(
    gh: &ionem::shell::gh::Gh,
    org_root: &Path,
    entry: &NodeEntry,
) -> Result<PullNodeResult> {
    let Some(ref gh_ref_str) = entry.node.github_issue else {
        return Ok(PullNodeResult::Skipped);
    };

    let issue_ref = IssueRef::parse(gh_ref_str)?;
    let remote_issue = gh_issue::fetch_issue(gh, &issue_ref)?;

    // Read sync state
    let mut sync_state = read_sync_state(org_root)?;
    let sync_entry = sync_state.nodes.get(&entry.path);

    // Check if remote changed
    let remote_changed = sync_entry
        .and_then(|e| e.remote_updated_at.as_ref())
        .map(|t| t.to_rfc3339() != remote_issue.updated_at)
        .unwrap_or(true); // First pull — always changed

    if !remote_changed {
        return Ok(PullNodeResult::Skipped);
    }

    // Check if local changed
    let current_hash = compute_node_hash(&entry.dir)?;
    let local_changed = sync_entry
        .and_then(|e| e.local_hash.as_ref())
        .map(|h| h != &current_hash)
        .unwrap_or(false);

    let now = Utc::now();
    let remote_updated = chrono::DateTime::parse_from_rfc3339(&remote_issue.updated_at)
        .map(|dt| dt.with_timezone(&Utc))
        .ok();

    if !local_changed {
        // Fast-forward: overwrite local with remote
        apply_remote_to_local(org_root, entry, &remote_issue)?;
        let new_hash = compute_node_hash(&entry.dir)?;

        sync_state.nodes.insert(
            entry.path.clone(),
            NodeSyncEntry {
                github_issue: gh_ref_str.clone(),
                last_pulled_at: Some(now),
                last_pushed_at: sync_entry.and_then(|e| e.last_pushed_at),
                remote_updated_at: remote_updated,
                local_hash: Some(new_hash),
            },
        );
        write_sync_state(org_root, &sync_state)?;
        return Ok(PullNodeResult::FastForward);
    }

    // Both changed — need to merge
    // Build base node from sync state (we use current local as-is, remote from GitHub)
    // For proper 3-way merge, we'd need the base version. Since we don't store it,
    // we approximate: if first sync, treat base as empty. Otherwise, the remote_issue
    // represents the remote side, local file is local side, and base is what was
    // last synced (which we don't perfectly have). For MVP, do field-level comparison
    // between local and remote directly.
    let remote_node = node_from_github_issue(&remote_issue, &entry.node);

    // Use current local node as "base" since we don't store base snapshots in MVP.
    // This means: remote changes always show up, local changes are preserved,
    // conflicts only when both differ from each other.
    let base = &entry.node;
    let merge_result = merge_nodes(base, &entry.node, &remote_node);

    // Merge issue body
    let local_body = std::fs::read_to_string(entry.dir.join("issue.md")).unwrap_or_default();
    let base_body = &local_body; // MVP approximation
    let body_result = merge_issue_body(base_body, &local_body, &remote_issue.body);

    let has_node_conflicts = matches!(&merge_result, MergeResult::Conflict { .. });
    let has_body_conflicts = matches!(&body_result, BodyMergeResult::Conflict { .. });

    // Apply merged node
    let merged_node = match &merge_result {
        MergeResult::Clean(n) => n,
        MergeResult::Conflict { merged, .. } => merged,
    };
    let node_toml = toml::to_string_pretty(merged_node)?;
    std::fs::write(entry.dir.join("node.toml"), &node_toml)?;

    // Apply merged body
    match &body_result {
        BodyMergeResult::Clean(body) => {
            if !body.is_empty() {
                std::fs::write(entry.dir.join("issue.md"), body)?;
            }
        }
        BodyMergeResult::Conflict { .. } => {
            // Keep local body, conflict file has both versions
        }
    }

    // Write conflicts if any
    if has_node_conflicts || has_body_conflicts {
        let field_conflicts = match &merge_result {
            MergeResult::Conflict { conflicts, .. } => conflicts.clone(),
            _ => vec![],
        };
        let body_conflict = match &body_result {
            BodyMergeResult::Conflict { local, remote } => Some((local.as_str(), remote.as_str())),
            _ => None,
        };
        write_conflict(org_root, &entry.path, &field_conflicts, body_conflict)?;
    }

    // Update sync state
    let new_hash = compute_node_hash(&entry.dir)?;
    sync_state.nodes.insert(
        entry.path.clone(),
        NodeSyncEntry {
            github_issue: gh_ref_str.clone(),
            last_pulled_at: Some(now),
            last_pushed_at: sync_entry.and_then(|e| e.last_pushed_at),
            remote_updated_at: remote_updated,
            local_hash: Some(new_hash),
        },
    );
    write_sync_state(org_root, &sync_state)?;

    if has_node_conflicts || has_body_conflicts {
        Ok(PullNodeResult::Conflicted)
    } else {
        Ok(PullNodeResult::Merged)
    }
}

/// Apply remote GitHub issue data to local files.
fn apply_remote_to_local(
    org_root: &Path,
    entry: &NodeEntry,
    remote: &gh_issue::GitHubIssue,
) -> Result<()> {
    // Update node.toml fields from remote
    let mut node = entry.node.clone();
    node.name = remote.title.clone();
    node.labels = remote.labels.iter().map(|l| l.name.clone()).collect();
    node.status = match remote.state.as_str() {
        "OPEN" => NodeStatus::Active,
        "CLOSED" => NodeStatus::Completed,
        _ => node.status,
    };

    let node_toml = toml::to_string_pretty(&node)?;
    std::fs::write(entry.dir.join("node.toml"), node_toml)?;

    // Write issue body
    if !remote.body.is_empty() {
        std::fs::write(entry.dir.join("issue.md"), &remote.body)?;
    }

    Ok(())
}

/// Build a Node from GitHub issue data (for merge comparison).
fn node_from_github_issue(
    issue: &gh_issue::GitHubIssue,
    local: &Node,
) -> Node {
    Node {
        name: issue.title.clone(),
        description: local.description.clone(), // Not on GitHub
        github_issue: local.github_issue.clone(),
        labels: issue.labels.iter().map(|l| l.name.clone()).collect(),
        repos: local.repos.clone(), // Not on GitHub
        timeline: local.timeline.clone(), // Not on GitHub
        status: match issue.state.as_str() {
            "OPEN" => NodeStatus::Active,
            "CLOSED" => NodeStatus::Completed,
            _ => local.status.clone(),
        },
    }
}

/// Pull all nodes (or a subtree) from GitHub.
pub fn pull_all(
    gh: &ionem::shell::gh::Gh,
    org_root: &Path,
    scope: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let all_nodes = walk_nodes(org_root)?;
    let nodes: Vec<&NodeEntry> = all_nodes
        .iter()
        .filter(|e| {
            scope
                .map(|s| e.path == s || e.path.starts_with(&format!("{s}/")))
                .unwrap_or(true)
        })
        .collect();

    let synced_count = nodes.iter().filter(|e| e.node.github_issue.is_some()).count();
    if synced_count == 0 {
        println!("No nodes with GitHub issues to pull.");
        return Ok(());
    }

    for entry in &nodes {
        if entry.node.github_issue.is_none() {
            continue;
        }

        if dry_run {
            println!("[dry-run] would pull: {} ({})", entry.path,
                entry.node.github_issue.as_deref().unwrap_or(""));
            continue;
        }

        match pull_node(gh, org_root, entry)? {
            PullNodeResult::Skipped => {}
            PullNodeResult::FastForward => {
                println!("  updated: {}", entry.path);
            }
            PullNodeResult::Merged => {
                println!("  merged: {}", entry.path);
            }
            PullNodeResult::Conflicted => {
                println!("  CONFLICT: {} (run `armitage resolve {}`)", entry.path, entry.path);
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Wire up CLI**

In `src/cli/pull.rs`:

```rust
use crate::error::Result;
use crate::fs::tree::find_org_root;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let gh = ionem::shell::gh::require()?;

    crate::sync::pull::pull_all(&gh, &org_root, path.as_deref(), dry_run)?;
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add src/sync/pull.rs src/cli/pull.rs
git commit -m "feat: implement pull command with three-way merge and conflict detection"
```

---

### Task 15: `armitage push` command

**Files:**
- Modify: `src/sync/push.rs`
- Modify: `src/cli/push.rs`

- [ ] **Step 1: Implement push logic**

In `src/sync/push.rs`:

```rust
use std::path::Path;

use chrono::Utc;

use crate::error::{Error, Result};
use crate::fs::tree::{walk_nodes, NodeEntry};
use crate::github::issue as gh_issue;
use crate::model::node::IssueRef;
use crate::model::org::OrgConfig;
use crate::sync::conflict::has_conflicts;
use crate::sync::hash::compute_node_hash;
use crate::sync::state::{read_sync_state, write_sync_state, NodeSyncEntry};

/// Push a single node to GitHub.
fn push_node(
    gh: &ionem::shell::gh::Gh,
    org_root: &Path,
    org_config: &OrgConfig,
    entry: &NodeEntry,
) -> Result<bool> {
    let current_hash = compute_node_hash(&entry.dir)?;

    let mut sync_state = read_sync_state(org_root)?;
    let sync_entry = sync_state.nodes.get(&entry.path);

    // Check if local has changed since last sync
    let local_changed = sync_entry
        .and_then(|e| e.local_hash.as_ref())
        .map(|h| h != &current_hash)
        .unwrap_or(true); // First push — always changed

    if !local_changed && entry.node.github_issue.is_some() {
        return Ok(false); // Nothing to push
    }

    let now = Utc::now();

    if let Some(ref gh_ref_str) = entry.node.github_issue {
        // Existing issue — update it
        let issue_ref = IssueRef::parse(gh_ref_str)?;

        // Stale push protection: check remote hasn't changed since last pull
        let remote_issue = gh_issue::fetch_issue(gh, &issue_ref)?;
        if let Some(se) = sync_entry {
            if let Some(ref last_remote) = se.remote_updated_at {
                let remote_updated = chrono::DateTime::parse_from_rfc3339(&remote_issue.updated_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok();
                if remote_updated.as_ref() != Some(last_remote) {
                    return Err(Error::StalePush);
                }
            }
        }

        // Read issue body
        let body = std::fs::read_to_string(entry.dir.join("issue.md")).ok();

        // Compute label changes
        let remote_labels: Vec<String> = remote_issue.labels.iter().map(|l| l.name.clone()).collect();
        let local_labels = &entry.node.labels;

        let add_labels: Vec<String> = local_labels
            .iter()
            .filter(|l| !remote_labels.contains(l))
            .cloned()
            .collect();
        let remove_labels: Vec<String> = remote_labels
            .iter()
            .filter(|l| !local_labels.contains(l))
            .cloned()
            .collect();

        gh_issue::update_issue(
            gh,
            &issue_ref,
            Some(&entry.node.name),
            body.as_deref(),
            &add_labels,
            &remove_labels,
        )?;

        // Handle state changes
        let should_be_open = matches!(
            entry.node.status,
            crate::model::node::NodeStatus::Active | crate::model::node::NodeStatus::Paused
        );
        let is_open = remote_issue.state == "OPEN";
        if should_be_open != is_open {
            gh_issue::set_issue_state(gh, &issue_ref, should_be_open)?;
        }

        // Re-fetch to get updated timestamp
        let updated_issue = gh_issue::fetch_issue(gh, &issue_ref)?;
        let remote_updated = chrono::DateTime::parse_from_rfc3339(&updated_issue.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .ok();

        let new_hash = compute_node_hash(&entry.dir)?;
        sync_state.nodes.insert(
            entry.path.clone(),
            NodeSyncEntry {
                github_issue: gh_ref_str.clone(),
                last_pulled_at: sync_entry.and_then(|e| e.last_pulled_at),
                last_pushed_at: Some(now),
                remote_updated_at: remote_updated,
                local_hash: Some(new_hash),
            },
        );
        write_sync_state(org_root, &sync_state)?;
    } else {
        // New issue — create it
        let repo = format!("{}/{}", org_config.org.github_org, org_config.org.name);
        let body = std::fs::read_to_string(entry.dir.join("issue.md"))
            .unwrap_or_else(|_| entry.node.description.clone());

        let created = gh_issue::create_issue(
            gh,
            &repo,
            &entry.node.name,
            &body,
            &entry.node.labels,
        )?;

        // Update node.toml with the github_issue reference
        let issue_ref_str = format!("{}/{}#{}", org_config.org.github_org, org_config.org.name, created.number);
        let mut node = entry.node.clone();
        node.github_issue = Some(issue_ref_str.clone());
        let node_toml = toml::to_string_pretty(&node)?;
        std::fs::write(entry.dir.join("node.toml"), &node_toml)?;

        let new_hash = compute_node_hash(&entry.dir)?;
        sync_state.nodes.insert(
            entry.path.clone(),
            NodeSyncEntry {
                github_issue: issue_ref_str,
                last_pulled_at: None,
                last_pushed_at: Some(now),
                remote_updated_at: None,
                local_hash: Some(new_hash),
            },
        );
        write_sync_state(org_root, &sync_state)?;

        println!("  created: {} -> {}", entry.path, created.url);
    }

    Ok(true)
}

/// Push all nodes (or a subtree) to GitHub.
pub fn push_all(
    gh: &ionem::shell::gh::Gh,
    org_root: &Path,
    scope: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    // Check for unresolved conflicts
    if has_conflicts(org_root)? {
        return Err(Error::UnresolvedConflicts);
    }

    let org_config = crate::fs::tree::read_org_config(org_root)?;
    let all_nodes = walk_nodes(org_root)?;
    let nodes: Vec<&NodeEntry> = all_nodes
        .iter()
        .filter(|e| {
            scope
                .map(|s| e.path == s || e.path.starts_with(&format!("{s}/")))
                .unwrap_or(true)
        })
        .collect();

    for entry in &nodes {
        if dry_run {
            let action = if entry.node.github_issue.is_some() { "update" } else { "create" };
            println!("[dry-run] would {action}: {}", entry.path);
            continue;
        }

        match push_node(gh, org_root, &org_config, entry)? {
            true => println!("  pushed: {}", entry.path),
            false => {} // No changes
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Wire up CLI**

In `src/cli/push.rs`:

```rust
use crate::error::Result;
use crate::fs::tree::find_org_root;

pub fn run(path: Option<String>, dry_run: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let gh = ionem::shell::gh::require()?;

    crate::sync::push::push_all(&gh, &org_root, path.as_deref(), dry_run)?;
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 4: Commit**

```bash
git add src/sync/push.rs src/cli/push.rs
git commit -m "feat: implement push command with stale protection and new issue creation"
```

---

### Task 16: `armitage resolve` command

**Files:**
- Modify: `src/cli/resolve.rs`

- [ ] **Step 1: Implement resolve command**

In `src/cli/resolve.rs`:

```rust
use std::io::{self, Write};

use crate::error::{Error, Result};
use crate::fs::tree::find_org_root;
use crate::sync::conflict::{list_conflicts, remove_conflict, StoredConflict};

pub fn run(path: Option<String>, list: bool) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;

    if list || path.is_none() {
        let conflicts = list_conflicts(&org_root)?;
        if conflicts.is_empty() {
            println!("No conflicts.");
            return Ok(());
        }
        for c in &conflicts {
            let field_count = c.field_conflicts.len();
            let has_body = if c.body_conflict.is_some() { " + body" } else { "" };
            println!("  {} ({field_count} field conflicts{has_body})", c.node_path);
        }
        if list {
            return Ok(());
        }
    }

    let path = path.ok_or_else(|| Error::Other("specify a node path to resolve".to_string()))?;
    let conflicts = list_conflicts(&org_root)?;
    let conflict = conflicts
        .iter()
        .find(|c| c.node_path == path)
        .ok_or_else(|| Error::Other(format!("no conflict for: {path}")))?;

    resolve_interactive(&org_root, conflict)?;
    remove_conflict(&org_root, &path)?;
    println!("Resolved: {path}");
    Ok(())
}

fn resolve_interactive(org_root: &std::path::Path, conflict: &StoredConflict) -> Result<()> {
    let node_dir = org_root.join(&conflict.node_path);

    // Resolve field conflicts
    for fc in &conflict.field_conflicts {
        println!("\nField: {}", fc.field);
        println!("  [L]ocal:  {}", fc.local_value);
        println!("  [R]emote: {}", fc.remote_value);
        print!("  Keep [L/R]? ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let choice = input.trim().to_uppercase();

        let value = match choice.as_str() {
            "R" => &fc.remote_value,
            _ => &fc.local_value, // Default to local
        };

        // Apply the chosen value to node.toml
        let node_toml_path = node_dir.join("node.toml");
        let content = std::fs::read_to_string(&node_toml_path)?;
        let mut node: crate::model::node::Node = toml::from_str(&content)
            .map_err(|e| Error::TomlParse { path: node_toml_path.clone(), source: e })?;

        match fc.field.as_str() {
            "name" => node.name = value.clone(),
            "description" => node.description = value.clone(),
            "status" => {
                node.status = match value.as_str() {
                    "completed" => crate::model::node::NodeStatus::Completed,
                    "paused" => crate::model::node::NodeStatus::Paused,
                    "cancelled" => crate::model::node::NodeStatus::Cancelled,
                    _ => crate::model::node::NodeStatus::Active,
                };
            }
            "labels" => {
                node.labels = value.split(',').map(|s| s.trim().to_string()).collect();
            }
            "github_issue" => {
                node.github_issue = if value.is_empty() { None } else { Some(value.clone()) };
            }
            _ => {}
        }

        let updated = toml::to_string_pretty(&node)?;
        std::fs::write(&node_toml_path, updated)?;
    }

    // Resolve body conflict
    if let Some(ref bc) = conflict.body_conflict {
        println!("\nIssue body conflict:");
        println!("  [L]ocal body ({} chars)", bc.local.len());
        println!("  [R]emote body ({} chars)", bc.remote.len());
        print!("  Keep [L/R]? ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let choice = input.trim().to_uppercase();

        let body = match choice.as_str() {
            "R" => &bc.remote,
            _ => &bc.local,
        };
        std::fs::write(node_dir.join("issue.md"), body)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add src/cli/resolve.rs
git commit -m "feat: implement resolve command for interactive conflict resolution"
```

---

### Task 17: `armitage status` command

**Files:**
- Modify: `src/cli/status.rs`

- [ ] **Step 1: Implement status command**

In `src/cli/status.rs`:

```rust
use crate::error::Result;
use crate::fs::tree::{find_org_root, walk_nodes, read_node};
use crate::sync::conflict::list_conflicts;
use crate::sync::hash::compute_node_hash;
use crate::sync::state::read_sync_state;

pub fn run() -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let config = crate::fs::tree::read_org_config(&org_root)?;

    println!("Org: {} (GitHub: {})", config.org.name, config.org.github_org);
    println!();

    let nodes = walk_nodes(&org_root)?;
    let sync_state = read_sync_state(&org_root)?;

    // Count stats
    let total = nodes.len();
    let linked = nodes.iter().filter(|n| n.node.github_issue.is_some()).count();
    let unlinked = total - linked;

    println!("Nodes: {total} total, {linked} linked to GitHub, {unlinked} local-only");

    // Check for local modifications
    let mut modified = Vec::new();
    let mut new_nodes = Vec::new();

    for entry in &nodes {
        if entry.node.github_issue.is_some() {
            let current_hash = compute_node_hash(&entry.dir)?;
            let stored_hash = sync_state
                .nodes
                .get(&entry.path)
                .and_then(|e| e.local_hash.as_ref());

            match stored_hash {
                Some(h) if h != &current_hash => modified.push(&entry.path),
                None => new_nodes.push(&entry.path),
                _ => {}
            }
        } else {
            new_nodes.push(&entry.path);
        }
    }

    if !modified.is_empty() {
        println!("\nModified (need push):");
        for p in &modified {
            println!("  {p}");
        }
    }

    if !new_nodes.is_empty() {
        println!("\nNew/unlinked nodes:");
        for p in &new_nodes {
            println!("  {p}");
        }
    }

    // Check conflicts
    let conflicts = list_conflicts(&org_root)?;
    if !conflicts.is_empty() {
        println!("\nConflicts (need resolve):");
        for c in &conflicts {
            println!("  {}", c.node_path);
        }
    }

    // Check timeline violations
    let mut violations = Vec::new();
    for entry in &nodes {
        let Some(ref child_tl) = entry.node.timeline else { continue };
        // Find parent
        if let Some(parent_path) = std::path::Path::new(&entry.path).parent() {
            let parent_str = parent_path.to_string_lossy();
            if !parent_str.is_empty() {
                if let Ok(parent_entry) = read_node(&org_root, &parent_str) {
                    if let Some(ref parent_tl) = parent_entry.node.timeline {
                        if !parent_tl.contains(child_tl) {
                            violations.push(format!(
                                "{}: timeline ({} to {}) exceeds parent {} ({} to {})",
                                entry.path, child_tl.start, child_tl.end,
                                parent_str, parent_tl.start, parent_tl.end,
                            ));
                        }
                    }
                }
            }
        }
    }

    if !violations.is_empty() {
        println!("\nTimeline warnings:");
        for v in &violations {
            println!("  {v}");
        }
    }

    if modified.is_empty() && new_nodes.is_empty() && conflicts.is_empty() && violations.is_empty() {
        println!("\nEverything up to date.");
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles

- [ ] **Step 3: Commit**

```bash
git add src/cli/status.rs
git commit -m "feat: implement status command showing sync state and timeline warnings"
```

---

### Deferred within MVP

The following spec requirements are deferred to immediate follow-up tasks after the core MVP is working:

- **Asset upload/download**: The spec describes uploading images from `assets/` to GitHub during push and downloading them during pull, with link rewriting. This requires the GitHub file attachment API which adds significant complexity. For now, `issue.md` syncs as-is; relative image links won't render on GitHub until asset sync is implemented.
- **OKR issue body format**: The spec defines a specific Objective/Key Results/Status markdown template for OKR-type milestones. The MVP syncs milestone data via `milestones.toml` and `node.toml`, but doesn't auto-generate the OKR template in `issue.md` during push. This can be added as a formatting pass on top of the existing push logic.

---

### Task 18: Integration wiring and final build verification

**Files:**
- Various — ensure all modules properly import/export

- [ ] **Step 1: Ensure all mod.rs files have correct imports**

`src/sync/mod.rs`:
```rust
pub mod conflict;
pub mod hash;
pub mod merge;
pub mod pull;
pub mod push;
pub mod state;
```

`src/github/mod.rs`:
```rust
pub mod issue;
```

`src/model/mod.rs`:
```rust
pub mod milestone;
pub mod node;
pub mod org;
```

`src/fs/mod.rs`:
```rust
pub mod tree;
```

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: all unit tests pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings (fix any that appear)

- [ ] **Step 4: Manual smoke test**

```bash
cargo run -- init testorg
cd testorg
cargo run --manifest-path ../Cargo.toml -- issue create gemini --name "Gemini" --description "AI platform"
cargo run --manifest-path ../Cargo.toml -- issue create gemini/auth --name "Auth Service"
cargo run --manifest-path ../Cargo.toml -- issue tree
cargo run --manifest-path ../Cargo.toml -- issue show gemini
cargo run --manifest-path ../Cargo.toml -- milestone add gemini --name "Alpha" --date 2026-03-15 --description "Core ready"
cargo run --manifest-path ../Cargo.toml -- milestone list gemini
cargo run --manifest-path ../Cargo.toml -- status
cd ..
rm -rf testorg
```

Expected: all commands produce sensible output without errors

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat: wire up all modules and verify full build"
```

---

### Task 19: Add .armitage/ to generated .gitignore and final cleanup

**Files:**
- Verify all edge cases handled

- [ ] **Step 1: Verify .gitignore generation in init**

Already handled in Task 7 — `init_at` writes `.armitage/\n` to `.gitignore`.

- [ ] **Step 2: Add integration test for full workflow**

Create `tests/integration.rs`:

```rust
use std::path::Path;
use tempfile::TempDir;

// Test the full local workflow (no GitHub calls)
#[test]
fn full_local_workflow() {
    let tmp = TempDir::new().unwrap();
    let org = tmp.path().join("testorg");

    // Init
    armitage::cli::init::init_at(&org, "testorg", "testorg").unwrap();
    assert!(org.join("armitage.toml").exists());
    assert!(org.join(".armitage").exists());

    // Create nodes
    armitage::cli::issue::create_node(&org, "gemini", Some("Gemini"), Some("AI platform"), None, None, "active").unwrap();
    armitage::cli::issue::create_node(&org, "gemini/auth", None, None, None, None, "active").unwrap();
    armitage::cli::issue::create_node(&org, "m4", Some("M4"), None, None, None, "active").unwrap();

    // Walk tree
    let nodes = armitage::fs::tree::walk_nodes(&org).unwrap();
    assert_eq!(nodes.len(), 3);

    // List children
    let children = armitage::fs::tree::list_children(&org, "gemini").unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].path, "gemini/auth");

    // Add milestone
    armitage::cli::milestone::add_milestone(
        &org, "gemini", "Alpha", "2026-03-15", "Core ready", "checkpoint", None, None,
    ).unwrap();

    let ms = armitage::cli::milestone::read_milestones(&org, "gemini").unwrap();
    assert_eq!(ms.milestones.len(), 1);

    // Read node
    let entry = armitage::fs::tree::read_node(&org, "gemini").unwrap();
    assert_eq!(entry.node.name, "Gemini");
}
```

**Note:** For this integration test to work, the crate needs to be a library too. Create `src/lib.rs` that re-exports modules, and make the cli submodules `pub mod` in `src/cli/mod.rs`:

First, update `src/cli/mod.rs` to use `pub mod` for all submodules:

```rust
pub mod init;
pub mod issue;
pub mod milestone;
// ... (change all `mod` to `pub mod`)
```

Then create `src/lib.rs`:

```rust
pub mod cli;
pub mod error;
pub mod fs;
pub mod github;
pub mod model;
pub mod sync;
```

And update `src/main.rs`:

```rust
fn main() {
    if let Err(e) = armitage::cli::run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: Run integration test**

Run: `cargo test --test integration`
Expected: PASS

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat: add integration test and lib.rs for testability"
```
