# Ticket 02: launch-contract

**Status:** ✅ Done

## Local Test Result

`CARGO_BUILD_JOBS=4 cargo test -p eario -p eario-runtime -p data-explorer -p signal-lab` (final tree; log in `.scratch/tests/2026-11-26__standalone-and-hub/evidence/02-verification.log`): all pass. eario 25 unit + 5 end_to_end, eario-runtime 68, data-explorer 1 unit + 4 launch, signal-lab 9 unit + 3 launch, doc-tests 7. The launch tests run the real `CARGO_BIN_EXE_data-explorer` / `CARGO_BIN_EXE_signal-lab`, each with its own temporary `EARIO_HUB_DIR` and 30 s bounded waits. They check: the manifest JSON (slug, version = CARGO_PKG_VERSION, protocol 1), with extra missing-file arguments and the hub directory untouched; install without a terminal and without `--yes` refused (message names the slug and `--yes`); install `--yes` makes an absolute copy under `bin/<slug>/` whose size and SHA-256 match the registry and the original; reinstall keeps one entry; uninstall without `--yes` refused, with `--yes` it removes the entry and the directory, and a second uninstall gives "not installed"; child mode writes a run file (pid, protocol) whose token answers `GET /api/app` (probe and raw request), install and uninstall are refused while it runs, `POST /api/shutdown` gives 204, the process exits 0 with nothing on stdout, and the run file is gone with the log file written.

Also run: `cargo fmt --all -- --check` (clean); `cargo clippy --workspace --all-targets --all-features -- -D warnings` (clean, `evidence/02-clippy.log`); `RUSTDOCFLAGS="-D warnings" cargo doc -p eario -p eario-runtime -p eario-core --no-deps --all-features` (clean, `evidence/02-doc.log`). `cargo xtask check` (`evidence/02-xtask-check.log`): fmt, clippy, every Rust test and docs pass, but `npm run test` (vitest) fails to start workers: `undici 8.13.1` in `frontend/node_modules` needs Node >= 22.19, and this machine has Node 22.14 (`createZstdDecompress` is undefined). This is the local environment (CI uses Node 24) and the frontend is unchanged by this ticket. Not run: `cargo xtask e2e` (no frontend or example UI change; the examples gained only `.version(...)`), `cargo deny` (not installed; no new crate in the tree), MSRV toolchain (APIs used: `std::path::absolute` 1.79, `IsTerminal` 1.70).

## Review Notes

Parent: base 286b9c4a71a1ea918fecabcb67bbe8d3cd6bfbbb. No pre-existing changes outside .work/. Mode: delegate.

