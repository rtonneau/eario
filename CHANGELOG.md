# Changelog

All notable changes to this project. Versions follow [semantic versioning](https://semver.org/).

## 0.2.0 (2026-11-26)

- Custom Rust computations: register a named function with `Dashboard::computation` and declare a derived dataset from it with `Dashboard::derived`; every component reads a derived dataset like an imported one.
- Derived datasets are computed when first read and cached until one of their inputs or parameters changes; a failing or panicking computation shows an alert naming it and leaves the application running.
- New inputs: number, slider, text, select (fixed options or the distinct values of a column) and checkbox, each with a default and server-side validation.
- Typed state handles: widget constructors and computations take `State<T>` instead of a raw `StateKey` (breaking change for existing dashboards); `filtered(&dataset, &filter_state)` builds a filtered view.
- Polars is re-exported as `eario::polars`, with its lazy API enabled.
- The MCP endpoint lists derived datasets and reads only their cached result; it never starts a computation.
- Specification format version 2: dataset slots record whether they are imported or computed. Version 1 specifications are still read.
- New `signal-lab` example: a moving average and histogram of a time series, and a per-group summary with outlier exclusion.
- Minimum supported Rust version declared: 1.95, tested in CI.
- Scrollable tables (rows, statistics, dataset columns) can now be focused and scrolled with the keyboard.
- Browser tests in Chromium, with an accessibility check (WCAG 2.1 A/AA) of every visited state in both themes: `cargo xtask e2e`.
- Dependency licences and security advisories are checked with `cargo deny` in CI.
- `cargo xtask bench` records dated benchmark results in `docs/benchmarks/`.
