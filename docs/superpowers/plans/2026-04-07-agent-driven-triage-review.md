# Agent-Driven Triage Review Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable an agent (Claude Code) to drive the triage review loop programmatically — auto-approving obvious classifications, presenting uncertain ones to the user, and adapting batch sizes based on correction rates.

**Architecture:** Three changes: (1) a new `triage decide` CLI subcommand for non-interactive single-issue decision submission, (2) enriched `triage suggestions --format json` output with issue body, current labels, and suggestion ID, (3) a SKILL.md workflow section teaching the agent the adaptive review loop.

**Tech Stack:** Rust (clap, rusqlite, serde_json), SKILL.md (prompt)

---

### Task 1: Add `db::get_suggestion_by_issue()` with test

**Files:**
- Modify: `src/triage/db.rs`

- [ ] **Step 1: Write the failing test**

Add at the end of the `mod tests` block in `src/triage/db.rs`:

```rust
#[test]
fn get_suggestion_by_issue_test() {
    let conn = memory_db();

    // No suggestion → None
    let result = get_suggestion_by_issue(&conn, "owner/repo", 999).unwrap();
    assert!(result.is_none());

    // Insert issue + suggestion, no decision
    let issue = sample_issue("owner/repo", 42);
    let issue_id = upsert_issue(&conn, &issue).unwrap();
    let sug = sample_suggestion(issue_id, "some/node", 0.85);
    upsert_suggestion(&conn, &sug).unwrap();

    let result = get_suggestion_by_issue(&conn, "owner/repo", 42).unwrap();
    assert!(result.is_some());
    let (i, s, d) = result.unwrap();
    assert_eq!(i.number, 42);
    assert_eq!(s.suggested_node, Some("some/node".to_string()));
    assert!(d.is_none()); // no decision yet

    // Add a decision → should appear
    insert_decision(
        &conn,
        &ReviewDecision {
            id: 0,
            suggestion_id: s.id,
            decision: "approved".to_string(),
            final_node: Some("some/node".to_string()),
            final_labels: vec![],
            decided_at: "2026-04-07T00:00:00Z".to_string(),
            applied_at: None,
        },
    )
    .unwrap();

    let (_, _, d) = get_suggestion_by_issue(&conn, "owner/repo", 42)
        .unwrap()
        .unwrap();
    assert!(d.is_some());
    assert_eq!(d.unwrap().decision, "approved");
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo nextest run -E 'test(get_suggestion_by_issue_test)'`
Expected: compilation error — `get_suggestion_by_issue` does not exist yet.

- [ ] **Step 3: Implement `get_suggestion_by_issue`**

Add this function in `src/triage/db.rs` after `delete_decision_by_suggestion_id` (around line 635), in the "Review Decision CRUD" section:

```rust
/// Look up a single issue's suggestion and optional decision by repo and issue number.
/// Returns `None` if the issue has no suggestion.
pub fn get_suggestion_by_issue(
    conn: &Connection,
    repo: &str,
    number: u64,
) -> Result<Option<(StoredIssue, TriageSuggestion, Option<ReviewDecision>)>> {
    let number = number as i64;
    let mut stmt = conn.prepare(
        "SELECT i.id, i.repo, i.number, i.title, i.body, i.state, i.labels_json,
                i.updated_at, i.fetched_at, i.sub_issues_count,
                ts.id, ts.issue_id, ts.suggested_node, ts.suggested_labels,
                ts.confidence, ts.reasoning, ts.llm_backend, ts.created_at,
                ts.is_tracking_issue, ts.suggested_new_categories, ts.is_stale,
                rd.id, rd.suggestion_id, rd.decision, rd.final_node, rd.final_labels,
                rd.decided_at, rd.applied_at
         FROM triage_suggestions ts
         JOIN issues i ON i.id = ts.issue_id
         LEFT JOIN review_decisions rd ON rd.suggestion_id = ts.id
         WHERE i.repo = ?1 AND i.number = ?2",
    )?;

    let mut rows = stmt.query_map(params![repo, number], |row| {
        let issue = row_to_issue(row)?;
        let labels_json: String = row.get(13)?;
        let suggested_labels: Vec<String> = serde_json::from_str(&labels_json).unwrap_or_default();
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
            is_stale: row.get::<_, i64>(20)? != 0,
        };

        let decision = if row.get::<_, Option<i64>>(21)?.is_some() {
            let final_labels_json: String = row.get(25)?;
            Some(ReviewDecision {
                id: row.get(21)?,
                suggestion_id: row.get(22)?,
                decision: row.get(23)?,
                final_node: row.get(24)?,
                final_labels: serde_json::from_str(&final_labels_json).unwrap_or_default(),
                decided_at: row.get(26)?,
                applied_at: row.get(27)?,
            })
        } else {
            None
        };

        Ok((issue, suggestion, decision))
    })?;

    match rows.next() {
        Some(Ok(row)) => Ok(Some(row)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo nextest run -E 'test(get_suggestion_by_issue_test)'`
