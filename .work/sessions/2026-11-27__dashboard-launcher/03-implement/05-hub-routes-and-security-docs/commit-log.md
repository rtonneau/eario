# Ticket 05: hub-routes-and-security-docs

**Status:** ✅ Done

## Local Test Result

Evidence in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/`, prefix `05-`, one file per command. Every Rust command ran on the final code (the last edit after them was to `AGENTS.md` wording only).

- `cargo test -p eario-hub --all-features -j 4`: exit 0. 70 unit tests, 30 integration tests (4 new), 0 failed (`05-hub-tests.txt`).
- `cargo test --workspace --all-features -j 4`: exit 0, every test binary `ok`, 0 failed (`05-workspace-tests.txt`).
- `cargo fmt --all -- --check`: exit 0 (`05-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings`: exit 0 (`05-clippy.txt`).
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features -j 4`: exit 0 (`05-doc.txt`).
- `npm run check` in `frontend/`: 399 files, 0 errors, 0 warnings, exit 0 (`05-npm-check.txt`).
- `cargo xtask gen-types`: exit 0, and `git status frontend/` shows no change afterwards (`05-gen-types.txt`). No contract type was added or changed.
- `cargo check -p eario-hub --lib --bins` (the hub as shipped, without `fake-github`): exit 0 (`05-check-without-feature.txt`).
- `cargo xtask check` (the ticket's Verification Step):
  - Plain: exit 1 at its `cargo test --workspace --all-features` step (`05-xtask-check.txt`). fmt and clippy passed. The test step rebuilt the workspace at full parallelism and failed with `crate ... required to be available in rlib format, but was not found` for polars, ring and schannel crates. That is the memory pressure of ticket 04, not a code error: the same tests pass with `-j 4`.
  - With `CARGO_BUILD_JOBS=4`: fmt, clippy, the workspace tests (22 `test result: ok`, 0 failed), doc and svelte-check all passed. It stops at `npm run test` (vitest): `Failed to start forks worker ... TypeError: Cannot read properties of undefined (reading 'bind')` in `undici/lib/interceptor/decompress.js`. That is the known Node 22.14 gap (`05-xtask-check-jobs4.txt`).
- Mutation check (`05-mutate.py`, output `05-mutation-check.txt`): 5 of 5 mutations of `http.rs` killed by the new tests, and the file was restored byte for byte:
  - `on|off` accepting anything;
  - `hub_no_preview` mapped to 409;
  - the `update/{sha256}` route removed;
  - `github_invalid_repo` falling through to 502;
  - `POST /check` answering without checking.
- Not run:
  - vitest: Node 22.14, as in tickets 01 to 04.
  - `cargo xtask e2e`: no frontend view or example changed.
  - `cargo deny check`: no dependency changed.
  - The MSRV build (`cargo +1.95.0`): the change adds handlers and tests only, with no new language feature or dependency.
  - macOS and Linux: only Windows x86_64 was run.

## Review Notes

Parent: base 43d30fe2222b77bc60c3cd19f3e01f3856a57bb4; tree clean except session files (.work/GLOSSARY.md wording fix by parent). Mode delegate (implementer sonnet/high). Recovery count: 0.

Implementer self-check against the Acceptance Criteria (review rounds: 0 so far).

1. **Routes** (`crates/eario-hub/src/http.rs`). They are thin adapters over the supervisor of ticket 04. Each segment is validated by the supervisor method before it acts (`check_slug`, `check_repo`, `check_sha256`); `on|off` is parsed in the handler. Blocking work stays in the supervisor's `spawn_blocking`. No route reads a body or query. All of them sit in `routes()`, so `eario_runtime::start_routes` puts them behind the Host/Origin check and the token. The module doc lists every route. The routes:
   - `GET /catalogs` returns `HubCatalog[]` (`catalog_listing`).
   - `POST /catalogs/{owner}/{repo}/add` and `/remove` return the listing again.
   - `POST /sources/{owner}/{repo}/preview` returns an `InstallPreview`.
   - `POST /sources/{owner}/{repo}/accept-key/{key_sha256}` returns the full `InstallPreview`. Per ticket 04 it takes the key's SHA-256, not a fingerprint.
   - `POST /sources/{owner}/{repo}/install/{sha256}` returns `HubInfo`.
   - `POST /check` returns `HubInfo`, whose `last_check` is the new check.
   - `POST /dashboards/{slug}/update-preview` returns an `InstallPreview`.
   - `POST /dashboards/{slug}/update/{sha256}` returns `HubInfo` (`apply_update`).
   - `POST /dashboards/{slug}/update` returns `HubInfo` (`update`, one step; it is refused with `hub_key_changed` when the key changed and keeps its preview for accept-key).
   - `POST /dashboards/{slug}/rollback` returns `HubInfo`.
   - `GET /settings` and `POST /settings/check-at-start/{on|off}` return `HubSettings`.

   Deviations from the spec's list:
   - `accept-key` takes `{key_sha256}`.
   - `update-preview` and `update/{sha256}` were added, because a changed key on an update needs preview, then accept, then apply by hash.
   - "GET|POST /settings/check-at-start/{on|off}" was read as `GET /settings` plus `POST .../{on|off}`.
   - No new contract type. Mutating routes answer `HubInfo` or the catalog listing, so the view refreshes with one request.
2. **Status mapping.**
   - 400: `hub_invalid_slug`, `github_invalid_repo`, `hub_invalid_sha256`, and the new handler-level `hub_invalid_switch`.
   - 404: `hub_not_installed`, `hub_no_preview`, `hub_preview_gone`, `hub_unknown_catalog`.
   - 409: key not accepted, changed or mismatched; preview mismatch; slug taken; already installed; not newer; not comparable; no previous; previous unusable; changed meanwhile; running or starting.
   - 502: what GitHub served was refused (verify errors, assets, manifest, catalog version) and GitHub failures.
   - 504: `github_timeout` (new) and `hub_stop_timeout`.
   - 500: local failures (disk, certificates, spawn, task).
   - The ticket 04 placeholder comment was removed.
3. **HTTP tests** (`crates/eario-hub/tests/hub.rs`, 4 new, against the fake GitHub, through a real `eario_hub::start` server):
   - `remote_routes_take_strict_path_segments_and_nothing_else`. Bad `{owner}/{repo}` gives 400 on all 5 repository routes; this includes `%2F` and `%5C` inside a segment, `..`, `.`, a space, NUL, `https:` and a 40-character owner. Bad hashes give 400 on 3 routes; they include upper case, 63 and 65 digits, non-hex and a 16-digit key id. Bad slugs give 400 on 4 routes. Bad switches give 400. Extra segments give 404 `unknown_endpoint`. A query and a JSON body are ignored. None of these reached the fake GitHub, and no catalogs file or registry entry was written. For all 13 new routes: no token or a wrong token gives 401, and a foreign `Origin` (`https://evil.example`, `http://localhost:1`, `null`) with the right token gives 403.
   - `an_install_over_http_needs_the_preview_its_hash_and_an_accepted_key`: install with no preview gives 404. After a key-pending preview (`unknown`, no manifest), a wrong hash gives 409 `hub_preview_mismatch`, another repository gives 404, and the right hash gives 409 `hub_key_not_accepted`; nothing is in `bin/`. A wrong key hash, or the asset hash used as a key hash, gives 409 `hub_key_mismatch`. The right key hash gives the full preview; the install then gives `HubInfo` with the GitHub entry and its key pinned. The preview is used up (404), a second preview gives 409 `hub_already_installed`, and the dashboard starts.
   - `updates_checks_and_rollbacks_run_over_http`:
     - rollback with no previous version: 409;
     - update of an unknown slug: 404;
     - check: available version reported in `last_check` and on the dashboard;
     - update-preview (`pinned`), a wrong hash (409), then apply by hash;
     - one-step update when nothing is newer: 409 `hub_not_newer`;
     - update and rollback while running: 409 `hub_running`;
     - one-step update;
     - rollback restores the previous copy; a second rollback gives 409;
     - a new signing key: a one-step update gives 409 `hub_key_changed`, update-preview gives `changed` with no manifest, apply gives 409, accept-key by the key's SHA-256 succeeds, then apply installs it and pins the new key.
   - `catalogs_and_the_check_at_start_setting_change_over_http`: list, add (a repository without a catalog gives 502 `github_not_found`), remove, remove again (404), the stored list, and the setting off and on (stored). A check with nothing from GitHub makes no request.
4. **Docs.**
   - ADR 0013: Accepted (27 November 2026), with the decisions as built and the consequences. "Fingerprint" is gone; keys are confirmed by the full key line's SHA-256. Self-update is said to be not built yet.
   - ADR 0011: status line, plus an amendment section pointing to 0013 (install over HTTP allowed under its rules; one previous version for GitHub entries).
   - `docs/adr/README.md`: entry for 0013.
   - `docs/architecture/security.md`:
     - 10 threat rows rewritten or added (the 7 asked for, plus key-pending execution and the two widened rows);
     - the hub section renamed and its bullets updated;
     - a new subsection with the routes, the CLI, the trust order and the check order, the outbound network rule (hub only, 4 hosts, HTTPS, default port, 5 redirects, caps, timeouts, start check opt-out, debug-only `EARIO_HUB_GITHUB_API`), and the lock and staging;
     - checklist updated, with an outbound-request item added.
   - `AGENTS.md`: the local-first bullet, the repository line, the crate-boundary note (network code only in `eario-hub`'s `github.rs`) and the hub-exception bullet.
   - Outside the ticket's list: `docs/architecture/overview.md`. Its hub route table said "the only handle they accept is the slug" and "installing is not an API route", which this change made false. The new routes were added to the table, and the crate row and the closing sentence were fixed. Roadmap, project status and README were not touched (ticket 09), nor the frontend (ticket 07).

For later tickets:
- **Ticket 07.** Install takes preview, then (when `manifest` is null) accept-key, then install by `sha256`. Update takes update-preview, then (when the key changed) accept-key on `/sources/{owner}/{repo}/...`, then `update/{sha256}`. The one-step `update` suits a pinned key. `GET /catalogs` reads every catalog from GitHub on each call, and unauthenticated GitHub allows about 60 requests per hour, so the view should not poll it.

Parent note: implementer ran as opus/high (hint was medium; raised for a security-relevant ticket).
Independent review 1 (opus/high, base 43d30fe): 4 findings, no P0/P1. P2 docs claimed every install needs SHA-256 confirmation but the one-step update route applies directly; P3 'before any download' false; P3 'only network code' false (runtime has a loopback client); P3 no foreign-Host test. Correction pass 1 (parent, inline): exception stated in ADR 0013, security.md, AGENTS.md; wording fixed; `request_with_host` + foreign-Host loop (403 forbidden_host) added. Re-review (sonnet/medium): corrections verified against Supervisor::update; one P3 wording (an already-accepted key also allows the one-step update) → parent fixed the wording in the three docs; not re-reviewed again (wording only, parent checked it against remote.rs Trust::status). Parent re-ran fmt, workspace clippy -D warnings, `cargo test -p eario-hub --all-features -j 4` (70+30 pass). Reviewer-noted possible pre-existing issue (non-UTF-8 path segment gives a plain-text 400 without ApiError code) left as a known limitation. Accepted.

## Blockers / Challenges

- `cargo xtask check` without a job limit fails at its test step with missing rlibs (memory pressure, as in ticket 04); with `CARGO_BUILD_JOBS=4` it reaches vitest, which the local Node 22.14 cannot start. See Local Test Result.
- A long inline Bash heredoc failed to parse (`unexpected EOF while looking for matching '`), as earlier tickets found. Doc edits were written as Python scripts in the session scratchpad and run from there.
- My first header rewrite of `http.rs` dropped its `use` lines (the slice ended at the first item after them). The build caught it, and they were restored before any test ran.

## Commits

- caaa002 feat: hub routes for catalogs, install, update and rollback; ADR 0013 accepted, security docs updated (ticket 05)

## Time Spent

43m (ticket-start.js to ticket-complete.js)
