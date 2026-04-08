# Armitage MVP Design Spec

A CLI tool for project management across GitHub repositories, tracking initiatives, projects, and tasks in a recursive hierarchy backed by a local git repository with bidirectional GitHub sync.

## Scope

### MVP (this spec)

- `armitage init` — scaffold org directory
- `armitage issue create/list/show/edit/move/remove/tree` — recursive node management
- `armitage milestone add/list/remove` — milestones and OKRs (as quarterly milestones)
- `armitage pull/push` — bidirectional GitHub sync with conflict detection
- `armitage resolve` — conflict resolution
- `armitage status` — sync state and issue overview

### Post-MVP (see backlog.md)

- Reports and dashboards
- Multi-repo issue aggregation
- LLM-based auto-triage for incoming issues
- Database-backed comment/discussion tracking
- Jira integration

## Key Decisions

- **GitHub-first**: all issue tracking goes through GitHub. Jira deferred.
- **Auth via `ionem`**: delegates to `gh` CLI auth. No tokens in config.
- **TOML + Markdown**: TOML for structured metadata, Markdown for issue bodies and documentation.
- **Fully recursive hierarchy**: no fixed depth. Any node can have children.
- **Labels are flat**: conventional prefixes (P-, A-, I-) for human readability, not enforced by armitage. Hierarchy lives in directory structure only.
- **OKRs are milestones**: OKRs are a special type of milestone with `type = "okr"` and `expected_progress`.
- **Separate pull/push**: bidirectional sync with explicit direction control, matching git mental model.
- **Conflict detection with auto-merge**: field-level merge for non-conflicting changes, explicit conflict resolution for true conflicts.
- **`.armitage/` is gitignored**: sync state is per-machine. Shared state lives in committed node files.

## Data Model

### Node

The universal recursive unit. Every initiative, project, sub-project, and task is a node. A node is a directory containing a `node.toml`:

```toml
# gemini/node.toml
name = "Gemini"
description = "Next-gen multimodal AI platform"
github_issue = "anthropic/gemini#1"        # optional, links to GitHub issue
labels = ["I-gemini", "P-high"]
repos = ["anthropic/gemini", "anthropic/gemini-infra"]
timeline = { start = "2026-01-01", end = "2026-12-31" }
status = "active"                           # active | completed | paused | cancelled
```

**Rules:**
- Parent is derived from directory structure, never stored in TOML.
- A child node's timeline must be a subset of its parent's timeline (warning, not hard error).
- `github_issue` is the bidirectional sync anchor. Format: `owner/repo#number`.
- `labels` stores conventional prefixed labels for reference, not enforced.
- A node may have additional `.md` files for local-only documentation.

### Issue body

Each node that links to a GitHub issue has `issue.md` — the file that syncs bidirectionally with the GitHub issue's main post body.

- Only exists if `github_issue` is set in `node.toml`.
- All other `.md` files in the node directory are local-only.
- Discussion/comments on the GitHub issue are not tracked locally.

### Assets

An optional `assets/` directory per node for images and diagrams referenced in `issue.md`.

- `issue.md` references assets via relative paths: `![diagram](assets/architecture.png)`
- During push: armitage uploads assets to GitHub and rewrites image links in the issue body.
- During pull: armitage downloads referenced images to `assets/` and rewrites links back to relative paths.

### Milestones

```toml
# gemini/milestones.toml
[[milestone]]
name = "Alpha ready"
date = "2026-03-15"
description = "Core inference pipeline working end-to-end"
github_issue = "anthropic/gemini#45"       # optional
type = "checkpoint"                         # checkpoint | okr

[[milestone]]
name = "Q1 OKR: 50% training coverage"
date = "2026-03-31"
description = "Objective: Reach training milestone..."
github_issue = "anthropic/gemini#80"
type = "okr"
expected_progress = 0.5                     # fractional progress, OKR-type only
```

