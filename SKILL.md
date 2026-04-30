---
name: armitage
description: >
  Project management CLI for tracking initiatives, projects, and tasks across GitHub repositories
  as a recursive directory hierarchy backed by a local git repo, with bidirectional GitHub issue sync
  and LLM-powered triage. Use this skill whenever working inside an armitage org directory
  (has armitage.toml at root), managing roadmap nodes, triaging GitHub issues, syncing with GitHub,
  or planning project structure. Also use when the user mentions armitage commands, node.toml files,
  triage workflows, or issue classification.
---

# Armitage

Project management CLI that tracks initiatives, projects, and tasks as a recursive directory
hierarchy backed by a local git repo, with bidirectional GitHub issue sync and LLM-powered triage.

## Invocation

How to invoke `armitage` depends on how the user installed it. Check the user's CLAUDE.md or
AGENTS.md for guidance — they should specify one of these patterns:

| Installation method | Command pattern |
|---|---|
| `ion add --bin Roger-luo/armitage` | `ion run armitage <subcommand>` |
| Standalone binary (curl install or cargo install) | `armitage <subcommand>` |

All command examples below use bare `armitage` for brevity. Prefix with `ion run` when the user's
setup requires it.

## Core Concepts

**Org** — a git repository containing `armitage.toml` at its root. This config file defines the
org name, associated GitHub orgs, default repo, label schema, sync settings, and triage LLM config.

**Node** — a directory containing `node.toml`. Nodes form a tree (parent-child via filesystem
nesting). Each node represents an initiative, project, or task.

**node.toml fields:**
- `name` — display name
- `description` — what this node covers (used by the triage LLM to classify issues)
- `status` — `active`, `completed`, `paused`, `cancelled`
- `repos` — associated GitHub repos, with optional `@branch` qualifier (see below)
- `labels` — labels to apply to issues classified under this node
- `owners` — GitHub usernames responsible for this node (references `team.toml`)
- `team` — functional team that owns this node (e.g. `circuit`, `flair`, `shuttle`, `kirin`)
- `track` — tracking issue in `owner/repo#number` format; also forces that issue to appear under this node in `okr show` without going through the full triage pipeline
- `timeline` — `start` and `end` dates

**Repo `@branch` convention** — nodes can declare which branch of a repo they cover:
- `repos = ["owner/repo"]` — covers the repo's default branch
- `repos = ["owner/repo@feature"]` — covers only code on the `feature` branch

This is critical when multiple nodes cover the same repo but different codebases (e.g. a legacy
Python implementation on `main` vs a Rust rewrite on a `rust` branch). The triage LLM uses this
to avoid misclassifying issues about existing behavior into nodes for planned/new work.

## Issue Cache

After `triage fetch`, `triage classify`, or `triage reset`, armitage writes lightweight per-repo
cache files to `.armitage/issue-cache/{owner}--{repo}.toml`. These contain every open issue's
number, title, state, labels, and (if triaged) suggested node and confidence — but no body text.

Read these files to quickly understand a repo's issue landscape without querying GitHub or SQLite.
Example:

```toml
repo = "acme/widget"
cached_at = "2026-04-06T12:00:00Z"
open_count = 42
triaged_count = 18

[[issues]]
number = 204
title = "login fails on Safari"
state = "OPEN"
labels = ["bug", "auth"]
node = "widget/backend/auth"
confidence = 0.92
```

## Commands

### Organization & Nodes

```
armitage init <name> [--github-org <org>...] [--default-repo <owner/repo>]
# after init, set up AGENTS.md for AI workflows:
# ion agents init Roger-luo/armitage/templates/org-agents.md
armitage node new [<path>] [--name ...] [--description ...] [--repos ...] [--owners ...] [--team ...] [--timeline "START END"]
armitage node list [<path>] [-r]
armitage node tree [--depth N]         # -d N for short; omit for full tree
armitage node show <path>
armitage node edit <path>
armitage node set <path> [--name ...] [--description ...] [--triage-hint ...] [--owners ...] [--team ...] [--repos ...] [--labels ...] [--status ...] [--timeline-start YYYY-MM-DD] [--timeline-end YYYY-MM-DD]
armitage node move <from> <to>
armitage node merge <from> <to> [-y]   # merge source into target
armitage node remove <path> [-y]
armitage node check [--check-repos] [--check-dates]  # timeline violations, issue date validation, owner/repo warnings
armitage node fmt [<path>...]          # re-serialize node.toml files
```

Use `--timeline-start` and `--timeline-end` independently — each updates only its own bound.
If only one bound is provided and the node has no existing timeline, both flags are required.
To clear a timeline, edit `node.toml` directly.

`node check` validates: (1) parent-child timeline containment, (2) issue start/target dates
from the GitHub Project board against node timelines (requires `triage fetch` with `[triage.project]`
configured), (3) node owners exist in `team.toml` (warnings, not errors — external collaborators
may legitimately be owners without a team entry). `node set --owners` also warns interactively
when an unrecognized username is entered.

