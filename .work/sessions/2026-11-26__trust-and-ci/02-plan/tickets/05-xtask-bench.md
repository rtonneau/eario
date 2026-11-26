# Ticket 05: xtask-bench

**Model:** medium
**Effort:** medium

**Acceptance Criteria:**
- [ ] `cargo xtask bench` runs `cargo bench -p eario-data`, reads criterion's `estimates.json` under `target/criterion`, and writes `docs/benchmarks/YYYY-MM-DD.md`: a table of benchmark, mean and confidence interval, plus CPU, logical cores, RAM, OS, rustc version and git commit (with a dirty flag).
- [ ] Running it twice on the same day overwrites that day's file, not another day's.
- [ ] One result file from the development machine is committed.
- [ ] The development guide documents the command.

**Files to Touch:**
- `xtask/src/main.rs` (or `xtask/src/bench.rs`)
- `xtask/Cargo.toml`
- `docs/benchmarks/2026-11-26.md` (date of the run)
- `docs/development/guide.md`

**Verification Step:**

Run:
```bash
cargo xtask bench
```

Expected:
`Benchmark results: docs/benchmarks/<date>.md`, and the file lists every benchmark of `tabular.rs`.

**Notes:**

Get the machine information without a new dependency where possible: `sysinfo` is already in the lock file, but check whether it is a direct dependency before adding it to xtask, and justify it. Take the date from the system clock. A unit test should cover the Markdown rendering from parsed estimates.
