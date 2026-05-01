# Changelog

All notable changes to this project will be documented in this file.

### Added

- Add --require-label-prefix to okr check (0f443b0)
- Add triage label command for batch label changes without LLM classification (28d83bb)
- Store and expose GitHub sub-issue relationships (e50d419)
- Add `issue create` command with auto project board setup (2c09fec)
- Detect GitHub project board updates in triage watch (93b4f02)
- Detect issue closure in watch — track state_at_watch (d770726)
- Add `triage watch add` command for manually watching issues (2dd6b92)
- Implement watch list/dismiss handlers and surface reply counts in status (ef3b494)
- Add Watch CLI subcommand registration (8ee9a8b)
- Add `project set` command to update board dates for individual issues (738961b)
- Per-node sync, --check-dates for node check (219e7a9)
- Add overdue command for issues past their target date (45706b6)
- Detect archived/renamed repos in node check --check-repos (fa09778)
- Warn when node owners are not in team.toml (00f8bf9)
- Add --timeline-start and --timeline-end to node set (089615d)
- Sync node timelines to GitHub Project board (447df33)
- Add inactive issue detection with LLM follow-up assessment (87713bb)
- Distinguish pull requests from issues with purple visual treatment (a87a5ef)
- Add markdown rendering, bidirectional hover, and serve-by-default ([#63](https://github.com/Roger-luo/armitage/pull/63)) (0977558)
- Add assignees/participants to issue pipeline and side panel ([#62](https://github.com/Roger-luo/armitage/pull/62)) (04370ef)
- Add issue description, author, and labels to chart panel ([#61](https://github.com/Roger-luo/armitage/pull/61)) (2325b22)
- Show all active issues as sub-bars, gray for undated ([#57](https://github.com/Roger-luo/armitage/pull/57)) (22f0619)
- Chart --watch serves via local HTTP with live reload ([#45](https://github.com/Roger-luo/armitage/pull/45)) (08808cd)
- Add --watch flag to chart command ([#44](https://github.com/Roger-luo/armitage/pull/44)) (e398be6)
- Chart Phase 3 — project date visualization with overflow bars ([#40](https://github.com/Roger-luo/armitage/pull/40)) (4ef8d43)
- Validate issue project dates against node timelines in node check ([#39](https://github.com/Roger-luo/armitage/pull/39)) (c0c2f16)
- Add GitHub Projects v2 metadata fetching (Phase 1) ([#38](https://github.com/Roger-luo/armitage/pull/38)) (18dee77)
- Skip repo-implied labels via [triage.repo_labels] config ([#25](https://github.com/Roger-luo/armitage/pull/25)) (d115322)
- Add --all-pending to triage decide and refs format to suggestions ([#21](https://github.com/Roger-luo/armitage/pull/21)) (961804c)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)
- Add triage_hint field to node.toml (bc8ae9c)
- Add node fmt command for canonical node.toml formatting (a71b5d2)
- Auto-convert long strings to multi-line TOML in node.toml (e5bf84c)
- Add node set command for non-interactive field updates (b21c942)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add --depth flag to node tree command (70e82bd)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Safe title truncation, remove Debug derive, warn on add_watch failure (86cab94)
- Use track field name instead of github_issue in check-dates (8595d15)
- Clean up stale root-level triage.db during migration ([#27](https://github.com/Roger-luo/armitage/pull/27)) (9a533e9)

### Refactored

- Make `triage review` interactive-only (f783ef8)
- Apply pedantic clippy lints with idiomatic Rust patterns ([#59](https://github.com/Roger-luo/armitage/pull/59)) (3c1822b)
- Use idiomatic Rust patterns across codebase ([#56](https://github.com/Roger-luo/armitage/pull/56)) (424915a)
- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)

### Testing

- Cover sync namespace and rejection of old top-level forms (b948f9e)
- Cover `decide --all-pending` and `suggestions --status pending` as review replacements (42b81c6)

### Merge

- Combine sync namespace, review simplification, milestones drop (aa4c9ac)

### Added

- Skip repo-implied labels via [triage.repo_labels] config ([#25](https://github.com/Roger-luo/armitage/pull/25)) (d115322)
- Add --all-pending to triage decide and refs format to suggestions ([#21](https://github.com/Roger-luo/armitage/pull/21)) (961804c)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)
- Add triage_hint field to node.toml (bc8ae9c)
- Add node fmt command for canonical node.toml formatting (a71b5d2)
- Auto-convert long strings to multi-line TOML in node.toml (e5bf84c)
- Add node set command for non-interactive field updates (b21c942)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add --depth flag to node tree command (70e82bd)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Clean up stale root-level triage.db during migration ([#27](https://github.com/Roger-luo/armitage/pull/27)) (9a533e9)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)

### Added

- Skip repo-implied labels via [triage.repo_labels] config ([#25](https://github.com/Roger-luo/armitage/pull/25)) (d115322)
- Add --all-pending to triage decide and refs format to suggestions ([#21](https://github.com/Roger-luo/armitage/pull/21)) (961804c)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)
- Add triage_hint field to node.toml (bc8ae9c)
- Add node fmt command for canonical node.toml formatting (a71b5d2)
- Auto-convert long strings to multi-line TOML in node.toml (e5bf84c)
- Add node set command for non-interactive field updates (b21c942)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add --depth flag to node tree command (70e82bd)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Clean up stale root-level triage.db during migration ([#27](https://github.com/Roger-luo/armitage/pull/27)) (9a533e9)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)
