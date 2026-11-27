# Implementation Plan

**Session:** dashboard-launcher
**Date:** 2026-11-27T07:34:06.365Z
**Estimated effort:** 4-6 days

## Strategy

Build from the inside out. First the contract and registry format, then the pure verification (signature, checksums, key pinning), then the GitHub client against a fake loopback server, then the supervisor services that combine them. Then the HTTP routes and security docs, then self-update, then the hub view. The publishing workflow and author docs come last. Every network-facing step is testable offline through the fake server, so `cargo xtask check` never needs the internet. Routes keep the existing invariant (no body, no query): repositories and hashes travel as path segments, each validated strictly.

## Tickets Overview

- **Ticket 1:** Contract and registry v2: `Source`, pinned key, previous version, catalog and update types in `eario-core`; registry format 2 reads format 1 as local entries; bindings regenerated.
- **Ticket 2:** Release verification: parse `SHA256SUMS.txt`, verify its minisign signature against a pinned key, key fingerprints, asset name for a target triple.
- **Ticket 3:** GitHub client: latest release, catalog file, bounded download from allowed HTTPS hosts; fake loopback server for tests; debug-only API override.
- **Ticket 4:** Supervisor services and CLI: catalogs, preview and confirmed install, check, update with one previous version, rollback, trust on first use, the check-at-start preference.
- **Ticket 5:** Hub HTTP routes for those services, plus ADR 0013 accepted and the security document, AGENTS.md and ADR 0011 amended.
- **Ticket 6:** Hub self-update: verify against the Eario release key compiled in, swap the executable, restart, leave children running.
- **Ticket 7:** Hub view: catalogs, add repository, install confirmation, update badges, rollback, check now, setting, self-update; Vitest and Playwright with axe against the fake server.
- **Ticket 8:** Reusable release workflow: four targets, signed `SHA256SUMS.txt`; Eario's release uses it for data-explorer, signal-lab and the hub.
- **Ticket 9:** Author and catalog docs: publishing a dashboard, catalog format with an example `catalog.json`, project status, roadmap, README.
- **Ticket 10** (added during ship, user decision 27 Nov 2026; shipped before 9): several dashboards per repository, selected by slug, so the default catalog can list Eario's own examples.

## Sequencing Rationale

Contract types (1) are needed by everything. Verification (2) and the client (3) are independent leaves that the supervisor (4) composes. Routes (5) adapt the supervisor, and that ticket is where the security exception widens, so the docs ship with it. Self-update (6) reuses 2–4. The view (7) needs the routes. The workflow (8) produces what the hub consumes, and docs (9) describe the finished system.

## Risks & Mitigation

- **Risk:** a compromised or spoofed release runs code on the user's machine → **Mitigation:** minisign signature with a pinned key, SHA-256 from the signed list, HTTPS to GitHub hosts only, nothing written to `bin/` before every check passes, a test for each refusal.
- **Risk:** a malicious web page drives the new install routes → **Mitigation:** routes stay behind the token and Origin check, take only validated path segments, and install needs the previewed hash as a second step.
- **Risk:** tests depend on the network or on GitHub → **Mitigation:** the client sits behind a trait and is tested against a fake loopback server; the base-URL override exists in debug builds only.
- **Risk:** self-update leaves a broken hub on Windows → **Mitigation:** the running exe is renamed aside and restored if placing the new one fails; the `.old` file is removed at the next start.
- **Risk:** new dependencies fail cargo-deny or the MSRV → **Mitigation:** ticket 3 checks `cargo deny check` and the 1.95 build, and records each crate's justification.
- **Risk:** macOS targets have never been built → **Mitigation:** only CI verifies them (ticket 8); listed as unverified at run time.

## Assumptions

- Public GitHub repositories only, unauthenticated API (60 requests/hour is enough for a check at start).
- The user creates `rtonneau/eario-catalog`, generates the minisign keys, stores them as Actions secrets, and replaces the placeholder Eario public key; until then self-update reports "not configured".
- A downloaded executable is run with `--eario-manifest` only after its signature and hash have been verified.
- Local installs (`--eario-install`) keep working and are never updated from GitHub.
