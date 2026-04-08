# `triage categories refine` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `triage categories refine` — an LLM-powered consolidation pass over raw category suggestions with interactive review, refinement loop, and auto-accept mode.

**Architecture:** A new `Refine` variant in `TriageCategoryCommands` dispatches to `run_categories_refine()` in `cli/triage.rs`. The LLM prompt/parse/refine functions go in `triage/llm.rs` following the existing `reconcile_labels` pattern. The interactive review loop lives in `cli/triage.rs` and reuses `create_node_full()`, `delete_suggestions_for_reclassify()`, and `categories::dismiss()` for apply actions.

**Tech Stack:** Rust, clap, rusqlite, serde_json, dialoguer (for interactive prompts)

**Spec:** `docs/superpowers/specs/2026-04-06-categories-refine-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/triage/llm.rs` | Modify | Add `RefineResponse`, `RefinedGroup` types, `build_refine_prompt()`, `refine_categories()`, `build_refine_feedback_prompt()`, `refine_category_group()` |
| `src/cli/mod.rs` | Modify | Add `Refine` variant to `TriageCategoryCommands`, add dispatch arm |
| `src/cli/triage.rs` | Modify | Add `run_categories_refine()` with interactive review loop |

---

### Task 1: Response Types and Prompt Builder in llm.rs

**Files:**
- Modify: `src/triage/llm.rs`

- [ ] **Step 1: Write test for refine prompt building**

In `src/triage/llm.rs`, add to the existing `#[cfg(test)] mod tests` block:

```rust
#[test]
fn build_refine_prompt_includes_categories_and_tree() {
    let nodes = vec![crate::fs::tree::NodeEntry {
        path: "flair".to_string(),
        dir: std::path::PathBuf::from("/tmp/flair"),
        node: crate::model::node::Node {
            name: "FLAIR".to_string(),
            description: "FLAIR language".to_string(),
            github_issue: None,
            labels: vec![],
            repos: vec![],
            timeline: None,
            status: crate::model::node::NodeStatus::Active,
        },
    }];
    let votes = vec![
        db::CategoryVote {
            category: "circuit/emulator".to_string(),
            vote_count: 5,
            issue_refs: vec!["owner/repo#1".to_string(), "owner/repo#2".to_string()],
        },
        db::CategoryVote {
            category: "circuit/pyqrack".to_string(),
            vote_count: 2,
            issue_refs: vec!["owner/repo#1".to_string()],
        },
    ];
    let prompt = build_refine_prompt(&nodes, &votes);
    assert!(prompt.contains("circuit/emulator"));
    assert!(prompt.contains("5 votes"));
    assert!(prompt.contains("circuit/pyqrack"));
    assert!(prompt.contains("flair"));
    assert!(prompt.contains("\"groups\""));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -E 'test(build_refine_prompt_includes)'`
Expected: FAIL — `build_refine_prompt` doesn't exist

- [ ] **Step 3: Add response types and build_refine_prompt**

In `src/triage/llm.rs`, after the `refine_label_suggestions` function (around line 895), add:

```rust
// ---------------------------------------------------------------------------
// Category refinement (LLM-driven consolidation of raw suggestions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RefinedGroup {
    pub raw_suggestions: Vec<String>,
    pub covered_by: Option<String>,
    pub proposed_path: Option<String>,
    pub proposed_name: Option<String>,
    pub proposed_description: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RefineResponse {
    pub groups: Vec<RefinedGroup>,
}

fn build_refine_prompt(nodes: &[NodeEntry], votes: &[db::CategoryVote]) -> String {
    use std::fmt::Write;

    let mut prompt = String::new();
    writeln!(
        prompt,
        "You are consolidating suggested new categories for a project roadmap.\n"
    )
    .unwrap();

    // Include the current roadmap tree
    prompt.push_str(&build_roadmap_section(nodes));

    writeln!(prompt, "\n## Raw Category Suggestions\n").unwrap();
    writeln!(
        prompt,
        "Each line shows a suggested category, vote count, and example issues.\n"
    )
    .unwrap();
    for vote in votes {
        let refs: Vec<&str> = vote.issue_refs.iter().take(5).map(|s| s.as_str()).collect();
        writeln!(
            prompt,
            "  {:<40} {} votes  {}",
            vote.category,
            vote.vote_count,
            refs.join(", ")
        )
        .unwrap();
    }

    writeln!(
        prompt,
        "\n## Instructions\n\
         1. Group suggestions that refer to the same concept (e.g. \"backend/api\" and \
         \"research/api-design\")\n\
         2. For each group, propose a single node: path (must be a valid child of an existing \
         node or a new top-level node), name, and description\n\
         3. If a suggestion is already covered by an existing roadmap node, mark it as \
         \"covered\" with the existing node path — set covered_by to the node path and leave \
         proposed_path/name/description as null\n\
         4. Only propose nodes that would meaningfully organize 2+ issues\n\
         \n\
         Respond with JSON only:\n\
         {{\n\
           \"groups\": [\n\
             {{\n\
               \"raw_suggestions\": [\"category-a\", \"category-b\"],\n\
               \"covered_by\": null,\n\
               \"proposed_path\": \"parent/child\",\n\
               \"proposed_name\": \"Display Name\",\n\
               \"proposed_description\": \"What this node covers\",\n\
               \"reason\": \"Why these are grouped\"\n\
             }}\n\
           ]\n\
         }}"
    )
    .unwrap();

    prompt
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo nextest run -E 'test(build_refine_prompt_includes)'`
Expected: PASS

