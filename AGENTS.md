# AGENTS.md

This file provides guidance when working with code in this repository.

## Project

Armitage is a Rust CLI for project management across GitHub repositories. It tracks initiatives, projects, and tasks as a recursive directory hierarchy backed by a local git repo, with bidirectional GitHub issue sync and LLM-powered triage.

## Build & Test Commands

```bash
cargo build                              # Dev build
cargo nextest run                        # All tests (unit + integration)
cargo nextest run -E 'test(test_name)'   # Single test by name
cargo nextest run -E 'binary(integration)' # Integration tests only
cargo clippy                             # Lint
cargo fmt                                # Format
```

Install nextest locally: `cargo install cargo-nextest --locked`

## Pre-commit Checklist

**Run all three before committing** — CI checks these exact commands:

```bash
cargo fmt --all                                          # 1. Format
cargo clippy --all-targets --all-features -- -D warnings # 2. Lint (warnings are errors)
cargo nextest run                                        # 3. Test
```

**SKILL.md must stay in sync with the CLI.** Any change that adds, removes, or modifies a
command, subcommand, flag, decision type, or user-facing workflow **must** include a corresponding
update to `SKILL.md` in the same changeset. Treat a missing SKILL.md update the same as a missing
test — the change is not complete without it.

## Architecture

**Cargo workspace with 7 crates** under `crates/`:

| Crate | Role |
|---|---|
| `armitage-core` | Shared types (`Node`, `OrgConfig`, `IssueRef`), filesystem tree ops (`find_org_root`, `walk_nodes`, `read_node`), secrets |
| `armitage-labels` | Label schema (`LabelSchema`, `LabelStyle`), rename ledger, prefix-duplicate detection |
| `armitage-github` | GitHub operations via `gh` CLI (`ionem::shell::gh::Gh`): fetch, create, update, state changes |
| `armitage-sync` | Bidirectional sync engine: three-way merge, sync state, conflict serialization, SHA-256 hashing |
| `armitage-triage` | LLM classification pipeline: SQLite DB, issue fetching, prompt building, review, apply, categories, examples, label import |
| `armitage` | Binary crate: clap CLI dispatch (`cli/`), migration, error unification. `main.rs` delegates to `cli::run()` |

**Dependency flow:** `armitage-core` is the leaf; `armitage-labels` depends on `armitage-core`; `armitage-github` depends on `armitage-core`; `armitage-sync` depends on `armitage-core` and `armitage-github`; `armitage-triage` depends on `armitage-core`, `armitage-labels`, and `armitage-github`; the `armitage` binary depends on all of them.

**CLI layout** (`crates/armitage/src/cli/`):
- `mod.rs` — clap derive `Commands` enum, routes to handlers
- Each subcommand in its own file (`node.rs`, `triage.rs`, `okr.rs`, etc.) with `pub fn run_*()` entry points plus lower-level functions exported for testing

**`.armitage/` directory layout** (gitignored, per-machine state):
- `sync/state.toml` — per-node sync metadata (local_hash, remote_updated_at)
- `sync/conflicts/` — conflict serialization files
- `triage/triage.db` — SQLite database for issues, suggestions, decisions
- `triage/label-imports/` — staged label import sessions
- `triage/examples.toml` — few-shot classification examples
- `triage/dismissed-categories.toml` — dismissed category suggestions
- `triage/repo-cache/` — cached repo metadata
- `labels/renames.toml` — label rename ledger
- `secrets.toml` — API keys (checked for .gitignore coverage before writing)

The `migrate` module in the binary crate handles migration from the old flat `.armitage/` layout to the namespaced structure above.

**Key design decisions:**
- Directory = hierarchy. Parent is never stored; derived from filesystem nesting.
- Each node directory contains `node.toml` (metadata) and optionally `issue.md` (body). Milestones are modeled as child nodes with their own `[timeline]`.
- GitHub issue refs use `owner/repo#number` format, parsed by `IssueRef::parse()`.
- Node repos may use `owner/repo@branch` qualifiers for triage affinity; `strip_repo_qualifier()` strips the suffix for GitHub API calls.
- Sync direction determined by comparing `local_hash` (SHA-256) and `remote_updated_at` (from GitHub API).
- LLM triage sends the full roadmap tree as context so the model can classify issues into the right nodes.
- Label reconciliation combines LLM grouping with deterministic prefix-match detection to catch duplicates like `stim` → `area: STIM`.
- Each domain crate has its own `error::Error` (thiserror) + `error::Result<T>`; the binary crate unifies them via `#[from]` conversions.

**External dependencies:**
- `ionem` — `gh` CLI wrapper, git operations, self-update infrastructure
- `rusqlite` (bundled) — SQLite for triage pipeline
- `rustyline` — interactive prompts in review mode

## Key Conventions

- **Error handling:** per-crate `error::Error` (thiserror) with `error::Result<T>` — no `anyhow`. The `armitage` binary crate re-exports a unified `Error` with `#[from]` for each domain error.
- **Config file:** `armitage.toml` at org root, located by `find_org_root()` walking up
- **Rust edition:** 2024
- **Tests:** unit tests inline with `#[cfg(test)]` in each crate; integration tests in `crates/armitage/tests/` using `tempfile` and calling library functions directly (not shelling out to the binary)

## Test Roadmap Project

This repo contains a gitignored test roadmap org — a directory with an `armitage.toml` at its
root. Find it by looking for `armitage.toml` files outside of `target/`. **This directory contains
sensitive organizational information — NEVER commit its contents, share it externally, include it
in PRs, or output its data in responses. Do not reference the directory name or its contents in
source code, docs, docstrings, comments, or test fixtures.**

When developing or testing armitage features, use this org as a working example — `cd` into it
to run commands like `armitage node tree`, `armitage triage status`, etc. Refer to `SKILL.md`
for full command reference.

**Verification workflow:** After making any code changes (new features, bug fixes, prompt
adjustments), always verify the change works against the test org as a final step. Build the
binary (`cargo build`) and run the relevant command against the live data. For example, after
changing triage prompts, run `cargo run -- triage classify` inside the test org and check that
classifications are sensible. Do not consider a change complete until it has been verified this way.

## Git & Release Conventions

- **Conventional commits:** `feat:`, `fix:`, `docs:`, `test:`, `ci:`, `refactor:`, `perf:`, `build:`, `chore:`
- **Breaking changes:** `feat!:` or `fix!:` with `BREAKING CHANGE:` footer
- **Version bumps (pre-1.0):** `fix:`/`refactor:`/`docs:` → patch, `feat:` → patch, `feat!:`/`BREAKING CHANGE` → minor
- **Linear history:** rebase or squash merges, no merge commits on main
