# Ticket 09: author-and-catalog-docs

**Model:** fast
**Effort:** medium

**Acceptance Criteria:**
- [ ] `docs/development/publishing-a-dashboard.md`: key generation, secrets, calling the reusable workflow, asset convention, adding to a catalog.
- [ ] Catalog format documented, with an example `docs/catalog/catalog.json` listing data-explorer and signal-lab (keys as placeholders).
- [ ] `docs/project-status.md`, `docs/roadmap.md` (M11 "updating from the hub" done, private repos and a tray still open), README and `docs/architecture/overview.md` updated; only existing commands documented.

**Files to Touch:**
- `docs/development/publishing-a-dashboard.md`
- `docs/catalog/catalog.json`
- `docs/project-status.md`
- `docs/roadmap.md`
- `docs/architecture/overview.md`
- `README.md`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
Passes.

**Notes:**

List the manual steps left to the user: create `rtonneau/eario-catalog`, generate keys, set secrets, replace the placeholder public key, tag a release.
