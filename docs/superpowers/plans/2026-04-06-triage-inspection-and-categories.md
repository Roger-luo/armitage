# Triage Inspection, Agent Interface, and Category Workflow — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add triage inspection commands (`summary`, `suggestions`, `decisions`), `--format json` support, streaming DB writes in classify, and a `triage categories` subcommand group for managing LLM-suggested new categories.

**Architecture:** New subcommands are added to the `TriageCommands` enum in `src/cli/mod.rs` and dispatched to handler functions in `src/cli/triage.rs`. New DB queries go in `src/triage/db.rs`. The classify refactor changes `src/triage/llm.rs` to write results per-batch via `Arc<Mutex<Connection>>` instead of accumulating in memory. A new `src/triage/categories.rs` module manages dismissed categories via a TOML file.

**Tech Stack:** Rust, clap (derive), rusqlite (bundled), serde/serde_json, toml

**Spec:** `docs/superpowers/specs/2026-04-06-triage-inspection-and-categories-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/cli/mod.rs` | Modify | Add enum variants + dispatch arms for `Summary`, `Suggestions`, `Decisions`, `Categories` |
| `src/cli/triage.rs` | Modify | Add `run_summary()`, `run_suggestions()`, `run_decisions()`, `run_categories_list()`, `run_categories_apply()`, `run_categories_dismiss()`. Modify `run_classify()` and `run_status()` for `--format` |
| `src/triage/db.rs` | Modify | Add `Serialize` derives, filter structs, new query functions, `delete_suggestions_for_reclassify()` |
| `src/triage/llm.rs` | Modify | Refactor `triage_issues()` to stream writes via `Arc<Mutex<Connection>>` |
| `src/triage/categories.rs` | Create | Dismissed categories TOML read/write |
| `src/triage/mod.rs` | Modify | Add `pub mod categories;` |
| `src/cli/node.rs` | Modify | Make `create_node_full()` visibility `pub(crate)` |

---

### Task 1: Add `Serialize` to DB Structs and `OutputFormat` Enum

**Files:**
- Modify: `src/triage/db.rs:83-129` (struct derives)
- Modify: `src/cli/triage.rs` (add `OutputFormat` enum)

- [ ] **Step 1: Write test for JSON serialization of PipelineCounts**

