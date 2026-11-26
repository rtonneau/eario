---
bump: minor
floor: minor
---
- Minimum supported Rust version declared: 1.95, tested in CI.
- Scrollable tables (rows, statistics, dataset columns) can now be focused and scrolled with the keyboard.
- Browser tests in Chromium, with an accessibility check (WCAG 2.1 A/AA) of every visited state in both themes: `cargo xtask e2e`.
- Dependency licences and security advisories are checked with `cargo deny` in CI.
- `cargo xtask bench` records dated benchmark results in `docs/benchmarks/`.
