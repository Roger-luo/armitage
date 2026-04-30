# Changelog

All notable changes to this project will be documented in this file.

### Added

- Add triage label command for batch label changes without LLM classification (28d83bb)
- Store and expose GitHub sub-issue relationships (e50d419)
- Detect GitHub project board updates in triage watch (93b4f02)
- Detect issue closure in watch — track state_at_watch (d770726)
- Add `triage watch add` command for manually watching issues (2dd6b92)
- Check watched issues for replies during fetch (6c490b1)
- Auto-watch issues after posting inquire comment (3db2c6f)
- Add watched_issues table and DB functions for reply detection (1ece3f8)
- Add overdue command for issues past their target date (45706b6)
- Add inactive issue detection with LLM follow-up assessment (87713bb)
- Distinguish pull requests from issues with purple visual treatment (a87a5ef)
- Add assignees/participants to issue pipeline and side panel ([#62](https://github.com/Roger-luo/armitage/pull/62)) (04370ef)
- Add issue description, author, and labels to chart panel ([#61](https://github.com/Roger-luo/armitage/pull/61)) (2325b22)
- Validate issue project dates against node timelines in node check ([#39](https://github.com/Roger-luo/armitage/pull/39)) (c0c2f16)
- Add GitHub Projects v2 metadata fetching (Phase 1) ([#38](https://github.com/Roger-luo/armitage/pull/38)) (18dee77)
- Skip repo-implied labels via [triage.repo_labels] config ([#25](https://github.com/Roger-luo/armitage/pull/25)) (d115322)
- Inject node labels into issues on triage apply ([#22](https://github.com/Roger-luo/armitage/pull/22)) (4d7128a)
- Write approved classifications to issues.toml on triage apply (25fd7f4)
- Add triage_hint field to node.toml (bc8ae9c)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Safe title truncation, remove Debug derive, warn on add_watch failure (86cab94)
- Use parameterized queries in get_watches; fix fragile timestamp match in check_watches_for_replies (9203f9a)
- Remove repo-implied labels from issues on triage apply ([#26](https://github.com/Roger-luo/armitage/pull/26)) (0ec0328)
- Node label injection preserves existing issue labels ([#23](https://github.com/Roger-luo/armitage/pull/23)) (85f8693)
- Cargo fmt (dafc7a6)

### Refactored

- Apply pedantic clippy lints with idiomatic Rust patterns ([#59](https://github.com/Roger-luo/armitage/pull/59)) (3c1822b)
- Use idiomatic Rust patterns across codebase ([#56](https://github.com/Roger-luo/armitage/pull/56)) (424915a)
- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)

### Added

- Skip repo-implied labels via [triage.repo_labels] config ([#25](https://github.com/Roger-luo/armitage/pull/25)) (d115322)
- Inject node labels into issues on triage apply ([#22](https://github.com/Roger-luo/armitage/pull/22)) (4d7128a)
- Write approved classifications to issues.toml on triage apply (25fd7f4)
- Add triage_hint field to node.toml (bc8ae9c)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Remove repo-implied labels from issues on triage apply ([#26](https://github.com/Roger-luo/armitage/pull/26)) (0ec0328)
- Node label injection preserves existing issue labels ([#23](https://github.com/Roger-luo/armitage/pull/23)) (85f8693)
- Cargo fmt (dafc7a6)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)

### Added

- Skip repo-implied labels via [triage.repo_labels] config ([#25](https://github.com/Roger-luo/armitage/pull/25)) (d115322)
- Inject node labels into issues on triage apply ([#22](https://github.com/Roger-luo/armitage/pull/22)) (4d7128a)
- Write approved classifications to issues.toml on triage apply (25fd7f4)
- Add triage_hint field to node.toml (bc8ae9c)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Remove repo-implied labels from issues on triage apply ([#26](https://github.com/Roger-luo/armitage/pull/26)) (0ec0328)
- Node label injection preserves existing issue labels ([#23](https://github.com/Roger-luo/armitage/pull/23)) (85f8693)
- Cargo fmt (dafc7a6)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)
