# Ticket 08: m1-documentation

**Status:** ✅ Done

## Local Test Result

Ran `CARGO_BUILD_JOBS=4 cargo xtask check` (output: `.scratch/tests/2026-11-26__custom-rust-logic/08-check.txt`): fmt, clippy `-D warnings`, workspace tests (every binary `ok`: eario-core 63, eario-data 30 + import 12, eario-runtime 53, eario 18 + end_to_end 5, eario-mcp 4, signal-lab 9, doc-tests ok), `cargo doc`, svelte-check (`269 FILES 0 ERRORS 0 WARNINGS`), vitest (`44 passed`); `exit=0`. No code was changed, so the run covers the final tree. Commands named in the docs exist: `cargo xtask {setup,frontend,gen-types,fixtures,test,check,dist}` (match arms in xtask/src/main.rs) and `cargo run -p signal-lab -- fixtures/csv/time-series.csv fixtures/csv/groups.csv` (both fixture files are tracked). The guide example was compiled and `build()` run in a temporary example file of crates/eario (removed afterwards).

## Review Notes

AC1: ADR 0001 decision text, consequences and open question now point to ADR 0012; ADR 0002 marks the cache (per derived slot) and typed state as done and says imported/filtered views are still recomputed; ADR 0009 states MCP is cache-only. AC2: project-status lists the new capabilities, drops the two obsolete limitations and records the required limits (no progress/cancel, latest-result cache, MCP never computes, untested stale-version retries, handle reads, `&&str`/`Cow`, ColumnSelect trap, frontend rebuild); verification section and "Where to start next" updated with the real numbers. AC3: roadmap M1 done with what landed; overview has the new `DatasetSlot` shape, input kinds, computations, evaluation and cache, reconciliation, MCP cache-only; guide has a compiled example, memory tips (`CARGO_BUILD_JOBS=4`, `line-tables-only`); README lists the feature, the example and its run command, and no longer lists computations as planned. Claims checked against code: error codes (`derived_dataset`, `dataset_not_computed`, `unbound_state`, `select_depends_on_its_state`), `cached_dataset_frame`, `MAX_SELECT_OPTIONS`. ADR 0012 and docs/adr/README.md left unchanged.

**Parent review record.** Base 316e563c9e47dc64db30d6478f208ce2bc81a77e. Delegated implementer (sonnet, medium). No independent reviewer: documentation only. Parent checked the README diff (lists only built features; removed "custom Rust computations" from Planned; MCP cache-only stated) and that every `cargo xtask` subcommand and `cargo run -p` line mentioned in README.md and docs/ exists (xtask/src/main.rs). Also commits the design-phase ADR 0012 and its docs/adr/README.md row. Evidence 08-check.txt: `cargo xtask check` exit 0. Accepted.

## Blockers / Challenges

None. A first heredoc with an apostrophe failed to parse in Git Bash, so the overview edits went through the Edit tool; nothing had been applied. Not run: the example in a browser (signal-lab is covered by tests only).

## Commits

- 69e70d7 docs: M1 custom Rust logic, ADR 0012 and status (ticket 08)

## Time Spent

8m (ticket-start.js to ticket-complete.js)
