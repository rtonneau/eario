# Ticket 01: polars-lazy-reexport

**Model:** fast
**Effort:** low

**Acceptance Criteria:**
- [ ] Workspace Polars dependency enables `lazy` in addition to `csv` and `fmt`, still with `default-features = false`.
- [ ] `eario_data` re-exports `polars` and the facade exposes it as `eario::polars` with a doc comment.
- [ ] A doctest or unit test in `eario` builds a `DataFrame` through `eario::polars` and runs one lazy expression.

**Files to Touch:**
- `Cargo.toml`
- `crates/eario-data/src/lib.rs`
- `crates/eario/src/lib.rs`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
All steps pass (fmt, clippy -D warnings, tests including the new lazy test, docs, svelte-check, vitest).

**Notes:**

Justify the feature in the PR (what it enables, build-time impact). Do not add other Polars features.
