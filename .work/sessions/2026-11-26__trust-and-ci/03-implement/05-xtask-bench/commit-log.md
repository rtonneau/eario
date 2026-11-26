# Ticket 05: xtask-bench

**Status:** ✅ Done

## Local Test Result

`cargo xtask bench`: exit 0, printed `Benchmark results: docs/benchmarks/2026-11-26.md`. The file lists all 6 benchmarks of `tabular.rs` (chart_data, csv_import, filter, statistics, table_page, xlsx_import) with mean and 95 % interval, plus CPU (i9-10900X), cores 10/20, 32 GiB, Windows 11 Pro, rustc 1.99.1, commit 897efe596cc0. Log: `.scratch/tests/2026-11-26__trust-and-ci/05-bench.log`. `cargo test -p xtask`: 4 passed. `cargo clippy -p xtask --all-targets -D warnings` clean.

## Review Notes

Base: 6557279 (code) / 897efe5 (HEAD with session record). Pre-existing, not owned: `.work/work-map.*`.

- `xtask/src/bench.rs`: runs `cargo bench -p eario-data`, then reads `target/criterion/*/new/{benchmark,estimates}.json` modified since the run started (stale directories of earlier runs or removed benchmarks are skipped). Honours `CARGO_TARGET_DIR`. Fails if no result is found.
- The date comes from the system clock (UTC, Hinnant days-to-civil), so a run on the same day overwrites that day's file.
- Machine information comes from `sysinfo` 0.39 with feature `system`, as a direct xtask dependency. It is already in the tree through polars-utils with the same feature, so no new crate is compiled. `serde_json` (workspace) parses criterion's JSON.
- The commit is marked dirty, which is correct: the run happened before this ticket was committed, and the user's work-map edits are uncommitted. The measured code (`eario-data`) is unchanged since 897efe5.
- Unit tests cover date conversion, duration units, estimate parsing (including malformed input) and report rendering.
- Guide: commands table and Benchmarks section.

## Blockers / Challenges

None.

## Commits

- 52a559f feat(xtask): record dated benchmark results (ticket 05)

## Time Spent

8m (ticket-start.js to ticket-complete.js)
