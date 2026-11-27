# Handoff: dashboard-launcher

**Session:** 2026-11-27__dashboard-launcher
**Saved:** 2026-11-27T16:30:32.691Z
**Current phase:** ship
**Active ticket:** 09-author-and-catalog-docs

## Where I Stopped

Ticket 10 (several-dashboards-per-repo, started with ticket-start.js 10, In Progress; the queue shows 09 as next only because 10 was added later) was being implemented by a delegated opus implementer, which was cut off by the 5-hour usage limit. Its partial, unverified changes (25 files) are in the working tree. Tickets 01–08 are done and committed.

## Reasoning So Far

Ticket 10 was added during ship by the user's decision (one repo may host several dashboards, chosen by slug) so Eario's examples can be installed from the default catalog; shipped before 09 so the docs describe the final system. Every ticket so far: delegated implementer, independent reviewer, at most one correction pass, re-review.

## Next Step

Resume ticket 10: inspect `git diff 6d955c1` for the partial work, run `cargo test -p eario-hub --all-features -j 4` and the ticket's other checks, finish the remaining acceptance criteria (fresh implementer with a recovery brief, recovery count 1), fill its commit log, review, then `ticket-complete.js 10`. Then ticket 09 (docs), then `/gps finish` (PR).

## Open Questions

None blocking. Manual steps left to the user after merge: create rtonneau/eario-catalog, generate minisign keys and set the Actions secrets, replace crates/eario-hub/eario-release.pub, tag a release.

## Settled Decisions (do not re-litigate)

Keys are confirmed by the SHA-256 of the full key line; an untrusted key never runs the executable; updates only to strictly newer signed versions; one previous version; the compiled Eario key is the sole trust anchor for self-update; routes take validated path segments only; signed builds use no caches and build the tag's commit. Local Node 22.14 cannot run vitest without the preload shim at .scratch/tests/2026-11-27__dashboard-launcher/evidence/07-zstd-shim.cjs; use -j 4 for cargo (memory).

## Uncommitted Work

- **Git status (project):**  M .github/workflows/release-dashboard.yml,  M crates/eario-core/src/launch.rs,  M crates/eario-hub/src/bin/fake-github.rs,  M crates/eario-hub/src/cli.rs,  M crates/eario-hub/src/http.rs,  M crates/eario-hub/src/remote.rs,  M crates/eario-hub/src/supervisor.rs,  M crates/eario-hub/tests/hub.rs,  M docs/adr/0013-github-install-and-update.md,  M docs/architecture/overview.md,  M docs/architecture/security.md,  M docs/development/guide.md,  M frontend/e2e/hub.spec.ts,  M frontend/src/lib/api.test.ts,  M frontend/src/lib/api.ts,  M frontend/src/lib/components/HubSources.svelte,  M frontend/src/lib/components/HubView.svelte,  M frontend/src/lib/components/HubView.test.ts,  M frontend/src/lib/generated/CatalogEntry.ts,  M frontend/src/lib/generated/HubCatalogEntry.ts,  M frontend/src/lib/hub.test.ts,  M frontend/src/lib/hub.ts,  M frontend/src/lib/session.svelte.ts
- **Git status (session dir):**  M .work/sessions/2026-11-27__dashboard-launcher/.session-config.json,  M .work/sessions/2026-11-27__dashboard-launcher/02-plan/plan.md, ?? .work/sessions/2026-11-27__dashboard-launcher/02-plan/tickets/10-several-dashboards-per-repo.md, ?? .work/sessions/2026-11-27__dashboard-launcher/03-implement/10-several-dashboards-per-repo/commit-log.md
- **Why not committed:** ticket 10 is incomplete and unverified (implementer cut off by the usage limit)

## Machine State (auto-filled)

- **Ticket queue:** 01-contract-and-registry-v2: done, 02-release-verification: done, 03-github-client: done, 04-supervisor-install-update: done, 05-hub-routes-and-security-docs: done, 06-hub-self-update: done, 07-hub-view-remote: done, 08-release-workflow: done, 09-author-and-catalog-docs: pending, 10-several-dashboards-per-repo: pending
- **Recent commits:** 6d955c1 chore(gps): ticket 08-release-workflow done, 20a7ec8 ci: reusable signed release workflow for dashboards; Eario releases the examples and the hub through it (ticket 08), c12dd2d chore(gps): ticket 07-hub-view-remote done, 6dccf8b feat: hub view for catalogs, GitHub installs with key trust, updates, rollback and self-update; e2e against a fake GitHub (ticket 07), a60ec33 chore(gps): ticket 06-hub-self-update done, a98a35f feat: hub self-update from releases signed with the compiled Eario key (ticket 06), 452dc0d chore(gps): ticket 05-hub-routes-and-security-docs done, caaa002 feat: hub routes for catalogs, install, update and rollback; ADR 0013 accepted, security docs updated (ticket 05), 43d30fe chore(gps): ticket 04-supervisor-install-update done, 76e6ad2 feat: hub installs, updates and rolls back dashboards from signed GitHub releases, with catalogs, key pinning and a CLI (ticket 04)