Expected: PASS

- [ ] **Step 5: Run full test suite and lint**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run`
Expected: all 155 tests pass, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/triage/db.rs
git commit -m "feat: add get_suggestion_by_issue DB lookup for triage decide"
```

---

### Task 2: Add `triage decide` subcommand

**Files:**
- Modify: `src/cli/mod.rs` (add `Decide` variant + dispatch)
- Modify: `src/cli/triage.rs` (add `run_decide()`)

- [ ] **Step 1: Add the `Decide` variant to `TriageCommands`**

In `src/cli/mod.rs`, add this variant after the `Apply` variant (around line 265), before the `Reset` variant:

```rust
    /// Submit a review decision for a single issue (non-interactive)
    Decide {
        /// Issue reference (owner/repo#number)
        issue_ref: String,
        /// Decision: approve, reject, modify, or stale
        #[arg(long)]
        decision: String,
        /// Override the suggested node (only with --decision modify)
        #[arg(long)]
        node: Option<String>,
        /// Override the suggested labels, comma-separated (only with --decision modify)
        #[arg(long)]
        labels: Option<String>,
        /// Optional note explaining the decision
        #[arg(long)]
        note: Option<String>,
    },
```

- [ ] **Step 2: Add the dispatch arm**

In `src/cli/mod.rs`, add this match arm after the `Apply` dispatch (around line 731), before the `Reset` arm:

```rust
            TriageCommands::Decide {
                issue_ref,
                decision,
                node,
                labels,
                note,
            } => {
                triage::run_decide(issue_ref, decision, node, labels, note)?;
            }
```

- [ ] **Step 3: Implement `run_decide()`**

In `src/cli/triage.rs`, add this function after `run_apply()` (around line 1093):

