# Ticket 09: author-and-catalog-docs

**Status:** ✅ Done

## Local Test Result

`CARGO_BUILD_JOBS=4 cargo xtask check` (`.scratch/tests/2026-11-27__dashboard-launcher/evidence/09-xtask-check.txt`): fmt, clippy `-D warnings`, the Rust tests (407 passed, 0 failed, doc tests included, the new `catalog_example` among them), `cargo doc` and `npm run check` (406 files, 0 errors, 0 warnings) passed. It stops at `npm run test` with the known Node 22.14 gap (11 "Failed to start forks worker"), exit 1. Vitest with the zstd shim from `evidence/07-zstd-shim.cjs`, from `frontend/`: 11 files, 113 tests passed.
New doc test: `cargo test -p eario-hub --test catalog_example`: 2 passed (the example reads as `eario_core::Catalog`, format version 1, slugs data-explorer and signal-lab, repos and slugs valid; the placeholder key is refused by `PublicKey::from_base64` with `release_invalid_key`, and both entries carry the same key).
Link check of the six changed or new documents (every relative link and anchor resolves): no finding. Commands and paths documented were checked by grep against `eario-hub help` (cli.rs USAGE), `cargo xtask help`, the workflow files, `hub_dir.rs` file names and the hub routes in http.rs.
Not run: `cargo xtask e2e` (no code or frontend changed; ticket 10 reported 15/15).

## Review Notes

Parent: base cdcf799cebb03c3364a53bb22761d8b2ee5f5340; tree clean except session files. Mode delegate (implementer sonnet/high). Recovery count: 0.

AC1 (`docs/development/publishing-a-dashboard.md`): key generation (`minisign -G -p minisign.pub -s minisign.key`, `-W`, `rsign generate`), the secrets `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD` (names from release-dashboard.yml), a caller YAML with the real inputs, the asset convention, `SHA256SUMS.txt`, `.minisig`, `minisign.pub`, versions and tags, several dashboards by slug, a user's flow, how to get into a catalog. The key-generation and secrets text now lives only there: guide.md's "Publishing a release" links to it and keeps Eario's own release.
Ref to pin: a full commit SHA of `rtonneau/eario` that contains the workflow (v0.2.0 predates it and cannot be used), or a release tag once one has it; never a branch.
AC2: catalog format documented from `Catalog`/`CatalogEntry` (public_key is required, slug and description optional; unknown fields ignored). Placeholder question checked in remote.rs: the key is parsed only in `catalog_key` when a user previews an install, so a placeholder lists fine and its install fails with `release_invalid_key`; the whole catalog is not rejected. `public_key` cannot be omitted (a missing field makes the catalog unreadable), so the example keeps a placeholder text, which the doc explains. Example `docs/catalog/catalog.json` and test `crates/eario-hub/tests/catalog_example.rs`.
AC3: project-status (what works, verification, not verified, known limitations incl. every item the ticket lists, stale limitations removed: no install from the hub, no update action, the unlocked registry read-modify-write), roadmap (M11 updating from the hub done, with a section under M3; remaining: private repos and tokens, tray, OS signing and notarization, the first real release), README, overview (hub directory files, install-from-GitHub subsection). Only existing commands are documented. The maintainer's manual steps are listed once, in guide.md "What only the maintainer can do", and referenced from project-status, README-free of dates.
Found, not changed (outside the allowed scope): `.github/workflows/release.yml` still has the comment "The examples are downloads: a hub installs from a repository whose release has one executable per target", which ticket 10 made false. ADR 0011 line "There is no side-by-side install" is historical and was left. The guide step "raise the version" is needed because `v0.2.0` is already tagged and the workspace version is 0.2.0.

Parent review (inline; docs-only, low-risk, no independent reviewer spawned): read README and roadmap diffs, checked the documented 'Install from GitHub' label (HubSources.svelte:140) and `eario-hub help` (cli.rs:216) exist; catalog example test passes (2). Fixed the stale release.yml comment the implementer flagged (examples are now installed by slug); actionlint clean. Accepted.

## Blockers / Challenges

None. The only local limit is the known Node 22.14 gap at vitest, handled with the shim.

## Commits

- f658844 docs: publishing a dashboard, catalog format and example, status and roadmap for GitHub installs (ticket 09)

## Time Spent

19m (ticket-start.js to ticket-complete.js)
