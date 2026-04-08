# Triage Inspection, Agent Interface, and Category Workflow

**Date:** 2026-04-06
**Status:** Draft

## Problem

After running `triage classify`, there is no way to inspect results without raw SQLite queries.
Classification results are only written to the DB after all workers finish, so partial progress
is lost on crash and live inspection is impossible. Suggested new categories are printed once at
the end of classify and then forgotten — there is no workflow to act on them.

## Changes Overview

Five new subcommands, one new subcommand group, and two cross-cutting changes:

**Cross-cutting:**
1. **Streaming DB writes in classify** — write results as they complete instead of batching
2. **`--format json` on existing commands** — machine-readable output for agents

**New subcommands:**
3. **`triage summary`** — aggregate analysis of classification results
4. **`triage suggestions`** — list/filter all triage suggestions
5. **`triage decisions`** — list/filter review decisions

**New subcommand group (`triage categories`):**
6. **`triage categories list`** — view suggested new categories
7. **`triage categories apply`** — create a node from a suggestion and reset for reclassification
8. **`triage categories dismiss`** — hide a suggested category from future listings

No database schema changes. One new file: `.armitage/dismissed-categories.toml`.

---

## 1. Streaming DB Writes in Classify

### Current behavior

`triage_issues()` in `src/triage/llm.rs` accumulates all `ClassifyResult` values in an
`Arc<Mutex<Vec<ClassifyResult>>>`. After all worker threads join, the main thread iterates the
vector and calls `upsert_suggestion()` for each result.

### New behavior

Wrap the DB connection in `Arc<Mutex<Connection>>` and pass it to each worker thread. After
each successful LLM call + parse, the worker acquires the lock and calls `upsert_suggestion()`
immediately. The lock is uncontended in practice because LLM calls take seconds while a DB
write takes microseconds.

The post-loop code that currently iterates `all_results` to write to DB is removed. Node
validation (checking `suggested_node` against `valid_nodes`) moves into the worker loop, which
requires passing `valid_nodes: Arc<HashSet<String>>` to workers.

New-category vote collection for the end-of-run summary remains post-loop: query the DB for
all suggestions created in this run (filter by `created_at >= run_start_timestamp`) and
aggregate `suggested_new_categories`. This avoids a second shared mutex.

The `all_results` vector is removed entirely. The return value is computed by counting rows
inserted (each worker increments an `AtomicUsize`).

### Benefits

- Partial progress survives crashes
- `triage summary` shows live results during a long classify run
- Memory usage is constant instead of O(issues)

---

## 2. `--format json` on Existing Commands

Add a `--format` flag accepting `"table"` (default) or `"json"` to:

- **`triage classify`** — After completion, emit a JSON summary to stdout:
  ```json
  {
    "classified": 493,
    "errors": 2,
    "confidence": { "mean": 0.81, "p25": 0.75, "median": 0.85, "p75": 0.9 },
    "top_nodes": [
      { "node": "flair", "count": 92, "avg_confidence": 0.82 }
    ],
    "null_node_count": 28,
    "suggested_new_categories": [
      { "category": "circuit/emulator", "vote_count": 4, "issue_refs": ["owner/repo#1"] }
    ]
  }
  ```
  Progress bar already goes to stderr. In JSON mode, per-issue log lines are suppressed
  (or also sent to stderr). Only the final JSON object goes to stdout.

- **`triage status`** — Emit `PipelineCounts` as JSON.

- **`triage review --list`** — Emit pending suggestions as a JSON array.

### Implementation

Define an `OutputFormat` enum (`Table`, `Json`) and a helper `parse_format()`. Each command
checks the format and branches between the existing display code and `serde_json` serialization.
Derive `Serialize` on `PipelineCounts`, `TriageSuggestion`, `StoredIssue`, and
`ReviewDecision` (or define lightweight serialization structs).

---

## 3. `triage summary`

```
armitage triage summary [--format table|json] [--repo <repo>]
```

Produces three analysis blocks from the current DB state:

### Confidence distribution

Histogram bands: `<0.5`, `0.5-0.7`, `0.7-0.8`, `0.8-0.9`, `0.9-1.0` with count and
percentage.