**Rules:**
- Milestones belong to the node whose directory they're in.
- A milestone's date must fall within its node's timeline.
- OKR-type milestones have `expected_progress` for fractional completion tracking.
- OKRs on GitHub follow this issue body format, which armitage parses and generates during sync:

```markdown
## Objective
<objective description>

## Key Results
- [ ] KR1: <description> (target: <metric>)
- [ ] KR2: <description> (target: <metric>)

## Status
Progress: <percentage>%
Last updated: <date>
```

### Org config

```toml
# armitage.toml
[org]
name = "anthropic"
github_org = "anthropic"

[label_schema]
prefixes = [
    { prefix = "P-", category = "priority", examples = ["P-high", "P-medium", "P-low"] },
    { prefix = "A-", category = "area", examples = ["A-compiler", "A-infra"] },
    { prefix = "I-", category = "initiative", examples = ["I-gemini", "I-M4"] },
]

[sync]
conflict_strategy = "detect"    # detect | github-wins | local-wins
```

## Directory Structure

```
anthropic/                          # org root
├── armitage.toml                   # org config
├── gemini/                         # top-level node (initiative)
│   ├── node.toml
│   ├── issue.md                   # ↔ GitHub issue body
│   ├── assets/                    # images/diagrams for issue.md
│   │   └── architecture.png
│   ├── milestones.toml
│   ├── overview.md                # local-only documentation
│   ├── vision.md                  # local-only documentation
│   ├── auth-service/              # child node (project)
│   │   ├── node.toml
│   │   ├── issue.md
│   │   └── oauth-flow/            # grandchild node (sub-project)
│   │       └── node.toml
│   └── training-pipeline/
│       ├── node.toml
│       └── data-preprocessing/
│           └── node.toml
├── m4/                             # another top-level node
│   ├── node.toml
│   └── milestones.toml
└── .armitage/                      # gitignored, per-machine sync state
    ├── sync.toml
    └── conflicts/
```

**Conventions:**
- Any directory containing `node.toml` is a node.
- Directories without `node.toml` are ignored by armitage.
- `milestones.toml` is optional per node.
- `.armitage/` is gitignored entirely.

## CLI Commands

### `armitage init <org-name>`

Scaffolds a new org directory with `armitage.toml` and `.armitage/`. Prompts for GitHub org name.

### `armitage issue`

```
armitage issue create <path>          # create node at path
armitage issue list                   # list top-level nodes
armitage issue list <path>            # list children of a node
armitage issue list --recursive       # full tree view
armitage issue show <path>            # show node details, milestones, children
armitage issue edit <path>            # open node.toml in $EDITOR
armitage issue move <from> <to>       # reparent a node
armitage issue remove <path>          # remove a node (with confirmation)
armitage issue tree                   # display full hierarchy as tree
```

`<path>` is relative to org root: `gemini`, `gemini/auth-service`, `gemini/auth-service/oauth-flow`.

### `armitage milestone`

```
armitage milestone add <node-path>            # add milestone to a node
armitage milestone list <node-path>           # list milestones for a node
armitage milestone list --type okr            # filter OKR-type milestones
armitage milestone list --quarter 2026-Q1     # filter by quarter
armitage milestone remove <node-path> <name>  # remove a milestone
```

### `armitage pull`

```
armitage pull                    # pull all nodes from GitHub
armitage pull <path>             # pull specific node and children
armitage pull --dry-run          # show what would change
```

### `armitage push`

```
armitage push                    # push all local changes to GitHub
armitage push <path>             # push specific node and children
armitage push --dry-run          # show what would be pushed
```

### `armitage push` (new nodes)

For nodes with no `github_issue` set, push creates a new GitHub issue and stores the reference back in `node.toml`.

### `armitage resolve`

```
armitage resolve <path>          # interactively resolve conflicts
armitage resolve --list          # list conflicted nodes
```

### `armitage status`

Shows overview: nodes with pending local changes, unresolved conflicts, sync state, timeline violations.

## Sync Engine

### Sync state

`.armitage/sync.toml` tracks per-node metadata:

