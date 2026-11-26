# Session: standalone-and-hub

**Date:** 2026-11-26T21:32:08.215Z
**Status:** Grill phase complete

## Problem Statement

Milestone M3 (ADR 0011): every dashboard executable must run both standalone and from an Eario hub, from the same build and with no code from its author. Today a dashboard only runs standalone; a user with several dashboards has no single place to see, start, open and stop them.

Size: high-risk. It adds a public command-line contract, a new crate, and the one bounded exception to "no endpoint may start a process".

## Context & Constraints

- **Current behavior:** `Dashboard::run()` builds the app (loading `load_file` entries), serves it on `127.0.0.1`, opens the browser, stops on Quit or Ctrl+C. The examples turn every command-line argument into `load_file(...)`. Preferences live under `<config dir>/eario/<title slug>/`. The `/api` router accepts requests without an `Origin` header, so a non-browser client with the token can call `/api/app` and `/api/shutdown`.
- **Pain point:** no launch contract, no hub, no way to run a dashboard without a console window or browser, no place listing installed dashboards.
- **Dependencies:** standalone behaviour with no `--eario-*` argument must stay exactly as today (ADR 0010). Crate direction `eario` → `eario-mcp` → `eario-runtime` → `eario-data` → `eario-core`; contract types in `eario-core` with regenerated TypeScript bindings. Security rules: loopback only, Host/Origin check and token on every route, no user-supplied path, no command executed on request (except the bounded hub exception). MSRV 1.95, no `unsafe`, no `unwrap`/`expect` outside tests, `cargo deny` policy.
- **Tech stack:** Rust (axum, tokio, serde, directories), Svelte 5 + TypeScript frontend embedded in `eario-runtime`, ts-rs bindings, Playwright + axe for browser tests. New: a SHA-256 implementation and a Windows tray crate, each justified in its ticket.

## Success Metrics

- With no `--eario-*` argument every dashboard behaves exactly as before; existing tests and e2e specs still pass.
- `--eario-manifest` prints a JSON manifest (name, slug, version or null, Eario version, launch protocol version 1, description) and exits, without loading any file, for both examples.
- `--eario-install [--yes]` copies the executable into `hub/bin/<slug>/` and records it in `registry.json` with size and SHA-256 of the copy; reinstall replaces it after confirmation; both install and uninstall are refused while that slug runs, and refused without a terminal unless `--yes`.
- `--eario-child <run-file>` serves without browser, console use or signal handler, ignores the `load_file` entries equal to those two arguments, logs to `hub/logs/<slug>.log` (1 MB rotation, one old file), writes the run file atomically once listening and removes it on stop.
- The `eario-hub` executable lists installed dashboards and starts, opens, stops, stops all and uninstalls them by slug only; a start checks size, hash and protocol version and reports a failed start with its last log lines within 30 s.
- Quitting the hub leaves children running; at the next start the hub re-attaches to run files whose child answers `GET /api/app` with the recorded token and deletes the others. A second hub start opens the running hub and exits.
- On Windows the hub has no console window and a tray icon (open hub, running dashboards, stop all, quit); children have no console window. On Linux and macOS the hub runs in the terminal and prints its address.
- Rust end-to-end tests run the real example executables in each mode against a temporary `EARIO_HUB_DIR`; a Playwright spec drives the hub view with axe checks in both themes; `cargo xtask check`, `cargo xtask e2e` and `cargo deny check` pass.

## Architecture & Approach

Launch contract, in the `eario` facade (no author code):

