# Ticket 07: signal-lab-example

**Status:** ✅ Done

## Local Test Result

`cargo xtask fixtures` (run twice, `diff -r` against the first run: identical), `cargo test -p signal-lab`: 9 passed, `cargo xtask check`: exit 0 (fmt, clippy, Rust tests, docs, svelte-check, vitest 44 passed). Logs in `.scratch/tests/2026-11-26__custom-rust-logic/07-*.log`. The example was not started or opened in a browser (manual).

## Review Notes

- Fixtures: `time-series.csv` (300 rows, time/signal) and `groups.csv` (144 rows, category/dose/value, 4 planted outliers) in `xtask/src/fixtures.rs`, noise computed from the row number. A check outside the tests confirmed the 2.5-sigma per-dose rule removes exactly the 4 outliers.
- `examples/signal-lab` uses only `eario` (prelude, `eario::polars`); `serde_json` is a dev-dependency for test parameters. Moving average and histogram read columns into `Vec<f64>`; group summary uses the Polars lazy API (`filter`, `over`, `group_by`, `agg`). `smoothed` -> `histogram` is derived-of-derived.
- Charts use `line_chart` (no bar mark exists) with column selects for the axes, as data-explorer does.
- Tests: spec validates, app builds, each computation's columns and values on a small frame, and an import into the built app produces the chained histogram.

**Parent review record.** Base f1a2f6e2b6d5c4bf00dfbcdf5d0abbe76761e042. Delegated implementer (sonnet, medium). No independent reviewer: localized example crate using the public API only, plus fixtures and an xtask generator change; no change under crates/. Parent read examples/signal-lab/src/main.rs against the AC (two pages; slider-driven moving average; number-driven histogram computed from the smoothed derived dataset; select_from_column category; checkbox outlier exclusion; Polars via eario::polars; no unwrap/expect outside tests) and re-ran `cargo test -p signal-lab` (9 passed), clippy -D warnings and fmt --check for the crate, and `cargo xtask fixtures` (no diff). Not done: running the example in a browser. Accepted.

## Blockers / Challenges

None blocking. `Expr::over` returns a `Result` and `abs` is not enabled in Polars, so the outlier test compares squared deviation to squared limit. `cargo fmt` was needed once after writing the fixtures code.

## Commits

- 4f3de70 feat: signal-lab example with custom computations (ticket 07)

## Time Spent

14m (ticket-start.js to ticket-complete.js)
