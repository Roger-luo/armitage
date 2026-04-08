# Agent-Driven Triage Review

**Date:** 2026-04-07
**Status:** Design

## Problem

The triage review workflow requires a human at the terminal pressing single keys for each of 300+
issues. The iterative batch workflow (`classify --limit` → `review -i` → `reset --unreviewed` →
repeat) helps, but it's still tedious for large backlogs.

An agent (Claude Code) can drive this loop: auto-approving obvious classifications, presenting
uncertain ones to the user via `AskUserQuestion`, and adapting batch sizes based on correction
rates. The agent accumulates memory of user preferences to improve its judgment over time.

Both workflows must coexist — the interactive terminal review (`review -i`) stays unchanged for
users who prefer it.

## Design

### 1. `triage decide` — Non-Interactive Decision Submission

New CLI subcommand for submitting a single review decision programmatically.

**Interface:**

```
armitage triage decide <issue-ref> --decision <approve|reject|modify|stale> \
    [--node <path>] [--labels <l1,l2,...>] [--note <text>]
```

- `<issue-ref>` — positional arg in `owner/repo#number` format.
- `--decision` — required. One of: `approve`, `reject`, `modify`, `stale`.
- `--node` — only valid with `--decision modify`. If omitted, keeps the suggestion's node.
- `--labels` — only valid with `--decision modify`. Comma-separated. If omitted, keeps the
  suggestion's labels.
- `--note` — optional for all decision types. Saved to the example on reject/modify/stale.

**Behavior:**

1. Parse the issue ref via `IssueRef::parse()`.
2. Look up the suggestion via new `db::get_suggestion_by_issue(conn, repo, number)`.
3. Error if no suggestion exists.
4. Error if a decision has already been applied (`applied_at IS NOT NULL`) — prevent overwriting
   pushed labels.
5. Write the `ReviewDecision` via `db::insert_decision()` (upserts, so re-deciding an unapplied
   issue overwrites the previous decision).
6. On reject/modify/stale: auto-save a `TriageExample` to `triage-examples.toml` (same behavior
   as interactive mode).
7. Print a confirmation line to stdout and exit.

**Decision semantics match interactive mode:**

| Decision | `final_node` | `final_labels` | Example saved? |
|----------|-------------|----------------|----------------|
| approve  | suggestion's node | suggestion's labels | No |
| reject   | None | [] | Yes |
| modify   | `--node` or suggestion's node | `--labels` or suggestion's labels | Yes |
| stale    | None | [] | Yes (with `is_stale = true`) |

**Validation:**

- `--node` / `--labels` with `--decision approve|reject|stale` → error with clear message.
- Unknown issue ref → error: "No suggestion found for owner/repo#123".
- Already-applied decision → error: "Decision for owner/repo#123 has already been applied to
  GitHub. Use `triage reset --issue` to clear it first."

**New DB function:**

```rust
pub fn get_suggestion_by_issue(
    conn: &Connection,
    repo: &str,
    number: u64,
) -> Result<Option<(StoredIssue, TriageSuggestion, Option<ReviewDecision>)>>
```

Joins `issues`, `triage_suggestions`, and LEFT JOINs `review_decisions`. Returns `None` if no
suggestion exists for the issue. The `Option<ReviewDecision>` is needed to check `applied_at`.

**CLI structure:**

New `TriageCommands::Decide` variant (sibling of `Review`, not nested under it):

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

### 2. Enrich `triage suggestions --format json`

The agent needs richer issue data than the current JSON output provides.

**Fields to add:**

| Field | Source | Purpose |
|-------|--------|---------|
| `suggestion_id` | `sug.id` | Tracing and debugging |
| `body` | `issue.body` | Agent reads body to assess classification quality |
| `current_labels` | `issue.labels` | Compare current vs suggested labels |
| `is_stale` | `sug.is_stale` | Know if LLM flagged issue as stale |
| `sub_issues_count` | `issue.sub_issues_count` | Context for tracking issues |
| `status` | derived from `rd.decision` / `rd.applied_at` | Know if pending/approved/rejected/applied |

**Not adding:** `issue_url` — trivially computed as `https://github.com/{repo}/issues/{number}`.

**New flag:**

```
--body-max <chars>   Truncate issue body in JSON output (default: 500, 0 = unlimited)
```

This is backwards-compatible — existing fields stay, new fields appear alongside them. No new DB
queries; the data is already fetched by `get_suggestions_filtered()`.

### 3. Agent Orchestration (Skill)

The orchestration logic lives in SKILL.md as a "Typical Workflow" section. No Rust code for the
agent loop — the agent reads the skill, runs shell commands, and uses `AskUserQuestion` for user
interaction.

**The loop:**