- `Dashboard::run()` reads `std::env::args()` before `build()`. `--eario-manifest`, `--eario-install [--yes]` and `--eario-uninstall [--yes]` never build the app or read files. `--eario-child <run-file>` builds the app after dropping the `load_file` entries whose path is exactly `--eario-child` or the run-file path.
- Identity: the slug is the title slug the preferences directory already uses (`preferences::slug`, made shareable). Version from an optional new `Dashboard::version(env!("CARGO_PKG_VERSION"))`; `null` otherwise and shown as "unknown". The examples call it.
- Install confirmation in the terminal shows old and new version; `--yes` skips it; without a terminal and without `--yes` it is refused.
- Contract types in `eario-core`: `Manifest`, `RunFile`, `LAUNCH_PROTOCOL_VERSION = 1`, and the hub API payloads, with regenerated bindings. Hub directory, registry and run-file I/O live in `eario-runtime` (shared by the facade and the hub). Hub directory `<config dir>/eario/hub/`, overridable by `EARIO_HUB_DIR`. Registry has its own format version; files are written atomically (temp file + rename).
- `eario-runtime` gains a server entry point serving another route set behind the same Host/Origin and token middleware and embedded frontend, plus a child mode of `run` (no browser, no Ctrl+C, file logging, run file).

Hub (`crates/eario-hub`, library plus executable):

- Interface: a dedicated hub view in the same generic Svelte bundle, chosen from what `/api/app` reports; hub routes are contract types. No new `ComponentKind`.
- HTTP API by slug only: list, start, stop, stop all, uninstall, open (returns the child URL with token to the hub's own page), quit. No route accepts a path, executable or arguments. The registry is re-read on each request.
- Start: size, SHA-256 and protocol-version check, spawn without a console window (Windows `CREATE_NO_WINDOW`) or in its own process group (Unix), working directory the copy's directory; wait up to 30 s for the child to answer with its run file's token; otherwise state `failed` with the last log lines.
- Liveness: a run file is live when `GET /api/app` at its address answers with its token (no process-id check). Re-attach at start, delete stale files. `hub/hub.json` (address, token) makes one hub per user; a second start opens the browser on the running hub and exits.
- Stop: `POST /api/shutdown` to the child; kill after a timeout only for children this hub process started. Quit leaves children running.
- The frontend hub view polls the hub state every 2 s.
- Windows: `windows_subsystem = "windows"` for the hub, tray on the main thread, Tokio on other threads. Linux/macOS: terminal process printing its address.

Docs: ADR 0011 amended during the grill; AGENTS.md and `architecture/security.md` record the bounded process-start exception; overview, project status, roadmap, README updated; `cargo xtask dist` also builds the hub.

## Assumptions & Trade-offs

- Install copies the executable (about 30 MB per dashboard) so the download can be moved or deleted; no side-by-side versions; a bad update is rolled back by reinstalling the older file.
- Liveness by token only: only the real child holds the token; the pid is kept for display.
- The child-mode flag stays `--eario-child`; filtering the two matching `load_file` entries relies on authors passing raw arguments as paths, which is the case in both examples.
- Not doing: tray on Linux/macOS (later item), install from the hub interface, a hub install subcommand, a compile-time hub, update from the hub (M11), side-by-side versions, a desktop shell.
- The contract only adds behaviour behind `--eario-*` arguments, so a broken hub never breaks standalone use.

## Open Questions

- The tray crate: it must work with no `unsafe` in Eario's code. If it needs a heavy event-loop dependency (winit or tao), stop and ask the user before adding it.
- SHA-256 implementation choice (for example `sha2`), justified in its ticket.

## Notes

- Grill decisions: dedicated hub view (closes ADR 0011's open question); all of M3 in this session; slug from title plus optional `.version()`; tray on Windows only; child-mode flag filtered by `run()`; install by copy; one hub per user; liveness by HTTP and token only.
- ADR 0011 already amended with these decisions; glossary gained the "Standalone and hub" terms (Standalone, Hub, Child, Launch contract, Slug, Manifest, Registry, Run file).
- Windows children inherit no console when spawned with `CREATE_NO_WINDOW`; on Unix `process_group(0)` keeps Ctrl+C in the hub's terminal from reaching children.
- The user chose `/gps auto` after approving the design; high-risk tickets still need explicit approval before `write:plan`.