`--check-repos` queries GitHub for each unique repo in node.toml files (deduplicated) and warns
when a repo is archived or has been renamed (canonical name differs from what's stored). Gated
behind a flag because it makes one `gh repo view` call per unique repo.

`--check-dates` queries the GitHub project board (requires `[github_project]` in `armitage.toml`
and a populated field cache) and warns for each node that has both `track` and `[timeline]` but
shows empty start or target date fields on the board. Use this after creating new nodes to confirm
the local timeline has been pushed to the project board.

### GitHub Sync

```
armitage sync pull [<path>] [--dry-run]   # pull changes from GitHub into local nodes
armitage sync push [<path>] [--dry-run]   # push local changes to GitHub issues
armitage sync resolve [<path>] [--list]   # resolve sync conflicts
armitage status                           # show org overview
```

The `sync` namespace will grow to host other GitHub-sync verbs (e.g. `sync issues`,
`sync project`, `sync labels`) — keep new sync-style commands under this namespace.

### GitHub Project Board Sync

```
armitage project sync [<node_path>] [--dry-run]                         # add nodes to board and set date/status fields
armitage project set <owner/repo#N> [--start-date YYYY-MM-DD] [--target-date YYYY-MM-DD] [--dry-run]  # set dates for a single issue
armitage project clear-cache                                            # force re-fetch of project field metadata
```

Syncs every node that has both `track` and `[timeline]` to a configured GitHub Projects v2
board: adds the issue if not already present, then sets the start date, target date, and status
fields based on the node's timeline. Pass an optional `<node_path>` to sync only that node and
its descendants rather than the entire org.

### Repo Visibility

```
armitage repo list [--format table|json]   # list all repos in the org with public/private visibility
```

Lists every unique repo referenced by `node.toml` files, queries GitHub once per repo, and outputs
visibility (`public` / `private` / `unknown`) plus which nodes reference each repo. Sorted
private-first. Use `--format json` for agent-readable output. Run this before creating issues or
comments to confirm which repos are safe for internal details.

**Setup:** Add a `[github_project]` section to `armitage.toml`:

```toml
[github_project]
org = "MyOrg"
number = 42                       # project number from the URL
start_date_field = "Start date"   # display name of the date field
target_date_field = "Target date" # display name of the target date field
status_field = "Status"           # optional; skip status sync if omitted

[github_project.status_values]
backlog     = "Backlog"           # maps rule → project option name
todo        = "Todo"
sprint_todo = "Sprint Todo"
in_progress = "In Progress"       # never auto-set; requires explicit user action
```

**Status auto-assignment** (based on `target_date` relative to today):
- Target date > 1 quarter away → `Backlog`
- Target date within this quarter → `Todo`
- Target date within next 2 weeks → `Sprint Todo`
- `In Progress` is never set automatically

**No-op detection:** compares current field values on the board before mutating; skips items
that are already up to date.

**Cache:** field IDs are cached in `.armitage/project/field-cache.toml`. Run
`project clear-cache` to force a re-fetch (e.g. after renaming fields on the board).

### Triage Pipeline

The triage pipeline: **fetch** → **classify** → **review** → **apply**.

```
armitage triage fetch [--repo <r>...] [--since <date>]
armitage triage classify [--backend claude|codex|gemini|gemini-api] [--model <m>] [--effort <e>] [--batch-size N] [--parallel N] [--limit N] [--repo <r>] [--format table|json]
armitage triage review -i [--min-confidence N] [--max-confidence N]
armitage triage review --list [--min-confidence N] [--max-confidence N] [--format table|json]
armitage triage review --auto-approve <threshold>
armitage triage decide <issue-ref>... --decision <approve|reject|modify|stale|inquire> [--node <path>] [--labels <l,...>] [--note <text>] [--question <text>]
armitage triage decide --all-pending --decision <approve|reject|stale> [--min-confidence N] [--max-confidence N] [--note <text>]
armitage triage apply [--dry-run]
armitage triage label <issue-ref>... --add <labels> [--remove <labels>] [--dry-run]
armitage triage reset [--below <threshold> | --node <path> | --issue <owner/repo#N> | --all | --unreviewed]
armitage triage status [--format table|json]
armitage triage summary [--repo <r>] [--format table|json]
armitage triage suggestions [--issues 247,276,32] [--node <prefix>] [--repo <r>] [--min-confidence N] [--max-confidence N] [--status pending|approved|rejected|applied] [--tracking-only] [--unclassified] [--stale-only] [--sort confidence|node|repo] [--limit N] [--format table|json|summary|refs] [--body-max N]
armitage triage inactive [--days N] [--since <date>] [--repo <r>] [--format table|json] [--inquire "message"]
armitage triage overdue [--days N] [--repo <r>] [--format table|json] [--comment "message"]
armitage triage decisions [--status <s>] [--unapplied] [--node <prefix>] [--repo <r>] [--limit N] [--format table|json]
armitage triage watch add <issue-ref>...
armitage triage watch list [--status active|watching|replied|dismissed|all] [--format table|json]
armitage triage watch dismiss <issue-ref>...
```

- **fetch** — pulls issues from GitHub repos into a local SQLite DB (`.armitage/triage.db`), then refreshes the issue cache. For any issue with sub-issues, also fetches the full sub-issue list and stores the parent→child relationships in the DB — no raw GraphQL query needed to discover sub-issues after a fetch
- **classify** — sends untriaged issues to an LLM with the roadmap tree, label schema, curated labels, and any classification examples as context; stores suggestions in the DB (including `is_stale` for issues referencing removed/deprecated features, `is_inactive` for issues with 180+ days of no GitHub activity, and `needs_followup`/`followup_reason` when the discussion lacks concrete next steps), then refreshes the issue cache. **Labels are additive only:** the LLM is prompted to suggest only labels the issue does not already have, and any existing labels that slip through are filtered out before storage. `--limit N` classifies at most N issues per run (default: all) — useful for iterative batch workflows. `--batch-size` controls how many issues are sent per LLM call (prompt granularity), while `--limit` controls the total number of issues processed in the run
- **inactive** — queries the local DB for open issues with no GitHub activity for at least N days (default 180). Scoped to repos in `node.toml` files only. Outputs a table with the issue ref, title, time since last update, and suggested node (if classified). `--days N` sets the inactivity threshold. `--since <date>` sets an absolute cutoff date (ISO 8601). `--repo <r>` scopes to one repo. `--inquire "message"` stages all matching unreviewed issues as `inquire` decisions with the given comment text (applied by `triage apply`)
- **overdue** — queries the local DB for open issues whose project-board target date is more than N days in the past (default: 0, meaning any overdue issue). Orthogonal to `inactive`: an issue can be actively discussed but still past its deadline. `--comment "message"` stages a follow-up comment on each matching issue (posted via `triage apply`), analogous to `inactive --inquire`. Useful as a regular OKR review step to surface deadline drift before it compounds.
- **review** — three modes:
  - `-i` / `--interactive` — step through each pending suggestion one at a time. For each:
    **[a]pprove** accepts as-is, **[r]eject** marks as wrong, **[m]odify** lets you correct the
    node and labels (with tab completion), s**[t]**ale marks as stale (references removed/deprecated
    features — saved as an example so the LLM learns; after the note prompt, optionally generates
    a staleness inquiry via LLM asking the author if the issue is still relevant or can be closed
    — posted as a comment on `triage apply`), **[i]nquire** generates a clarification question
    via LLM and lets you edit it before storing (the question is posted as a comment on
    `triage apply`), **[s]kip** moves on, **[b]ack** undoes the previous decision and returns to
    that item, **[q]uit** exits. On reject or modify, you are prompted for an optional note
    explaining *why* the LLM was wrong — this note is saved as a classification example for future
    runs (see Examples below).
  - `--list` — show all pending suggestions as a table (default when neither `-i` nor `--auto-approve` is given)
  - `--auto-approve <threshold>` — auto-approve all suggestions with confidence >= threshold

  **Label handling in review:** Existing issue labels are human-applied and authoritative. Approving
  a suggestion merges existing labels with suggested additions (never removes). The modify prompt
  shows the full merged label set so the reviewer can edit freely — including removing existing
  labels when appropriate. This is the only way to remove labels through the triage pipeline.
- **apply** — pushes approved label changes to GitHub. Computes a diff between the decision's final labels and the issue's current labels, adding new ones and removing only those explicitly removed by a modify decision. For **inquired** and **stale-with-question** decisions, posts the stored question as a comment on the GitHub issue instead of changing labels. Stale decisions without a question are marked as applied with no GitHub action
- **label** — queue label additions/removals for one or more issues without going through LLM classification. Use `--add "label1,label2"` and/or `--remove "label3"` (comma-separated). Creates a synthetic suggestion record if the issue has never been triaged, so the changes slot into the existing pipeline and are pushed via `triage apply`. Unlike `triage decide`, re-labels issues that already have an applied decision (useful for bulk backfills like adding `priority:` labels). Use `--dry-run` to preview without writing to the DB.
- **decide** — submit review decisions non-interactively for one or more issues. Accepts multiple issue refs in a single command (e.g., `triage decide ref1 ref2 ref3 --decision stale`). Use `--all-pending` to decide on all pending suggestions at once (e.g., `triage decide --all-pending --decision approve`), optionally filtered with `--min-confidence`/`--max-confidence`. Used by agents and scripts. Supports `approve`, `reject`, `modify` (with optional `--node`/`--labels` overrides), `stale` (with optional `--question` for staleness inquiry), and `inquire` (with required `--question`). Auto-saves examples on reject/modify/stale (same as interactive mode). Errors on individual issues are reported but don't stop the batch; a summary error is returned at the end if any failed. Errors if a decision has already been applied to GitHub
- **reset** — clears suggestions so issues can be re-classified, then refreshes the issue cache. Modes: `--below <threshold>` (confidence), `--node <path>` (subtree), `--issue <owner/repo#N>` (single issue), `--all`, or `--unreviewed` (deletes unreviewed and rejected suggestions while preserving approved/modified ones — useful for reclassifying with improved examples after a partial review)
- **summary** — confidence distribution, per-node breakdown, and suggested new categories
- **suggestions** — query and filter individual suggestions with flexible criteria. Use `--issues 247,276,32` to select specific issue numbers. The `--format summary` option groups results into **AUTO-APPROVE** (confidence >= 0.80 with a suggested node) and **NEEDS REVIEW** (low confidence or no node), with extra detail (stale flags, new categories, reasoning) for uncertain ones — ideal for agent-driven workflows that need to partition suggestions without post-processing. Use `--format refs` to output just issue refs one per line (e.g., for piping to `triage decide`)
- **decisions** — query and filter review decisions
- **watch add** — start watching one or more issues for activity. Fetches the current comment count and project board state (target date, status) from GitHub as the baseline, so any subsequent change triggers detection on the next `triage fetch`. Use this after posting follow-up comments on overdue or inquired issues: `triage watch add owner/repo#N ...`
- **watch list** — show watched issues and their status (`watching` / `replied` / `closed` / `project_updated` / `dismissed`). Use `--status all` to include dismissed items
- **watch dismiss** — stop watching one or more issues: `triage watch dismiss owner/repo#N ...`

Activity is detected automatically during `triage fetch` — which prints `[watch]` lines for:
- `replied`: comment count increased since the watch was set
- `closed`: issue state changed to closed
- `project_updated`: project board target date or status changed since the watch was set

Note: project board changes are only detected if the org has `[github_project]` configured in `armitage.toml`, since that data is fetched during `triage fetch`. Run `triage watch list` after a fetch to see the full status.

### Category Management

When the LLM classifies issues that don't fit existing nodes, it suggests new categories. These
commands help consolidate and act on those suggestions.

```
armitage triage categories list [--min-votes N] [--format table|json]
armitage triage categories apply <path> --name "..." --description "..." [--reclassify] [--reclassify-backend <b>] [--reclassify-model <m>]
armitage triage categories dismiss <path>
armitage triage categories refine [--backend <b>] [--model <m>] [--auto-accept] [--min-votes N]
```

- **list** — show suggested categories and vote counts
- **apply** — create a node from a suggested category, then reset affected suggestions for reclassification
- **dismiss** — hide a suggested category from listings
- **refine** — LLM-driven consolidation of raw suggestions (merges duplicates like `circuit/emulator`
  and `circuit/emulation`, decides whether to create new nodes or dismiss as covered by existing ones)

### Classification Examples

Past review decisions (especially rejections and modifications) are saved as few-shot examples in
`triage-examples.toml` at the org root. These are included in the LLM classification prompt to
teach it from past corrections, improving accuracy over time. The file is git-committable.

```
armitage triage examples list
armitage triage examples export [--status <s>] [--limit N]
armitage triage examples remove <issue-ref>
```

- **list** — show current examples
- **export** — bulk-export reviewed decisions (default: rejected and modified) from the DB into
  the examples file, deduplicating against existing entries
- **remove** — remove an example by issue reference (e.g. `owner/repo#123`)

Examples are also auto-saved during `triage review -i` whenever you reject or modify a suggestion.
The optional note you enter at the prompt is stored in the `note` field and included in the LLM
prompt as reasoning guidance.

### Label Management

```
armitage triage labels fetch [--repo <r>...] [--org]
armitage triage labels merge [--session <id>] [--all-new] [--update-drifted] [--no-llm] [--auto-accept] [--backend <b>] [--model <m>] [--effort <e>]
armitage triage labels sync [--repo <r>...] [--org] [--dry-run] [--prune]
armitage triage labels push [--repo <r>...] [--org] [--dry-run] [--delete-extra]
```

Labels are curated in `labels.toml` at the org root. The label pipeline: fetch remote labels →
stage as import session → merge into `labels.toml` (with optional LLM-driven dedup/rename) →
sync renames and push to GitHub.

**Repo scoping:** `sync` and `push` default to repos referenced by node.toml files. Use `--org`
to target all non-archived repos in configured github_orgs. `fetch` always requires `--repo` or
`--org` to be explicit about discovery scope.

**Reconciliation:** `merge` runs LLM-based reconciliation by default (disable with `--no-llm`).
After the LLM pass, a deterministic sweep catches remote labels whose bare name matches a local
prefixed label (e.g. remote `stim` → local `area: STIM`). This ensures the LLM's blind spots
are covered for obvious prefix-match duplicates.

### Milestones

Milestones are modeled as **child nodes** with their own timeline and issues. For example,
`gemini/logical/mvp` is a milestone node under `gemini/logical` with a tight deadline and
MVP-critical issues. This is the recommended approach for bounded deliverables.

For lightweight date markers on the chart (OKR targets, checkpoints), use `milestones.toml`:

```
armitage milestone add <node_path> --name <name> --date <YYYY-MM-DD> [--description ...] [--milestone-type checkpoint|okr]
armitage milestone list [<node_path>] [--milestone-type ...] [--quarter ...]
armitage milestone remove <node_path> <name>
```

### OKR View

```
armitage okr show [--period <YYYY-Qn|YYYY|current>] [--goal <slug>] [--team <t>] [--person <u>] [--depth N] [--format table|json|markdown]
armitage okr check [--period <YYYY-Qn|YYYY|current>] [--goal <slug>] [--team <t>] [--depth N] [--require-label-prefix <prefix>]...
```

- **show** — list nodes whose timelines overlap the period as OKR objectives, with their open issues as key results. Open issues always appear regardless of their project-board target date (a project can span multiple OKR periods); closed issues appear only if their target date falls within the period. Issues that have sub-issues (tracked in the DB after `triage fetch`) show them as nested `↳` rows in the markdown output and as `sub_issues` arrays in JSON output.
- **check** — flag nodes with no key results, unowned nodes, and issues whose target dates fall outside the node's timeline. `--require-label-prefix <prefix>` (repeatable) additionally flags open OKR issues that have no label matching that prefix — e.g. `--require-label-prefix "priority:"` surfaces every issue missing a priority label. Reports as kind `missing-label` in JSON and with a 🏷 icon in table output.
- `--goal <slug>` — filter to nodes that belong to a named cross-cutting goal from `goals.toml`.
- `--person <github-username>` — filter to nodes where that person is listed as an owner **or** has at least one assigned issue. A node with no `owners` set is still included when the person has assigned issues under it.

**Node timeline is required.** A node must have a `[timeline]` section with a `start` and `end` that overlaps the OKR period to appear in `okr show`. Nodes without a timeline are silently excluded. If a node is missing from the OKR output, add a timeline: `armitage node set <path> --timeline-start YYYY-MM-DD --timeline-end YYYY-MM-DD`.

**`track` field as OKR shortcut.** Setting `track = "owner/repo#N"` in a node's `node.toml` forces that issue to appear under the node in `okr show` immediately — without running `triage fetch`/`classify`/`decide`/`apply`. Useful for tracking issues that represent the node itself on a GitHub project board.

**How issues appear in OKR:** The OKR reads exclusively from the triage DB — `issues.toml` files are NOT used. To make a newly-created or re-classified issue appear:
1. `triage fetch --repo <owner/repo>` — pull the issue into the DB
2. `triage classify --repo <owner/repo> --limit N` — get an LLM node assignment
3. `triage decide <ref> --decision approve` (or `modify --node <path>`) — confirm the assignment
4. `triage apply` — required when modifying an issue that already had a prior applied decision; updates the effective node_path in the DB

**Diagnosing why an issue doesn't appear:**
- `triage suggestions --issues N --format json` — check the effective node, confidence, and project-board target date
- `triage decisions --node <path>` — see pending vs applied decisions for a node
- `triage status` — overall triage state

**Never query the SQLite DB directly** with `sqlite3`. All relevant state is accessible through the commands above.

### Goals

Cross-cutting external commitments that span multiple roadmap initiatives (e.g. a hardware milestone that requires work across circuit, FLAIR, and shuttle).

```
armitage goal list [--format table|json]
armitage goal show <slug> [--format table|json]
armitage goal add <slug> --name "..." [--description ...] [--deadline YYYY-MM-DD] [--owners u1,u2] [--track owner/repo#N] [--nodes path1,path2]
armitage goal set <slug> [--name ...] [--description ...] [--deadline ...] [--owners ...] [--track ...] [--nodes ...] [--add-nodes ...] [--remove-nodes ...]
armitage goal remove <slug> [-y]
```

Goals are stored in `goals.toml` at the org root. Each goal has a `nodes` list of roadmap paths (exact match or subtree prefix). Use `okr show --goal <slug>` to see only the nodes and issues that belong to that goal.

### Roadmap Chart

```
armitage chart [--output PATH] [--no-open] [--offline] [--watch|-w]
```

- Default: generates `.armitage/chart.html` and opens it
- `--watch` / `-w`: starts a live-reload dev server on `http://127.0.0.1:<port>`, watches for
  changes to node.toml, issues.toml, milestones.toml, armitage.toml, labels.toml, team.toml,
  and triage.db, and auto-rebuilds with browser refresh
- `--offline`: embeds ECharts JS inline for offline/GitHub Pages deployment
- Light/dark/auto theme toggle in the nav bar (persisted in localStorage)
- Fitted/global range toggle for the x-axis time range

**Chart visualization:**
- Nodes render as horizontal bars with nested sub-bars for children and issues
- Child node sub-bars: solid colored by status (blue=active, gray=completed, amber=paused)
- Issue sub-bars (from issues.toml + project board dates):
  - **Green pills**: on-track (target date within node timeline)
  - **Green→purple split pills**: overflowing (transitions at the violated deadline)
  - **Gray dashed pills**: no project board dates assigned (spans full width)
  - Closed issues are excluded
- Sub-issue bars: issues that have sub-issues (from `triage fetch`) render their children as indented `↳` rows with thinner bars directly below the parent — blue if open, red if overdue, green if closed
- Red overflow on outer bars: only shown when overflow exceeds the node's own timeline.
  If a child milestone overflows but the product line accommodates it, only the child
  sub-bar shows red — the outer bar stays clean
- Double-click a bar to drill in; click to show details in the side panel
- The panel shows: description, timeline, people, milestones, children, and all descendant
  issues with clickable GitHub links and target dates

### Configuration

```
armitage config show
armitage config set <key> <value>          # e.g. triage.backend, triage.model
armitage config set-secret <name>          # store in .armitage/secrets.toml
```

### GitHub Project Board Integration

Armitage can fetch timeline metadata (start date, target date, status) from a GitHub Projects v2
board. This data is used for timeline validation and chart visualization.

**Setup:** Add a `[triage.project]` section to `armitage.toml`. The field names must match the
exact display names of the date fields on the project board.

To discover the field names, run:
```
gh api graphql -f query='query { organization(login: "YOUR_ORG") { projectV2(number: N) {
  title fields(first: 30) { nodes { ... on ProjectV2FieldCommon { name dataType } } } } } }'
```

Look for fields with `dataType: DATE` — those are the ones to map.

```toml
[triage.project]
url = "https://github.com/orgs/<org>/projects/<number>"

[triage.project.fields]
start_date = "Start date"     # maps to the project's date field for start
target_date = "Target date"   # maps to the project's date field for deadline
```

Once configured, `triage fetch` automatically pulls project metadata alongside issues. The agent
should help the user discover field names via the GraphQL query above and configure the mapping.

## Typical Workflows

### Full triage from scratch

1. `triage fetch` — pull issues from GitHub
2. `triage classify --parallel 3` — LLM classification (use `--parallel` to speed up)
3. `triage summary` — check confidence distribution and suggested new categories
4. `triage categories refine --min-votes 1 --auto-accept` — let LLM consolidate category suggestions into new nodes
5. `triage classify` — re-classify issues affected by new nodes
6. `triage review -i --max-confidence 0.7` — interactively review low-confidence issues
7. `triage review --auto-approve 0.8` — auto-approve high-confidence suggestions
8. `triage apply --dry-run` then `triage apply` — push to GitHub

### Reviewing classifications interactively

`triage review -i` walks through each pending suggestion. Use confidence filters to focus on
uncertain classifications first:

```
armitage triage review -i --max-confidence 0.7   # review low-confidence first
armitage triage review -i --min-confidence 0.7 --max-confidence 0.85  # then medium
```

At each suggestion you see the issue title, body excerpt, existing labels, suggested node, new
labels to add (highlighted in green), confidence, and LLM reasoning. Press:
- **a** — approve (merges existing + suggested labels; never removes existing labels)
- **r** — reject (mark as wrong; prompted for a note explaining why)
- **m** — modify (shows full merged label set for editing — can add or remove any labels; prompted for a note)
- **t** — stale (mark as referencing removed/deprecated features; optionally post a staleness inquiry)
- **i** — inquire (generate a clarification question via LLM, edit it, store for posting on apply)
- **s** — skip (leave for later)
- **b** — back (undo the previous decision and return to that item)
- **q** — quit

Notes entered on reject/modify are saved to `triage-examples.toml` and fed back to the LLM as
few-shot examples on the next `triage classify` run, so classification improves over time.

### Iterative batch triage

For large backlogs (hundreds of issues), classify in small batches to avoid wasting LLM calls on
a prompt that hasn't yet learned from your corrections:

```
# Round 1: classify a small batch, review, record feedback
armitage triage classify --limit 30
armitage triage review -i --min-confidence 0.7   # review confident ones first (quick wins)
armitage triage reset --unreviewed               # put skipped/rejected back in the untriaged pool

# Round 2: classify more — the LLM now has examples from round 1
armitage triage classify --limit 30
armitage triage review -i
armitage triage reset --unreviewed

# Repeat until the backlog is empty or quality is satisfactory
# Then bulk-approve remaining high-confidence suggestions:
armitage triage review --auto-approve 0.85
armitage triage apply
```

Each round benefits from the correction examples accumulated during review. Starting with
high-confidence issues (`--min-confidence 0.7`) lets you quickly validate or correct the
LLM's best guesses, generating high-quality examples that improve subsequent rounds.

### Agent-driven triage review

An agent (Claude Code) can drive the review loop, auto-approving obvious classifications and
presenting uncertain ones to the user. This is faster than interactive terminal review for large
backlogs.

**The agent loop:**

1. **Classify a batch:** `triage classify --limit N` (starts at 20, adapts over time)
2. **Read & partition suggestions:** `triage suggestions --status pending --format summary`
   — this outputs two pre-partitioned groups: **AUTO-APPROVE** (confidence >= 0.80 with a
   suggested node) and **NEEDS REVIEW** (low confidence or no node), with reasoning and stale
   flags on uncertain items. Use `--format json --body-max 500` only when you need full
   structured data for programmatic processing.
3. **Auto-decide:** Batch-approve all AUTO-APPROVE issues in one command:
   `triage decide <ref1> <ref2> ... --decision approve`. If memory of past corrections suggests
   the user would disagree on specific issues, pull those out and reject/modify them separately.
4. **Present auto-decides for override:** Show the user what was auto-approved as a summary list.
   The user can override specific issues by number.
5. **Present uncertain issues:** For each issue in the NEEDS REVIEW group, ask the user via `AskUserQuestion`:
   - Show: issue ref (link), title, body excerpt, current labels, suggested node + labels,
     confidence, reasoning
   - Ask: approve / reject / modify / stale / inquire / skip
   - On modify: suggest 2-3 alternative nodes from the roadmap tree
   - On inquire: the issue lacks enough info to classify — compose a concise question asking
     the author for clarification (what component, scope, priority) and pass it via `--question`
   - On stale: optionally compose a staleness inquiry via `--question` asking the author if
     the issue is still relevant or can be closed. Omit `--question` for internal-only stale marking
6. **Submit decisions:** Batch by decision type where possible:
   `triage decide <ref1> <ref2> ... --decision <d> [--node ...] [--labels ...] [--note ...]`
7. **Adapt batch size:**
   - If auto-override rate > 20%: shrink batch by 50%, raise auto-decide threshold
   - If correction rate > 40%: shrink batch by 50%
   - If correction rate < 15%: grow batch by 50%
   - Otherwise: hold steady
   - Bounds: min 10, max 100
8. **Reset:** `triage reset --unreviewed` to return skipped items to the untriaged pool
9. **Check:** `triage status --format json` — if 0 untriaged remain or user says stop, exit

**Memory:** The agent saves feedback memories for patterns it learns (e.g., "user always corrects
circuit/synthesis → circuit/qasm for QASM issues") and applies them in future auto-decide
partitioning.

**Boundaries:** The agent does not run `triage apply` (pushing to GitHub), `triage fetch` (pulling
issues), or modify the roadmap tree. Those are separate user-initiated actions.

### OKR review and deadline maintenance

When reviewing a person's OKR (e.g. comparing a manually written plan against the generated view):

1. **Generate the view:** `okr show --period 2026-Q2 --person <username> --format markdown`
2. **Surface deadline drift:** `triage overdue` — lists all open issues with past target dates org-wide; scope with `--repo` if needed
3. **Fix node tracking issue dates:** `project sync <node_path>` — pushes the node's local `[timeline]` to the project board for any node that has `track` set
4. **Fix individual issue dates on the board:** `project set <owner/repo#N> --target-date YYYY-MM-DD` — sets start/target dates for any single issue without raw GraphQL.
5. **Stage follow-up comments on overdue issues:** `triage overdue --comment "This issue's target date has passed. Please update the target date or leave a status comment." && triage apply`
6. **Watch for responses:** After `triage apply` posts the comments, run `triage watch add <issue-refs>...` to track each issue. Responses can be a new comment (`replied`), an issue closure (`closed`), or a project board update like a new target date (`project_updated`). Run `triage fetch` to detect activity, then `triage watch list` to see the full status.
7. **Dismiss resolved watches:** Once a response is satisfactory, run `triage watch dismiss <issue-refs>...` to remove them from the active list.
8. **Classify new unclassified issues:** `triage fetch --repo <r>` then `triage classify --repo <r> --limit N`, review, decide, apply

### Improving classification accuracy

1. Review low-confidence issues with `triage review -i --max-confidence 0.7`
2. When rejecting or modifying, enter a note explaining the correct reasoning
3. Run `triage examples list` to see accumulated examples
4. Run `triage reset --unreviewed` to reset unreviewed and rejected suggestions (keeping approved/modified ones), then `triage classify` to reclassify with examples in the LLM prompt
5. You can also bulk-export past decisions: `triage examples export`
6. To revert a single issue's decision: `triage reset --issue owner/repo#123`

## Planning Projects

When creating new nodes for a project, consider:

1. **Write good descriptions** — the triage LLM uses node descriptions to classify issues. Be
   specific about what code/functionality each node covers.
2. **Set repos with branch qualifiers** — if the same GitHub repo has work on multiple branches,
   use `@branch` to disambiguate (e.g. `repos = ["owner/repo@rust"]` for a rewrite branch).
3. **Match granularity to issue volume** — create sub-nodes for areas with many issues. Don't
   over-plan areas with few issues.
4. **Use the issue cache** — read `.armitage/issue-cache/{owner}--{repo}.toml` to understand what
   issues exist before planning node structure. Group issues by theme to identify natural clusters.
5. **After adding nodes, reset and re-classify** — run `triage reset --all` (or `--unreviewed` to
   preserve reviewed decisions) then `triage classify` so the LLM can route issues to the new nodes.
6. **Merge duplicate nodes** — if two nodes cover the same area, use `node merge <from> <to>` to
   fold one into the other. This reassigns all triage suggestions in the DB (no reclassification
   needed), moves any child nodes, and removes the source. Much cheaper than reset + re-classify.

## Key Files

| Path | Description |
|------|-------------|
| `armitage.toml` | Org config (name, github_orgs, label_schema, triage settings) |
| `*/node.toml` | Node metadata |
| `*/issues.toml` | Manual issue list for `push`/`pull` sync — **not read by `okr show`** |
| `*/issue.md` | Issue body (synced with GitHub) |
| `*/milestones.toml` | Node milestones |
| `goals.toml` | Cross-cutting external commitments spanning multiple nodes |
| `labels.toml` | Curated label definitions |
| `triage-examples.toml` | Human-verified classification examples for few-shot LLM prompts |
| `.armitage/triage.db` | SQLite DB for triage pipeline (gitignored) |
| `.armitage/sync-state.toml` | Per-node sync metadata (gitignored) |
| `.armitage/issue-cache/*.toml` | Lightweight issue cache per-repo (gitignored) |
| `.armitage/secrets.toml` | Local secrets / API keys (gitignored) |
| `.armitage/label-renames.toml` | Pending label rename ledger |
| `.armitage/dismissed-categories.toml` | Categories dismissed from suggestions |

## Common Pitfalls

**`issues.toml` does not affect `okr show`.** Adding an issue to a node's `issues.toml` has no effect on the OKR view. OKR reads only from the triage DB. Use the `triage fetch` → `classify` → `decide` pipeline instead.

**New GitHub issues need `triage fetch` before they appear anywhere.** After creating an issue with `gh issue create`, run `triage fetch --repo <owner/repo>` to pull it into the local DB, then classify it.

**Set labels and assignees at creation time.** When opening a GitHub issue on behalf of the roadmap, pass `--label` and `--assignee` directly to `gh issue create`. Don't rely on a follow-up `gh issue edit` or `triage apply` to add labels the issue should have from the start.

**`triage decide modify` on a previously-applied issue requires `triage apply` to take effect in OKR.** For issues whose prior decision was already applied to GitHub, `triage decide modify` records the new node but the effective node_path in the DB is only updated when `triage apply` runs. Check with `triage decisions --node <path> --unapplied` before running OKR to ensure nothing is stale.

**Never query the SQLite DB directly.** Use `triage suggestions --issues N --format json`, `triage decisions`, and `triage status` instead of `sqlite3`. Direct SQL queries bypass armitage's abstractions and break when the schema changes.

**A newly created node won't appear in `okr show --person` until it has issues.** A node where the person is an owner but has zero issues in the triage DB is silently excluded from the OKR view. The `track` field is the fastest fix: set it and run `triage fetch` — the tracking issue appears immediately via the shortcut without going through the classify/decide pipeline.

**Classify cross-cutting blockers to the dependent area, not the implementation area.** When an issue is a blocker for one initiative (e.g. Gemini Logical STAR injection) but the implementation work lives in another repo/area (e.g. bloqade-lanes / shuttle), classify it to the initiative that depends on it. That's where the person tracking the work will look for it. Low confidence from the LLM on such issues is expected and not a reason to reclassify — add a note on approval explaining the reasoning so the example doesn't mislead future classification runs.

**`triage overdue` is the right tool after an OKR review.** When comparing a manually written OKR against the generated view and finding stale target dates, run `triage overdue` first to get a full picture of deadline drift across the org before editing individual issues. Then fix dates on the GitHub project board (via `project sync <node_path>` for nodes with `track`, or via `project set <owner/repo#N> --target-date YYYY-MM-DD` for individual issues).

**Moving an issue on the project board counts as a response.** When you ask a collaborator to update a timeline, they may respond by moving the issue to a new target date or status on the GitHub project board rather than posting a comment. The watch system fires `project_updated` for this — it won't show as `replied`. Always check `triage watch list` after a fetch (not just the `[watch]` stdout lines), and don't assume silence means no response if they're the kind of person who works through the project board.
