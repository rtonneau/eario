# Ticket 06: m2-docs

**Model:** fast
**Effort:** low

**Acceptance Criteria:**
- [ ] `docs/project-status.md`: real-browser e2e, accessibility check, cargo deny, the minimum Rust version and benchmarks are listed as verified; the "Not verified" items that are now covered are removed or updated.
- [ ] `docs/roadmap.md`: M2 is marked done.
- [ ] AGENTS.md command list includes `cargo xtask e2e` and `cargo xtask bench`.
- [ ] README mentions only what works.
- [ ] `cargo xtask check` passes.

**Files to Touch:**
- `docs/project-status.md`
- `docs/roadmap.md`
- `AGENTS.md`
- `README.md`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
Exit code 0.

**Notes:**

Only document commands that exist.