```toml
[nodes."gemini"]
github_issue = "anthropic/gemini#1"
last_pulled_at = "2026-03-30T10:00:00Z"
last_pushed_at = "2026-03-30T10:05:00Z"
remote_updated_at = "2026-03-30T10:00:00Z"
local_hash = "a3f2b1..."
```

### Pull flow

1. Walk the node tree (or scoped subtree).
2. For each node with `github_issue`:
   - Fetch issue via `ionem` gh API.
   - Compare `remote_updated_at` with GitHub's current `updated_at`.
   - If unchanged on remote: skip.
   - If unchanged locally (hash matches): fast-forward, overwrite local.
   - If both changed: field-level merge.
3. Field-level merge:
   - **`node.toml` fields**: compare each field independently. One side changed → take it. Both changed same field → conflict.
   - **`issue.md`**: both sides changed → write conflict file to `.armitage/conflicts/`.
   - **Labels**: union by default. Removal on one side (other untouched) → take removal. Added on one side, removed on other → conflict.
4. Update `sync.toml`.

### Push flow

1. Abort if unresolved conflicts exist.
2. Walk the node tree.
3. For each node with local changes:
   - Check remote hasn't changed since last pull (stale push protection). If changed → abort, tell user to pull first.
   - Update GitHub issue: title, body, labels, status.
   - Upload new/changed assets, rewrite image links.
   - Update `sync.toml`.
4. For new nodes without `github_issue`:
   - Create GitHub issue, store reference in `node.toml`.

### Conflict resolution

Conflicts stored in `.armitage/conflicts/` as files showing both versions. `armitage resolve <path>` provides interactive prompts for each conflicted field.

## Validation

- **Timeline**: child timeline must be subset of parent. Warning, not error.
- **Milestone dates**: must fall within node's timeline.
- **Push safety**: aborts on unresolved conflicts or stale local state.
- **Pull/push `--dry-run`**: always available to preview.
- **Sync atomicity**: per-node. Failed sync doesn't update that node's sync state, so next sync retries.
- **TOML parsing**: fail with line-level error messages on invalid `node.toml`.
- **Parent existence**: `armitage issue create` refuses to create child if parent node doesn't exist.

## Rust Architecture

```
src/
├── main.rs                 # CLI entry point, clap setup
├── cli/
│   ├── mod.rs
│   ├── init.rs
│   ├── issue.rs
│   ├── milestone.rs
│   ├── pull.rs
│   ├── push.rs
│   ├── resolve.rs
│   └── status.rs
├── model/
│   ├── mod.rs
│   ├── node.rs             # Node struct, TOML serde, validation
│   ├── milestone.rs        # Milestone struct, OKR variant
│   ├── org.rs              # Org config (armitage.toml)
│   └── tree.rs             # Recursive tree operations
├── sync/
│   ├── mod.rs
│   ├── state.rs            # SyncState (.armitage/sync.toml)
│   ├── pull.rs
│   ├── push.rs
│   ├── merge.rs            # Field-level merge, conflict detection
│   ├── conflict.rs         # Conflict storage and resolution
│   └── assets.rs           # Asset upload/download, link rewriting
├── github/
│   ├── mod.rs
│   ├── issue.rs            # Issue operations via ionem
│   └── labels.rs           # Label operations via ionem
└── fs/
    ├── mod.rs
    └── tree.rs             # Filesystem: scan for node.toml, walk dirs
```

**Dependencies:**
- `ionem` (gh + git features) — GitHub API via `gh` CLI, self-management via `self` subcommand
- `clap` — CLI argument parsing with derive
- `serde` + `toml` — TOML serialization
- `chrono` — timeline/date handling
- `sha2` — hashing for change detection

**Principles:**
- `model/` is pure data — no IO, fully testable.
- `github/` uses `ionem` for all GitHub operations.
- `sync/` orchestrates between model, github, and fs.
- `cli/` is thin — parses args, delegates, formats output.
