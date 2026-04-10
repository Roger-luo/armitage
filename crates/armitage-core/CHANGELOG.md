# Changelog

All notable changes to this project will be documented in this file.

### Added

- Add issues.toml per-node file for approved issue classification (b0a6382)
- Add triage_hint field to node.toml (bc8ae9c)
- Auto-convert long strings to multi-line TOML in node.toml (e5bf84c)
- Add teams/site fields to team.toml and team field to node.toml (50d0f7f)
- Add team.toml support for org-level team member records (10fe8e0)
- Add owners field to node.toml (4c6864b)
- Armitage workspace with 7 domain-driven crates (4b25454)

### Fixed

- Normalize newlines in description before wrapping (94afe90)
- Drop trailing backslash in multi-line TOML strings (5ca6d5c)
- Word-wrap content inside multi-line TOML strings (3d9dcd9)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes ([#24](https://github.com/Roger-luo/armitage/pull/24)) (daa3a4c)
