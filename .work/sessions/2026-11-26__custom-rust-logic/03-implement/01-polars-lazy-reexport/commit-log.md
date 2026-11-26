# Ticket 01: polars-lazy-reexport

**Status:** ✅ Done

## Local Test Result

`cargo xtask check` → exit 0 (second run). fmt, clippy, all Rust tests (eario-runtime 29 passed, doctests 2 passed incl. new `polars` lazy doctest), docs, svelte-check, vitest 25 passed. Output: `.scratch/tests/2026-11-26__custom-rust-logic/01-check.txt`.

## Review Notes

Base: e58c1801f114dfe536fc522fced86a79ff625ad3. Pre-existing uncommitted files not owned by this ticket: `.gitignore` (gps), `docs/adr/0012-*.md` and `docs/adr/README.md` (grill output, committed with ticket 08). Inline, low-risk, no independent review. AC1: workspace polars features `csv, fmt, lazy`, default-features=false. AC2: `eario_data::polars` and `eario::polars` re-exports, documented. AC3: doctest on `eario::polars` builds a frame with `df!` and runs a lazy filter. Build impact: `lazy` adds polars-plan/expr/lazy/stream/mem-engine crates to a clean build.

## Blockers / Challenges

First `cargo xtask check` failed with `crate glob/fastrand required to be available in rlib format` (stale artifacts after the feature change); the immediate re-run passed without changes.

## Commits

- 37160a7 feat: enable Polars lazy and re-export it as eario::polars (ticket 01)

## Time Spent

8m (ticket-start.js to ticket-complete.js)
