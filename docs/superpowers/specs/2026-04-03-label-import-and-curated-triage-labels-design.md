# Label Import And Curated Triage Labels Design

## Goal

Add a curated label-management workflow to Armitage so users can import GitHub labels from one or more repos into a reviewable staging area, selectively merge chosen labels into `labels.toml`, and make those curated labels available to `armitage triage classify`.

## Scope

In scope:
- Fetch remote GitHub labels from one or more repos using `gh`.
- Stage fetched labels into a temporary import session under `.armitage/`.
- Provide an interactive terminal picker for reviewing and selecting labels to merge.
- Provide a non-interactive merge mode for automation and scripting.
- Extend `labels.toml` to store curated label metadata.
- Include curated labels from `labels.toml` in the triage classification prompt as label name plus short description.
- Detect and surface metadata drift for labels that already exist locally.

Out of scope:
- Automatically deleting stale labels from `labels.toml` during import.
- Persisting source repo provenance in `labels.toml`.
- Sending example issues for labels in the first classification pass.
- Multi-step LLM retrieval loops where the model requests more label context.
- Tool-style interactive LLM classification workflows.

## Goals And Non-Goals

Primary goals:
- Keep `labels.toml` as the curated source of truth for labels across repos.
- Identify labels by unique name only.
- Make importing safe and reviewable before any write to `labels.toml`.
- Improve classification quality by exposing the curated label catalog to the LLM.

Non-goals:
- Mirroring any repo's label set into `labels.toml`.
- Treating the same label name in different repos as different labels.
- Tracking long-term per-repo ownership or provenance for curated labels.

## User Experience

The feature adds a new `triage labels` command group.

Fetch remote labels into a staging session:

```bash
armitage triage labels fetch --repo owner/repo
armitage triage labels fetch --repo owner/repo --repo owner/infra
```

Review and merge labels interactively:

```bash
armitage triage labels merge
```

Use non-interactive merge for automation:

```bash
armitage triage labels merge --all-new
armitage triage labels merge --all-new --update-drifted
armitage triage labels merge --name bug --name priority:high
```

The workflow is intentionally two-step:
1. `fetch` stages candidate labels and computes their status relative to the curated local catalog.
2. `merge` reviews those candidates and writes only approved changes to `labels.toml`.

This keeps remote discovery separate from curation and makes it possible to build richer UIs later on top of the same staging format.

## CLI Design

Add a nested command group:

```text
armitage triage labels fetch ...
armitage triage labels merge ...
```

### `armitage triage labels fetch`

Purpose:
- Fetch labels from one or more GitHub repos.
- Normalize and deduplicate labels by name.
- Persist a reviewable import session.

Flags:
- `--repo <owner/repo>` repeatable and required.

Behavior:
- Query each repo with `gh label list --repo <repo> --json name,description,color`.
- Collapse labels with the same name across repos into one candidate entry.
- Mark each candidate with comparison status against the current `labels.toml`.
- Write a session file under `.armitage/label-imports/`.
- Print a concise summary: number fetched, number unique by name, number new, number drifted, number unchanged, and the session path or identifier.

### `armitage triage labels merge`

Purpose:
- Review the latest or explicitly selected import session.
- Select candidates to write into `labels.toml`.

Flags:
- `--session <id>` optional, defaults to the latest session.
- `--all-new` select all candidates with status `new`.
- `--update-drifted` select all candidates with status `metadata-drift`.
- `--name <label>` repeatable, include only the named labels.
- `--exclude-name <label>` repeatable, exclude named labels.
- `--prefer-repo <owner/repo>` optional tie-breaker when the same label has conflicting remote metadata.
- `--yes` optional confirmation bypass in non-interactive mode.

Behavior:
- Without selection flags, open an interactive terminal picker.
- With selection flags, compute the selected set non-interactively and print a preview before applying.
- Write only approved additions and updates to `labels.toml`.
- Never delete labels from `labels.toml`.

## Interactive Merge UX

The first version uses a terminal-based interactive picker inside Armitage.

The picker should present one row per unique label name with:
- label name
- short description
- color
- status: `new`, `metadata-drift`, `unchanged`, or `duplicate-remote`

Expected interactive behaviors:
- `new` labels are selected by default.
- `unchanged` labels are hidden or unselected by default.
- `metadata-drift` labels require explicit user action before update.
- For `duplicate-remote`, the user can inspect the per-repo variants and choose which metadata to keep if the remote values differ.
- Confirming the selection writes the resulting curated catalog to `labels.toml`.

The interactive picker does not need to be a full-screen TUI. A line-oriented prompt flow using existing terminal tooling is acceptable for v1 as long as it supports:
- reviewing candidate labels,
- toggling selections,
- inspecting conflicts,
- confirming the final write.

## Data Model

`labels.toml` remains the curated global label catalog and is keyed by `name` only.