```rust
pub fn run_decide(
    issue_ref_str: String,
    decision: String,
    node: Option<String>,
    labels: Option<String>,
    note: Option<String>,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;

    // Parse and validate the decision type
    let decision_type = match decision.as_str() {
        "approve" | "reject" | "modify" | "stale" => decision.as_str(),
        other => {
            return Err(Error::Other(format!(
                "unknown decision '{other}', expected: approve, reject, modify, stale"
            )));
        }
    };

    // Validate --node/--labels only used with modify
    if decision_type != "modify" && (node.is_some() || labels.is_some()) {
        return Err(Error::Other(format!(
            "--node and --labels can only be used with --decision modify"
        )));
    }

    // Look up the suggestion
    let issue_ref = crate::model::node::IssueRef::parse(&issue_ref_str)?;
    let (issue, suggestion, existing_decision) =
        db::get_suggestion_by_issue(&conn, &issue_ref.repo_full(), issue_ref.number)?
            .ok_or_else(|| {
                Error::Other(format!("No suggestion found for {issue_ref_str}"))
            })?;

    // Prevent overwriting an already-applied decision
    if let Some(ref d) = existing_decision {
        if d.applied_at.is_some() {
            return Err(Error::Other(format!(
                "Decision for {issue_ref_str} has already been applied to GitHub. \
                 Use `triage reset --issue {issue_ref_str}` to clear it first."
            )));
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    let note = note.unwrap_or_default();

    match decision_type {
        "approve" => {
            db::insert_decision(
                &conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "approved".to_string(),
                    final_node: suggestion.suggested_node.clone(),
                    final_labels: suggestion.suggested_labels.clone(),
                    decided_at: now,
                    applied_at: None,
                },
            )?;
            println!("Approved {issue_ref_str}");
        }
        "reject" => {
            db::insert_decision(
                &conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "rejected".to_string(),
                    final_node: None,
                    final_labels: vec![],
                    decided_at: now,
                    applied_at: None,
                },
            )?;
            save_decide_example(&org_root, &issue, &suggestion, None, &[], false, &note);
            println!("Rejected {issue_ref_str}");
        }
        "modify" => {
            let final_node = node.or_else(|| suggestion.suggested_node.clone());
            let final_labels = labels
                .map(|l| {
                    l.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_else(|| suggestion.suggested_labels.clone());
            db::insert_decision(
                &conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "modified".to_string(),
                    final_node: final_node.clone(),
                    final_labels: final_labels.clone(),
                    decided_at: now,
                    applied_at: None,
                },
            )?;
            save_decide_example(
                &org_root,
                &issue,
                &suggestion,
                final_node.as_deref(),
                &final_labels,
                suggestion.is_stale,
                &note,
            );
            println!("Modified {issue_ref_str}");
        }
        "stale" => {
            db::insert_decision(
                &conn,
                &db::ReviewDecision {
                    id: 0,
                    suggestion_id: suggestion.id,
                    decision: "rejected".to_string(),
                    final_node: None,
                    final_labels: vec![],
                    decided_at: now,
                    applied_at: None,
                },
            )?;
            save_decide_example(&org_root, &issue, &suggestion, None, &[], true, &note);
            println!("Marked {issue_ref_str} as stale");
        }
        _ => unreachable!(),
    }

    Ok(())
}

/// Save a classification example from a decide command. Same logic as
/// `review::save_review_example` but usable without the review module's
/// private helpers.
fn save_decide_example(
    org_root: &std::path::Path,
    issue: &db::StoredIssue,
    suggestion: &db::TriageSuggestion,
    final_node: Option<&str>,
    final_labels: &[String],
    is_stale: bool,
    note: &str,
) {
    use crate::triage::examples::{self, TriageExample};

    let body_excerpt = if issue.body.len() > 300 {
        let mut end = 300;
        while !issue.body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &issue.body[..end])
    } else {
        issue.body.clone()
    };

    let example = TriageExample {
        issue_ref: format!("{}#{}", issue.repo, issue.number),
        title: issue.title.clone(),
        body_excerpt,
        original_node: suggestion.suggested_node.clone(),
        node: final_node.map(String::from),
        labels: final_labels.to_vec(),
        is_tracking_issue: suggestion.is_tracking_issue,
        is_stale,
        note: note.to_string(),
    };

    if let Err(e) = examples::append_example(org_root, example) {
        eprintln!("(warning: failed to save example: {e})");
    }
}
```

- [ ] **Step 4: Verify it compiles and all tests pass**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run`
Expected: all tests pass, no warnings.

- [ ] **Step 5: Verify help text**

Run: `cargo run -- triage decide --help`
Expected output should show the positional `issue_ref` and all optional flags.

- [ ] **Step 6: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage decide command for non-interactive review decisions"
```

---

### Task 3: Enrich `triage suggestions --format json`

**Files:**
- Modify: `src/cli/mod.rs` (add `body_max` to `Suggestions`)
- Modify: `src/cli/triage.rs` (add fields to JSON serialization + `body_max` param)

- [ ] **Step 1: Add `--body-max` flag to `Suggestions` variant**

In `src/cli/mod.rs`, add this field to the `Suggestions` variant, after the `format` field (around line 357):

