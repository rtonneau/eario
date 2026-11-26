# Ticket 04: mcp-derived-cache-only

**Status:** ✅ Done

## Local Test Result

`cargo test -p eario-mcp -p eario-runtime` (eario-mcp 2 passed, eario-runtime 46 passed), then `cargo xtask check` (fmt, clippy, tests, docs, svelte-check, vitest 25 passed): exit 0. Output in .scratch/tests/2026-11-26__custom-rust-logic/04-test.txt and 04-check.txt.


**Correction pass:** reran `CARGO_BUILD_JOBS=4 cargo test -p eario-mcp -p eario-runtime && cargo xtask check`: eario-mcp 4 passed, eario-runtime 46 passed, xtask check exit 0 (vitest 25 passed). Output in 04-test2.txt and 04-check2.txt.

## Review Notes

- Rule lives in App: new `cached_dataset_frame` and `cached_component_data` (an internal `Access` mode threaded through `slot_table`, `resolve_view`, `dataset_info`); a derived dataset without a current cached table gives new `AppError::NotComputed` (code `dataset_not_computed`, HTTP 409) or `ComponentData::Empty` "Not computed yet: ...".
- MCP tools preview_dataset, dataset_statistics, get_component_data call only those. `describe_dashboard` gains `derived_datasets` (id, computation, status); `list_datasets` already showed computation (`source_name`) and `derived` status via `App::datasets()`.
- Tests in eario-mcp cover not-computed (counted computation run 0 times across all MCP tools) and cached (run once, by a dashboard read, MCP reads reuse it).
- Ran `cargo fmt` after xtask check flagged formatting.

**Correction pass:** added `stale_cache_is_reported_as_not_computed` (input re-imported after a UI read; all MCP reads say not computed yet, counter stays 1) and `chained_derived_datasets_are_never_computed` (a -> b, nothing computed; MCP reads of b and a statistics component over b leave both counters at 0). No production code changed. The docs finding (P3) is deferred to ticket 08 by plan.

**Parent review record.** Base a346cb2cb18f7a66942073ad19e70a148b4f5598. Independent reviewer (sonnet, medium, fresh context): 0 correctness findings; P3 test gap (stale cache, chained derived datasets) accepted and fixed in one correction pass (tests only); P3 docs (project-status, ADR 0009) rejected here: owned by ticket 08 by plan. Correction diff is test-only and was assessed by the parent directly (no re-review): both new tests pass (04-test2.txt), `cargo xtask check` passed (04-check2.txt). Accepted.

## Blockers / Challenges

None

## Commits

- 3ec0627 feat: MCP reads derived datasets from the cache only (ticket 04)

## Time Spent

19m (ticket-start.js to ticket-complete.js)
