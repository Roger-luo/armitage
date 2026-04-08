# Workspace Refactor Design Spec

Refactor the armitage single-crate codebase into a Cargo workspace with 7 domain-driven crates, enforcing clean boundaries through the type system and enabling reuse of individual components.

## Goals

1. **Reuse** — individual crates (labels, milestones, sync, triage) can be consumed as libraries independent of the CLI.
2. **Enforced boundaries** — the compiler prevents cross-domain coupling. No more `sync → triage` or `triage → cli` imports.
3. **Domain ownership** — each crate owns its full vertical: types, file I/O, database, config section. No monolithic "schema" or "model" crate that defines everything for everyone.

## Crate Layout

```
armitage/
├── Cargo.toml                    (workspace manifest)
├── crates/
│   ├── armitage-core/            # Layer 0: Org, Domain trait, Node, tree walking
│   ├── armitage-labels/          # Layer 1: label types, schema, rename ledger
│   ├── armitage-milestones/      # Layer 1: milestone types and management
│   ├── armitage-github/          # Layer 1: GitHub API via ionem
│   ├── armitage-sync/            # Layer 2: bidirectional sync engine
│   ├── armitage-triage/          # Layer 2: LLM triage pipeline
│   └── armitage/                 # Layer 3: CLI binary + orchestration
├── tests/                        (workspace-level integration tests)
├── SKILL.md
└── CLAUDE.md
```

### Dependency Graph

```
                  armitage-core
                /    |     \     \
  armitage-labels  armitage-milestones  armitage-github
        |                                  |
        |    ┌─────────────────────────────┤
        |    │                             │
        │  armitage-sync                   │
        │    (core + github)               │
        │                                  │
        └── armitage-triage ───────────────┘
             (core + labels + github)
                       │
                   armitage (CLI)
                (all 6 crates)
```

No cycles. Each arrow points toward core. Sync depends on core + github only. Triage depends on core + labels + github. The CLI is the only crate that depends on everything.

## Key Design Decisions

### Domain-driven splits, not layer-driven

Crates are split by domain (labels, milestones, sync, triage), not by technical layer (model, fs, db). Each domain crate owns its types, file formats, database schemas, and config sections. Two domain crates must not write to the same file.

### Plugin system via `Domain` trait

Each domain crate implements a `Domain` trait defined in `armitage-core`. This trait declares:

- **Config section** — which key in `armitage.toml` this domain owns
- **Node files** — which files this domain creates in node directories (git-tracked)
- **Data directory** — a gitignored subdirectory under `.armitage/` for machine-local state

### Triage provides logic, CLI provides interaction

The triage crate exposes review logic (iterate suggestions, validate decisions, record results) but has no interactive UI. All rustyline, console, and termimad usage lives in the CLI crate. This allows non-CLI consumers of triage.

### CLI orchestrates cross-domain coordination

When an operation spans domains (e.g., pull from GitHub then translate labels), the CLI calls into each domain crate sequentially. Domain crates do not import each other except through declared dependencies.

### Per-crate error types

Each crate defines its own `Error` enum. The CLI crate has a top-level `Error` that wraps all sub-crate errors via `#[from]`.

## Crate Details

### `armitage-core`

The foundation. Defines the `Org` abstraction, the `Domain` trait, and the core node types that every other crate depends on.

#### `Org` struct

The entry point for all operations. Created once at the CLI layer and passed to domain crates.

```rust
pub struct Org {
    root: PathBuf,
    raw: toml::Table,   // full armitage.toml as raw TOML
    info: OrgInfo,      // parsed [org] section
}

impl Org {
    /// Walk up from CWD to find armitage.toml, load config
    pub fn discover() -> Result<Self>;
    /// Open at a known path
    pub fn open(path: impl AsRef<Path>) -> Result<Self>;

    pub fn root(&self) -> &Path;
    pub fn info(&self) -> &OrgInfo;

    /// Extract a domain's config section, or Default if absent
    pub fn domain_config<D: Domain>(&self) -> Result<D::Config>;

    // Node discovery
    pub fn walk_nodes(&self) -> Result<Vec<NodeEntry>>;
    pub fn read_node(&self, rel_path: &str) -> Result<Node>;
    pub fn list_children(&self, rel_path: &str) -> Result<Vec<NodeEntry>>;

    // Secrets
    pub fn read_secret(&self, key: &str) -> Result<Option<String>>;
    pub fn write_secret(&self, key: &str, value: &str) -> Result<()>;
}
```

#### `Domain` trait

The plugin mechanism. Each domain crate implements this to declare its config, files, and data directory.