- [ ] **Step 5: Add parse + public refine_categories function**

After `build_refine_prompt`, add:

```rust
fn parse_refine_response(raw: &str) -> Result<RefineResponse> {
    let trimmed = raw.trim();

    if let Ok(r) = serde_json::from_str::<RefineResponse>(trimmed) {
        return Ok(r);
    }
    if let Some(json) = extract_json_block(trimmed)
        && let Ok(r) = serde_json::from_str::<RefineResponse>(&json)
    {
        return Ok(r);
    }
    if let Some(json) = extract_json_object(trimmed)
        && let Ok(r) = serde_json::from_str::<RefineResponse>(&json)
    {
        return Ok(r);
    }

    Err(Error::LlmParse(format!(
        "could not parse category refine response: {trimmed}"
    )))
}

/// Ask LLM to consolidate raw category suggestions into grouped proposals.
pub fn refine_categories(
    nodes: &[NodeEntry],
    votes: &[db::CategoryVote],
    config: &LlmConfig,
) -> Result<RefineResponse> {
    let prompt = build_refine_prompt(nodes, votes);
    tracing::info!(
        categories = votes.len(),
        "refining category suggestions via LLM"
    );
    let raw = invoke_llm(config, &prompt)?;
    let response = parse_refine_response(&raw)?;
    tracing::debug!(groups = response.groups.len(), "category refinement complete");
    Ok(response)
}
```

- [ ] **Step 6: Add refine_category_group for the refinement feedback loop**

```rust
/// Re-invoke LLM to refine a single category group based on user feedback.
pub fn refine_category_group(
    nodes: &[NodeEntry],
    group: &RefinedGroup,
    user_feedback: &str,
    config: &LlmConfig,
) -> Result<RefinedGroup> {
    use std::fmt::Write;

    let mut prompt = String::new();
    writeln!(prompt, "You previously proposed this for a category group:").unwrap();
    writeln!(
        prompt,
        "  Raw suggestions: {}",
        group.raw_suggestions.join(", ")
    )
    .unwrap();
    if let Some(ref path) = group.proposed_path {
        writeln!(prompt, "  Proposed path: {path}").unwrap();
    }
    if let Some(ref name) = group.proposed_name {
        writeln!(prompt, "  Proposed name: {name}").unwrap();
    }
    if let Some(ref desc) = group.proposed_description {
        writeln!(prompt, "  Proposed description: {desc}").unwrap();
    }
    if let Some(ref covered) = group.covered_by {
        writeln!(prompt, "  Covered by: {covered}").unwrap();
    }
    writeln!(prompt, "  Reason: {}", group.reason).unwrap();

    writeln!(prompt, "\n{}", build_roadmap_section(nodes)).unwrap();

    writeln!(
        prompt,
        "The user wants changes: {user_feedback}\n\n\
         Respond with an updated JSON proposal:\n\
         {{\n\
           \"covered_by\": null,\n\
           \"proposed_path\": \"...\",\n\
           \"proposed_name\": \"...\",\n\
           \"proposed_description\": \"...\",\n\
           \"reason\": \"...\"\n\
         }}"
    )
    .unwrap();

    tracing::debug!(user_feedback, "refining category group via LLM");
    let raw = invoke_llm(config, &prompt)?;
    let trimmed = raw.trim();

    // Parse single group (not wrapped in RefineResponse)
    if let Ok(mut g) = serde_json::from_str::<RefinedGroup>(trimmed) {
        g.raw_suggestions = group.raw_suggestions.clone();
        return Ok(g);
    }
    if let Some(json) = extract_json_block(trimmed)
        && let Ok(mut g) = serde_json::from_str::<RefinedGroup>(&json)
    {
        g.raw_suggestions = group.raw_suggestions.clone();
        return Ok(g);
    }
    if let Some(json) = extract_json_object(trimmed)
        && let Ok(mut g) = serde_json::from_str::<RefinedGroup>(&json)
    {
        g.raw_suggestions = group.raw_suggestions.clone();
        return Ok(g);
    }

    Err(Error::LlmParse(format!(
        "could not parse category refinement response: {trimmed}"
    )))
}
```