### Node breakdown

Top nodes by issue count. Each row shows: node path, count, avg confidence, min confidence,
max confidence. Sorted by count descending. Null-node shown as `(unclassified)`.

### Suggested new categories

Aggregated from `suggested_new_categories` across all suggestions. Sorted by vote count
descending. Each entry shows: category name, vote count, up to 5 issue refs. Dismissed
categories are excluded.

### `--repo` filter

When set, all three blocks are scoped to issues from that repo only.

### DB queries

Three new aggregate query functions in `db.rs`:
- `get_confidence_distribution(conn, repo?) -> Vec<(band, count)>`
- `get_node_breakdown(conn, repo?) -> Vec<(node, count, avg, min, max)>`
- `get_new_category_votes(conn, repo?) -> Vec<(category, count, issue_refs)>`

Alternatively, a single `get_summary(conn, repo?) -> Summary` that runs all three queries.

---

## 4. `triage suggestions`

```
armitage triage suggestions [filters...] [--format table|json]
```

### Filters

| Flag | Type | Description |
|------|------|-------------|
| `--node <prefix>` | String | Node path prefix match (`flair` matches `flair/*`) |
| `--repo <repo>` | String | Source repo filter |
| `--min-confidence <f>` | f64 | Minimum confidence |
| `--max-confidence <f>` | f64 | Maximum confidence |
| `--status <s>` | Enum | Pipeline state: `pending`, `approved`, `rejected`, `applied`. `approved` includes `modified` decisions (approved with edits). |
| `--tracking-only` | bool | Only tracking issues |
| `--unclassified` | bool | Only null-node suggestions |
| `--sort <field>` | Enum | Sort by: `confidence` (default), `node`, `repo` |
| `--limit <n>` | usize | Max rows, default 50. 0 = unlimited |

### Table output

Columns: `issue_ref`, `title` (truncated 55 chars), `node`, `confidence`, `status`, `labels`
(comma-joined, truncated). Reasoning omitted for density.

### JSON output

Full objects including reasoning, suggested_labels, suggested_new_categories, is_tracking_issue.

### DB implementation

New function `get_suggestions_filtered(conn, &SuggestionFilters) -> Vec<SuggestionRow>`.

Builds SQL dynamically with a `SuggestionFilters` struct:
```rust
pub struct SuggestionFilters {
    pub node_prefix: Option<String>,
    pub repo: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_confidence: Option<f64>,
    pub status: Option<SuggestionStatus>,  // pending|approved|rejected|applied
    pub tracking_only: bool,
    pub unclassified: bool,
    pub sort: SuggestionSort,
    pub limit: usize,
}
```

Joins `triage_suggestions` + `issues` + left join `review_decisions`. The `status` filter
maps to:
- `pending`: `rd.id IS NULL`
- `approved`: `rd.decision IN ('approved', 'modified') AND rd.applied_at IS NULL`
- `rejected`: `rd.decision = 'rejected'`
- `applied`: `rd.applied_at IS NOT NULL`

---

## 5. `triage decisions`

```
armitage triage decisions [filters...] [--format table|json]
```

### Filters

| Flag | Type | Description |
|------|------|-------------|
| `--status <s>` | Enum | `approved`, `rejected`, `modified`, `applied` |
| `--unapplied` | bool | Shorthand: approved+modified where applied_at IS NULL |
| `--node <prefix>` | String | Filter by final_node prefix |
| `--repo <repo>` | String | Source repo |
| `--limit <n>` | usize | Max rows, default 50 |

### Table output

Columns: `issue_ref`, `title` (truncated), `decision`, `final_node`, `final_labels`
(comma-joined), `applied_at`.

### JSON output

Full objects.

### DB implementation

New function `get_decisions_filtered(conn, &DecisionFilters) -> Vec<DecisionRow>`. Similar
dynamic SQL pattern as suggestions.

---

## 6. `triage categories list`

```
armitage triage categories list [--format table|json] [--min-votes <n>]
```

Aggregates `suggested_new_categories` across all triage suggestions. Groups by category name,
counts distinct issues that voted for each, collects issue refs.