```rust
pub trait Domain {
    /// Unique domain identifier (e.g., "sync", "triage")
    const NAME: &'static str;

    /// Config section key in armitage.toml
    const CONFIG_KEY: &'static str;

    /// Deserialized config type for this domain
    type Config: DeserializeOwned + Default;

    /// Git-tracked files this domain creates in node directories
    const NODE_FILES: &'static [&'static str] = &[];

    /// Git-tracked files this domain creates at the org root
    const ROOT_FILES: &'static [&'static str] = &[];

    /// Gitignored data directory: .armitage/<NAME>/
    fn data_dir(org: &Org) -> Result<PathBuf> {
        let dir = org.root().join(".armitage").join(Self::NAME);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}
```

#### Types owned

- `Node`, `Timeline`, `NodeStatus` — node.toml shape
- `IssueRef` — `owner/repo#number` parsing
- `OrgInfo` — the `[org]` section of armitage.toml
- `NodeEntry` — a discovered node (relative path + parsed Node)

#### `Domain` implementation

```rust
pub struct CoreDomain;
impl Domain for CoreDomain {
    const NAME: &'static str = "core";
    const CONFIG_KEY: &'static str = "org";
    type Config = OrgInfo;
    const NODE_FILES: &'static [&'static str] = &["node.toml", "issue.md"];
    const ROOT_FILES: &'static [&'static str] = &["armitage.toml"];
}
```

#### Files owned

- `armitage.toml` at org root (reads full file, owns `[org]` section)
- `node.toml` and `issue.md` in node directories

#### Error type

`CoreError` — `Io`, `TomlParse`, `TomlSerialize`, `InvalidIssueRef`, `NotInOrg`, `NodeNotFound`, `ParentNotFound`, `NodeExists`.

#### Dependencies

`serde`, `chrono`, `toml`, `thiserror`

---

### `armitage-labels`

Label types, schema configuration, and the rename ledger. Separated from core because label management is expected to grow into a richer feature set.

#### Types owned

- `LabelDef`, `LabelsFile` — per-node label definitions with read/write/has/names/add/remove/upsert
- `LabelSchema`, `LabelPrefix`, `LabelStyle`, `LabelStyleExample` — org-level label taxonomy (the `[labels]` config section)
- `LabelRename`, `LabelRenameLedger` — rename tracking (old→new mappings with per-repo sync state)
- `translate_labels()` — pure function: given labels and a ledger, returns translated labels

#### `Domain` implementation

```rust
pub struct LabelsDomain;
impl Domain for LabelsDomain {
    const NAME: &'static str = "labels";
    const CONFIG_KEY: &'static str = "labels";
    type Config = LabelSchema;
    const NODE_FILES: &'static [&'static str] = &["labels.toml"];
}
```

#### Files owned

- `labels.toml` in node directories (git-tracked)
- `.armitage/labels/renames.toml` — rename ledger (gitignored)

#### Error type

`LabelsError` — `Io`, `TomlParse`, `TomlSerialize`, plus `#[from] CoreError`.

#### Dependencies

`armitage-core`, `serde`, `chrono`, `toml`, `thiserror`

---

### `armitage-milestones`

Milestone types and management. Separated to support future growth (timeline tracking, progress computation, reporting).

#### Types owned

- `MilestoneFile`, `Milestone`, `MilestoneType`

#### `Domain` implementation

```rust
pub struct MilestonesDomain;
impl Domain for MilestonesDomain {
    const NAME: &'static str = "milestones";
    const CONFIG_KEY: &'static str = "milestones";
    type Config = MilestonesConfig; // currently empty, Default
    const NODE_FILES: &'static [&'static str] = &["milestones.toml"];
}
```

#### Files owned

- `milestones.toml` in node directories (git-tracked)

#### Error type

`MilestonesError` — `Io`, `TomlParse`, `TomlSerialize`, plus `#[from] CoreError`.

#### Dependencies

`armitage-core`, `serde`, `chrono`, `toml`, `thiserror`

---

### `armitage-github`

All GitHub API operations via ionem's `gh` CLI wrapper. The single place where `gh` commands are issued.

#### Types owned

- `GitHubIssue`, `GitHubLabel`, `GitHubRepoLabel`, `CreatedIssue` — GitHub API response types

#### Public API

- Issue operations: `fetch_issue()`, `create_issue()`, `update_issue()`, `set_issue_state()`, `add_comment()`
- Label operations: `fetch_repo_labels()`, `rename_label()`, `create_label()`, `update_label_metadata()`, `delete_label()`, `list_issues_with_label()`
- Repo operations: `list_org_repos()`