- [ ] **Step 7: Run all tests, clippy, format**

Run: `cargo nextest run && cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: All pass, clean

- [ ] **Step 8: Commit**

```bash
git add src/triage/llm.rs
git commit -m "feat: add LLM category refinement types and prompt builders"
```

---

### Task 2: CLI Subcommand and Interactive Review

**Files:**
- Modify: `src/cli/mod.rs` (add Refine variant + dispatch)
- Modify: `src/cli/triage.rs` (add run_categories_refine)

- [ ] **Step 1: Add Refine variant to TriageCategoryCommands**

In `src/cli/mod.rs`, add to the `TriageCategoryCommands` enum (after the `Dismiss` variant):

```rust
    /// Consolidate raw category suggestions via LLM and interactively apply
    Refine {
        /// LLM backend (overrides config)
        #[arg(long)]
        backend: Option<String>,
        /// Model (overrides config)
        #[arg(long)]
        model: Option<String>,
        /// Skip interactive review, apply all recommendations
        #[arg(long)]
        auto_accept: bool,
        /// Minimum vote count to include (default: 2)
        #[arg(long, default_value_t = 2)]
        min_votes: usize,
    },
```

- [ ] **Step 2: Add dispatch arm**

In `src/cli/mod.rs`, add to the `TriageCategoryCommands` match block (after the Dismiss arm):

```rust
TriageCategoryCommands::Refine {
    backend,
    model,
    auto_accept,
    min_votes,
} => {
    triage::run_categories_refine(backend, model, auto_accept, min_votes)?;
}
```

- [ ] **Step 3: Implement run_categories_refine() in cli/triage.rs**

Add the following function to `src/cli/triage.rs`:

```rust
pub fn run_categories_refine(
    backend: Option<String>,
    model: Option<String>,
    auto_accept: bool,
    min_votes: usize,
) -> Result<()> {
    let org_root = find_org_root(&std::env::current_dir()?)?;
    let conn = db::open_db(&org_root)?;
    let nodes = walk_nodes(&org_root)?;
    let org_config = read_org_config(&org_root)?;

    // Gather and filter raw votes
    let dismissed = crate::triage::categories::read_dismissed(&org_root)?;
    let mut votes = db::get_new_category_votes(&conn, None)?;
    votes.retain(|v| {
        !crate::triage::categories::is_dismissed(&dismissed, &v.category)
            && v.vote_count >= min_votes
    });

    if votes.is_empty() {
        println!("No category suggestions with >= {min_votes} votes.");
        return Ok(());
    }

    println!("Found {} category suggestions to refine.", votes.len());

    // Invoke LLM
    let config = resolve_classify_config(backend, model, None, &org_config.triage)?;
    let response = llm::refine_categories(&nodes, &votes, &config)?;

    if response.groups.is_empty() {
        println!("LLM found no groups to consolidate.");
        return Ok(());
    }

    println!("LLM proposed {} group(s).\n", response.groups.len());

    // Build a lookup from category name -> vote count for display
    let vote_map: std::collections::HashMap<&str, usize> = votes
        .iter()
        .map(|v| (v.category.as_str(), v.vote_count))
        .collect();

    let mut applied = 0usize;
    let mut dismissed_count = 0usize;
    let mut skipped = 0usize;
    let total = response.groups.len();

    for (i, group) in response.groups.iter().enumerate() {
        let mut group = group.clone();

        loop {
            // Display group
            let raw_display: Vec<String> = group
                .raw_suggestions
                .iter()
                .map(|s| {
                    let count = vote_map.get(s.as_str()).copied().unwrap_or(0);
                    format!("{s} ({count} votes)")
                })
                .collect();

            println!("--- Group {}/{total} ---", i + 1);
            println!("  Raw suggestions: {}", raw_display.join(", "));

            if let Some(ref covered) = group.covered_by {
                println!("  LLM says: Covered by \"{covered}\"");
                println!("  Reason:   {}", group.reason);
                println!();

                if auto_accept {
                    // Dismiss all raw suggestions
                    for cat in &group.raw_suggestions {
                        crate::triage::categories::dismiss(&org_root, cat)?;
                    }
                    dismissed_count += 1;
                    break;
                }

                let choice =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Action")
                        .items(&["Dismiss suggestions", "Skip", "Quit"])
                        .default(0)
                        .interact()
                        .map_err(|e| Error::Other(e.to_string()))?;

                match choice {
                    0 => {
                        for cat in &group.raw_suggestions {
                            crate::triage::categories::dismiss(&org_root, cat)?;
                        }
                        dismissed_count += 1;
                        println!("  Dismissed.\n");
                        break;
                    }
                    1 => {
                        skipped += 1;
                        println!("  Skipped.\n");
                        break;
                    }
                    _ => {
                        println!("  Quit.\n");
                        print_refine_summary(applied, dismissed_count, skipped);
                        cache::refresh_all(&conn, &org_root)?;
                        return Ok(());
                    }
                }
            } else {
                // New node proposal
                let path = group.proposed_path.as_deref().unwrap_or("???");
                let name = group.proposed_name.as_deref().unwrap_or("???");
                let desc = group.proposed_description.as_deref().unwrap_or("");
                println!("  LLM says: Create new node");
                println!("  Path:        {path}");
                println!("  Name:        {name}");
                println!("  Description: {desc}");
                println!("  Reason:      {}", group.reason);
                println!();

                if auto_accept {
                    if let (Some(p), Some(n), Some(d)) = (
                        &group.proposed_path,
                        &group.proposed_name,
                        &group.proposed_description,
                    ) {
                        apply_refined_group(&org_root, &conn, p, n, d, &group.raw_suggestions)?;
                        applied += 1;
                    }
                    break;
                }

                let choice =
                    dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                        .with_prompt("Action")
                        .items(&["Apply", "Skip", "Refine", "Quit"])
                        .default(0)
                        .interact()
                        .map_err(|e| Error::Other(e.to_string()))?;

                match choice {
                    0 => {
                        // Apply
                        if let (Some(p), Some(n), Some(d)) = (
                            &group.proposed_path,
                            &group.proposed_name,
                            &group.proposed_description,
                        ) {
                            apply_refined_group(&org_root, &conn, p, n, d, &group.raw_suggestions)?;
                            applied += 1;
                        }
                        break;
                    }
                    1 => {
                        skipped += 1;
                        println!("  Skipped.\n");
                        break;
                    }
                    2 => {
                        // Refine: get user feedback and re-invoke LLM
                        let feedback: String =
                            dialoguer::Input::with_theme(&dialoguer::theme::ColorfulTheme::default())
                                .with_prompt("What should change?")
                                .interact_text()
                                .map_err(|e| Error::Other(e.to_string()))?;
                        match llm::refine_category_group(&nodes, &group, &feedback, &config) {
                            Ok(updated) => {
                                group = updated;
                                println!();
                                // Loop back to display updated group
                                continue;
                            }
                            Err(e) => {
                                eprintln!("  LLM refinement failed: {e}");
                                // Let user retry or skip
                                continue;
                            }
                        }
                    }
                    _ => {
                        println!("  Quit.\n");
                        print_refine_summary(applied, dismissed_count, skipped);
                        cache::refresh_all(&conn, &org_root)?;
                        return Ok(());
                    }
                }
            }
        }
    }

    print_refine_summary(applied, dismissed_count, skipped);
    cache::refresh_all(&conn, &org_root)?;
    Ok(())
}

