# Session: dashboard-launcher

**Date:** 2026-11-27T07:20:41.277Z
**Status:** Grill phase complete

## Problem Statement

The Eario hub (M3) installs dashboards only from a local executable run with `--eario-install`. Users want the hub to work like a launcher (model: ArtCraft Launcher, `storytold/craft-launcher`): discover dashboards hosted in other GitHub repositories, install them, be told of new releases and update them, and update the hub itself, without a toolchain.

## Context & Constraints

- **Current behavior:** `--eario-install` copies the running executable into `hub/bin/<slug>/` and records size, SHA-256 and manifest in `registry.json`. The hub's HTTP API takes a slug only, reads no body or query, and can only list, start, open, stop and uninstall. Registry entries are added outside the HTTP API. No crate makes outbound network requests (AGENTS.md local-first rule). Updates are in place, no side-by-side copy (ADR 0011). The release workflow builds data-explorer for Linux and Windows x86_64 into a draft release; the hub is not published.
- **Pain point:** no way to find or install a dashboard from another repository, no update notice, no rollback, no hub update.
- **Dependencies:** keep the launch contract, slug-only start route, size/hash check before each start, one hub per user, children outliving the hub, local installs working. Amend ADR 0011 and the local-first rule for the hub only (ADR 0013). Update AGENTS.md and `docs/architecture/security.md` in the same change.
- **Tech stack:** Rust workspace (MSRV 1.95, no `unsafe`, thiserror, tracing), `eario-hub` (axum, tokio), `eario-core` contract types with ts-rs bindings, Svelte 5 hub view, Playwright + axe, cargo-deny. New dependencies to justify: a blocking HTTPS client with rustls (e.g. `ureq`), a minisign verifier (e.g. `minisign-verify`), run through `spawn_blocking`.

## Success Metrics

- A user adds a catalog or an `owner/repo`, confirms repo, version, key fingerprint and hash, and the hub installs the release asset for its target; the dashboard then starts like a local install. Verified against a fake GitHub server on loopback in Rust tests and in Playwright.
- A tampered asset, a bad or missing signature, a changed key (not yet accepted), a slug clash, a manifest slug or protocol mismatch, an oversized download or a non-GitHub host is refused before anything is written to `bin/<slug>/`; each refusal has a test.
- At hub start (unless the preference is off) and on "Check now", the hub reports available updates; Update replaces the copy (refused while running) and keeps one previous version; Rollback restores it. Offline shows "could not check".
- The hub updates itself from a signed `rtonneau/eario` release ("Update and restart"); running children keep running.
- Eario's release workflow publishes signed data-explorer, signal-lab and hub assets for Windows x86_64, Linux x86_64, macOS aarch64 and macOS x86_64 through the reusable workflow dashboard repos use; documented catalog format with an example `catalog.json`.
- `cargo xtask check`, `cargo xtask e2e` and `cargo deny check` pass.

## Architecture & Approach

- **Source** on each registry entry: `local` or `github { repo }`, plus pinned key. Local entries never update from GitHub.
- **Catalog:** `catalog.json` in a GitHub repo (entries: repo, name, description, optional minisign public key). Default `rtonneau/eario-catalog`; user adds/removes others by `owner/repo`.
- **Release asset convention:** `<slug>-<target triple>[.exe]` + `SHA256SUMS.txt` + `SHA256SUMS.txt.minisig`. Latest non-draft, non-prerelease release only.
- **Verification before install:** HTTPS to GitHub hosts only (API, github.com, release-asset CDN), size cap, minisign signature with the pinned key (catalog key, else trust on first use after fingerprint confirmation; key change refused until accepted), SHA-256 from the signed list, then `--eario-manifest` of the downloaded file must match slug and protocol. Write to `bin/<slug>/` only after all pass.
- **Updates:** check at every start (preference to turn off) and on demand; notify; Update on click, refused while running; one previous version kept in `bin/<slug>/`; Rollback once.
- **Self-update:** hub release signed with the Eario key compiled into the hub; verify, rename the running exe aside (Windows), place the new one, restart; children untouched.
- **Interface:** hub view and `eario-hub` CLI both: add/remove catalog, add repo, install, update, rollback, check, self-update. New routes take a strictly validated `owner/repo` or slug, never a URL, path or arguments; behind token and Origin check; install requires a two-step confirm (preview, then confirm by the previewed hash).
- **Network code** in `eario-hub` only, behind a small GitHub client trait so tests use a fake loopback server; the API base URL can be overridden by a debug-only environment variable (release builds ignore it, like `EARIO_DEV_ORIGIN`).
- **Publishing:** reusable GitHub Actions workflow in this repo that builds the four targets, writes and signs `SHA256SUMS.txt`, and attaches assets; Eario's own release uses it. Example catalog and author docs.

## Assumptions & Trade-offs

- Public repositories only; no token storage (private repos are a known limitation).
- Trust on first use for repos added by hand: the first key seen is trusted once the user confirms its fingerprint.
- One previous version only, no general side-by-side install.
- No automatic application of updates; every change needs a click or a CLI command.
- macOS builds are unsigned and never run before (Gatekeeper warnings); verified only by CI building them.
- Manual steps left to the user: create `rtonneau/eario-catalog`, generate minisign keys and add them as Actions secrets, push tags/publish releases.
- Not doing: building from source, archives, signatures beyond minisign, a curated marketplace inside Eario itself.

## Open Questions

- Exact crate choices (HTTP client, minisign verifier) are settled in the plan and justified in the PR, after cargo-deny confirms their licences.

## Notes

- Model studied: craft-launcher downloads official GitHub release assets, checks SHA-256 against `SHA256SUMS.txt`, never replaces a running app, offers "Update and restart" for itself, and has a loopback local-mirror dev mode. Its catalog docs could not be read (HTTP 503).
- Glossary terms added: Source, Catalog, Release asset, Pinned key, Previous version, Self-update. ADR 0013 drafted (status Proposed).
- Sized high-risk: downloads and runs executables from the network, widens the hub's security exception.