All functions take `&ionem::shell::gh::Gh`. The `Gh` instance is created at the CLI layer via `ionem::shell::gh::require()` and passed down.

#### Files owned

None. Pure API layer.

#### Error type

`GithubError` — `Cli(ionem::shell::CliError)`, `Json(serde_json::Error)`, `Io`.

#### Dependencies

`armitage-core`, `ionem` (gh feature), `serde`, `serde_json`, `thiserror`, `tracing`

---

### `armitage-sync`

Bidirectional sync between local node directories and GitHub issues. Hash-based change detection, three-way merge, conflict management.

#### Types owned

- `SyncState`, `NodeSyncEntry` — per-node sync metadata
- `StoredConflict`, `FieldConflict` — conflict types
- `MergeResult`, `BodyMergeResult` — merge outcomes

#### Public API

- `compute_node_hash()` — SHA-256 of node directory contents
- `read_sync_state()`, `write_sync_state()` — sync metadata I/O
- `write_conflict()`, `list_conflicts()`, `remove_conflict()`, `has_conflicts()` — conflict management
- `merge_nodes()`, `merge_issue_body()` — three-way merge logic
- `pull_node()`, `pull_all()` — pull from GitHub (returns raw labels, no translation)
- `push_all()` — push to GitHub

#### `Domain` implementation

```rust
pub struct SyncDomain;
impl Domain for SyncDomain {
    const NAME: &'static str = "sync";
    const CONFIG_KEY: &'static str = "sync";
    type Config = SyncConfig;
}
```

#### Files owned

- `.armitage/sync/state.toml` — per-node sync metadata (gitignored)
- `.armitage/sync/conflicts/` — serialized conflicts (gitignored)

#### Key change from current code

`pull_node()` no longer calls `translate_labels()`. It returns raw labels from GitHub. The CLI orchestration layer calls `armitage_labels::translate_labels()` after pull.

#### Error type

`SyncError` — `Io`, `StalePush`, `UnresolvedConflicts`, plus `#[from]` for `GithubError`, `CoreError`.

#### Dependencies

`armitage-core`, `armitage-github`, `sha2`, `hex`, `chrono`, `toml`, `thiserror`

Note: `armitage-sync` does not depend on `armitage-labels` or `armitage-triage`.

---

### `armitage-triage`

LLM-powered issue classification pipeline. Fetching, classifying, reviewing (logic only), applying, and label import/reconciliation.

#### Types owned

- **DB types:** `StoredIssue`, `TriageSuggestion`, `ReviewDecision`, `PipelineCounts`, `SuggestionStatus`, `SuggestionSort`, `SuggestionFilters`, `DecisionFilters`
- **LLM types:** `LlmBackend`, `LlmConfig`, `PromptCatalog`, `LlmClassification`
- **Label import types:** `LabelSuggestion`, `MergeGroup`, `ReconcileResponse`, `CandidateStatus`, `LabelImportCandidate`, `LabelImportSession`
- **Review types:** `ReviewStats`
- **Apply types:** `ApplyStats`
- **Cache types:** `CachedIssue`, `RepoCache`
- **Example types:** `TriageExample`, `TriageExamplesFile`
- **Category types:** `DismissedCategories`

#### Public API

- **Fetch:** `fetch_repo_issues()`, `fetch_all()`, `collect_repos_from_nodes()`, `strip_repo_qualifier()`
- **LLM:** `triage_issues()`, `reconcile_labels()`, `refine_label_suggestions()`, `refine_categories()`, `generate_question()`, `generate_stale_question()`
- **Review logic:** `review_auto_approve()`, iteration/query functions for pending suggestions, recording decisions — no interactive UI
- **Apply:** `apply_all()`
- **Cache:** `build_repo_cache()`, `refresh_all()`, `write_repo_cache()`, `read_repo_cache()`
- **Examples:** `load_examples()`, `save_examples()`, `append_example()`, `remove_example()`, `build_examples_section()`
- **Categories:** `read_dismissed()`, `write_dismissed()`, `dismiss()`

#### `Domain` implementation

```rust
pub struct TriageDomain;
impl Domain for TriageDomain {
    const NAME: &'static str = "triage";
    const CONFIG_KEY: &'static str = "triage";
    type Config = TriageConfig;
}
```

#### Files owned

- `.armitage/triage/triage.db` — SQLite database (gitignored)
- `.armitage/triage/examples.toml` — triage examples (gitignored)
- `.armitage/triage/dismissed-categories.toml` — dismissed categories (gitignored)
- `.armitage/triage/repo-cache/` — cached issue data (gitignored)

#### Key change from current code