fn apply_refined_group(
    org_root: &std::path::Path,
    conn: &rusqlite::Connection,
    path: &str,
    name: &str,
    description: &str,
    raw_suggestions: &[String],
) -> Result<()> {
    crate::cli::node::create_node_full(
        org_root,
        path,
        Some(name),
        Some(description),
        None,
        None,
        &[],
        "active",
        None,
    )?;

    let mut total_reset = 0;
    for cat in raw_suggestions {
        total_reset += db::delete_suggestions_for_reclassify(conn, cat)?;
    }
    println!("  Created node '{path}'. Reset {total_reset} suggestion(s).\n");
    Ok(())
}

fn print_refine_summary(applied: usize, dismissed: usize, skipped: usize) {
    println!(
        "Summary: applied {applied} node(s), dismissed {dismissed} group(s), skipped {skipped} group(s)."
    );
    if applied > 0 {
        println!("Run 'armitage triage classify' to reclassify affected issues.");
    }
}
```

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Run full test suite and lint**

Run: `cargo nextest run && cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: All pass, clean

- [ ] **Step 6: Commit**

```bash
git add src/cli/mod.rs src/cli/triage.rs
git commit -m "feat: add triage categories refine with interactive review"
```

---

### Task 3: Final Verification

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo nextest run`
Expected: All pass

- [ ] **Step 2: Run clippy and format check**

Run: `cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --all -- --check`
Expected: Clean

- [ ] **Step 3: Verify help text**

Run: `cargo run -- triage categories refine --help`
Expected: Shows --backend, --model, --auto-accept, --min-votes flags with descriptions

- [ ] **Step 4: Smoke test against test org**

```bash
cd <test-org-dir>
cargo run -- triage categories refine --min-votes 3
```

Expected: Shows LLM refinement results, interactive prompts work. Verify the groups make sense (e.g., `backend/api` and `research/api-design` are grouped together).

- [ ] **Step 5: Commit any fixups**

If smoke testing revealed issues, fix and commit.
