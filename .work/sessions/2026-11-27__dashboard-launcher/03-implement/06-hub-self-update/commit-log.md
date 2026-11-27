# Ticket 06: hub-self-update

**Status:** ✅ Done

## Local Test Result

Evidence in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/`, prefix `06-`, one file per command. Every command below ran on the final tree.

- Verification Step, `cargo test -p eario-hub --all-features -j 4 self_update`: exit 0. 10 unit tests (`self_update::tests`) and 3 integration tests (`tests/hub.rs`), 0 failed (`06-verification-step.txt`).
- `cargo test -p eario-hub --all-features -j 4`: exit 0. 80 unit tests, 33 integration tests, 0 failed (`06-hub-tests.txt`).
- `cargo test --workspace --all-features -j 4`: exit 0, 22 `test result: ok`, no `FAILED` or panic (`06-workspace-tests.txt`).
- `cargo fmt --all -- --check`: exit 0 (`06-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings`: exit 0 (`06-clippy.txt`). Also `cargo clippy -p eario-hub --lib --bins -- -D warnings`, the hub as shipped without `fake-github`: exit 0 (`06-clippy-without-feature.txt`).
- `cargo +1.95.0 check -p eario-hub --all-features --all-targets --locked` (separate target dir): exit 0 (`06-msrv-1.95-check.txt`). `tokio::sync::watch::Sender::new` and `Command::get_*` are within 1.95.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`: exit 0 (`06-doc.txt`). `cargo doc -p eario-hub` without features is clean too.
- `cargo xtask gen-types`: exit 0, run again on the final tree (`06-gen-types.txt`), then a third time with the SHA-256 over all generated files the same before and after (`06-gen-types-second.txt`). Bindings, uncommitted: new `HubUpdate.ts` and `HubSelfUpdate.ts`; modified `HubInfo.ts` (`available_version`) and `UpdateCheck.ts` (`hub`).
- `npm run check` in `frontend/`: 401 files, 0 errors, 0 warnings, exit 0 (`06-npm-check.txt`), after adding `available_version: null` to the `HubView.test.ts` fixture.
- Mutation check (`06-mutate.py`, output in `06-mutation-check.txt`). 12 of 12 mutations were killed by the test meant to catch each, and every source file was restored byte for byte (SHA-256 of each file and the diff stat compared). The mutations:
  - the swap does not rename the original back;
  - an older or equal tag is not refused before the download;
  - the version the new executable prints is not checked (replay of an older signed build under a newer tag);
  - the key the release carries as `minisign.pub` is used (trust on first use);
  - the update check reads the hub's release without a key;
  - an available hub version is listed when it is no longer newer;
  - a second self-update in one process is not refused;
  - the server does not stop after a self-update;
  - the hub start does not remove `<exe>.old`;
  - the command line updates beside a running hub;
  - a self-update refusal answers 500, not 409;
  - a release without a signature is checked with an empty one.
- Not run:
  - `npm test` (vitest): the local Node 22.14 cannot start it, as in tickets 01 to 05.
  - `cargo xtask e2e`: no view or example changed, only generated types, a test fixture and one field in the hub's list answer.
  - `cargo deny check`: no dependency was added.
  - A full restart, by re-spawning a real hub from a self-update: see Review Notes, item 6.
  - macOS and Linux: only Windows x86_64 ran here. The Unix-only code (`copy_permissions`, test `the_new_version_gets_the_permissions_of_the_executable`) was not compiled locally; CI covers it.

### Correction pass 1 (independent review 1)

Evidence prefix `06-fix-`, in the same folder. Every command below ran on the final tree of this pass.

