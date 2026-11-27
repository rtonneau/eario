# Ticket 10: several-dashboards-per-repo

Added during ship (27 November 2026) by the user's decision: one GitHub repository may host several dashboards, so Eario's own release (data-explorer, signal-lab) can be installed from the default catalog. Shipped before ticket 09.

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `CatalogEntry` gains an optional `slug`; `Source::Github` records the slug it was installed with when the repository hosts several (or always: choose the simpler model and keep registry format 2 readable); bindings regenerated.
- [ ] A preview may name the slug: the hub picks exactly `<slug>-<triple>[.exe]` (via `verify::asset_name`); without a slug, today's rule (a single asset ending in `-<triple>`) still applies and several matches stay `hub_ambiguous_asset`, with a message saying to choose a slug.
- [ ] Routes `POST /sources/{owner}/{repo}/{slug}/preview`, `.../{slug}/accept-key/{key_sha256}`, `.../{slug}/install/{sha256}` (or an equivalent path-segment-only form), validated like the others; the two-segment routes keep working.
- [ ] Update, check and rollback of an entry installed by slug pick the same asset; two dashboards from one repository can be installed side by side and update independently; the pinned key is per repository and shared consistently (a key change for one is a key change for both).
- [ ] CLI `eario-hub install <owner/repo> [--slug <slug>]`; catalog installs pass the entry's slug.
- [ ] Hub view: catalog entries with a slug install that dashboard; "Add repository" offers an optional slug field (labelled) or, on `hub_ambiguous_asset`, lets the user pick from the release's dashboard slugs.
- [ ] Tests at each level (Rust unit/integration against the fake GitHub with a two-dashboard release, HTTP routes, vitest, Playwright with axe); ADR 0013 and security.md updated.

**Files to Touch:**
- `crates/eario-core/src/launch.rs`
- `crates/eario-runtime/src/hub_dir.rs`
- `crates/eario-hub/src/remote.rs`
- `crates/eario-hub/src/supervisor.rs`
- `crates/eario-hub/src/http.rs`
- `crates/eario-hub/src/cli.rs`
- `crates/eario-hub/src/bin/fake-github.rs`
- `crates/eario-hub/tests/hub.rs`
- `frontend/src/lib/api.ts`
- `frontend/src/lib/components/` (hub components)
- `frontend/src/lib/generated/`
- `frontend/e2e/hub.spec.ts`
- `docs/adr/0013-github-install-and-update.md`
- `docs/architecture/security.md`

**Verification Step:**

Run:
```bash
cargo test -p eario-hub --all-features -j 4 && cargo xtask e2e
```

Expected:
All Rust tests pass, including two dashboards installed from one repository; e2e passes with no axe violation.

**Notes:**

The release workflow (ticket 08) already names assets `<slug>-<triple>[.exe]` and lists them all in one signed `SHA256SUMS.txt`, so no workflow change should be needed. Keep every route parameter a validated path segment; no body, no query.
