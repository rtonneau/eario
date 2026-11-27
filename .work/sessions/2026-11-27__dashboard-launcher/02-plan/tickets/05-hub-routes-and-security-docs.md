# Ticket 05: hub-routes-and-security-docs

**Model:** medium
**Effort:** high

**Acceptance Criteria:**
- [ ] Routes (no body, no query; each segment validated): `GET /catalogs`, `POST /catalogs/{owner}/{repo}/add|remove`, `POST /sources/{owner}/{repo}/preview`, `POST /sources/{owner}/{repo}/install/{sha256}`, `POST /sources/{owner}/{repo}/accept-key/{fingerprint}`, `POST /check`, `POST /dashboards/{slug}/update`, `POST /dashboards/{slug}/rollback`, `GET|POST /settings/check-at-start/{on|off}`.
- [ ] HTTP tests: invalid segments → 400, missing token → 401, foreign Origin → 403, install without a matching preview refused.
- [ ] ADR 0013 status Accepted with its consequences; ADR 0011 points to it; `docs/architecture/security.md` threat table and exception section, and AGENTS.md security bullets, describe the widened exception and the outbound-network rule for the hub only.

**Files to Touch:**
- `crates/eario-hub/src/http.rs`
- `crates/eario-hub/tests/hub.rs`
- `docs/adr/0013-github-install-and-update.md`
- `docs/adr/0011-standalone-and-hub.md`
- `docs/adr/README.md`
- `docs/architecture/security.md`
- `AGENTS.md`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
fmt, clippy, tests, docs, svelte-check and vitest all pass.

**Notes:**

Response types come from ticket 1. Keep handlers thin adapters over the supervisor.