In `src/triage/db.rs`, add to the bottom of the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn pipeline_counts_serializes_to_json() {
    let counts = PipelineCounts {
        total_fetched: 100,
        untriaged: 20,
        pending_review: 30,
        approved_unapplied: 10,
        applied: 40,
    };
    let json = serde_json::to_string(&counts).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["total_fetched"], 100);
    assert_eq!(parsed["applied"], 40);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -E 'test(pipeline_counts_serializes_to_json)'`
Expected: FAIL — `PipelineCounts` doesn't derive `Serialize`

- [ ] **Step 3: Add Serialize derives to all DB structs**

In `src/triage/db.rs`, change the four struct derives:

```rust
// Line 83: was #[derive(Debug, Clone)]
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoredIssue {

// Line 97: was #[derive(Debug, Clone)]
#[derive(Debug, Clone, serde::Serialize)]
pub struct TriageSuggestion {

// Line 111: was #[derive(Debug, Clone)]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewDecision {

// Line 122: was #[derive(Debug, Clone, Default)]
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PipelineCounts {
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -E 'test(pipeline_counts_serializes_to_json)'`
Expected: PASS

- [ ] **Step 5: Add OutputFormat enum to cli/triage.rs**

At the top of `src/cli/triage.rs`, after the existing imports, add:

```rust
/// Output format for commands that support machine-readable output.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "table" => Ok(Self::Table),
            "json" => Ok(Self::Json),
            other => Err(Error::Other(format!(
                "unknown format '{other}', expected 'table' or 'json'"
            ))),
        }
    }
}
```

- [ ] **Step 6: Run clippy and format**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: Clean

- [ ] **Step 7: Commit**

```bash
git add src/triage/db.rs src/cli/triage.rs
git commit -m "feat: add Serialize derives to DB structs and OutputFormat enum"
```

---

### Task 2: `triage status --format json`

**Files:**
- Modify: `src/cli/mod.rs:261-262` (Status variant)
- Modify: `src/cli/mod.rs` (dispatch arm ~line 557)
- Modify: `src/cli/triage.rs:995-1007` (run_status)

- [ ] **Step 1: Add `--format` to Status variant in TriageCommands**

In `src/cli/mod.rs`, change the `Status` variant from a unit variant to:

```rust
    /// Show triage pipeline status
    Status {
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
```

- [ ] **Step 2: Update dispatch arm for Status**

In `src/cli/mod.rs`, change the Status dispatch arm from:

```rust
TriageCommands::Status => {
    triage::run_status()?;
}
```

to:

```rust
TriageCommands::Status { format } => {
    triage::run_status(format)?;
}
```

- [ ] **Step 3: Update run_status() to accept format**

In `src/cli/triage.rs`, replace `run_status()`:

```rust
pub fn run_status(format: String) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let counts = db::get_pipeline_counts(&conn)?;

    if fmt == OutputFormat::Json {
        println!("{}", serde_json::to_string(&counts).map_err(|e| Error::Other(e.to_string()))?);
    } else {
        println!("Triage pipeline:");
        println!("  Fetched issues:       {}", counts.total_fetched);
        println!("  Untriaged:            {}", counts.untriaged);
        println!("  Pending review:       {}", counts.pending_review);
        println!("  Approved (unapplied): {}", counts.approved_unapplied);
        println!("  Applied:              {}", counts.applied);
    }
    Ok(())
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add --format json to triage status"
```

---

### Task 3: `triage suggestions` — DB Query Layer

**Files:**
- Modify: `src/triage/db.rs` (add filter structs + query function)

- [ ] **Step 1: Write test for get_suggestions_filtered**

In `src/triage/db.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn suggestions_filtered_by_status_and_node() {
    let conn = memory_db();

    // Create 3 issues with suggestions
    let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
    let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
    let id3 = upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();

    let s1 = upsert_suggestion(&conn, &sample_suggestion(id1, "flair", 0.9)).unwrap();
    upsert_suggestion(&conn, &sample_suggestion(id2, "flair/rust", 0.7)).unwrap();
    upsert_suggestion(&conn, &sample_suggestion(id3, "devops", 0.5)).unwrap();

    // Approve suggestion 1
    insert_decision(
        &conn,
        &ReviewDecision {
            id: 0,
            suggestion_id: s1,
            decision: "approved".to_string(),
            final_node: Some("flair".to_string()),
            final_labels: vec![],
            decided_at: "2026-04-01T00:00:00Z".to_string(),
            applied_at: None,
        },
    )
    .unwrap();

    // Filter: pending only
    let filters = SuggestionFilters {
        status: Some(SuggestionStatus::Pending),
        ..Default::default()
    };
    let results = get_suggestions_filtered(&conn, &filters).unwrap();
    assert_eq!(results.len(), 2);

    // Filter: node prefix "flair"
    let filters = SuggestionFilters {
        node_prefix: Some("flair".to_string()),
        ..Default::default()
    };
    let results = get_suggestions_filtered(&conn, &filters).unwrap();
    assert_eq!(results.len(), 2); // flair and flair/rust

    // Filter: min confidence
    let filters = SuggestionFilters {
        min_confidence: Some(0.8),
        ..Default::default()
    };
    let results = get_suggestions_filtered(&conn, &filters).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.suggested_node.as_deref(), Some("flair"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -E 'test(suggestions_filtered_by_status_and_node)'`
Expected: FAIL — `SuggestionFilters`, `SuggestionStatus`, `get_suggestions_filtered` don't exist

- [ ] **Step 3: Implement filter types and query function**

In `src/triage/db.rs`, after the `PipelineCounts` struct (around line 129), add:

```rust
// ---------------------------------------------------------------------------
// Suggestion filters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionStatus {
    Pending,
    Approved,
    Rejected,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionSort {
    #[default]
    Confidence,
    Node,
    Repo,
}

#[derive(Debug, Clone, Default)]
pub struct SuggestionFilters {
    pub node_prefix: Option<String>,
    pub repo: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_confidence: Option<f64>,
    pub status: Option<SuggestionStatus>,
    pub tracking_only: bool,
    pub unclassified: bool,
    pub sort: SuggestionSort,
    pub limit: usize, // 0 = unlimited
}
```

Then, after the `get_pipeline_counts` function (around line 564), add:

```rust
// ---------------------------------------------------------------------------
// Filtered suggestion queries
// ---------------------------------------------------------------------------

pub fn get_suggestions_filtered(
    conn: &Connection,
    f: &SuggestionFilters,
) -> Result<Vec<(StoredIssue, TriageSuggestion)>> {
    let mut sql = String::from(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count,
                ts.id, ts.issue_id, ts.suggested_node, ts.suggested_labels,
                ts.confidence, ts.reasoning, ts.llm_backend, ts.created_at,
                ts.is_tracking_issue, ts.suggested_new_categories,
                rd.id AS rd_id, rd.decision, rd.applied_at
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
         WHERE 1=1",
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref prefix) = f.node_prefix {
        sql.push_str(&format!(
            " AND (ts.suggested_node = ?{p} OR ts.suggested_node LIKE ?{p2})",
            p = param_idx,
            p2 = param_idx + 1
        ));
        params.push(Box::new(prefix.clone()));
        params.push(Box::new(format!("{prefix}/%")));
        param_idx += 2;
    }

    if let Some(ref repo) = f.repo {
        sql.push_str(&format!(" AND i.repo = ?{param_idx}"));
        params.push(Box::new(repo.clone()));
        param_idx += 1;
    }

    if let Some(min) = f.min_confidence {
        sql.push_str(&format!(
            " AND COALESCE(ts.confidence, 0.0) >= ?{param_idx}"
        ));
        params.push(Box::new(min));
        param_idx += 1;
    }

    if let Some(max) = f.max_confidence {
        sql.push_str(&format!(
            " AND COALESCE(ts.confidence, 0.0) <= ?{param_idx}"
        ));
        params.push(Box::new(max));
        param_idx += 1;
    }

    match f.status {
        Some(SuggestionStatus::Pending) => sql.push_str(" AND rd.id IS NULL"),
        Some(SuggestionStatus::Approved) => {
            sql.push_str(
                " AND rd.decision IN ('approved', 'modified') AND rd.applied_at IS NULL",
            );
        }
        Some(SuggestionStatus::Rejected) => sql.push_str(" AND rd.decision = 'rejected'"),
        Some(SuggestionStatus::Applied) => sql.push_str(" AND rd.applied_at IS NOT NULL"),
        None => {}
    }

    if f.tracking_only {
        sql.push_str(" AND ts.is_tracking_issue = 1");
    }

    if f.unclassified {
        sql.push_str(" AND ts.suggested_node IS NULL");
    }

    let order = match f.sort {
        SuggestionSort::Confidence => "ts.confidence DESC, i.repo, i.number",
        SuggestionSort::Node => "ts.suggested_node, i.repo, i.number",
        SuggestionSort::Repo => "i.repo, i.number",
    };
    sql.push_str(&format!(" ORDER BY {order}"));

    if f.limit > 0 {
        sql.push_str(&format!(" LIMIT {}", f.limit));
    }

    let _ = param_idx; // suppress unused warning
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(13)?;
        let suggested_labels: Vec<String> =
            serde_json::from_str(&labels_json).unwrap_or_default();
        let suggestion = TriageSuggestion {
            id: row.get(10)?,
            issue_id: row.get(11)?,
            suggested_node: row.get(12)?,
            suggested_labels,
            confidence: row.get(14)?,
            reasoning: row.get(15)?,
            llm_backend: row.get(16)?,
            created_at: row.get(17)?,
            is_tracking_issue: row.get::<_, i64>(18)? != 0,
            suggested_new_categories: {
                let json: String = row.get(19)?;
                serde_json::from_str(&json).unwrap_or_default()
            },
        };
        Ok((issue, suggestion))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -E 'test(suggestions_filtered_by_status_and_node)'`
Expected: PASS

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: Clean

- [ ] **Step 6: Commit**

```bash
git add src/triage/db.rs
git commit -m "feat: add get_suggestions_filtered with SuggestionFilters"
```

---

### Task 4: `triage suggestions` — CLI Subcommand

**Files:**
- Modify: `src/cli/mod.rs` (add Suggestions variant + dispatch)
- Modify: `src/cli/triage.rs` (add run_suggestions)

- [ ] **Step 1: Add Suggestions variant to TriageCommands**

In `src/cli/mod.rs`, add after the `Status` variant:

```rust
    /// List triage suggestions with filtering
    Suggestions {
        /// Filter by node path prefix (e.g. "flair" matches flair/*)
        #[arg(long)]
        node: Option<String>,
        /// Filter by source repo
        #[arg(long)]
        repo: Option<String>,
        /// Minimum confidence (0.0-1.0)
        #[arg(long)]
        min_confidence: Option<f64>,
        /// Maximum confidence (0.0-1.0)
        #[arg(long)]
        max_confidence: Option<f64>,
        /// Pipeline state: pending, approved, rejected, applied
        #[arg(long)]
        status: Option<String>,
        /// Only show tracking issues
        #[arg(long)]
        tracking_only: bool,
        /// Only show suggestions with no node
        #[arg(long)]
        unclassified: bool,
        /// Sort by: confidence (default), node, repo
        #[arg(long, default_value = "confidence")]
        sort: String,
        /// Max rows (default 50, 0 = unlimited)
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
```

- [ ] **Step 2: Add dispatch arm**

In `src/cli/mod.rs`, add the dispatch arm after the Status arm:

```rust
TriageCommands::Suggestions {
    node,
    repo,
    min_confidence,
    max_confidence,
    status,
    tracking_only,
    unclassified,
    sort,
    limit,
    format,
} => {
    triage::run_suggestions(
        node,
        repo,
        min_confidence,
        max_confidence,
        status,
        tracking_only,
        unclassified,
        sort,
        limit,
        format,
    )?;
}
```

- [ ] **Step 3: Implement run_suggestions()**

In `src/cli/triage.rs`, add:

```rust
pub fn run_suggestions(
    node: Option<String>,
    repo: Option<String>,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
    status: Option<String>,
    tracking_only: bool,
    unclassified: bool,
    sort: String,
    limit: usize,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let status_filter = status
        .as_deref()
        .map(|s| match s {
            "pending" => Ok(db::SuggestionStatus::Pending),
            "approved" => Ok(db::SuggestionStatus::Approved),
            "rejected" => Ok(db::SuggestionStatus::Rejected),
            "applied" => Ok(db::SuggestionStatus::Applied),
            other => Err(Error::Other(format!(
                "unknown status '{other}', expected: pending, approved, rejected, applied"
            ))),
        })
        .transpose()?;

    let sort_field = match sort.as_str() {
        "confidence" => db::SuggestionSort::Confidence,
        "node" => db::SuggestionSort::Node,
        "repo" => db::SuggestionSort::Repo,
        other => {
            return Err(Error::Other(format!(
                "unknown sort '{other}', expected: confidence, node, repo"
            )))
        }
    };

    let filters = db::SuggestionFilters {
        node_prefix: node,
        repo,
        min_confidence,
        max_confidence,
        status: status_filter,
        tracking_only,
        unclassified,
        sort: sort_field,
        limit,
    };

    let results = db::get_suggestions_filtered(&conn, &filters)?;

    if fmt == OutputFormat::Json {
        let json_rows: Vec<serde_json::Value> = results
            .iter()
            .map(|(issue, sug)| {
                serde_json::json!({
                    "issue_ref": format!("{}#{}", issue.repo, issue.number),
                    "title": issue.title,
                    "repo": issue.repo,
                    "number": issue.number,
                    "suggested_node": sug.suggested_node,
                    "suggested_labels": sug.suggested_labels,
                    "confidence": sug.confidence,
                    "reasoning": sug.reasoning,
                    "is_tracking_issue": sug.is_tracking_issue,
                    "suggested_new_categories": sug.suggested_new_categories,
                    "llm_backend": sug.llm_backend,
                    "created_at": sug.created_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json_rows)
                .map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        if results.is_empty() {
            println!("No suggestions match the given filters.");
            return Ok(());
        }
        println!(
            "{:<30} {:<55} {:<25} {:>6}",
            "ISSUE", "TITLE", "NODE", "CONF"
        );
        println!("{}", "-".repeat(120));
        for (issue, sug) in &results {
            let issue_ref = format!("{}#{}", issue.repo, issue.number);
            let title: String = issue.title.chars().take(53).collect();
            let node = sug
                .suggested_node
                .as_deref()
                .unwrap_or("(unclassified)");
            let conf = sug
                .confidence
                .map(|c| format!("{:.0}%", c * 100.0))
                .unwrap_or_else(|| "—".to_string());
            println!("{:<30} {:<55} {:<25} {:>6}", issue_ref, title, node, conf);
        }
        println!("\n{} suggestion(s)", results.len());
    }
    Ok(())
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage suggestions command with filtering"
```

---

### Task 5: `triage decisions` — DB Query + CLI

**Files:**
- Modify: `src/triage/db.rs` (add DecisionFilters + query)
- Modify: `src/cli/mod.rs` (add Decisions variant + dispatch)
- Modify: `src/cli/triage.rs` (add run_decisions)

- [ ] **Step 1: Write test for get_decisions_filtered**

In `src/triage/db.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn decisions_filtered_by_status() {
    let conn = memory_db();

    let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
    let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();

    let s1 = upsert_suggestion(&conn, &sample_suggestion(id1, "flair", 0.9)).unwrap();
    let s2 = upsert_suggestion(&conn, &sample_suggestion(id2, "devops", 0.8)).unwrap();

    insert_decision(
        &conn,
        &ReviewDecision {
            id: 0,
            suggestion_id: s1,
            decision: "approved".to_string(),
            final_node: Some("flair".to_string()),
            final_labels: vec!["area: FLAIR".to_string()],
            decided_at: "2026-04-01T00:00:00Z".to_string(),
            applied_at: None,
        },
    )
    .unwrap();

    insert_decision(
        &conn,
        &ReviewDecision {
            id: 0,
            suggestion_id: s2,
            decision: "rejected".to_string(),
            final_node: None,
            final_labels: vec![],
            decided_at: "2026-04-01T00:00:00Z".to_string(),
            applied_at: None,
        },
    )
    .unwrap();

    // All decisions
    let filters = DecisionFilters::default();
    let results = get_decisions_filtered(&conn, &filters).unwrap();
    assert_eq!(results.len(), 2);

    // Unapplied only
    let filters = DecisionFilters {
        unapplied: true,
        ..Default::default()
    };
    let results = get_decisions_filtered(&conn, &filters).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.decision, "approved");

    // Rejected only
    let filters = DecisionFilters {
        status: Some("rejected".to_string()),
        ..Default::default()
    };
    let results = get_decisions_filtered(&conn, &filters).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1.decision, "rejected");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -E 'test(decisions_filtered_by_status)'`
Expected: FAIL — `DecisionFilters` and `get_decisions_filtered` don't exist

- [ ] **Step 3: Implement DecisionFilters and query**

In `src/triage/db.rs`, after the `SuggestionFilters` struct, add:

```rust
#[derive(Debug, Clone, Default)]
pub struct DecisionFilters {
    pub status: Option<String>,   // approved, rejected, modified, applied
    pub unapplied: bool,          // shorthand: approved+modified, applied_at IS NULL
    pub node_prefix: Option<String>,
    pub repo: Option<String>,
    pub limit: usize,             // 0 = unlimited
}
```

After `get_suggestions_filtered`, add:

```rust
pub fn get_decisions_filtered(
    conn: &Connection,
    f: &DecisionFilters,
) -> Result<Vec<(StoredIssue, ReviewDecision)>> {
    let mut sql = String::from(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count,
                rd.id, rd.suggestion_id, rd.decision, rd.final_node, rd.final_labels,
                rd.decided_at, rd.applied_at
         FROM review_decisions rd
         JOIN triage_suggestions ts ON ts.id = rd.suggestion_id
         JOIN issues i ON i.id = ts.issue_id
         WHERE 1=1",
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if f.unapplied {
        sql.push_str(" AND rd.decision IN ('approved', 'modified') AND rd.applied_at IS NULL");
    } else if let Some(ref status) = f.status {
        if status == "applied" {
            sql.push_str(" AND rd.applied_at IS NOT NULL");
        } else {
            sql.push_str(&format!(" AND rd.decision = ?{param_idx}"));
            params.push(Box::new(status.clone()));
            param_idx += 1;
        }
    }

    if let Some(ref prefix) = f.node_prefix {
        sql.push_str(&format!(
            " AND (rd.final_node = ?{p} OR rd.final_node LIKE ?{p2})",
            p = param_idx,
            p2 = param_idx + 1
        ));
        params.push(Box::new(prefix.clone()));
        params.push(Box::new(format!("{prefix}/%")));
        param_idx += 2;
    }

    if let Some(ref repo) = f.repo {
        sql.push_str(&format!(" AND i.repo = ?{param_idx}"));
        params.push(Box::new(repo.clone()));
        param_idx += 1;
    }

    let _ = param_idx;
    sql.push_str(" ORDER BY rd.decided_at DESC");

    if f.limit > 0 {
        sql.push_str(&format!(" LIMIT {}", f.limit));
    }

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(14)?;
        let final_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
        let decision = ReviewDecision {
            id: row.get(10)?,
            suggestion_id: row.get(11)?,
            decision: row.get(12)?,
            final_node: row.get(13)?,
            final_labels,
            decided_at: row.get(15)?,
            applied_at: row.get(16)?,
        };
        Ok((issue, decision))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -E 'test(decisions_filtered_by_status)'`
Expected: PASS

- [ ] **Step 5: Add Decisions variant to TriageCommands**

In `src/cli/mod.rs`, add after the Suggestions variant:

```rust
    /// List review decisions with filtering
    Decisions {
        /// Decision status: approved, rejected, modified, applied
        #[arg(long)]
        status: Option<String>,
        /// Show only unapplied approved/modified decisions
        #[arg(long)]
        unapplied: bool,
        /// Filter by final node path prefix
        #[arg(long)]
        node: Option<String>,
        /// Filter by source repo
        #[arg(long)]
        repo: Option<String>,
        /// Max rows (default 50, 0 = unlimited)
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
```

- [ ] **Step 6: Add dispatch arm and implement run_decisions()**

In `src/cli/mod.rs`, add dispatch arm:

```rust
TriageCommands::Decisions {
    status,
    unapplied,
    node,
    repo,
    limit,
    format,
} => {
    triage::run_decisions(status, unapplied, node, repo, limit, format)?;
}
```

In `src/cli/triage.rs`, add:

```rust
pub fn run_decisions(
    status: Option<String>,
    unapplied: bool,
    node: Option<String>,
    repo: Option<String>,
    limit: usize,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let filters = db::DecisionFilters {
        status,
        unapplied,
        node_prefix: node,
        repo,
        limit,
    };

    let results = db::get_decisions_filtered(&conn, &filters)?;

    if fmt == OutputFormat::Json {
        let json_rows: Vec<serde_json::Value> = results
            .iter()
            .map(|(issue, dec)| {
                serde_json::json!({
                    "issue_ref": format!("{}#{}", issue.repo, issue.number),
                    "title": issue.title,
                    "decision": dec.decision,
                    "final_node": dec.final_node,
                    "final_labels": dec.final_labels,
                    "decided_at": dec.decided_at,
                    "applied_at": dec.applied_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json_rows)
                .map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        if results.is_empty() {
            println!("No decisions match the given filters.");
            return Ok(());
        }
        println!(
            "{:<30} {:<40} {:<10} {:<25} {}",
            "ISSUE", "TITLE", "DECISION", "NODE", "APPLIED"
        );
        println!("{}", "-".repeat(115));
        for (issue, dec) in &results {
            let issue_ref = format!("{}#{}", issue.repo, issue.number);
            let title: String = issue.title.chars().take(38).collect();
            let node = dec.final_node.as_deref().unwrap_or("—");
            let applied = dec.applied_at.as_deref().unwrap_or("—");
            println!(
                "{:<30} {:<40} {:<10} {:<25} {}",
                issue_ref, title, dec.decision, node, applied
            );
        }
        println!("\n{} decision(s)", results.len());
    }
    Ok(())
}
```

- [ ] **Step 7: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 8: Commit**

```bash
git add src/triage/db.rs src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage decisions command with filtering"
```

---

### Task 6: `triage summary` — DB Queries + CLI

**Files:**
- Modify: `src/triage/db.rs` (add summary query functions)
- Modify: `src/cli/mod.rs` (add Summary variant + dispatch)
- Modify: `src/cli/triage.rs` (add run_summary)

- [ ] **Step 1: Write test for summary queries**

In `src/triage/db.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn summary_confidence_distribution_and_node_breakdown() {
    let conn = memory_db();

    let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
    let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
    let id3 = upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();
    let id4 = upsert_issue(&conn, &sample_issue("other/repo", 4)).unwrap();

    upsert_suggestion(&conn, &sample_suggestion(id1, "flair", 0.95)).unwrap();
    upsert_suggestion(&conn, &sample_suggestion(id2, "flair", 0.85)).unwrap();
    upsert_suggestion(&conn, &sample_suggestion(id3, "devops", 0.6)).unwrap();
    // null-node suggestion
    upsert_suggestion(&conn, &TriageSuggestion {
        suggested_node: None,
        suggested_new_categories: vec!["circuit/emulator".to_string()],
        ..sample_suggestion(id4, "", 0.3)
    }).unwrap();

    let dist = get_confidence_distribution(&conn, None).unwrap();
    assert_eq!(dist.len(), 5); // 5 bands always returned
    let total: usize = dist.iter().map(|d| d.count).sum();
    assert_eq!(total, 4);

    let nodes = get_node_breakdown(&conn, None).unwrap();
    assert!(nodes.len() >= 2); // flair + devops + null
    assert_eq!(nodes[0].node.as_deref(), Some("flair"));
    assert_eq!(nodes[0].count, 2);

    // Repo filter
    let dist = get_confidence_distribution(&conn, Some("owner/repo")).unwrap();
    let total: usize = dist.iter().map(|d| d.count).sum();
    assert_eq!(total, 3);

    let votes = get_new_category_votes(&conn, None).unwrap();
    assert_eq!(votes.len(), 1);
    assert_eq!(votes[0].category, "circuit/emulator");
    assert_eq!(votes[0].vote_count, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -E 'test(summary_confidence_distribution)'`
Expected: FAIL

- [ ] **Step 3: Implement summary query types and functions**

In `src/triage/db.rs`, after `DecisionFilters`, add the types:

```rust
// ---------------------------------------------------------------------------
// Summary types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfidenceBand {
    pub label: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeBreakdown {
    pub node: Option<String>,
    pub count: usize,
    pub avg_confidence: f64,
    pub min_confidence: f64,
    pub max_confidence: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryVote {
    pub category: String,
    pub vote_count: usize,
    pub issue_refs: Vec<String>,
}
```

Then add the query functions after `get_decisions_filtered`:

```rust
// ---------------------------------------------------------------------------
// Summary queries
// ---------------------------------------------------------------------------

pub fn get_confidence_distribution(
    conn: &Connection,
    repo: Option<&str>,
) -> Result<Vec<ConfidenceBand>> {
    let bands = [
        ("<0.5", 0.0, 0.5),
        ("0.5-0.7", 0.5, 0.7),
        ("0.7-0.8", 0.7, 0.8),
        ("0.8-0.9", 0.8, 0.9),
        ("0.9-1.0", 0.9, 1.01),
    ];

    let total: i64 = if let Some(repo) = repo {
        conn.query_row(
            "SELECT COUNT(*) FROM triage_suggestions ts
             JOIN issues i ON i.id = ts.issue_id WHERE i.repo = ?1",
            params![repo],
            |r| r.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM triage_suggestions", [], |r| r.get(0))?
    };

    let mut result = Vec::new();
    for (label, lo, hi) in bands {
        let count: i64 = if let Some(repo) = repo {
            conn.query_row(
                "SELECT COUNT(*) FROM triage_suggestions ts
                 JOIN issues i ON i.id = ts.issue_id
                 WHERE i.repo = ?1
                   AND COALESCE(ts.confidence, 0.0) >= ?2
                   AND COALESCE(ts.confidence, 0.0) < ?3",
                params![repo, lo, hi],
                |r| r.get(0),
            )?
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM triage_suggestions
                 WHERE COALESCE(confidence, 0.0) >= ?1
                   AND COALESCE(confidence, 0.0) < ?2",
                params![lo, hi],
                |r| r.get(0),
            )?
        };
        let pct = if total > 0 {
            count as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        result.push(ConfidenceBand {
            label: label.to_string(),
            count: count as usize,
            percentage: pct,
        });
    }
    Ok(result)
}

pub fn get_node_breakdown(
    conn: &Connection,
    repo: Option<&str>,
) -> Result<Vec<NodeBreakdown>> {
    let sql = if repo.is_some() {
        "SELECT ts.suggested_node, COUNT(*) as cnt,
                AVG(COALESCE(ts.confidence, 0.0)),
                MIN(COALESCE(ts.confidence, 0.0)),
                MAX(COALESCE(ts.confidence, 0.0))
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         WHERE i.repo = ?1
         GROUP BY ts.suggested_node
         ORDER BY cnt DESC"
    } else {
        "SELECT suggested_node, COUNT(*) as cnt,
                AVG(COALESCE(confidence, 0.0)),
                MIN(COALESCE(confidence, 0.0)),
                MAX(COALESCE(confidence, 0.0))
         FROM triage_suggestions
         GROUP BY suggested_node
         ORDER BY cnt DESC"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(repo) = repo {
        stmt.query_map(params![repo], |row| {
            Ok(NodeBreakdown {
                node: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
                avg_confidence: row.get(2)?,
                min_confidence: row.get(3)?,
                max_confidence: row.get(4)?,
            })
        })?
    } else {
        stmt.query_map([], |row| {
            Ok(NodeBreakdown {
                node: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
                avg_confidence: row.get(2)?,
                min_confidence: row.get(3)?,
                max_confidence: row.get(4)?,
            })
        })?
    };
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn get_new_category_votes(
    conn: &Connection,
    repo: Option<&str>,
) -> Result<Vec<CategoryVote>> {
    let sql = if repo.is_some() {
        "SELECT ts.suggested_new_categories, i.repo, i.number
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         WHERE ts.suggested_new_categories != '[]' AND i.repo = ?1"
    } else {
        "SELECT ts.suggested_new_categories, i.repo, i.number
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         WHERE ts.suggested_new_categories != '[]'"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<(String, String, u64)> = if let Some(repo) = repo {
        stmt.query_map(params![repo], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, u64>(2)?))
        })?
    } else {
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, u64>(2)?))
        })?
    }
    .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut votes: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (json, repo_name, number) in &rows {
        let categories: Vec<String> = serde_json::from_str(json).unwrap_or_default();
        let issue_ref = format!("{repo_name}#{number}");
        for cat in categories {
            votes.entry(cat).or_default().push(issue_ref.clone());
        }
    }

    let mut result: Vec<CategoryVote> = votes
        .into_iter()
        .map(|(category, issue_refs)| CategoryVote {
            vote_count: issue_refs.len(),
            category,
            issue_refs,
        })
        .collect();
    result.sort_by(|a, b| b.vote_count.cmp(&a.vote_count));
    Ok(result)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -E 'test(summary_confidence_distribution)'`
Expected: PASS

- [ ] **Step 5: Add Summary variant to TriageCommands + dispatch**

In `src/cli/mod.rs`, add after Status:

```rust
    /// Show classification summary (confidence distribution, node breakdown, suggested categories)
    Summary {
        /// Filter by source repo
        #[arg(long)]
        repo: Option<String>,
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
```

Add dispatch arm:

```rust
TriageCommands::Summary { repo, format } => {
    triage::run_summary(repo, format)?;
}
```

- [ ] **Step 6: Implement run_summary()**

In `src/cli/triage.rs`, add:

```rust
pub fn run_summary(repo: Option<String>, format: String) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let dist = db::get_confidence_distribution(&conn, repo.as_deref())?;
    let nodes = db::get_node_breakdown(&conn, repo.as_deref())?;
    let votes = db::get_new_category_votes(&conn, repo.as_deref())?;

    if fmt == OutputFormat::Json {
        let json = serde_json::json!({
            "confidence_distribution": dist,
            "node_breakdown": nodes,
            "suggested_new_categories": votes,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        // Confidence distribution
        println!("Confidence distribution:");
        for band in &dist {
            println!(
                "  {:<10} {:>4} ({:.1}%)",
                band.label, band.count, band.percentage
            );
        }

        // Node breakdown
        println!("\nNode breakdown:");
        println!(
            "  {:<30} {:>5} {:>8} {:>8} {:>8}",
            "NODE", "COUNT", "AVG", "MIN", "MAX"
        );
        for nb in &nodes {
            let name = nb
                .node
                .as_deref()
                .unwrap_or("(unclassified)");
            println!(
                "  {:<30} {:>5} {:>7.0}% {:>7.0}% {:>7.0}%",
                name,
                nb.count,
                nb.avg_confidence * 100.0,
                nb.min_confidence * 100.0,
                nb.max_confidence * 100.0,
            );
        }

        // Suggested new categories
        if !votes.is_empty() {
            println!("\nSuggested new categories:");
            for vote in &votes {
                let refs: Vec<&str> = vote.issue_refs.iter().take(5).map(|s| s.as_str()).collect();
                println!(
                    "  {:<30} {} vote(s)  {}",
                    vote.category,
                    vote.vote_count,
                    refs.join(", ")
                );
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 7: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 8: Commit**

```bash
git add src/triage/db.rs src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage summary command"
```

---

### Task 7: `--format json` on `triage classify`

**Files:**
- Modify: `src/cli/mod.rs` (add --format to Classify variant)
- Modify: `src/cli/triage.rs:805-837` (run_classify)

- [ ] **Step 1: Add --format to Classify variant**

In `src/cli/mod.rs`, add to the Classify variant:

```rust
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
```

- [ ] **Step 2: Update dispatch arm**

Pass `format` through in the dispatch:

```rust
TriageCommands::Classify {
    backend, model, effort, batch_size, parallel, repo, format,
} => {
    triage::run_classify(backend, model, effort, batch_size, parallel, repo, format)?;
}
```

- [ ] **Step 3: Update run_classify() to emit JSON summary**

In `src/cli/triage.rs`, update `run_classify` signature and body:

```rust
pub fn run_classify(
    backend: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    batch_size: usize,
    parallel: usize,
    repo: Option<String>,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;
    let nodes = walk_nodes(&org_root)?;
    let org_config = read_org_config(&org_root)?;
    let curated_labels = LabelsFile::read(&org_root)?;

    let config = resolve_classify_config(backend, model, effort, &org_config.triage)?;
    let count = llm::triage_issues(
        &conn,
        &nodes,
        llm::PromptCatalog {
            label_schema: &org_config.label_schema,
            curated_labels: &curated_labels,
        },
        &config,
        batch_size,
        parallel,
        repo.as_deref(),
    )?;

    let repos_cached = cache::refresh_all(&conn, &org_root)?;

    if fmt == OutputFormat::Json {
        let dist = db::get_confidence_distribution(&conn, repo.as_deref())?;
        let nodes = db::get_node_breakdown(&conn, repo.as_deref())?;
        let votes = db::get_new_category_votes(&conn, repo.as_deref())?;
        let null_count = nodes
            .iter()
            .find(|n| n.node.is_none())
            .map(|n| n.count)
            .unwrap_or(0);

        let json = serde_json::json!({
            "classified": count,
            "confidence_distribution": dist,
            "top_nodes": nodes.iter().filter(|n| n.node.is_some()).take(20).collect::<Vec<_>>(),
            "null_node_count": null_count,
            "suggested_new_categories": votes,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        println!("Classified {count} issues");
        println!("Issue cache refreshed ({repos_cached} repos)");
    }
    Ok(())
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add --format json to triage classify"
```

---

### Task 8: Streaming DB Writes in Classify

**Files:**
- Modify: `src/triage/llm.rs:950-1250` (triage_issues refactor)

This is the most complex task. The key change: wrap `Connection` in `Arc<Mutex<>>`, write to DB inside each worker loop iteration, remove the post-loop `all_results` write.

- [ ] **Step 1: Refactor triage_issues() — change conn to Arc<Mutex<Connection>>**

In `src/triage/llm.rs`, change the `triage_issues()` signature. The `conn` parameter stays as `&Connection` for the initial issue fetch, but we create an `Arc<Mutex<Connection>>` by opening a second connection for writes. Actually — `rusqlite::Connection` can't be cloned, and we already have it. The simplest approach: move the connection into an Arc<Mutex<>> after the initial read.

Replace the function body from the worker setup through the end. The key changes are:

1. After fetching issues and building work items, wrap `conn` usage:

Replace the lines from `let queue = Arc::new(...)` through the worker thread spawns and the post-join result-writing section. The new version:

```rust
    // After building `items` (around line 1056), replace everything through end of function:

    let queue = Arc::new(Mutex::new(items));
    let err_count = Arc::new(Mutex::new(0usize));
    let classified_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let config = Arc::new(config.clone());
    let backend_name = Arc::new(backend_name);
    let now = Arc::new(now);
    let main_pb = Arc::new(main_pb);
    let total_issues = total_issues as u64;

    // Valid node set for validation inside workers
    let valid_nodes: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.path.clone()).collect();
    let valid_nodes = Arc::new(valid_nodes);

    // Open a second connection for writes (same DB, WAL mode allows concurrent readers+writer)
    let db_path = conn.path().unwrap().to_string();
    let write_conn = Arc::new(Mutex::new(db::open_db_from_path(
        std::path::Path::new(&db_path),
    )?));

    set_terminal_progress(1, 0);
    set_terminal_title(&format!("armitage classify: 0/{total_issues}"));

    let num_workers = parallel;
    let mut handles = Vec::new();

    for worker_id in 0..num_workers {
        let queue = Arc::clone(&queue);
        let config = Arc::clone(&config);
        let err_count = Arc::clone(&err_count);
        let bn = Arc::clone(&backend_name);
        let now = Arc::clone(&now);
        let main_pb = Arc::clone(&main_pb);
        let valid_nodes = Arc::clone(&valid_nodes);
        let write_conn = Arc::clone(&write_conn);
        let classified_count = Arc::clone(&classified_count);
        let worker_pb = if worker_id < worker_pbs.len() {
            Some(worker_pbs[worker_id].clone())
        } else {
            None
        };

        handles.push(thread::spawn(move || {
            SUPPRESS_SPINNER.with(|s| s.set(true));
            loop {
                let wi = { queue.lock().unwrap().pop() };
                let Some(wi) = wi else { break };

                let desc = if wi.is_batch {
                    format!("{} (+{} more)", wi.issue_refs[0], wi.issue_refs.len() - 1)
                } else {
                    format!("{} {}", wi.issue_refs[0], truncate(&wi.issue_titles[0], 40))
                };

                if let Some(ref pb) = worker_pb {
                    pb.set_message(desc.clone());
                }

                let llm_result = invoke_llm(&config, &wi.prompt);
                match llm_result {
                    Ok(raw) => {
                        let parsed = if wi.is_batch {
                            parse_batch_classifications(&raw)
                        } else {
                            parse_classification(&raw).map(|c| vec![c])
                        };
                        match parsed {
                            Ok(cs) => {
                                let conn_guard = write_conn.lock().unwrap();
                                for (i, c) in cs.into_iter().enumerate() {
                                    if i < wi.issue_ids.len() {
                                        let validated_node =
                                            c.suggested_node.as_deref().and_then(|n| {
                                                if valid_nodes.contains(n) {
                                                    Some(n.to_string())
                                                } else {
                                                    main_pb.println(format!(
                                                        "  WARN {}: non-existent node '{}', set to null",
                                                        wi.issue_refs[i], n,
                                                    ));
                                                    None
                                                }
                                            });
                                        let node_str =
                                            validated_node.as_deref().unwrap_or("none");
                                        let conf = c.confidence * 100.0;
                                        let title = truncate(&wi.issue_titles[i], 50);
                                        main_pb.println(format!(
                                            "  {} {title} -> {node_str} ({conf:.0}%)",
                                            wi.issue_refs[i],
                                        ));

                                        let sug = db::TriageSuggestion {
                                            id: 0,
                                            issue_id: wi.issue_ids[i],
                                            suggested_node: validated_node,
                                            suggested_labels: c.suggested_labels.clone(),
                                            confidence: Some(c.confidence),
                                            reasoning: c.reasoning.clone(),
                                            llm_backend: bn.to_string(),
                                            created_at: now.to_string(),
                                            is_tracking_issue: c.is_tracking_issue,
                                            suggested_new_categories: c
                                                .suggested_new_categories
                                                .clone(),
                                        };
                                        if let Err(e) =
                                            db::upsert_suggestion(&conn_guard, &sug)
                                        {
                                            main_pb.println(format!(
                                                "  ERROR {}: DB write: {e}",
                                                wi.issue_refs[i]
                                            ));
                                        } else {
                                            classified_count.fetch_add(
                                                1,
                                                std::sync::atomic::Ordering::Relaxed,
                                            );
                                        }
                                        main_pb.inc(1);
                                        let pct =
                                            (main_pb.position() * 100 / total_issues) as u8;
                                        set_terminal_progress(1, pct);
                                        set_terminal_title(&format!(
                                            "armitage classify: {}/{total_issues}",
                                            main_pb.position()
                                        ));
                                    }
                                }
                                drop(conn_guard);
                            }
                            Err(e) => {
                                main_pb.println(format!("  ERROR {desc}: parse error: {e}"));
                                *err_count.lock().unwrap() += 1;
                                main_pb.inc(wi.issue_ids.len() as u64);
                                set_terminal_progress(
                                    2,
                                    (main_pb.position() * 100 / total_issues) as u8,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        main_pb.println(format!("  ERROR {desc}: {e}"));
                        *err_count.lock().unwrap() += 1;
                        main_pb.inc(wi.issue_ids.len() as u64);
                        set_terminal_progress(
                            2,
                            (main_pb.position() * 100 / total_issues) as u8,
                        );
                    }
                }

                if let Some(ref pb) = worker_pb {
                    pb.set_message("idle");
                }
            }
            if let Some(ref pb) = worker_pb {
                pb.finish_and_clear();
            }
        }));
    }

    for h in handles {
        h.join().expect("worker thread panicked");
    }

    main_pb.finish_with_message("done");
    clear_terminal_status();

    let errors = *err_count.lock().unwrap();
    let classified = classified_count.load(std::sync::atomic::Ordering::Relaxed);

    if errors > 0 {
        eprintln!("{errors} issue(s) failed to classify.");
    }

    // Post-run: collect new category votes from DB for summary output
    let category_votes = db::get_new_category_votes(conn, None)?;
    if !category_votes.is_empty() {
        println!("\n--- Suggested new categories ---");
        for vote in &category_votes {
            println!("  {} ({} issue(s))", vote.category, vote.vote_count);
            for issue_ref in vote.issue_refs.iter().take(5) {
                println!("    - {issue_ref}");
            }
        }
        println!("To create a new node: armitage node new <path> --name \"...\" --description \"...\"");
    }

    Ok(classified)
```

- [ ] **Step 2: Add `open_db_from_path` helper to db.rs**

In `src/triage/db.rs`, after `open_db`, add:

```rust
/// Open a DB connection from an explicit path (for second connections in WAL mode).
pub fn open_db_from_path(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    // Don't migrate — the primary connection already did.
    Ok(conn)
}
```

- [ ] **Step 3: Build and run existing tests**

Run: `cargo build && cargo nextest run`
Expected: Build succeeds, all existing tests pass

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: Clean (fix any warnings)

- [ ] **Step 5: Commit**

```bash
git add src/triage/llm.rs src/triage/db.rs
git commit -m "refactor: stream classify results to DB instead of batching in memory"
```

---

### Task 9: `triage categories` — Dismissed Categories Module

**Files:**
- Create: `src/triage/categories.rs`
- Modify: `src/triage/mod.rs` (add `pub mod categories;`)

- [ ] **Step 1: Write test for dismissed categories read/write**

Create `src/triage/categories.rs` with:

```rust
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DismissedCategories {
    #[serde(default)]
    pub dismissed: Vec<String>,
}

const DISMISSED_FILE: &str = ".armitage/dismissed-categories.toml";

pub fn read_dismissed(org_root: &Path) -> Result<DismissedCategories> {
    let path = org_root.join(DISMISSED_FILE);
    if !path.exists() {
        return Ok(DismissedCategories::default());
    }
    let content = std::fs::read_to_string(&path)?;
    toml::from_str(&content).map_err(|e| Error::Other(format!("parse {DISMISSED_FILE}: {e}")))
}

pub fn write_dismissed(org_root: &Path, dc: &DismissedCategories) -> Result<()> {
    let path = org_root.join(DISMISSED_FILE);
    let content = toml::to_string(dc).map_err(|e| Error::Other(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn is_dismissed(dc: &DismissedCategories, category: &str) -> bool {
    dc.dismissed.iter().any(|d| d == category)
}

pub fn dismiss(org_root: &Path, category: &str) -> Result<bool> {
    let mut dc = read_dismissed(org_root)?;
    if is_dismissed(&dc, category) {
        return Ok(false); // already dismissed
    }
    dc.dismissed.push(category.to_string());
    dc.dismissed.sort();
    write_dismissed(org_root, &dc)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_write_dismissed_categories() {
        let tmp = TempDir::new().unwrap();
        let org = tmp.path();
        std::fs::create_dir_all(org.join(".armitage")).unwrap();

        // Empty initially
        let dc = read_dismissed(org).unwrap();
        assert!(dc.dismissed.is_empty());

        // Dismiss one
        let was_new = dismiss(org, "circuit/emulator").unwrap();
        assert!(was_new);

        // Read back
        let dc = read_dismissed(org).unwrap();
        assert_eq!(dc.dismissed, vec!["circuit/emulator"]);

        // Dismiss same one again — no-op
        let was_new = dismiss(org, "circuit/emulator").unwrap();
        assert!(!was_new);

        // Dismiss another
        dismiss(org, "docs/tutorials").unwrap();
        let dc = read_dismissed(org).unwrap();
        assert_eq!(dc.dismissed.len(), 2);
        assert!(is_dismissed(&dc, "circuit/emulator"));
        assert!(is_dismissed(&dc, "docs/tutorials"));
        assert!(!is_dismissed(&dc, "other"));
    }
}
```

- [ ] **Step 2: Add module to mod.rs**

In `src/triage/mod.rs`, add:

```rust
pub mod categories;
```

- [ ] **Step 3: Run test**

Run: `cargo nextest run -E 'test(read_write_dismissed_categories)'`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/triage/categories.rs src/triage/mod.rs
git commit -m "feat: add dismissed categories module"
```

---

### Task 10: `triage categories list/dismiss` — CLI

**Files:**
- Modify: `src/cli/mod.rs` (add Categories variant + TriageCategoryCommands)
- Modify: `src/cli/triage.rs` (add run_categories_list, run_categories_dismiss)

- [ ] **Step 1: Add TriageCategoryCommands enum and Categories variant**

In `src/cli/mod.rs`, after the `TriageLabelCommands` enum, add:

```rust
#[derive(Subcommand)]
enum TriageCategoryCommands {
    /// List suggested new categories from classification
    List {
        /// Minimum vote count to show (default: 1)
        #[arg(long, default_value_t = 1)]
        min_votes: usize,
        /// Output format: "table" (default) or "json"
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Create a node from a suggested category and reset for reclassification
    Apply {
        /// Category path (e.g. "circuit/emulator")
        path: String,
        /// Display name (required)
        #[arg(long)]
        name: String,
        /// Description (required)
        #[arg(long)]
        description: String,
        /// Immediately reclassify affected issues
        #[arg(long)]
        reclassify: bool,
        /// LLM backend for reclassification
        #[arg(long)]
        reclassify_backend: Option<String>,
        /// Model for reclassification
        #[arg(long)]
        reclassify_model: Option<String>,
    },
    /// Dismiss a suggested category so it no longer appears in listings
    Dismiss {
        /// Category path to dismiss
        path: String,
    },
}
```

Add to `TriageCommands`:

```rust
    /// Manage suggested new categories
    Categories {
        #[command(subcommand)]
        command: TriageCategoryCommands,
    },
```

- [ ] **Step 2: Add dispatch arm**

```rust
TriageCommands::Categories { command } => match command {
    TriageCategoryCommands::List { min_votes, format } => {
        triage::run_categories_list(min_votes, format)?;
    }
    TriageCategoryCommands::Apply {
        path,
        name,
        description,
        reclassify,
        reclassify_backend,
        reclassify_model,
    } => {
        triage::run_categories_apply(
            path,
            name,
            description,
            reclassify,
            reclassify_backend,
            reclassify_model,
        )?;
    }
    TriageCategoryCommands::Dismiss { path } => {
        triage::run_categories_dismiss(path)?;
    }
},
```

- [ ] **Step 3: Implement run_categories_list()**

In `src/cli/triage.rs`, add:

```rust
pub fn run_categories_list(min_votes: usize, format: String) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    let dismissed = crate::triage::categories::read_dismissed(&org_root)?;
    let mut votes = db::get_new_category_votes(&conn, None)?;
    votes.retain(|v| {
        !crate::triage::categories::is_dismissed(&dismissed, &v.category)
            && v.vote_count >= min_votes
    });

    if fmt == OutputFormat::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&votes)
                .map_err(|e| Error::Other(e.to_string()))?
        );
    } else {
        if votes.is_empty() {
            println!("No suggested new categories.");
            return Ok(());
        }
        println!("Suggested categories:");
        for vote in &votes {
            let refs: Vec<&str> = vote.issue_refs.iter().take(5).map(|s| s.as_str()).collect();
            println!(
                "  {:<30} {} vote(s)  {}",
                vote.category,
                vote.vote_count,
                refs.join(", ")
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Implement run_categories_dismiss()**

```rust
pub fn run_categories_dismiss(path: String) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let was_new = crate::triage::categories::dismiss(&org_root, &path)?;
    if was_new {
        println!("Dismissed category '{path}'");
    } else {
        println!("Category '{path}' was already dismissed");
    }
    Ok(())
}
```

- [ ] **Step 5: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 6: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage categories list and dismiss commands"
```

---

### Task 11: `triage categories apply` — Node Creation + Reclassify

**Files:**
- Modify: `src/cli/node.rs:730` (make create_node_full pub(crate))
- Modify: `src/triage/db.rs` (add delete_suggestions_for_reclassify)
- Modify: `src/cli/triage.rs` (implement run_categories_apply)

- [ ] **Step 1: Write test for delete_suggestions_for_reclassify**

In `src/triage/db.rs` `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn delete_suggestions_for_reclassify() {
    let conn = memory_db();

    let id1 = upsert_issue(&conn, &sample_issue("owner/repo", 1)).unwrap();
    let id2 = upsert_issue(&conn, &sample_issue("owner/repo", 2)).unwrap();
    let id3 = upsert_issue(&conn, &sample_issue("owner/repo", 3)).unwrap();
    let id4 = upsert_issue(&conn, &sample_issue("owner/repo", 4)).unwrap();

    // id1: null node (should be reset)
    upsert_suggestion(&conn, &TriageSuggestion {
        suggested_node: None,
        ..sample_suggestion(id1, "", 0.3)
    }).unwrap();

    // id2: has the target category in suggestions (should be reset)
    upsert_suggestion(&conn, &TriageSuggestion {
        suggested_new_categories: vec!["circuit/emulator".to_string()],
        ..sample_suggestion(id2, "circuit", 0.7)
    }).unwrap();

    // id3: classified normally, no category vote (should NOT be reset)
    upsert_suggestion(&conn, &sample_suggestion(id3, "devops", 0.9)).unwrap();

    // id4: null node AND voted for category (should be reset, counted once)
    upsert_suggestion(&conn, &TriageSuggestion {
        suggested_node: None,
        suggested_new_categories: vec!["circuit/emulator".to_string()],
        ..sample_suggestion(id4, "", 0.25)
    }).unwrap();

    let deleted = super::delete_suggestions_for_reclassify(&conn, "circuit/emulator").unwrap();
    assert_eq!(deleted, 3); // id1, id2, id4

    // id3 should still exist
    let remaining = get_suggestions_filtered(&conn, &SuggestionFilters::default()).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].1.suggested_node.as_deref(), Some("devops"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -E 'test(delete_suggestions_for_reclassify)'`
Expected: FAIL

- [ ] **Step 3: Implement delete_suggestions_for_reclassify**

In `src/triage/db.rs`, after `delete_all_suggestions`, add:

```rust
/// Delete suggestions that are candidates for reclassification after a new category is created:
/// - All null-node suggestions
/// - All suggestions that voted for the given category in suggested_new_categories
/// Also deletes associated review decisions. Returns count deleted.
pub fn delete_suggestions_for_reclassify(conn: &Connection, category: &str) -> Result<usize> {
    // Find suggestion IDs to delete
    let category_pattern = format!("%\"{category}\"%");
    conn.execute(
        "DELETE FROM review_decisions WHERE suggestion_id IN (
            SELECT id FROM triage_suggestions
            WHERE suggested_node IS NULL
               OR suggested_new_categories LIKE ?1
        )",
        params![category_pattern],
    )?;
    let deleted = conn.execute(
        "DELETE FROM triage_suggestions
         WHERE suggested_node IS NULL
            OR suggested_new_categories LIKE ?1",
        params![category_pattern],
    )?;
    Ok(deleted)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -E 'test(delete_suggestions_for_reclassify)'`
Expected: PASS

- [ ] **Step 5: Make create_node_full pub(crate)**

In `src/cli/node.rs`, change line 730 from:

```rust
fn create_node_full(
```

to:

```rust
pub(crate) fn create_node_full(
```

- [ ] **Step 6: Implement run_categories_apply()**

In `src/cli/triage.rs`, add:

```rust
pub fn run_categories_apply(
    path: String,
    name: String,
    description: String,
    reclassify: bool,
    reclassify_backend: Option<String>,
    reclassify_model: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    // Create the node
    crate::cli::node::create_node_full(
        &org_root,
        &path,
        Some(&name),
        Some(&description),
        None,  // github_issue
        None,  // labels
        &[],   // repos
        "active",
        None,  // timeline
    )?;

    // Reset suggestions for reclassification
    let deleted = db::delete_suggestions_for_reclassify(&conn, &path)?;
    println!("Created node '{path}'. Reset {deleted} suggestion(s).");

    if reclassify && deleted > 0 {
        let nodes = walk_nodes(&org_root)?;
        let org_config = read_org_config(&org_root)?;
        let curated_labels = LabelsFile::read(&org_root)?;
        let config = resolve_classify_config(
            reclassify_backend,
            reclassify_model,
            None,
            &org_config.triage,
        )?;
        let count = llm::triage_issues(
            &conn,
            &nodes,
            llm::PromptCatalog {
                label_schema: &org_config.label_schema,
                curated_labels: &curated_labels,
            },
            &config,
            10,
            1,
            None,
        )?;
        println!("Reclassified {count} issues.");
        cache::refresh_all(&conn, &org_root)?;
    } else if deleted > 0 {
        println!("Run 'armitage triage classify' to reclassify affected issues.");
        cache::refresh_all(&conn, &org_root)?;
    }
    Ok(())
}
```

- [ ] **Step 7: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 8: Run all tests**

Run: `cargo nextest run`
Expected: All pass

- [ ] **Step 9: Commit**

```bash
git add src/triage/db.rs src/cli/node.rs src/cli/triage.rs
git commit -m "feat: add triage categories apply with node creation and reclassify"
```

---

### Task 12: `--format json` on `triage review --list`

**Files:**
- Modify: `src/cli/mod.rs` (add --format to Review variant)
- Modify: `src/cli/triage.rs` (pass format to run_review)
- Modify: `src/triage/review.rs` (update review_list to accept format)

- [ ] **Step 1: Add --format to Review variant**

In `src/cli/mod.rs`, add to the Review variant:

```rust
        /// Output format: "table" (default) or "json" (only used with --list)
        #[arg(long, default_value = "table")]
        format: String,
```

- [ ] **Step 2: Update dispatch arm**

Pass `format` through to `run_review`:

```rust
TriageCommands::Review {
    interactive, list, auto_approve, min_confidence, max_confidence, format,
} => {
    triage::run_review(
        interactive, list, auto_approve, min_confidence, max_confidence, format,
    )?;
}
```

- [ ] **Step 3: Update run_review() and review_list()**

In `src/cli/triage.rs`, update `run_review` to pass format:

```rust
pub fn run_review(
    interactive: bool,
    list: bool,
    auto_approve: Option<f64>,
    min_confidence: Option<f64>,
    max_confidence: Option<f64>,
    format: String,
) -> Result<()> {
    let fmt = OutputFormat::parse(&format)?;
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    if list || (!interactive && auto_approve.is_none()) {
        if fmt == OutputFormat::Json {
            let suggestions =
                db::get_pending_suggestions_filtered(&conn, min_confidence, max_confidence)?;
            let json_rows: Vec<serde_json::Value> = suggestions
                .iter()
                .map(|(issue, sug)| {
                    serde_json::json!({
                        "issue_ref": format!("{}#{}", issue.repo, issue.number),
                        "title": issue.title,
                        "suggested_node": sug.suggested_node,
                        "suggested_labels": sug.suggested_labels,
                        "confidence": sug.confidence,
                        "reasoning": sug.reasoning,
                        "is_tracking_issue": sug.is_tracking_issue,
                        "suggested_new_categories": sug.suggested_new_categories,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json_rows)
                    .map_err(|e| Error::Other(e.to_string()))?
            );
        } else {
            review::review_list(&conn, min_confidence, max_confidence)?;
        }
    } else if let Some(threshold) = auto_approve {
        let stats = review::review_auto_approve(&conn, threshold)?;
        println!("Approved: {}, Skipped: {}", stats.approved, stats.skipped);
    } else if interactive {
        let stats = review::review_interactive(&conn, &org_root, min_confidence, max_confidence)?;
        println!(
            "Approved: {}, Rejected: {}, Modified: {}, Skipped: {}",
            stats.approved, stats.rejected, stats.modified, stats.skipped
        );
    }
    Ok(())
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add --format json to triage review --list"
```

---

### Task 13: Final Verification

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo nextest run`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --all-targets --all-features -- -D warnings`
Expected: Clean

- [ ] **Step 3: Run format check**

Run: `cargo fmt --all -- --check`
Expected: Clean

- [ ] **Step 4: Verify against test org (build + smoke test)**

Run:

```bash
cargo build
cd <test-org-dir>
../target/debug/armitage triage status --format json
../target/debug/armitage triage summary
../target/debug/armitage triage suggestions --limit 10
../target/debug/armitage triage suggestions --unclassified --format json
../target/debug/armitage triage decisions --limit 5
../target/debug/armitage triage categories list
```

Expected: All commands produce sensible output matching the DB state from the earlier classify run.

- [ ] **Step 5: Commit any final fixups**

If smoke testing revealed issues, fix and commit.