### Table output

```
Suggested categories:
  compute/emulator       4 votes  acme/widgets#428, #431, #447, #455
  backend/api            3 votes  acme/internal#187, #188, #189
  docs/tutorials         2 votes  acme/platform#339, #340
```

`--min-votes` filters (default: 1).

### Dismissed categories

Reads `.armitage/dismissed-categories.toml` and excludes matching entries. If the file doesn't
exist, no filtering.

### DB implementation

Query: `SELECT suggested_new_categories, issue_id FROM triage_suggestions WHERE
suggested_new_categories != '[]'`. Aggregate in Rust (parse each JSON array, build
`BTreeMap<String, Vec<String>>`). Join issue_id back to issues table for refs.

---

## 7. `triage categories apply`

```
armitage triage categories apply <category-path> \
  --name <name> --description <description> \
  [--reclassify] [--reclassify-backend <backend>] [--reclassify-model <model>]
```

### Steps

1. **Validate** — path doesn't exist, parent exists (delegate to existing `create_node_full`
   validation)
2. **Create node** — call `create_node_full()` with the provided name, description, status
   "active", no labels/repos/timeline
3. **Collect reset targets** — query DB for:
   - All suggestions where `suggested_node IS NULL` (null-node issues)
   - All suggestions where `suggested_new_categories` contains this category path
   - Union of both sets (deduplicated by issue_id)
4. **Reset suggestions** — delete the collected suggestions (and their review decisions),
   making those issues untriaged. New DB function:
   `delete_suggestions_for_reclassify(conn, category) -> usize`
5. **Report** — `Created node '<path>'. Reset N suggestion(s). Run 'triage classify' to
   reclassify.`
6. **Optional reclassify** — if `--reclassify` is set, immediately invoke `triage_issues()`
   scoped to the reset issues. This requires extending `triage_issues()` to accept an optional
   `Vec<i64>` of issue IDs (in addition to the existing repo filter). When provided, classify
   only those issues instead of all untriaged. The `--reclassify-backend` and
   `--reclassify-model` flags are passed through `resolve_classify_config()` (same path as
   `triage classify --backend/--model`). If omitted, defaults from `armitage.toml` are used.

`--name` and `--description` are required flags (no interactive prompt).

---

## 8. `triage categories dismiss`

```
armitage triage categories dismiss <category-path>
```

Appends the category path to `.armitage/dismissed-categories.toml`:

```toml
dismissed = ["backend/api"]
```

If the file doesn't exist, creates it. If the category is already dismissed, no-op with a
message.

### Undismiss

To undismiss, the user edits the TOML file directly. No dedicated command — keeps scope small.

---

## File Changes Summary

| File | Changes |
|------|---------|
| `src/cli/mod.rs` | Add `Summary`, `Suggestions`, `Decisions` to `TriageCommands`. Add `TriageCategoryCommands` subcommand group. Add `--format` to `Classify`, `Status`, `Review`. |
| `src/cli/triage.rs` | Add `run_summary()`, `run_suggestions()`, `run_decisions()`, `run_categories_list()`, `run_categories_apply()`, `run_categories_dismiss()`. Modify `run_classify()` and `run_status()` for JSON output. |
| `src/triage/llm.rs` | Refactor `triage_issues()`: wrap conn in `Arc<Mutex<Connection>>`, write results in worker loop, remove `all_results` vec. Add optional `issue_ids: Option<Vec<i64>>` parameter. Move node validation into worker. |
| `src/triage/db.rs` | Add `get_suggestions_filtered()`, `get_decisions_filtered()`, `get_confidence_distribution()`, `get_node_breakdown()`, `get_new_category_votes()`, `delete_suggestions_for_reclassify()`. Derive `Serialize` on data structs. |
| `src/triage/categories.rs` | New file. `read_dismissed()`, `write_dismissed()`, `is_dismissed()`. Reads/writes `.armitage/dismissed-categories.toml`. |
| `src/model/mod.rs` | Add `DismissedCategories` struct (or keep it in `triage/categories.rs`). |
