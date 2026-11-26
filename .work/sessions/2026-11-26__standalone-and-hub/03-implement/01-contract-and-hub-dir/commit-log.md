# Ticket 01: contract-and-hub-dir

**Status:** ✅ Done

## Local Test Result

`cargo test -p eario-core -p eario-runtime && cargo xtask gen-types && git status --short frontend/src/lib/generated` (CARGO_BUILD_JOBS=4): eario-core 37 passed, eario-runtime 62 passed, gen-types 36 export tests passed. Regeneration produced 5 new untracked bindings (DashboardState, HubDashboard, HubInfo, Manifest, RunFile .ts) for the parent to commit; no other generated file changed. Also run: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` (clean), `cargo doc -p eario-core -p eario-runtime --no-deps --all-features` (clean), `npm run check` in frontend (0 errors, 0 warnings). Not run: full `cargo xtask check`, e2e, cargo deny, MSRV.

## Review Notes

Parent: base c5843e4b7e20f72d87bd509edabf6c39a878ac4d. Pre-existing changes, owned by the parent (grill): `docs/adr/0011-standalone-and-hub.md` and `docs/adr/README.md` (ADR 0011 amendment); committed with this ticket. Mode: delegate.

Checked against the criteria: launch.rs holds LAUNCH_PROTOCOL_VERSION, Manifest, RunFile, DashboardState, HubDashboard, HubInfo (serde + ts-rs, documented). hub_dir.rs holds HubDir (EARIO_HUB_DIR or config dir/hub), Registry with format_version (newer version -> UnknownRegistryVersion naming the file), atomic JSON writes (temp file in same dir + rename), run file read/write/remove/list, file_digest (size + SHA-256, streamed), probe (GET /api/app with x-eario-token over a hand-written request on tokio TcpStream, 2 s timeout, loopback only, live only on 200). Probe choice: one fixed loopback request, so no HTTP client dependency; only tokio's io-util feature was added. sha2 0.11 added to workspace deps (already in the tree). slug and is_slug moved to eario-core and used by preferences (existing slug test moved, same results). Run-file and log paths require a slug so names never escape the directory. Errors: HubDirError in error.rs with stable code(). Registry types live in eario-runtime (not contract types); RunFile is in eario-core as the criteria require. Small additions beyond the spec: HUB_LOG_LINES constant, run_file_slugs(), log_path().

Independent review (fresh reviewer, base c5843e4, round 1): 0 P0/P1/P2, 2 P3.
- P3 `hub_dir.rs` temp name `<file>.<pid>.tmp` shared by concurrent writers in one process → fixed inline by the parent: per-process atomic counter in the temporary name.
- P3 `hub_dir.rs` `fs::rename` over an open destination can fail transiently on Windows → fixed inline: `rename_retrying`, 3 attempts 50 ms apart on `PermissionDenied`.
- Notes carried to later tickets, no change now: on Windows the default directory is `%APPDATA%\eario\config\hub` (same convention as preferences), so docs in ticket 07 must state the real path and ADR wording "<config dir>/eario/hub/" be read as "Eario's configuration directory"; ticket 02 needs a public way to write a run file at the path given by `--eario-child` (`write_json_atomic` is private); registry does not validate entry slugs (safe today, consumers go through `run_file_path`).
- Reviewer gap: probe timeout path (server accepts but never answers) untested; accepted, the timeout wraps the whole exchange.
Parent re-verification after the fixes: `cargo test -p eario-core -p eario-runtime` 37 + 62 pass; clippy `-D warnings` clean on both crates; `cargo xtask gen-types` produces only the 5 new bindings; `npm run check` 0 errors 0 warnings. `cargo deny` is not installed locally: not run (CI runs it; sha2 0.11 was already in the lock file through rust-embed-utils). Re-review of the two-line fix skipped: localized, covered by the existing atomic-write tests.
Acceptance: all five criteria met; requested model sonnet (medium), actual sonnet.

## Blockers / Challenges

None

## Commits

- d1f35fb feat: launch contract types and hub directory module (ticket 01)

## Time Spent

9m (ticket-start.js to ticket-complete.js)