Against the criteria:
- `Dashboard::version(impl Into<String>)` and the public `Dashboard::manifest()`. Both examples call `.version(env!("CARGO_PKG_VERSION"))`. `description` stays empty: there is no builder for it, and the ticket did not ask for one.
- `Dashboard::run` parses `std::env::args_os()` (`crates/eario/src/launch.rs`, `parse`). With no `--eario-*` argument it takes the unchanged `build()` + `eario_runtime::run` path; other arguments are skipped, and `--yes` counts only with install or uninstall. An unknown `--eario-*` argument, two actions together, or `--eario-child` without a value are errors with codes.
- `--eario-manifest` prints one line of JSON and returns before `build()`.
- Install: refuses while live (`read_run_file` + `probe_blocking`; an unparsable run file counts as not live). Confirmation shows the installed and new versions ("unknown" when none), `--yes` skips it, and stdin not a terminal without `--yes` is refused. It then copies `current_exe` atomically to the absolute `<hub>/bin/<slug>/<file name>` (skipped when it is already the hub's own copy) and records slug, path, size, SHA-256 and manifest. A reinstall replaces the copy and removes an earlier copy under another file name in the same directory, reporting if that fails.
- Uninstall: same live refusal and confirmation, "not installed" error naming the registry. It removes only `<hub>/bin/<slug>/` (never a path read from the registry), first, then the entry.
- Child: logging starts first (`eario_runtime::init_file_logging`, file `hub/logs/<slug>.log`, new `log_file::RotatingFile`, 1 MiB, one `.log.1` kept, no new dependency; default filter `info`, `EARIO_LOG` overrides). Then it drops `load_file` entries whose path is exactly `--eario-child` or the run-file argument and builds. `eario_runtime::run_child` forces `open_browser: false`, installs no Ctrl+C handler, prints nothing, writes the run file atomically once listening, and removes it after `server.wait()` (Quit and the hub both use `POST /api/shutdown`). A build or serve error is logged with its code before returning.
- Errors: new `LaunchError` (stable `code()`, each names the slug or file) wrapped as `Error::Launch`. `ServeError` gained `LogFile` and `RunFile` plus `code()`; existing variants keep `serve_failed`.
- hub_dir, extended minimally: `write_run_file_at`, `remove_run_file_at` (the `HubDir` methods now delegate to them), `HubDir::bin_dir`, `copy_atomic` (temporary file + rename, keeps permissions), `probe_blocking`, plus tests for each.

Changed after self-review:
- `LaunchError` has a hand-written `Debug` (message + code). `main` returning the error prints `Debug`, and the first test run showed `Error: Launch(Running { slug: .. })` instead of the actionable message. Standalone errors are unchanged.
- The real-executable tests got a kill-on-drop guard (`Spawned`). A failing assertion had left a child running, holding the test's inherited output pipe open.
- The install question was factored out and unit-tested.

Notes for later tickets: the facade now depends on `tracing` (already in the tree through eario-runtime). In child mode stderr is still where Rust's `main` prints an error that occurs before logging starts (hub directory unresolvable, log file not openable). The hub (ticket 04) should give the child null or captured stdio. The copy's directory as working directory is the hub's spawn concern (ticket 04). The interactive y/N path cannot be driven without a pseudo-terminal: `ask` is unit-tested, and the terminal path is not tested end to end. Docs (`project-status`, overview) are left to ticket 07.

Independent review (fresh reviewer, opus, base 286b9c4, round 1): 0 P0/P1/P2, 1 P3.
- P3 `crates/eario/src/launch.rs` install/uninstall checked "running" only before the y/N prompt, which can wait indefinitely → fixed inline by the parent: after a yes, `refuse_if_running` runs again and the registry is re-read before it is changed (install and uninstall).
- Reviewer note for ticket 04: a second child on the same run file overwrites it; the hub must not start a slug whose run file is live.
- Reviewer gaps: forced no-browser and absent Ctrl+C handler in child mode checked by reading only (tests set EARIO_NO_BROWSER=1); non-matching load_file kept in child mode untested; interactive y/N path not tested through a real terminal; rotation at 1 MiB only unit-tested with small limits. Accepted: the code paths are short and read by two agents; a pseudo-terminal test would need a new dependency.
Parent re-verification after the fix: `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean; `cargo test -p eario -p eario-runtime -p data-explorer -p signal-lab` all pass (evidence/02-parent-reverify.log). Re-review of the 6-line fix skipped: localized, same function, existing install/uninstall tests pass.
Gaps carried to ticket 07: local Node is 22.14, below the 22.19 undici 8.13.1 needs, so vitest cannot start locally (frontend unchanged here; CI uses Node 24); `cargo deny` not installed locally; e2e and MSRV run at ticket 05/07.
Acceptance: all criteria met; requested model opus (heavy, high), actual opus.

## Blockers / Challenges

- First run of the real-executable tests: 3 failures, because stderr held the `Debug` form of the error. Fixed with the `LaunchError` `Debug` described above. One failing child test also left an orphaned `data-explorer.exe --eario-child`, which held the output pipe and stalled the run until it was stopped by hand. Fixed with the kill-on-drop guard; no process was left after the later runs (checked).
- `cargo xtask check` vitest step fails locally, from the environment (Node 22.14 < 22.19 required by undici 8.13.1), not from this change. It needs Node >= 22.19 locally (CI uses 24).

## Commits

- dcda85c feat: launch contract in every dashboard executable (ticket 02)

## Time Spent

45m (ticket-start.js to ticket-complete.js)
