# Ticket 07: docs-and-dist

**Model:** fast
**Effort:** medium

**Acceptance Criteria:**
- [ ] `docs/project-status.md`, `docs/roadmap.md` (M3 done, what was left out), `docs/architecture/overview.md` (launch contract, hub, data flow, HTTP API of the hub), `README.md` (only what works) and `docs/development/guide.md` updated.
- [ ] `cargo xtask dist` also builds the hub release executable and prints its path.
- [ ] The full definition of done passes: `cargo xtask check`, `cargo xtask e2e`, `cargo deny check`, MSRV tests.

**Files to Touch:**
- `docs/project-status.md`
- `docs/roadmap.md`
- `docs/architecture/overview.md`
- `docs/development/guide.md`
- `README.md`
- `xtask/src/main.rs`

**Verification Step:**

Run:
```bash
cargo xtask check && cargo xtask e2e && cargo deny check && cargo +1.95 test --workspace --all-features
```

Expected:
All pass.

**Notes:**

Record in project-status what was verified by hand (tray, console-free start) versus by tests, and the limits (no tray on Linux/macOS, no install from the hub interface, no side-by-side versions).
