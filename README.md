# Armitage

CLI for project management across GitHub repositories. Track initiatives, projects, and tasks as a local directory tree with bidirectional GitHub issue sync and LLM-powered triage.

## Installation

Install via [ion](https://github.com/Roger-luo/ion):

```bash
ion add --bin Roger-luo/armitage
```

Then run with:

```bash
ion run armitage <command>
```

Alternatively, install via the standalone install script:

```bash
curl -fsSL https://raw.githubusercontent.com/Roger-luo/armitage/main/install.sh | sh
```

To install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/Roger-luo/armitage/main/install.sh | sh -s -- 0.1.0
```

Or install from source (requires Rust edition 2024):

```bash
cargo install --git https://github.com/Roger-luo/armitage
```

Armitage requires the [GitHub CLI](https://cli.github.com/) (`gh`) installed and authenticated.

## Quick Start

> **Note:** Examples below use `ion run armitage`. If you installed the standalone binary (via curl or cargo), use `armitage` directly.

### Initialize an org

```bash
ion run armitage init my-org --github-org my-github-org --default-repo my-github-org/main-repo
cd my-org
```

This creates a directory with `armitage.toml` and a `.armitage/` folder (gitignored) for local sync state.

### Create nodes

Nodes are the building blocks — initiatives, projects, and tasks arranged in a directory hierarchy:

```bash
ion run armitage node new backend --name "Backend" --description "Backend services"
ion run armitage node new backend/auth --name "Auth" --description "Authentication system"
ion run armitage node new backend/auth/oauth --name "OAuth" --description "OAuth2 provider support"
```

Or use interactive mode:

```bash
ion run armitage node new
```

### Browse the roadmap

```bash
ion run armitage node tree         # full hierarchy
ion run armitage node list         # top-level nodes
ion run armitage node show backend/auth  # details for a node
```

### Sync with GitHub

Each node can be linked to a GitHub issue via the `github_issue` field in its `node.toml` (format: `owner/repo#123`).

```bash
ion run armitage sync pull              # pull changes from GitHub issues
ion run armitage sync push              # push local changes to GitHub
ion run armitage sync push --dry-run    # preview what would change
```

Pull uses three-way merge with conflict detection. If conflicts arise:

```bash
ion run armitage sync resolve --list    # see conflicts
ion run armitage sync resolve           # resolve interactively
```

### Milestones

```bash
ion run armitage milestone add backend/auth \
  --name "Auth MVP" \
  --date 2026-06-01 \
  --description "Core auth flows working"

ion run armitage milestone add backend/auth \
  --name "Q2 Auth Coverage" \
  --date 2026-06-30 \
  --milestone-type okr \
  --expected-progress 0.7

ion run armitage milestone list
ion run armitage milestone list --quarter 2026-Q2
```

### Check status

```bash
ion run armitage status            # org sync overview + triage pipeline
```

## LLM-Powered Issue Triage

Armitage can fetch GitHub issues and use an LLM (Claude or Codex) to classify them into your roadmap. The workflow is a pipeline: **fetch** -> **classify** -> **review** -> **apply**.

### Configure triage

In `armitage.toml`:

```toml
[org]
name = "my-org"
github_orgs = ["my-github-org"]
default_repo = "my-github-org/main-repo"

[[label_schema.prefixes]]
prefix = "priority:"
category = "Priority"
examples = ["priority:high", "priority:medium", "priority:low"]

[[label_schema.prefixes]]
prefix = "area:"
category = "Area"
examples = ["area:backend", "area:frontend", "area:infra"]

[triage]
backend = "claude"    # or "codex"
model = "sonnet"      # optional, e.g. "opus", "o3"
effort = "medium"     # optional
```

Or set values individually:

```bash
ion run armitage config set triage.backend claude
ion run armitage config set triage.model sonnet
ion run armitage config show
```

### 1. Fetch issues

Pull GitHub issues into a local SQLite database (`.armitage/triage.db`):

```bash
ion run armitage triage fetch                        # from default_repo
ion run armitage triage fetch --repo owner/repo      # specific repo
ion run armitage triage fetch --since 2026-03-01     # only recent issues
```

### 2. Classify with LLM

The LLM receives your roadmap tree, label schema, curated labels from `labels.toml`, and each issue. It returns a suggested node placement, labels, confidence score, and reasoning.

```bash
ion run armitage triage classify                     # uses config defaults
ion run armitage triage classify --backend claude --model opus
ion run armitage triage classify --batch-size 10     # batch multiple issues per call
ion run armitage triage classify --repo owner/repo   # classify issues from one repo
```

CLI flags override `armitage.toml` defaults.

### Curated labels

Use `labels.toml` as the curated label catalog shared across repos. Import labels from GitHub into a staged session first, then selectively merge them into the curated file.

```bash
ion run armitage triage labels fetch --repo owner/repo --repo owner/infra
ion run armitage triage labels merge
ion run armitage triage labels merge --all-new --update-drifted --yes
```

`ion run armitage triage labels merge` is interactive by default. The non-interactive flags let you script imports when you already know which categories you want to accept.

`ion run armitage triage classify` uses the curated label catalog from `labels.toml` as part of the LLM prompt, passing only label names and descriptions.

### 3. Review suggestions

```bash
ion run armitage triage review --list                # see all pending suggestions
ion run armitage triage review --interactive         # walk through each (approve/reject/modify)
ion run armitage triage review --auto-approve 0.8    # auto-approve suggestions with >=80% confidence
```

### 4. Apply to GitHub

Push approved label changes back to GitHub:

```bash
ion run armitage triage apply                        # apply approved changes
ion run armitage triage apply --dry-run              # preview first
```

### 5. Check pipeline status

```bash
ion run armitage triage status
```

Shows counts across the pipeline: fetched -> untriaged -> pending review -> approved -> applied.

### Bootstrapping a roadmap

The LLM classifies issues based on your existing node hierarchy. To get started:

1. Create top-level nodes for your main initiatives/projects
2. Define a label schema in `armitage.toml`
3. Curate shared labels in `labels.toml`
4. Fetch and classify — the LLM will slot issues into the right nodes
5. Review, refine, and iterate as the roadmap grows

## Configuration

`armitage.toml` lives at the root of your org directory:

| Section | Key | Description |
|---------|-----|-------------|
| `org.name` | string | Org name |
| `org.github_orgs` | list | GitHub organizations |
| `org.default_repo` | string | Default repo for issues (e.g. `owner/repo`) |
| `labels.toml` | file | Curated label catalog used during label import and triage classification |
| `label_schema.prefixes` | list | Label prefix definitions with category and examples |
| `sync.conflict_strategy` | string | `detect` (default), `github-wins`, or `local-wins` |
| `triage.backend` | string | LLM backend: `claude` or `codex` |
| `triage.model` | string | Model name (e.g. `sonnet`, `opus`, `o3`) |
| `triage.effort` | string | Effort level (e.g. `low`, `medium`, `high`) |

## Data Model

- **Node**: A directory containing `node.toml` (metadata) and optionally `issue.md` (body) and `milestones.toml`. Hierarchy is defined by directory nesting — parent is never stored in the TOML.
- **Milestone**: A dated checkpoint or OKR attached to a node. OKR milestones include an `expected_progress` (0.0-1.0).
- **Sync state**: Stored in `.armitage/sync/state.toml` (gitignored). Tracks per-node hashes and timestamps for change detection.
- **Triage database**: SQLite at `.armitage/triage/triage.db` (gitignored). Stores fetched issues, LLM suggestions, and review decisions.

## Directory Structure

```
my-org/
  armitage.toml              # org config
  .armitage/                 # local state (gitignored)
    sync/
      state.toml
      conflicts/
    triage/
      triage.db
      examples.toml
      repo-cache/
    labels/
      renames.toml
    secrets.toml
  .gitignore
  backend/
    node.toml
    issue.md
    milestones.toml
    auth/
      node.toml
      issue.md
      oauth/
        node.toml
```

## License

MIT
