# Ticket 08: m1-documentation

**Model:** fast
**Effort:** medium

**Acceptance Criteria:**
- [ ] ADR 0001's "Custom computations" open question points to ADR 0012; ADR 0002's cache and typed-state paths are marked done.
- [ ] `docs/project-status.md` lists the new capabilities and removes the "only built-in components" limitation; remaining limits (no progress/cancel, cache-only MCP) recorded.
- [ ] `docs/roadmap.md` marks M1 done; `docs/architecture/overview.md` describes computations and derived datasets; `docs/development/guide.md` shows how to write a computation; README lists the feature and the new example.

**Files to Touch:**
- `docs/adr/0001-dashboard-specification.md`
- `docs/adr/0002-backend-frontend-separation.md`
- `docs/project-status.md`
- `docs/roadmap.md`
- `docs/architecture/overview.md`
- `docs/development/guide.md`
- `README.md`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
All checks pass; every command mentioned in the docs exists.

**Notes:**

The README lists only what works. Do not document planned features as available.