```
1. CLASSIFY
   Run `triage classify --limit N` (N starts at 20)

2. READ
   Run `triage suggestions --status pending --format json --body-max 500`

3. PARTITION into two buckets:
   a. Auto-decide: confidence >= 0.80 AND agent judges reasoning is sound
      (considering memory of past user corrections on similar issues)
   b. Uncertain: everything else

4. AUTO-DECIDE
   For each auto-decide issue, run `triage decide <ref> --decision approve`
   (Agent may reject/modify instead if memory indicates user would disagree)

5. PRESENT auto-decides to user for override:
   "Auto-approved 12 suggestions with high confidence:
    - owner/repo#101: Fix typo in docs → docs (95%)
    - owner/repo#205: Emulator crash → circuit/emulator (91%)
    - ...
    Override any? (enter issue numbers, or 'ok')"
   Any overrides count toward auto_override_rate.

6. PRESENT uncertain issues to user one at a time via AskUserQuestion:
   Show: ref (as link), title, body excerpt, current labels,
   suggested node + labels, confidence, reasoning.
   Ask: approve / reject / modify / stale / skip
   On modify: suggest 2-3 alternative nodes from the roadmap tree.

7. SUBMIT
   For each decision, run `triage decide` with appropriate flags.

8. ADAPT batch size based on correction rates (see below).

9. RESET
   Run `triage reset --unreviewed` to return skipped items to the untriaged pool.

10. CHECK
    Run `triage status --format json` to see remaining untriaged count.
    If 0 remaining or user says stop → exit. Otherwise → loop to step 1.
```

**Adaptive batch sizing:**

Starting parameters: N=20, auto-decide floor=0.80, min N=10, max N=100.

After each round:

```
correction_rate = (rejections + modifications) / total_reviewed_by_user
auto_override_rate = user_overrides_of_auto_decides / auto_decided_count

if auto_override_rate > 0.2:
    # Agent misjudging — shrink AND raise auto-decide threshold
    next_N = max(current_N * 0.5, 10)
    auto_confidence_floor += 0.05  (capped at 0.95)
elif correction_rate > 0.4:
    # Many corrections — keep batches small for faster example accumulation
    next_N = max(current_N * 0.5, 10)
elif correction_rate < 0.15:
    # User mostly agrees — expand
    next_N = min(current_N * 1.5, 100)
else:
    # Hold steady
    next_N = current_N
```

These are heuristics in the skill prompt, not Rust code. The agent uses them as guidelines and
can deviate when context warrants it (e.g., only 8 issues remain).

**AskUserQuestion format for uncertain issues:**

```
**acme/widgets#648**: REST API standard library for core service
https://github.com/acme/widgets/issues/648

> [body excerpt, ~3-4 lines]

Current labels: `enhancement`
Suggested: `backend/api` with labels `[enhancement, rest]` (82%)
Reasoning: "Issue discusses REST standard endpoints for the core service..."

**approve** / **reject** / **modify** / **stale** / **skip**?
```

On modify, the agent follows up:

```
I'd suggest one of:
1. `circuit/qasm` with labels `[enhancement, qasm]`
2. `circuit/compiler` with labels `[enhancement, qasm]`
3. Something else — tell me the node and labels

Which one?
```

**Memory patterns the agent saves:**

- Classification corrections: "User corrects backend/api → backend/rest for REST issues"
- Stale preferences: "User rejects stale flag on issues proposing API replacements"
- Confidence calibration: "User auto-approves emulator issues above 75%"

These inform future auto-decide partitioning without the user re-explaining.

**What the agent does NOT do:**

- Does not run `triage apply` — pushing labels to GitHub is a separate, explicit user action.
- Does not run `triage fetch` — user controls when to pull new issues.
- Does not modify the roadmap tree or create nodes.

## Implementation Scope

### In scope

1. `TriageCommands::Decide` variant in `cli/mod.rs` + `run_decide()` in `cli/triage.rs`
2. `db::get_suggestion_by_issue()` in `triage/db.rs`
3. Enrich `suggestions --format json` output + `--body-max` flag in `cli/triage.rs` and `cli/mod.rs`
4. "Agent-driven triage review" workflow section in `SKILL.md`
5. Unit tests for `get_suggestion_by_issue()` and `run_decide()` paths
6. Integration test: `decide` + example round-trip

### Out of scope

- `reset --pending` (separate feature)
- Changes to interactive `review -i`
- Changes to `triage apply`
- Standalone skill file (extends existing SKILL.md)

## Files to Modify

| File | Change |
|------|--------|
| `src/cli/mod.rs` | Add `Decide` variant to `TriageCommands`, add `body_max` to `Suggestions`, dispatch |
| `src/cli/triage.rs` | Add `run_decide()`, enrich JSON in `run_suggestions()` |
| `src/triage/db.rs` | Add `get_suggestion_by_issue()` |
| `SKILL.md` | Add agent-driven triage review workflow section |
