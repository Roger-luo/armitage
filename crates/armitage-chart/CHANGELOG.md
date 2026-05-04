# Changelog

All notable changes to this project will be documented in this file.
## [0.1.1] - 2026-05-04

### Added

- Store and expose GitHub sub-issue relationships (e50d419)
- Include track field in chart data and display in side panel (facf28e)
- 45° milestone labels with dedicated zone, hover tooltip, and click-to-panel (51c3a26)
- Scope milestone visibility to current view with ancestor propagation (77608cf)
- Add okr show/check commands to derive OKR views from roadmap data (3308389)
- Distinguish pull requests from issues with purple visual treatment (a87a5ef)
- Add markdown rendering, bidirectional hover, and serve-by-default ([#63](https://github.com/Roger-luo/armitage/pull/63)) (0977558)
- Add assignees/participants to issue pipeline and side panel ([#62](https://github.com/Roger-luo/armitage/pull/62)) (04370ef)
- Add issue description, author, and labels to chart panel ([#61](https://github.com/Roger-luo/armitage/pull/61)) (2325b22)
- Redesign chart with D3 split-panel, test harness, and expandable issues ([#60](https://github.com/Roger-luo/armitage/pull/60)) (fdbbf03)
- Show all active issues as sub-bars, gray for undated ([#57](https://github.com/Roger-luo/armitage/pull/57)) (22f0619)
- Render issues as sub-bars alongside child nodes in chart ([#49](https://github.com/Roger-luo/armitage/pull/49)) (08ccaec)
- Add light/dark/auto theme toggle to chart ([#48](https://github.com/Roger-luo/armitage/pull/48)) (ebd3c27)
- Split bars into blue (planned) and red (exceeded) using overflow_start ([#43](https://github.com/Roger-luo/armitage/pull/43)) (679afe2)
- Chart Phase 3 — project date visualization with overflow bars ([#40](https://github.com/Roger-luo/armitage/pull/40)) (4ef8d43)
- Show "Today" vertical line in roadmap chart (0498cdd)
- Show issues in chart panel and fix offline mode ([#30](https://github.com/Roger-luo/armitage/pull/30)) (0525b68)
- Show approved issues with GitHub links in chart panel (51f870b)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)

### Fixed

- Rotate milestone labels clockwise so they extend into milestone zone (f719b8b)
- Move milestone labels above the time axis (5c07f14)
- Rotate milestone labels 90° to eliminate overlap (e128535)
- Remove redundant serde_json import in integration test (275783e)
- Only show direct issues as sub-bars, not descendant issues ([#55](https://github.com/Roger-luo/armitage/pull/55)) (57cae78)
- Only show outer bar red zone when overflow exceeds node timeline ([#54](https://github.com/Roger-luo/armitage/pull/54)) (892e348)
- Let issue bars extend into overflow region instead of clipping ([#53](https://github.com/Roger-luo/armitage/pull/53)) (4401910)
- Use overflow_start as green→purple boundary for issue bars ([#52](https://github.com/Roger-luo/armitage/pull/52)) (bbccd33)
- Issue bars split green→purple at node timeline boundary ([#51](https://github.com/Roger-luo/armitage/pull/51)) (4f89eb1)
- Show descendant issues as sub-bars, green vs purple for timeline conflicts ([#50](https://github.com/Roger-luo/armitage/pull/50)) (9980b92)
- Replace ambiguous range button with segmented toggle ([#47](https://github.com/Roger-luo/armitage/pull/47)) (399f45b)
- Render red overflow on nested child bars within parent bars ([#42](https://github.com/Roger-luo/armitage/pull/42)) (6f090f9)
- Bubble overflow_end, issue_start, issue_end up from children ([#41](https://github.com/Roger-luo/armitage/pull/41)) (1dbbb23)
- Move vertical lines to dedicated line series for reliable rendering (5233924)
- Move issues after children in panel, aggregate descendant issues ([#33](https://github.com/Roger-luo/armitage/pull/33)) (8e54e3e)
- Remove duplicate issues block that crashed panel ([#32](https://github.com/Roger-luo/armitage/pull/32)) (38b1508)
- Remove duplicate read_issues function from merge ([#31](https://github.com/Roger-luo/armitage/pull/31)) (af20db6)

### Refactored

- Remove milestone subcommand and armitage-milestones crate (7aef860)
- Muted milestone colors, text halo, theme-aware CSS vars (28b1d0b)
- Apply pedantic clippy lints with idiomatic Rust patterns ([#59](https://github.com/Roger-luo/armitage/pull/59)) (3c1822b)
- Use idiomatic Rust patterns across codebase ([#56](https://github.com/Roger-luo/armitage/pull/56)) (424915a)
- Polish codebase with idiomatic Rust patterns and efficiency fixes (d763e8d)

### Added

- Store and expose GitHub sub-issue relationships (e50d419)
- Include track field in chart data and display in side panel (facf28e)
- 45° milestone labels with dedicated zone, hover tooltip, and click-to-panel (51c3a26)
- Scope milestone visibility to current view with ancestor propagation (77608cf)
- Add okr show/check commands to derive OKR views from roadmap data (3308389)
- Distinguish pull requests from issues with purple visual treatment (a87a5ef)
- Add markdown rendering, bidirectional hover, and serve-by-default ([#63](https://github.com/Roger-luo/armitage/pull/63)) (0977558)
- Add assignees/participants to issue pipeline and side panel ([#62](https://github.com/Roger-luo/armitage/pull/62)) (04370ef)
- Add issue description, author, and labels to chart panel ([#61](https://github.com/Roger-luo/armitage/pull/61)) (2325b22)
- Redesign chart with D3 split-panel, test harness, and expandable issues ([#60](https://github.com/Roger-luo/armitage/pull/60)) (fdbbf03)
- Show all active issues as sub-bars, gray for undated ([#57](https://github.com/Roger-luo/armitage/pull/57)) (22f0619)
- Render issues as sub-bars alongside child nodes in chart ([#49](https://github.com/Roger-luo/armitage/pull/49)) (08ccaec)
- Add light/dark/auto theme toggle to chart ([#48](https://github.com/Roger-luo/armitage/pull/48)) (ebd3c27)
- Split bars into blue (planned) and red (exceeded) using overflow_start ([#43](https://github.com/Roger-luo/armitage/pull/43)) (679afe2)
- Chart Phase 3 — project date visualization with overflow bars ([#40](https://github.com/Roger-luo/armitage/pull/40)) (4ef8d43)
- Show "Today" vertical line in roadmap chart (0498cdd)
- Show issues in chart panel and fix offline mode ([#30](https://github.com/Roger-luo/armitage/pull/30)) (0525b68)
- Show approved issues with GitHub links in chart panel (51f870b)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)

### Fixed

- Rotate milestone labels clockwise so they extend into milestone zone (f719b8b)
- Move milestone labels above the time axis (5c07f14)
- Rotate milestone labels 90° to eliminate overlap (e128535)
- Remove redundant serde_json import in integration test (275783e)
- Only show direct issues as sub-bars, not descendant issues ([#55](https://github.com/Roger-luo/armitage/pull/55)) (57cae78)
- Only show outer bar red zone when overflow exceeds node timeline ([#54](https://github.com/Roger-luo/armitage/pull/54)) (892e348)
- Let issue bars extend into overflow region instead of clipping ([#53](https://github.com/Roger-luo/armitage/pull/53)) (4401910)
- Use overflow_start as green→purple boundary for issue bars ([#52](https://github.com/Roger-luo/armitage/pull/52)) (bbccd33)
- Issue bars split green→purple at node timeline boundary ([#51](https://github.com/Roger-luo/armitage/pull/51)) (4f89eb1)
- Show descendant issues as sub-bars, green vs purple for timeline conflicts ([#50](https://github.com/Roger-luo/armitage/pull/50)) (9980b92)
- Replace ambiguous range button with segmented toggle ([#47](https://github.com/Roger-luo/armitage/pull/47)) (399f45b)
- Render red overflow on nested child bars within parent bars ([#42](https://github.com/Roger-luo/armitage/pull/42)) (6f090f9)
- Bubble overflow_end, issue_start, issue_end up from children ([#41](https://github.com/Roger-luo/armitage/pull/41)) (1dbbb23)
- Move vertical lines to dedicated line series for reliable rendering (5233924)
- Move issues after children in panel, aggregate descendant issues ([#33](https://github.com/Roger-luo/armitage/pull/33)) (8e54e3e)
- Remove duplicate issues block that crashed panel ([#32](https://github.com/Roger-luo/armitage/pull/32)) (38b1508)
- Remove duplicate read_issues function from merge ([#31](https://github.com/Roger-luo/armitage/pull/31)) (af20db6)

### Refactored

- Remove milestone subcommand and armitage-milestones crate (7aef860)
- Muted milestone colors, text halo, theme-aware CSS vars (28b1d0b)
- Apply pedantic clippy lints with idiomatic Rust patterns ([#59](https://github.com/Roger-luo/armitage/pull/59)) (3c1822b)
- Use idiomatic Rust patterns across codebase ([#56](https://github.com/Roger-luo/armitage/pull/56)) (424915a)
- Polish codebase with idiomatic Rust patterns and efficiency fixes (d763e8d)

### Added

- Show "Today" vertical line in roadmap chart (0498cdd)
- Show issues in chart panel and fix offline mode ([#30](https://github.com/Roger-luo/armitage/pull/30)) (0525b68)
- Show approved issues with GitHub links in chart panel (51f870b)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)

### Fixed

- Move vertical lines to dedicated line series for reliable rendering (5233924)
- Move issues after children in panel, aggregate descendant issues ([#33](https://github.com/Roger-luo/armitage/pull/33)) (8e54e3e)
- Remove duplicate issues block that crashed panel ([#32](https://github.com/Roger-luo/armitage/pull/32)) (38b1508)
- Remove duplicate read_issues function from merge ([#31](https://github.com/Roger-luo/armitage/pull/31)) (af20db6)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes (d763e8d)

### Added

- Show approved issues with GitHub links in chart panel (51f870b)
- Add armitage-chart crate for interactive roadmap visualization ([#20](https://github.com/Roger-luo/armitage/pull/20)) (ca1fafb)

### Refactored

- Polish codebase with idiomatic Rust patterns and efficiency fixes (d763e8d)
