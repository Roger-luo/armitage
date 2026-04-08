# `triage categories refine` — LLM-Powered Category Consolidation

**Date:** 2026-04-06
**Status:** Draft

## Problem

After `triage classify`, the raw `suggested_new_categories` are fragmented. The same concept
appears under different names (`backend/api` vs `research/api-design`), some
suggestions overlap with existing nodes, and there is no mechanism to consolidate them into
a coherent set of new nodes.

## Solution

A new `triage categories refine` subcommand that sends the aggregated raw suggestions + the
current roadmap tree to an LLM, receives grouped and deduplicated proposals, and walks the user
through an interactive review to apply or dismiss each group. Follows the same pattern as
`triage labels merge` LLM reconciliation.

---

## Command Interface

```
armitage triage categories refine [--backend <backend>] [--model <model>] [--auto-accept] [--min-votes <n>]
```

| Flag | Type | Description |
|------|------|-------------|
| `--backend` | String | LLM backend (overrides config) |
| `--model` | String | Model (overrides config) |
| `--auto-accept` | bool | Skip interactive review, apply all recommendations |
| `--min-votes` | usize | Minimum vote count to include (default: 2) |

LLM config resolved via `resolve_classify_config()` — same fallback chain as `triage classify`.

---

## Pipeline

1. Gather raw category votes from DB (`get_new_category_votes`)
2. Filter by `--min-votes` and dismissed categories
3. Build prompt with raw categories + current roadmap tree
4. Invoke LLM, parse response into `RefinedGroup` structs
5. Interactive review per group (or auto-accept)
6. For each approved "new node" group: create node + reset suggestions
7. For each approved "covered" group: dismiss the raw category suggestions
8. Print summary, refresh cache

---

## LLM Prompt

**Inputs:**
- Raw category suggestions with vote counts and sample issue refs
- Current roadmap tree (reuse `build_roadmap_section()` from classify prompts)

**Prompt template:**

```
You are consolidating suggested new categories for a project roadmap.

## Current Roadmap Tree
{roadmap_tree}

## Raw Category Suggestions
Each line shows a suggested category, vote count, and example issues.

{formatted_suggestions}

## Instructions
1. Group suggestions that refer to the same concept
2. For each group, propose a single node: path (valid child of existing node or new
   top-level), name, and description
3. If a suggestion is already covered by an existing roadmap node, mark it as "covered"
4. Only propose nodes that would meaningfully organize 2+ issues

Respond with JSON only:
{
  "groups": [
    {
      "raw_suggestions": ["backend/api", "research/api-design"],
      "covered_by": null,
      "proposed_path": "backend/api",
      "proposed_name": "Circuit Synthesis",
      "proposed_description": "Encoding circuit synthesis strategies",
      "reason": "Both refer to circuit synthesis work"
    }
  ]
}
```

---

## Response Types

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RefineResponse {
    pub groups: Vec<RefinedGroup>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RefinedGroup {
    pub raw_suggestions: Vec<String>,
    pub covered_by: Option<String>,
    pub proposed_path: Option<String>,
    pub proposed_name: Option<String>,
    pub proposed_description: Option<String>,
    pub reason: String,
}
```

Two group types:
- **New node:** `covered_by` is null, `proposed_path/name/description` are set
- **Covered:** `covered_by` names an existing node, no new node needed

---

## Interactive Review

For each group, display and prompt:

**New node group:**
```
Group 1/5: Circuit Synthesis
  Raw suggestions: backend/api (3 votes), research/api-design (3 votes)
  LLM says: Create new node
  Path:        backend/api
  Name:        Circuit Synthesis
  Description: Encoding circuit synthesis strategies
  Reason:      Both refer to circuit synthesis work

  [a]pply  [s]kip  [r]efine  [q]uit
```

**Covered group:**
```
Group 3/5: PyQrack
  Raw suggestions: circuit/pyqrack (2 votes)
  LLM says: Covered by "circuit/stdlib"
  Reason:      PyQrack emulator work fits under existing node

  [d]ismiss suggestions  [s]kip  [q]uit
```

**Actions:**
- **apply** — Call `create_node_full()` + `delete_suggestions_for_reclassify()` for each
  raw suggestion in the group. Same logic as `triage categories apply`.
- **dismiss** — For covered groups: call `categories::dismiss()` for each raw suggestion.
- **skip** — Move to next group.
- **refine** — Prompt user for feedback ("What should change?"), re-invoke LLM with the
  group context + feedback, re-display updated proposal. Loop until apply/skip.
- **quit** — Exit early.

**`--auto-accept`:** Apply all "new node" groups, dismiss all "covered" groups.

**After all groups:** Print summary line and `cache::refresh_all()`.

---

## Refinement Sub-flow

When user picks "refine", collect feedback and re-invoke LLM:

**Refinement prompt:**
```
You previously proposed this for a category group:
  Raw suggestions: {raw_suggestions}
  Proposed path: {path}
  Proposed name: {name}
  Proposed description: {description}
  Reason: {reason}

Current roadmap tree:
{roadmap_tree}

The user wants changes: {user_feedback}

Respond with an updated JSON proposal:
{
  "covered_by": null,
  "proposed_path": "...",
  "proposed_name": "...",
  "proposed_description": "...",
  "reason": "..."
}
```

Parse response, update the group, re-display. Loop back to action prompt.

---

## File Changes

| File | Changes |
|------|---------|
| `src/cli/mod.rs` | Add `Refine` variant to `TriageCategoryCommands` |
| `src/cli/triage.rs` | Add `run_categories_refine()` — orchestrates the full pipeline |
| `src/triage/llm.rs` | Add `build_refine_prompt()`, `refine_categories()`, `build_refine_feedback_prompt()`, `refine_category_group()`. Add `RefineResponse` and `RefinedGroup` types. Reuse `invoke_llm()` and three-tier parsing. |
| `src/triage/categories.rs` | No changes — existing `dismiss()` is sufficient |
| `src/triage/db.rs` | No changes — existing `get_new_category_votes()` and `delete_suggestions_for_reclassify()` are sufficient |