Example:

```toml
[[labels]]
name = "bug"
description = "Something is broken"
color = "D73A4A"

[[labels]]
name = "priority:high"
description = "Needs prompt attention"
color = "B60205"
```

Proposed persisted fields:
- `name: String`
- `description: String`
- `color: Option<String>`

Identity rules:
- Label identity is the `name` only.
- `labels.toml` may contain at most one entry for a given name.
- Repo-specific provenance is not stored in `labels.toml`.

Temporary import sessions under `.armitage/label-imports/` may store additional context needed for review:
- fetched-at timestamp
- source repos
- remote metadata variants per label name
- comparison status against `labels.toml`

This session data is local review state, not curated source of truth.

## Merge Semantics

Comparison happens by label name only.

For each fetched label candidate:
- `new`: name does not exist in `labels.toml`
- `unchanged`: name exists and description/color match the curated entry
- `metadata-drift`: name exists but description and/or color differ
- `duplicate-remote`: multiple repos returned the same label name

Merge rules:
- Adding a `new` label appends a new curated entry.
- Updating a `metadata-drift` label replaces the curated description and/or color only if explicitly selected.
- `unchanged` labels produce no write.
- `duplicate-remote` labels are resolved to a single chosen metadata variant before merge.
- No merge path deletes an existing curated label.

If multiple repos return the same name with different metadata, the merge UI treats that as a conflict on metadata, not on identity.

## GitHub Integration

Fetching uses the GitHub CLI through the existing `ionem::shell::gh::Gh` wrapper.

Remote label fields required for v1:
- `name`
- `description`
- `color`

Planned `gh` invocation shape:

```bash
gh label list --repo owner/repo --json name,description,color
```

Armitage should normalize missing descriptions to an empty string and preserve color strings as returned by GitHub.

## Classification Prompt Changes

`armitage triage classify` currently passes roadmap context and prefix-based label schema from `armitage.toml`.

For v1, classification should also load `labels.toml` and provide a compact curated label catalog section containing only:
- label name
- short description

Example prompt section:

```text
## Curated Labels
- bug: Something is broken
- priority:high: Needs prompt attention
- area:infra: Infrastructure-related work
```

Prompt constraints:
- Do not include example issues by default.
- Do not include repo provenance.
- Keep the label catalog concise and deterministic.

Prompt behavior:
- The LLM should prefer labels from the curated catalog when suggesting labels.
- The existing prefix-based schema in `armitage.toml` remains useful for category guidance and examples.
- `labels.toml` provides the concrete label vocabulary.

## Deferred Retrieval Design

The design should leave room for a later second-pass retrieval workflow where the LLM can request more context for specific labels.

Deferred backlog items:
- A follow-up classification pass that requests example issues for selected labels from the local triage database.
- A tool-style interaction loop where the model can iteratively request more label context.

These are explicitly not part of the first release and should not shape the initial CLI surface beyond keeping prompt construction modular.

## File And Module Changes

Likely implementation areas:
- `src/cli/mod.rs`
  - add `triage labels` subcommands
- `src/cli/triage.rs`
  - add fetch and merge entry points
- `src/model/label.rs`
  - extend the persisted schema and add merge/update helpers
- `src/triage/llm.rs`
  - load `labels.toml` and build the curated label prompt section
- new label import module under `src/triage/` or `src/github/`
  - fetch remote labels
  - store import sessions
  - diff staged labels against local curated labels
  - drive interactive and non-interactive merge flows
- `README.md`
  - document the new label import and curated triage flow

## Error Handling

Expected user-facing failures:
- no repos provided to `triage labels fetch`
- `gh` not installed or not authenticated
- GitHub label fetch failure for one or more repos
- no import session available for `triage labels merge`
- conflicting duplicate-remote metadata with no explicit resolution in non-interactive mode
- invalid or duplicate local entries in `labels.toml`
- TOML parse or write failures

Non-interactive merge should fail clearly if the selection flags do not resolve metadata conflicts deterministically.

## Testing

Add focused unit and integration tests for:
- reading and writing extended `labels.toml`
- rejecting or normalizing duplicate label names
- diff classification: `new`, `unchanged`, `metadata-drift`, `duplicate-remote`
- remote label deduplication by name across repos
- non-interactive merge selection behavior
- prompt building includes curated labels from `labels.toml`
- prompt output contains only label name plus short description for curated labels

Interactive merge flow should be tested at the helper level where possible, keeping terminal I/O wrappers thin.

## Open Questions Resolved

- `labels.toml` is the curated source of truth.
- Label identity is unique name only.
- No source repo metadata is persisted in `labels.toml`.
- The first version includes an interactive picker and a non-interactive mode.
- The first classification pass uses only curated label names plus short descriptions.
- Retrieval-capable label context remains backlog work.
