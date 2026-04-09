# AGENTS.md — Armitage Org

This directory is an **armitage org** — a project management hierarchy backed by a local git repo,
with bidirectional GitHub issue sync and LLM-powered triage.

## Invoking Armitage

<!-- Uncomment the line that matches your installation: -->
<!-- ion run armitage <subcommand>       # installed via: ion add --bin Roger-luo/armitage -->
<!-- armitage <subcommand>               # installed via: cargo install or curl script -->

## Command Reference

Install the armitage skill for full command documentation:

```
ion skill add Roger-luo/armitage
```

Then ask Claude to use the `armitage` skill, or read `SKILL.md` inside the armitage repo for the
complete reference.

## Directory Structure

```
org-root/
├── armitage.toml          # Org config (name, github_orgs, label_schema, triage settings)
├── labels.toml            # Curated label definitions
├── triage-examples.toml   # Few-shot examples for LLM classification
├── .gitignore             # Excludes .armitage/
├── .armitage/             # Local state (gitignored)
│   ├── triage.db          # SQLite DB for triage pipeline
│   ├── issue-cache/       # Lightweight per-repo issue caches
│   ├── secrets.toml       # API keys
│   └── ...
└── <nodes>/               # Recursive directory tree
    ├── node.toml          # Node metadata
    ├── issue.md           # Issue body (synced with GitHub)
    └── <sub-nodes>/
        └── node.toml
```

## Key Concepts

**Node** — a directory containing `node.toml`. Nodes form a tree via filesystem nesting. Each
represents an initiative, project, or task.

**node.toml fields:** `name`, `description`, `status` (active/completed/paused/cancelled),
`repos` (with optional `@branch` qualifier), `labels`, `github_issue` (owner/repo#number),
`timeline` (start/end dates).

**Issue cache** — after triage operations, `.armitage/issue-cache/{owner}--{repo}.toml` contains
every open issue's number, title, state, labels, and triage suggestion. Read these for a quick
overview without querying GitHub.

## Workflows

### Triage pipeline: fetch -> classify -> review -> apply

1. `armitage triage fetch` — pull issues from GitHub into local DB
2. `armitage triage classify` — LLM classifies issues into nodes
3. `armitage triage review -i` — interactively review suggestions (or `--auto-approve 0.8`)
4. `armitage triage apply` — push approved decisions to GitHub

### Agent-driven triage

For large backlogs, use the batch workflow:

1. `armitage triage classify --limit 20` — classify a small batch
2. `armitage triage suggestions --status pending --format summary` — partition into AUTO-APPROVE
   and NEEDS REVIEW groups
3. `armitage triage decide <refs>... --decision approve` — batch-approve confident suggestions
4. Present uncertain issues to the user for manual decision
5. `armitage triage reset --unreviewed` — return skipped items for reclassification
6. Repeat, adapting batch size based on correction rate

### Planning nodes

- Write specific `description` fields — the triage LLM uses them to classify issues
- Use `@branch` qualifiers when the same repo has work on multiple branches
- Read issue cache files to understand issue landscape before planning node structure
- After adding nodes: `armitage triage reset --unreviewed && armitage triage classify`

### Label management

Labels are curated in `labels.toml`. Pipeline: fetch remote labels -> merge (with LLM dedup) ->
sync/push to GitHub.

## Rules

- **Never commit `.armitage/`** — it contains local-only state and is gitignored.
- **Never commit `secrets.toml`** — contains API keys.
- The triage pipeline does not auto-push to GitHub. `triage apply` is always a separate,
  user-initiated step.
- Labels are additive during triage — the LLM only suggests labels the issue doesn't already have.
  The only way to remove labels is via `modify` during review.
