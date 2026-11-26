# Ticket 07: signal-lab-example

**Model:** medium
**Effort:** medium

**Acceptance Criteria:**
- [ ] `cargo xtask fixtures` generates synthetic fixtures: a time series (time, signal columns with noise) and a categorical table (category, value columns with a few outliers).
- [ ] `examples/signal-lab` is a workspace member using the public API only, with two pages: Signal analysis (moving average via slider, histogram bins via number input, a histogram computed from the smoothed derived dataset) and Group summary (category via select, per-group aggregates, outlier exclusion via checkbox).
- [ ] Computations use Polars through `eario::polars`.
- [ ] Unit tests: the spec validates, and each computation produces the expected columns on a small frame.

**Files to Touch:**
- `examples/signal-lab/Cargo.toml`
- `examples/signal-lab/src/main.rs`
- `Cargo.toml` (workspace members)
- `xtask/src/fixtures.rs`
- `fixtures/csv/` (new generated fixtures)

**Verification Step:**

Run:
```bash
cargo xtask fixtures && cargo test -p signal-lab && cargo xtask check
```

Expected:
Fixtures regenerate deterministically, example tests pass, all checks pass.

**Notes:**

Fixtures are synthetic and deterministic (seeded). Running the example in a browser is manual; say so in the summary if not done.
