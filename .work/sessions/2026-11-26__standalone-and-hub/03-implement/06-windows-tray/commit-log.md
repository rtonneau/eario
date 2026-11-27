# Ticket 06: windows-tray

**Status:** ✅ Done

## Local Test Result

Run on Windows 11 (x86_64-pc-windows-msvc, rustc 1.100.0), `CARGO_BUILD_JOBS=4`.
Evidence in `.scratch/tests/2026-11-26__standalone-and-hub/evidence/`.

- `cargo build -p eario-hub --release`: Finished, exit 0.
- `cargo clippy -p eario-hub --all-targets -- -D warnings`: exit 0, no warning.
- `cargo test -p eario-hub` (`06-test-eario-hub.txt`): lib 7 passed (3 new tray
  tests), `tests/hub.rs` 8 passed, 0 failed. The first run failed
  `a_second_hub_finds_the_first_and_exits` with exit code 0xc0000139, which led
  to `build.rs` (see Blockers); it passed after that.
- `cargo xtask check` (`06-xtask-check.txt`): fmt, workspace clippy (all
  features), `cargo test --workspace --all-features` (22 result lines, all ok,
  0 FAILED), `cargo doc` and svelte-check (0 errors, 0 warnings) passed.
  **vitest failed with an environment problem, not from this change:** every
  worker fails to start with `TypeError: Cannot read properties of undefined
  (reading 'bind')` in `node_modules/undici/lib/interceptor/decompress.js:78`.
  The installed undici is 8.13.1 (node_modules dated 2026-11-26 22:05, after
  ticket 05's green vitest run), and local Node is v22.14.0, which it does not
  support. `npx vitest run` on its own fails the same way. This ticket changes
  no frontend file or npm lockfile.
- `cargo deny check`: not run (cargo-deny is not installed locally). Checked
  by reading instead. The new crates are `native-windows-gui` 1.0.13 (MIT),
  `bitflags` 1.3.2 (MIT/Apache-2.0) and `winapi-build` 0.1.1 (MIT, nwg's
  build dependency), all on the deny.toml allow-list. `winapi` and
  `lazy_static` were already in Cargo.lock. RustSec advisory-db has no entry
  for native-windows-gui, bitflags, winapi-build, lazy_static or winapi (the
  GitHub contents API returns 404 for each). bitflags 1 beside bitflags 2 adds
  one multiple-versions *warning* (bans are set to `warn`). deny.toml is
  unchanged.
- Not run: `cargo xtask e2e`. No frontend file or example changed, and e2e
  runs on ubuntu in CI, where the tray is not compiled.
- Not checked: a Linux or macOS build (no such target is installed). The
  tray module, the dependency and the build-script linker arguments are all
  Windows-only. The shared `begin`/`finish` code is used by `run()` on every
  platform.

Programmatic manual check (`tray-run/manual-check.ps1`, output in
`06-manual-check.txt`). Setup: the release hub with a temporary
`EARIO_HUB_DIR` and `EARIO_NO_BROWSER=1`, started detached with
`Start-Process` and no redirection, the way Explorer starts it.

- The PE subsystem of `target/release/eario-hub.exe` is 2 (Windows GUI), so
  there is no console. The hub started no child process (no conhost), and its
  MainWindowHandle is 0.
- `hub.json` names the hub's pid, and `GET /api/alive` answered 204.
- A second start exited with code 0 and left `hub.json` unchanged.
- data-explorer, installed and started through the hub API, reached
  `running`.
- `POST /api/shutdown` answered 204. It goes through the same
  `ShutdownHandle` as the tray's Quit. The hub exited with code 0, `hub.json`
  was removed, and the dashboard (pid 2636) **kept running**. The next hub
  re-attached to it and stop-all stopped it.
- `hub.log` has the start and stop lines, with the address and no token.

Still for the user to check by eye, because I cannot see the tray. Start
`target/release/eario-hub.exe` from Explorer:
1. No console window appears, and the browser opens on the hub.
2. An icon (the generic Windows application icon) is in the notification
   area, possibly in the overflow (^), with the tooltip "Eario hub".
3. A left click opens the hub. A right click shows: Open the hub, then the
   running dashboards (or "No dashboard running", greyed out), then Stop all
   dashboards and Quit the hub (dashboards keep running).
4. A dashboard started from the hub page appears in the menu within about 2 s,
   and clicking it opens it in the browser.
5. "Stop all dashboards" stops it. "Quit the hub" removes the icon and ends
   the process, and dashboards keep running.
6. Quit in the hub page also removes the tray icon.

## Review Notes

Parent: base b59fccf8d2272ebf774718a244581836758c435c. No pre-existing changes outside .work/. Mode: delegate.

Research (step 1). The candidates were added in a detached scratch crate,
which was deleted afterwards.

| Crate | Windows event loop | `unsafe` in Eario? | Windows dependency weight | Licence | Maintenance / MSRV |
| --- | --- | --- | --- | --- | --- |
| `tray-icon` 0.27 (tauri) | Creates its hidden window on the caller's thread and needs a Win32 message loop running there. The docs and examples use winit or tao. | Yes without winit/tao: GetMessageW/DispatchMessageW via windows-sys are `unsafe` | Light: muda, crossbeam-channel, windows-sys 0.61, dpi, keyboard-types, thiserror | MIT OR Apache-2.0 | Very active (2026-11-25), MSRV 1.92 |
| `trayicon` 0.4.1 | Same: the caller pumps messages (winit example) | Yes, or winit | winapi | MIT | Active (2026-06), one maintainer |
| `tray-item` 0.10 | Runs its own loop thread | No | windows-sys 0.52, padlock | MIT | Last release 2024-04. The menu is append-only, so the running dashboards cannot be listed. The icon must be an exe resource (embed-resource + .rc) or a raw HICON (unsafe). |
| `winsafe` 0.0.29 | `GetMessage` is safe but `DispatchMessage` is `unsafe`. The only safe loop is through a visible `WindowMain`. | Yes, or a lot of raw shell code | No dependencies, but a large crate | MIT | Active, 0.0.x API churn |
| **`native-windows-gui` 1.0.13** | **Safe** `dispatch_thread_events()` and `stop_thread_dispatch()`. Also has MessageWindow, TrayNotification, popup Menu, and a `Notice` that wakes the loop from another thread. | **No** | winapi 0.3 and lazy_static (both already in the lock), bitflags 1.3.2, build dependency winapi-build | MIT | Frozen since 2022-09 ("mature, backlog empty"), no RustSec advisory, edition 2018 (MSRV fine) |
| winit / tao | Heavy event-loop crates, excluded by the binding rule | | | | |

Chosen: **native-windows-gui**. It is the only candidate that needs neither
winit/tao nor `unsafe` in Eario's code. It is declared in
`[workspace.dependencies]` with `default-features = false` and only the
features `cursor, menu, message-window, notice, tray-notification`. eario-hub
uses it under `[target.'cfg(windows)'.dependencies]`, so it is never built on
Linux or macOS and CI does not need GTK or appindicator. Trade-off for the
reviewer: the crate is frozen, so there are no upstream fixes, and it needs
comctl32 v6 (see Blockers). The well-maintained alternative is tray-icon plus
winit, which would need the user's approval.

Acceptance Criteria:
- AC1: `#![cfg_attr(windows, windows_subsystem = "windows")]` is on
  `src/main.rs`, and on Windows `main` calls `eario_hub::run_in_tray()`.
  - The menu offers Open the hub, the running dashboards (click to open), Stop
    all dashboards, and Quit the hub (dashboards keep running). A left click on
    the icon opens the hub.
  - The menu is rebuilt on each right click from a `watch` snapshot. A Tokio
    task refreshes that snapshot every 2 s with `Supervisor::list()`, the same
    call `GET /api/app` makes.
  - The menu actions use the same services as the HTTP API: open calls
    `Supervisor::open`, stop all calls `Supervisor::stop_all`, and Quit calls
    the server's `ShutdownHandle`.
  - The missing console was verified programmatically. The menu and icon can
    only be checked by eye (list above).
- AC2: The message loop runs on the main thread. The server, supervisor and
  refresh run on the multi-thread Tokio runtime: `block_on` runs `begin`, and
  `finish` and the actions are spawned. When the server stops (tray Quit or
  `/api/shutdown`), `finish` removes `hub.json` and posts a `Notice` that ends
  the loop. Quit leaves the children running, verified through the same
  shutdown path. If the tray cannot be created, the hub logs the error and
  serves without an icon.
- AC3: The dependency is Windows-only, uses the narrowest features, and is
  justified in a Cargo.toml comment and in the table above. Licences were
  checked by reading, since cargo-deny is not installed. Eario has no
  `unsafe`: `unsafe_code = "forbid"` still compiles.
- AC4: `run()` (terminal mode, Linux and macOS) behaves as before. `serve` is
  now `begin` plus `Session::finish`, which the tray reuses, and it prints the
  same lines. A Linux build was not verified locally.

Without a console:
- The start-up and stop lines are still printed. A parent that pipes stdout
  still gets them, and `tests/hub.rs` relies on that.
- The same lines are logged at info level, without the token, to
  `<hub dir>/hub.log` through `eario_runtime::init_file_logging` (1 MiB
  rotation, one old file kept). The file is not in `logs/`, where `hub.log`
  would be the log of a dashboard with the slug `hub`.
- A fatal error is logged to `hub.log`, and `main` still prints it to stderr.
- The first start opens the browser on the new hub; a second start opens it
  on the running hub.

Files changed: `Cargo.toml`, `Cargo.lock`, `crates/eario-hub/Cargo.toml`,
`crates/eario-hub/build.rs` (new), `crates/eario-hub/src/lib.rs`,
`crates/eario-hub/src/main.rs`, `crates/eario-hub/src/tray.rs` (new).
`deny.toml` is unchanged: no licence or exception had to be added.

Left for ticket 07 (docs): project-status and guide entries for the tray,
`hub.log` and the manual-check items.

Known limits:
- The tray shows the generic Windows application icon; the repo has no .ico
  asset.
- The icon is not re-added after Explorer restarts (nwg ignores
  TaskbarCreated).
- A second start also writes its "already running" line to `hub.log`.
- `windows-gnu` targets get a build warning instead of the embedded manifest.

Review rounds: 0 (no parent findings yet).

Parent disposition: the only option meeting the binding rule (no winit/tao, no `unsafe` in Eario) was `native-windows-gui`, frozen since 2022-09. Asked the user (frozen crate vs tray-icon + winit vs defer). **User decision: defer the tray.** All code changes of this ticket were reverted before review; nothing of them is committed. The full diff is kept outside git at `.scratch/tests/2026-11-26__standalone-and-hub/deferred-tray/06-windows-tray.patch` (with this research log copied beside it). This ticket is closed as descoped: its commit records the deferral and the crate comparison in ADR 0011 ("Amendment: the tray is deferred"). No independent review: the commit is documentation only. Ticket 07 records the limit (Windows hub keeps its console, no tray) in project-status and roadmap.
Acceptance criteria AC1-AC4: not met by decision (descoped), not by failure.

## Blockers / Challenges

- **comctl32 v6 manifest (resolved).** The first hub built with nwg exited at
  load with 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND).
  - Cause: `dumpbin /IMPORTS` shows that nwg imports `SetWindowSubclass`,
    `GetWindowSubclass`, `RemoveWindowSubclass` and `DefSubclassProc` by name
    from comctl32.dll. Only comctl32 v6 exports them by name, and v6 is loaded
    only when the application manifest asks for it.
  - Fix: a new `crates/eario-hub/build.rs` passes `/MANIFEST:EMBED` and the
    Common-Controls 6.0 `/MANIFESTDEPENDENCY` to the MSVC linker, on Windows
    MSVC targets only. No new crate and no resource compiler are needed.
  - Result: the exe now contains the manifest, and the hub and all hub tests
    start.
  - This file was not in the ticket's Files to Touch, but the chosen crate
    cannot run without it.
- **vitest environment (outside scope, not fixed).** undici 8.13.1 in
  `frontend/node_modules` does not run on Node 22.14.0 (details under Local
  Test Result). A newer Node, or `cargo xtask setup` run again on a supported
  Node, should restore it. I did not touch it.
- **Accidental workspace edit (reverted).** `cargo init` of the trial crate
  added it to the workspace `members` list. I restored Cargo.toml and
  Cargo.lock with `git checkout --` at once, before any ticket edit.

## Commits

- 36578e5 docs: defer the hub tray, record the tray crate comparison (ticket 06)

## Time Spent

5h 41m (ticket-start.js to ticket-complete.js)
