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
- `github_issue` — linked issue in `owner/repo#number` format
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
armitage node set <path> [--name ...] [--description ...] [--triage-hint ...] [--owners ...] [--team ...] [--repos ...] [--labels ...] [--status ...]
armitage node move <from> <to>
armitage node merge <from> <to> [-y]   # merge source into target
armitage node remove <path> [-y]
armitage node check                    # timeline violations + issue date validation
armitage node fmt [<path>...]          # re-serialize node.toml files
```

Note: `node set` does not support `--timeline`. To set or change a timeline, use `node edit`
(interactive) or edit the `[timeline]` section in `node.toml` directly. `node new` supports
`--timeline "2026-01-01 2026-12-31"` for setting it at creation time.

`node check` validates: (1) parent-child timeline containment, (2) issue start/target dates
from the GitHub Project board against node timelines. Requires `triage fetch` to have run with
`[triage.project]` configured.

### GitHub Sync

```
armitage pull [<path>] [--dry-run]   # pull changes from GitHub into local nodes
armitage push [<path>] [--dry-run]   # push local changes to GitHub issues
armitage resolve [<path>] [--list]   # resolve sync conflicts
armitage status                      # show org overview
```

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
armitage triage reset [--below <threshold> | --node <path> | --issue <owner/repo#N> | --all | --unreviewed]
armitage triage status [--format table|json]
armitage triage summary [--repo <r>] [--format table|json]
armitage triage suggestions [--issues 247,276,32] [--node <prefix>] [--repo <r>] [--min-confidence N] [--max-confidence N] [--status pending|approved|rejected|applied] [--tracking-only] [--unclassified] [--stale-only] [--sort confidence|node|repo] [--limit N] [--format table|json|summary|refs] [--body-max N]
armitage triage inactive [--days N] [--since <date>] [--repo <r>] [--format table|json] [--inquire "message"]
armitage triage decisions [--status <s>] [--unapplied] [--node <prefix>] [--repo <r>] [--limit N] [--format table|json]
```

- **fetch** — pulls issues from GitHub repos into a local SQLite DB (`.armitage/triage.db`), then refreshes the issue cache
- **classify** — sends untriaged issues to an LLM with the roadmap tree, label schema, curated labels, and any classification examples as context; stores suggestions in the DB (including `is_stale` for issues referencing removed/deprecated features, `is_inactive` for issues with 180+ days of no GitHub activity, and `needs_followup`/`followup_reason` when the discussion lacks concrete next steps), then refreshes the issue cache. **Labels are additive only:** the LLM is prompted to suggest only labels the issue does not already have, and any existing labels that slip through are filtered out before storage. `--limit N` classifies at most N issues per run (default: all) — useful for iterative batch workflows. `--batch-size` controls how many issues are sent per LLM call (prompt granularity), while `--limit` controls the total number of issues processed in the run
- **inactive** — queries the local DB for open issues with no GitHub activity for at least N days (default 180). Scoped to repos in `node.toml` files only. Outputs a table with the issue ref, title, time since last update, and suggested node (if classified). `--days N` sets the inactivity threshold. `--since <date>` sets an absolute cutoff date (ISO 8601). `--repo <r>` scopes to one repo. `--inquire "message"` stages all matching unreviewed issues as `inquire` decisions with the given comment text (applied by `triage apply`)
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
- **decide** — submit review decisions non-interactively for one or more issues. Accepts multiple issue refs in a single command (e.g., `triage decide ref1 ref2 ref3 --decision stale`). Use `--all-pending` to decide on all pending suggestions at once (e.g., `triage decide --all-pending --decision approve`), optionally filtered with `--min-confidence`/`--max-confidence`. Used by agents and scripts. Supports `approve`, `reject`, `modify` (with optional `--node`/`--labels` overrides), `stale` (with optional `--question` for staleness inquiry), and `inquire` (with required `--question`). Auto-saves examples on reject/modify/stale (same as interactive mode). Errors on individual issues are reported but don't stop the batch; a summary error is returned at the end if any failed. Errors if a decision has already been applied to GitHub
- **reset** — clears suggestions so issues can be re-classified, then refreshes the issue cache. Modes: `--below <threshold>` (confidence), `--node <path>` (subtree), `--issue <owner/repo#N>` (single issue), `--all`, or `--unreviewed` (deletes unreviewed and rejected suggestions while preserving approved/modified ones — useful for reclassifying with improved examples after a partial review)
- **summary** — confidence distribution, per-node breakdown, and suggested new categories
- **suggestions** — query and filter individual suggestions with flexible criteria. Use `--issues 247,276,32` to select specific issue numbers. The `--format summary` option groups results into **AUTO-APPROVE** (confidence >= 0.80 with a suggested node) and **NEEDS REVIEW** (low confidence or no node), with extra detail (stale flags, new categories, reasoning) for uncertain ones — ideal for agent-driven workflows that need to partition suggestions without post-processing. Use `--format refs` to output just issue refs one per line (e.g., for piping to `triage decide`)
- **decisions** — query and filter review decisions

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
| `*/issue.md` | Issue body (synced with GitHub) |
| `*/milestones.toml` | Node milestones |
| `labels.toml` | Curated label definitions |
| `triage-examples.toml` | Human-verified classification examples for few-shot LLM prompts |
| `.armitage/triage.db` | SQLite DB for triage pipeline (gitignored) |
| `.armitage/sync-state.toml` | Per-node sync metadata (gitignored) |
| `.armitage/issue-cache/*.toml` | Lightweight issue cache per-repo (gitignored) |
| `.armitage/secrets.toml` | Local secrets / API keys (gitignored) |
| `.armitage/label-renames.toml` | Pending label rename ledger |
| `.armitage/dismissed-categories.toml` | Categories dismissed from suggestions |