- Verification Step, `cargo test -p eario-hub --all-features -j 4 self_update`: exit 0. 12 unit tests and 5 integration tests, 0 failed (`06-fix-verification-step.txt`).
- `cargo test -p eario-hub --all-features -j 4`: exit 0. 82 unit tests, 36 integration tests, 0 failed (`06-fix-hub-tests.txt`).
- `cargo test --workspace --all-features -j 4`: exit 0, 22 `test result: ok`, no `FAILED` or panic (`06-fix-workspace-tests.txt`). It includes the new eario-runtime test `appended_lines_are_kept_in_order`.
- `cargo fmt --all -- --check`: exit 0 (`06-fix-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings`: exit 0 (`06-fix-clippy.txt`). `cargo clippy -p eario-hub --lib --bins -j 4 -- -D warnings` (no `fake-github`): exit 0 (`06-fix-clippy-without-feature.txt`).
- `cargo +1.95.0 check -p eario-hub --all-features --all-targets --locked -j 4` (separate target dir): exit 0 (`06-fix-msrv-1.95-check.txt`). `lock_owned` and `tokio::join!` come from the tokio already locked.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`: exit 0 (`06-fix-doc.txt`). Its first run failed on two intra-doc links from the public `self_update` module docs to private items (`swap`, `restart`); they became plain text.
- Unix code:
  - `rustup target list --installed` has no `x86_64-unknown-linux-gnu`.
  - `x86_64-linux-android`, a `cfg(unix)` target, is installed. But `cargo check -p eario-hub --target x86_64-linux-android` stops in `ring`'s build script, which needs a C cross compiler (`clang`) that is not installed. `eario-runtime` stops there too.
  - So `06-fix-extract-unix.py` copies items verbatim from the sources into the scratch crate `06-fix-unix-check/`:
    - `swap`, both `link_aside`, both `copy_permissions`, `copy_executable`, `remove_old`, `in_use` and `SelfUpdateError`;
    - `remote::is_busy` and `remote::executable_file_options`;
    - the unit tests `a_swap_puts_the_new_version_in_place_and_keeps_the_old_one`, `a_failed_placement_restores_the_original`, `the_new_version_gets_the_permissions_of_the_executable` and `the_executable_is_linked_aside_before_it_is_replaced`.
  - For `x86_64-linux-android`, `cargo check --tests` and `cargo clippy --tests -- -D warnings` both exit 0 (`06-fix-unix-check.txt`). The Unix code is type-checked, not run.
  - The changed Unix test `hub_files_are_readable_by_the_user_only` in eario-runtime was not compiled for Unix. CI covers both.
- Mutation check (`06-fix-mutate.py`, output in `06-fix-mutation-check.txt`). 9 of 9 mutations were killed by the test meant to catch each, and both mutated files were restored byte for byte (SHA-256 compared). The mutations:
  - the self-update lock is held by the request future, not by the blocking work;
  - the restart is asked after the await, as before the fix;
  - a stopping hub does not wait for a self-update in progress;
  - a failed swap still asks for a restart;
  - a hub without the key lists a newer version of itself (once against the unit test, once against the integration test);
  - the restart is not tried again while the new file is busy;
  - a restart that fails is not recorded in the hub directory;
  - a refused swap keeps the new version beside the hub.
- Not run in this pass:
  - `cargo xtask gen-types` and `npm run check`: no contract type changed (`eario-core` untouched in this pass).
  - `cargo xtask e2e`: no view or example changed.
  - `cargo deny check`: no dependency added.
  - `npm test` (vitest): Node 22.14, as before.
  - The restart of a real hub by `serve()`: it needs a build holding a real key (see Review Notes, item 6). `serve()`'s call of `restart_after_self_update` was checked by reading only; a mutation there would survive.

## Review Notes

Parent: base 452dc0d3101e7dff3443c32954aa08ee00c1859c; tree clean except session files. Mode delegate (implementer opus/xhigh). Recovery count: 0.

Implementer self-check against the Acceptance Criteria (review rounds: 0 so far). The logic is in the new `crates/eario-hub/src/self_update.rs` (blocking work) and in `Supervisor` methods; `http.rs` and `cli.rs` only adapt.

1. **The compiled key.**
   - `crates/eario-hub/eario-release.pub` is embedded with `include_str!`. The committed file is a placeholder: its comment says PLACEHOLDER, NOT A KEY, and its key line `PLACEHOLDER-NOT-A-KEY` is no minisign key.
   - `SelfUpdateConfig::compiled()` reads it. The placeholder gives no key, so `self_update` refuses with `self_update_not_configured` before any request, and `check()` stores `hub.error.code = self_update_not_configured`, also without a request.
   - A unit test requires the committed file to be the placeholder or a valid minisign `.pub` file. A broken replacement therefore fails CI instead of leaving a hub that silently cannot update itself.
   - No other key is ever used for the hub: not the release's `minisign.pub`, not a catalog's, no trust on first use, and no route, command or environment variable can pass one. Tests inject a key through `SelfUpdateConfig::for_tests` and `Supervisor::with_self_update`, both `#[cfg(feature = "fake-github")]`, which a release build never enables. `Supervisor::new` and `with_github` always use the compiled key.
2. **Verification, the same path as dashboards.** Shared code moved out of `remote::stage` into `remote::signed_sums` (refuses a missing `.minisig` before any download) and `remote::download_verified` (signature, then bounded download into the staging area, then the SHA-256 against the signed list). `stage` and the self-update both use them. The read of an executable's output became `remote::probe`, shared by `--eario-manifest` and `--version`.
   - The steps: the tag must be newer than the running version (`self_update_not_newer`, before the download), then the asset `eario-hub-<triple>[.exe]` from `verify::asset_name`, the signature with the compiled key, and the SHA-256.
   - **Parent decision taken: the new executable is run, after verification, with `--version`.** The tag is not signed. Without the run, an older signed hub served again under a new tag would downgrade the hub, against ADR 0013's no-downgrade rule. The run is the manifest step's: no stdin, output bounded to 64 KiB, `manifest_timeout` (20 s), no console window, staging directory as working directory. The printed `eario-hub <version>` must be strictly newer by SemVer (`self_update_not_newer`, `self_update_version_not_comparable`). It also proves the new exe starts on this machine before it replaces the hub. A hub built before this ticket has no `--version`, so it can never be installed by self-update; this is a downgrade, so refusing it is right.
   - New CLI flag `--version`, printing `eario-hub <version>`.
3. **Swap (`self_update::swap`).**
   - `current_exe()`, or the test executable, must be a file. Before the download, `check_writable` creates and removes `<exe>.new` beside it; on failure it refuses with `self_update_not_writable`, naming the directory.
   - After verification: the staged file is copied to `<exe>.new` (with the exe's permissions on Unix), synced, and hashed again against the signed size and SHA-256.
   - Swap: remove a stale `<exe>.old`, rename the running exe to `<exe>.old`, then `<exe>.new` to `<exe>`. If the second rename fails, `<exe>.old` is renamed back and `.new` removed (`self_update_failed`, "left as it was"). If even that fails: `self_update_restore_failed`, naming both files so the user can rename one back.
   - Unix takes the same two renames, for one tested path; this is documented in the module and ADR.
   - One self-update per process: a second one gets `self_update_pending`. This also avoids Linux `current_exe()` then pointing at `.old`.
4. **Restart.**
   - `Supervisor::self_update` records the executable in a `watch` channel. `start()` spawns a task holding only a receiver and the shutdown handle; on that signal it calls the server's shutdown, so the response in flight is still sent (graceful shutdown).
   - `serve()` then, after `wait()` returns and `hub.json` is removed, starts `self_update::restart_command(exe)`: no arguments, stdin/stdout/stderr null, `detach()` (CREATE_NO_WINDOW and CREATE_NEW_PROCESS_GROUP on Windows, own process group on Unix), the exe's directory as working directory, and the hub's environment inherited, so the same hub directory. It does not wait for it and exits.
   - The new hub's `hub.json` check cannot bounce: the file is gone, and the old server no longer answers `/api/alive` anyway.
   - Dashboards are never touched. The integration test checks one still answers after the self-update, and that the next hub re-attaches to it.
5. **`.old` at the next start.** `start()` runs `remove_old_executable` in the background. NotFound is ignored. While removal fails with access denied or a sharing violation (the old hub has not exited yet), it retries for up to 10 s; it then logs a warning and gives up. Unit test `the_old_executable_is_removed_once_it_is_released` holds the file exclusively for 500 ms.
6. **Tests, and what is tested at function level.**
   - `self_update_replaces_the_running_hub_and_restarts_it`, over HTTP. The executable replaced is a copy of the real hub, running as a hub of another directory. So the test proves that Windows renames a running exe. It covers:
     - the check reports `HubUpdate` and `HubInfo.available_version`;
     - `POST /api/self-update` answers `HubSelfUpdate`, then the server stops by itself;
     - `restart_executable()` names the exe;
     - the new bytes are in place, `.old` holds the old ones, staging is empty;
     - a second self-update gets `self_update_pending`;
     - the dashboard and the old hub process still answer;
     - after the old hub quits, the next start removes `.old`.
   - `self_update_refuses_what_it_cannot_verify_and_changes_nothing`. After each case the exe is byte-identical, nothing is beside it and staging is empty. The cases:
     - no key, for `self_update`, `check_hub`, `check` and `list`, with zero requests;
     - an older tag;
     - no `.minisig`;
     - signed by another key that the release carries as `minisign.pub` (`release_wrong_key`);
     - a checksum list changed after signing (`release_bad_signature`);
     - a tampered asset (`release_hash_mismatch`);
     - a signed non-executable (`self_update_probe_failed`);
     - no asset for this machine;
     - a newer tag on a build that is not newer (replay);
     - HTTP 409 for a refusal, with the hub still serving.
   - `self_update_from_the_command_line_waits_for_the_hub_to_quit`: `--version`; `self_update_hub_running` naming the running hub's address; once that hub has quit, `self_update_not_configured` without a request to `rtonneau/eario`. This branches on `SelfUpdateConfig::compiled().is_configured()`, so it stays valid once the real key is committed.
   - Unit tests of the swap use a temporary copy of the test executable, never the running binary:
     - success, with a stale `.old` replaced;
     - failed placement restores the original (the new file is missing after the first rename);
     - an `.old` that cannot be removed stops the swap before any rename;
     - Windows only: the exe locked without share-delete, and the new file locked so that the second rename fails, both restore;
     - the writable check names the folder;
     - Unix only: permissions are kept.
   - **The restart itself is tested at the function level**: `restart_command` (program, no arguments, working directory, environment unchanged), plus the integration check that the server stops and hands over the executable. A real hub is not re-spawned after a self-update, because the binary under test holds the placeholder key. Making that possible would need a key override in the binary, which the trust model forbids.
7. **Contract.**
   - `UpdateCheck.hub: Option<HubUpdate>` (`#[serde(default)]`, so an `updates.json` written by ticket 04 still reads).
   - `HubUpdate { repo, latest_version, available_version, error }`.
   - `HubInfo.available_version`: the hub's newer version, kept only while it is still newer than the running hub, like the dashboards' field.
   - `HubSelfUpdate { previous_version, version }`, the answer of `POST /self-update`.
   - `HubInfo.eario_version` now comes from the self-update config: `CARGO_PKG_VERSION` in every non-test build.
8. **Route and CLI.**
   - `POST /self-update` takes no body or query and sits in `routes()`, so behind the token and the Origin/Host checks. It was added to the 401/403 loop of `remote_routes_take_strict_path_segments_and_nothing_else`.
   - Status mapping: 409 for not configured, unsupported platform, not writable, not newer, not comparable, pending and hub running; 502 for a failed probe; 500 for no executable and for swap or restore failures.
   - `eario-hub self-update`:
     - refuses while `hub.json` names a hub that answers;
     - otherwise reads the latest release (`Supervisor::check_hub`) and says "up to date" if nothing is newer;
     - asks for a confirmation, or takes `--yes`; without a terminal it refuses (`hub_confirmation_required`);
     - restarts nothing.
9. **Docs.**
   - ADR 0013: the Self-update bullet is replaced by "Self-update, from the compiled key only" and "Swap and restart", the check-at-start bullet mentions the hub, and a consequence covers the placeholder and a lost key.
   - `security.md`: three threat rows (hub replaced by a spoofed release, a failed self-update, the downgrade row extended), the route and CLI lists, a paragraph "The hub's own update", the outbound-request note and a checklist item.
   - `AGENTS.md`: one sentence in the hub-exception bullet.
   - `docs/architecture/overview.md`: the route row and the crate row.
   - Project status, roadmap and README are left to ticket 09, as in ticket 05.

For later tickets:
- **Ticket 07.**
  - Show `HubInfo.available_version` with a button for `POST /api/self-update`, and `last_check.hub.error` (`self_update_not_configured` means "this build does not update itself").
  - After a success, the hub stops and the new one opens its own browser tab at a new address and token, so the view should say so rather than poll.
  - `api.ts` has no `hubSelfUpdate` yet.
- **Ticket 08.**
  - The release must attach `eario-hub-<triple>[.exe]` for each target, listed in the signed `SHA256SUMS.txt` and signed with the Eario release key.
  - Replace the placeholder `crates/eario-hub/eario-release.pub` with that key's `.pub` file; the unit test rejects anything that is neither the placeholder nor a valid key.
  - The tag must be the workspace version (`v<CARGO_PKG_VERSION>`), because the hub compares both the tag and its `--version`.
  - `minisign.pub` on the hub release is ignored.
- **Ticket 09.** Document `eario-hub self-update`, `--version`, the `.old` file and the restart without a terminal.

Known limitations:
- The restarted hub has no console: its address is not printed. It opens the browser unless `EARIO_NO_BROWSER` was set for the old hub; `eario-hub` run again finds it and opens it.
- If `EARIO_PORT` fixed the old hub's port, the new hub binds the same port right after the old one closed. That was not tested; Windows could refuse it while connections linger.
- `.old` stays until the next hub start, or until a later self-update removes it first.
- If both renames fail (`self_update_restore_failed`), the user must rename a file by hand; the message names both files.

Independent review 1 (opus/high, base 452dc0d): 5 findings. P2 supervisor.rs:1596-1615 dropped request future → swap done but no restart, guard released, overlapping updates. P2 lib.rs:213-232 restart spawn not retried, failure leaves no hub and no durable trace. P3 hub.rs: no fake-server failed-placement test. P3 self_update.rs:559-563 Unix window with no executable (use hard_link + atomic rename). P3 supervisor.rs:750 available_version shown when not configured. Parent: all accepted; correction pass 1 (recovery count 1) via a fresh implementer.

Correction pass 1 (implementer opus, fresh context; recovery count 1). What changed, per finding of independent review 1:

1. **P2, a dropped request (supervisor.rs).**
   - `self_updating` is now an `Arc<tokio::sync::Mutex<()>>`. `Supervisor::self_update` takes it with `lock_owned()` and moves the guard into the blocking closure. The guard is therefore released only when the blocking work ends, whatever happens to the request future.
   - The restart is asked inside that closure, right after `self_update::update` succeeds and before the guard is released (`watch::Sender::send_replace` from the blocking thread). A second self-update, waiting on the lock, then finds it and gets `self_update_pending`.
   - New `Supervisor::restart_after_self_update()`: waits for the lock, then returns `restart_executable()`. `serve()` calls it after the server stopped.
     - When Quit's grace period drops the self-update request, the hub still restarts on the new executable. Before, it exited with the new executable in place and no new hub started.
     - The process already waited for that blocking work, because dropping the runtime waits for blocking tasks. It now also restarts at the end.
     - Decision taken here, not in the review: after a Quit, honouring the update the user asked for seemed less surprising than exiting with the executable replaced and no new hub started.
   - Test `a_dropped_self_update_still_restarts_the_hub_and_runs_alone` (fake GitHub). It polls `self_update()` once, which starts the blocking work (asserted pending), and drops it. Then it runs `tokio::join!(restart_after_self_update(), self_update())`:
     - the first gives the executable;
     - the second gets `self_update_pending`;
     - there was one download, the new build is in place, `.old` holds the original and staging is empty.
   - It fails under each of the three mutations of the fix (see Local Test Result).
2. **P2, a restart that fails (lib.rs).**
   - `remote::spawn_retrying` takes the number of attempts and is now `pub(crate)`. The probe keeps its 5.
   - New `self_update::restart(hub, executable)`: `restart_command` through `spawn_retrying`, with 200 attempts 50 ms apart (about 10 s) while `is_busy`, that is `ERROR_SHARING_VIOLATION` or `ETXTBSY`.
   - On the final failure it appends one line to `<hub dir>/self-update.log`: the Unix time, the error, and "The new version is in place at <path>; start it again to open the hub. Dashboards kept running."
   - The file is at the root of the hub directory, not in `logs/`: `logs/<slug>.log` belongs to the dashboards, and `hub` and `self-update` are valid slugs.
   - New in eario-runtime: `HubDir::self_update_log_path()` and `hub_dir::append_line_at`. The function creates the file and its directory when missing, `0600` on Unix like the other hub files, and syncs it. The hub directory layout in `hub_dir.rs` lists the file.
   - `serve()` runs the restart through `spawn_blocking`, and its terminal line also names the log file.
   - Tests:
     - `a_new_hub_that_does_not_start_is_recorded_in_the_hub_directory`: a missing file is not retried, and each failure appends a line naming the executable.
     - Windows `a_new_hub_held_for_a_moment_still_starts`: a copy of `System32\hostname.exe` stands in for the new hub, since a copy of the test executable would run every test. It is held with `share_mode(0)` for 300 ms, starts after it is released, and nothing is logged. This also shows that `CreateProcess` on a held file reports a sharing violation, which `is_busy` recognizes.
     - eario-runtime `appended_lines_are_kept_in_order`; the Unix permission test now covers `self-update.log`.
   - Docs:
     - ADR 0013, the "Swap and restart" bullet: the retry, the log file, the dropped request;
     - security.md: the row "A failed self-update leaves no working hub" and the paragraph "The hub's own update";
     - the `self_update` module docs and the docs of `run()`.
3. **P3, failed placement through `Supervisor::self_update` (tests/hub.rs).** New test `a_self_update_that_cannot_place_the_new_hub_leaves_it_as_it_was`. Each case runs after a real download and `--version` probe from the fake GitHub:
   - Every platform: `<name>.old` is a non-empty directory, which the swap cannot remove.
     - The update fails with `self_update_failed`, "left as it was", after one download of the asset.
     - The executable is byte-identical. Its directory holds only it and the `.old` folder, no `.new`.
     - Staging is empty, and no restart is asked.
   - Windows: the executable is held with `share_mode(0)`, so the rename aside fails. Same assertions, and nothing at all beside the executable.
   - With the obstacle gone, the update then succeeds and asks for the restart: a failed swap holds nothing back.
4. **P3, Unix swap window (self_update.rs).**
   - `swap` now calls `link_aside(executable, .old)`. On Unix that is `fs::hard_link`; on success one `rename(.new, executable)` replaces the executable atomically. If that rename fails, the link is removed and `.new` discarded; the executable was never touched.
   - When `hard_link` fails, the two-rename path with its restore stays: on a file system without hard links (FAT, some network shares; logged at info), and always on Windows, where `link_aside` returns `false`.
   - The remaining window, on Windows and on Unix without hard links, is documented in the module docs, the `swap` docs, ADR 0013 and security.md: a crash or power loss between the two renames leaves `<name>.old` and `<name>.new` and no executable.
   - New Unix test `the_executable_is_linked_aside_before_it_is_replaced` checks the inode and link count. It was type-checked only, see Local Test Result.
   - On Linux, after the atomic rename, `current_exe()` of the old hub names a deleted path. Nothing uses it after a swap: the restart uses the path recorded before, and a second self-update is refused as pending.
5. **P3, `available_version` without a key (supervisor.rs, self_update.rs).**
   - `self_update::available_version` takes the `SelfUpdateConfig` and returns `None` when it holds no key. `list()` passes it.
   - The unit test `a_check_reports_a_newer_hub_only_while_it_is_newer` gains the keyless case.
   - New integration test `a_hub_without_the_release_key_offers_no_update_of_itself`. A configured build checks and lists the newer hub. A keyless build on the same hub directory then lists `available_version: None`, while `last_check.hub` still holds what the other build stored.
   - ADR 0013's Self-update bullet and security.md say so.

Files changed in this pass:
- `crates/eario-hub/src/supervisor.rs`
- `crates/eario-hub/src/self_update.rs`
- `crates/eario-hub/src/lib.rs`
- `crates/eario-hub/src/remote.rs`
- `crates/eario-hub/tests/hub.rs`
- `crates/eario-runtime/src/hub_dir.rs`, not touched before this pass
- `docs/adr/0013-github-install-and-update.md`
- `docs/architecture/security.md`

No contract type changed, so no bindings were regenerated.

Known limitations added by this pass:
- On Windows, and on a Unix file system without hard links, a crash or power loss between the swap's two renames leaves no executable at the hub's name. Beside it are `<name>.old` (the previous version) and `<name>.new` (the verified new one), to rename by hand.
- If the new hub still cannot be started after about 10 s of attempts, the old hub exits anyway. Without a terminal, the only trace is `self-update.log` in the hub directory, one line per failed restart; nothing reads, shows or rotates it. Ticket 07 could show it; ticket 09 should document it.
- A Quit during a self-update waits for the update to end, then starts the new hub if the executable was replaced.

Re-review (opus/high, base 452dc0d, after correction pass 1): REVIEWED — no findings; reviewer ran hub tests 82+36 pass. Parent accepted: Quit during an update waits for it and restarts when the swap succeeded. Parent re-ran fmt and workspace clippy -D warnings: clean. Gaps carried to CI/ticket 09: Unix swap and permissions code not compiled locally; no end-to-end restart test; downloads have per-read timeouts only. Ticket 08 must replace the placeholder key, sign `eario-hub-<triple>[.exe]`, tag `v<version>`; ticket 07 adds the button (hub restarts at a new address); ticket 09 documents `self-update`, `--version`, `.old` and `self-update.log`. Accepted.

## Blockers / Challenges

- Long Bash heredocs failed to parse (`unexpected EOF while looking for matching '`), as in tickets 03 to 05. Edits were written as Python scripts in the session scratchpad and run from there; the docs kept their CRLF endings (the script detects and restores them), the Rust files their LF.
- `npx prettier --check` flags `HubView.test.ts`, but it flags the committed version too: prettier is not this project's formatter (no script or config), so the fixture was left in the file's own style.
- clippy's `cloned_ref_to_slice_refs` failed the first clippy run on one test assertion (`[name.clone()]`); fixed with `[name.as_str()]`, then clean.
- Building the examples after the contract change took about 2 minutes with `-j 4`; no memory failure this time.
- vitest cannot run on the local Node 22.14 (known gap).

## Commits

- a98a35f feat: hub self-update from releases signed with the compiled Eario key (ticket 06)

## Time Spent

1h 41m (ticket-start.js to ticket-complete.js)