```rust
        /// Truncate issue body in JSON output (default: 500 chars, 0 = unlimited)
        #[arg(long, default_value_t = 500)]
        body_max: usize,
```

- [ ] **Step 2: Update the dispatch to pass `body_max`**

In `src/cli/mod.rs`, update the `Suggestions` dispatch arm (around line 801) to capture and pass `body_max`:

```rust
            TriageCommands::Suggestions {
                node,
                repo,
                min_confidence,
                max_confidence,
                status,
                tracking_only,
                unclassified,
                stale_only,
                sort,
                limit,
                format,
                body_max,
            } => {
                triage::run_suggestions(
                    node,
                    repo,
                    min_confidence,
                    max_confidence,
                    status,
                    tracking_only,
                    unclassified,
                    stale_only,
                    sort,
                    limit,
                    format,
                    body_max,
                )?;
            }
```

- [ ] **Step 3: Update `run_suggestions` to accept `body_max` and enrich JSON**

In `src/cli/triage.rs`, update the `run_suggestions` function signature to add `body_max: usize` as the last parameter.

Then replace the JSON serialization block (the `if fmt == OutputFormat::Json` branch, around lines 1152-1175) with:

```rust
    if fmt == OutputFormat::Json {
        let json_rows: Vec<serde_json::Value> = results
            .iter()
            .map(|(issue, sug)| {
                let body = if body_max > 0 && issue.body.len() > body_max {
                    let mut end = body_max;
                    while !issue.body.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &issue.body[..end])
                } else {
                    issue.body.clone()
                };
                serde_json::json!({
                    "suggestion_id": sug.id,
                    "issue_ref": format!("{}#{}", issue.repo, issue.number),
                    "title": issue.title,
                    "repo": issue.repo,
                    "number": issue.number,
                    "body": body,
                    "current_labels": issue.labels,
                    "sub_issues_count": issue.sub_issues_count,
                    "suggested_node": sug.suggested_node,
                    "suggested_labels": sug.suggested_labels,
                    "confidence": sug.confidence,
                    "reasoning": sug.reasoning,
                    "is_tracking_issue": sug.is_tracking_issue,
                    "is_stale": sug.is_stale,
                    "suggested_new_categories": sug.suggested_new_categories,
                    "llm_backend": sug.llm_backend,
                    "created_at": sug.created_at,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json_rows).map_err(|e| Error::Other(e.to_string()))?
        );
    }
```

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run`
Expected: all tests pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: enrich triage suggestions JSON with body, labels, and suggestion_id"
```

---

### Task 4: Update SKILL.md with agent-driven workflow and `triage decide`

**Files:**
- Modify: `SKILL.md`

- [ ] **Step 1: Add `triage decide` to the Commands section**

In `SKILL.md`, add this line to the "Triage Pipeline" command listing, after the `triage review --auto-approve` line (around line 102):

```
armitage triage decide <issue-ref> --decision <approve|reject|modify|stale> [--node <path>] [--labels <l,...>] [--note <text>]
```

- [ ] **Step 2: Add `decide` description to the bullet list**

After the `- **apply**` bullet (around line 122), add:

```markdown
- **decide** — submit a single review decision non-interactively. Used by agents and scripts. Supports `approve`, `reject`, `modify` (with optional `--node`/`--labels` overrides), and `stale`. Auto-saves examples on reject/modify/stale (same as interactive mode). Errors if the decision has already been applied to GitHub
```

- [ ] **Step 3: Update the `suggestions` command line to show `--body-max`**

Update the `triage suggestions` command in the listing to include `[--body-max N]`.

- [ ] **Step 4: Add the agent-driven triage review workflow**

After the "Iterative batch triage" section (around line 252), add this new workflow section:

```markdown
### Agent-driven triage review

An agent (Claude Code) can drive the review loop, auto-approving obvious classifications and
presenting uncertain ones to the user. This is faster than interactive terminal review for large
backlogs.

**The agent loop:**

1. **Classify a batch:** `triage classify --limit N` (starts at 20, adapts over time)
2. **Read suggestions:** `triage suggestions --status pending --format json --body-max 500`
3. **Partition:** Split into auto-decide (confidence >= 0.80, sound reasoning) and uncertain
4. **Auto-decide:** For each high-confidence issue, run `triage decide <ref> --decision approve`.
   The agent may reject/modify instead if memory of past corrections suggests the user would
   disagree.
5. **Present auto-decides for override:** Show the user what was auto-approved as a summary list.
   The user can override specific issues by number.
6. **Present uncertain issues:** For each uncertain issue, ask the user via `AskUserQuestion`:
   - Show: issue ref (link), title, body excerpt, current labels, suggested node + labels,
     confidence, reasoning
   - Ask: approve / reject / modify / stale / skip
   - On modify: suggest 2-3 alternative nodes from the roadmap tree
7. **Submit decisions:** `triage decide <ref> --decision <d> [--node ...] [--labels ...] [--note ...]`
8. **Adapt batch size:**
   - If auto-override rate > 20%: shrink batch by 50%, raise auto-decide threshold
   - If correction rate > 40%: shrink batch by 50%
   - If correction rate < 15%: grow batch by 50%
   - Otherwise: hold steady
   - Bounds: min 10, max 100
9. **Reset:** `triage reset --unreviewed` to return skipped items to the untriaged pool
10. **Check:** `triage status --format json` — if 0 untriaged remain or user says stop, exit

**Memory:** The agent saves feedback memories for patterns it learns (e.g., "user always corrects
backend/api → backend/rest for REST issues") and applies them in future auto-decide
partitioning.

**Boundaries:** The agent does not run `triage apply` (pushing to GitHub), `triage fetch` (pulling
issues), or modify the roadmap tree. Those are separate user-initiated actions.
```

- [ ] **Step 5: Commit**

```bash
git add SKILL.md
git commit -m "docs: add triage decide command and agent-driven review workflow to SKILL.md"
```

---

### Task 5: Integration test — `decide` + example round-trip

**Files:**
- Modify: `src/triage/db.rs` (add test)

This test verifies the full path: insert issue → insert suggestion → decide (modify) → verify decision and example are saved correctly.

- [ ] **Step 1: Write the integration test**

Add to `mod tests` in `src/triage/db.rs`:

```rust
#[test]
fn decide_approve_does_not_create_decision_for_applied() {
    let conn = memory_db();

    let issue = sample_issue("owner/repo", 50);
    let issue_id = upsert_issue(&conn, &issue).unwrap();
    let sug = sample_suggestion(issue_id, "some/node", 0.9);
    upsert_suggestion(&conn, &sug).unwrap();

    // Look up and get suggestion id
    let (_, s, _) = get_suggestion_by_issue(&conn, "owner/repo", 50)
        .unwrap()
        .unwrap();

    // Insert an applied decision
    insert_decision(
        &conn,
        &ReviewDecision {
            id: 0,
            suggestion_id: s.id,
            decision: "approved".to_string(),
            final_node: Some("some/node".to_string()),
            final_labels: vec![],
            decided_at: "2026-04-07T00:00:00Z".to_string(),
            applied_at: Some("2026-04-07T01:00:00Z".to_string()),
        },
    )
    .unwrap();

    // Verify applied_at is visible
    let (_, _, d) = get_suggestion_by_issue(&conn, "owner/repo", 50)
        .unwrap()
        .unwrap();
    assert!(d.is_some());
    assert!(d.unwrap().applied_at.is_some());
}
```

- [ ] **Step 2: Run the test**

Run: `cargo nextest run -E 'test(decide_approve_does_not_create_decision_for_applied)'`
Expected: PASS

- [ ] **Step 3: Run full suite**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo nextest run`
Expected: all tests pass, no warnings.

- [ ] **Step 4: Commit**

```bash
git add src/triage/db.rs
git commit -m "test: add integration test for get_suggestion_by_issue with applied decisions"
```