No `rustyline`, `console`, or `termimad`. The interactive review UI moves to the CLI crate. This crate exposes review logic — iterating suggestions, validating decisions, recording results — and the CLI wraps it with prompts and completion helpers.

#### Error type

`TriageError` — `Sqlite(rusqlite::Error)`, `LlmInvocation(String)`, `LlmParse(String)`, `Io`, `Json`, plus `#[from]` for `GithubError`, `LabelsError`, `CoreError`.

#### Dependencies

`armitage-core`, `armitage-labels`, `armitage-github`, `rusqlite` (bundled), `serde`, `serde_json`, `ureq`, `indicatif`, `chrono`, `toml`, `thiserror`

---

### `armitage` (CLI binary)

Command dispatch, interactive UI, and cross-domain orchestration. The only crate that depends on all domain crates.

#### What lives here

- `main.rs` — entry point delegating to `cli::run()`
- `cli/mod.rs` — clap `Commands` enum, `Cli` struct, argument parsing, dispatch, tracing setup, self-update via `ionem::self_update::SelfManager`
- `cli/node.rs` — node create/move/tree/show with interactive prompts
- `cli/triage.rs` — triage subcommands + interactive review UI (rustyline prompts, terminal formatting, tab completion)
- `cli/pull.rs` — sync pull + label translation orchestration
- `cli/push.rs` — sync push
- `cli/init.rs` — org initialization
- `cli/config.rs` — config management with dialoguer prompts
- `cli/status.rs` — sync status display
- `cli/resolve.rs` — conflict resolution
- `cli/milestone.rs` — milestone management
- `cli/complete.rs` — rustyline completion helpers (`NodePathHelper`, `CommaCompleteHelper`)
- `build.rs` — `ionem::build::copy_skill_md()`, `ionem::build::emit_target()`

#### Cross-domain orchestration examples

1. **Pull + label translation:** `cli/pull.rs` calls `armitage_sync::pull_all()`, then calls `armitage_labels::translate_labels()` on the results.
2. **Interactive triage review:** `cli/triage.rs` calls triage logic functions, wraps them with rustyline prompts from `cli/complete.rs`.

#### Error type

Top-level `Error` with `#[from]` for `SyncError`, `TriageError`, `GithubError`, `LabelsError`, `MilestonesError`, `CoreError`, plus `Other(String)`.

#### Dependencies

All 6 crates + `clap`, `ionem` (self-update feature), `rustyline`, `console`, `dialoguer`, `termimad`, `tracing-subscriber`, `chrono`

## File Layout Migration

### Current `.armitage/` (flat)

```
.armitage/
├── sync-state.toml
├── conflicts/
├── triage.db
├── label-renames.toml
├── triage-examples.toml
├── dismissed-categories.toml
├── repo-cache/
└── secrets.toml
```

### New `.armitage/` (namespaced by domain)

```
.armitage/
├── sync/
│   ├── state.toml
│   └── conflicts/
├── triage/
│   ├── triage.db
│   ├── examples.toml
│   ├── dismissed-categories.toml
│   └── repo-cache/
├── labels/
│   └── renames.toml
└── secrets.toml              ← owned by armitage-core (shared)
```

A migration function in the CLI should move existing flat files to the namespaced layout on first run after upgrade.

## Cross-Cutting Concerns Resolved

| Current problem | Resolution |
|---|---|
| `sync::pull` → `triage::labels::translate_labels` | `translate_labels` lives in `armitage-labels`. CLI orchestrates pull + translation. `armitage-sync` does not depend on labels or triage. |
| `triage::review` → `cli::complete` | Interactive UI moves to CLI. Triage exposes logic only. |
| Monolithic `Error` enum with ionem + rusqlite | Per-crate error types. CLI wraps them all. |
| Monolithic `OrgConfig` with all domain configs | Each domain owns its config section via `Domain` trait. `Org` provides generic `domain_config::<D>()` accessor. |
| Flat `.armitage/` with implicit file ownership | Namespaced `.armitage/<domain>/` directories via `Domain::data_dir()`. |

## Migration Strategy

Build bottom-up:

1. `armitage-core` — extract `Org`, `Domain` trait, `Node`, tree walking
2. `armitage-labels` — extract label types + rename ledger
3. `armitage-milestones` — extract milestone types
4. `armitage-github` — extract GitHub API layer
5. `armitage-sync` — extract sync engine, remove `triage::labels` import
6. `armitage-triage` — extract triage pipeline, remove interactive UI
7. `armitage` — CLI binary with orchestration and interactive UI

Each crate is implemented and tested independently before moving to the next. Integration tests remain at the workspace root.
