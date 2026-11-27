# 0011. Each dashboard runs standalone or from a hub

Status: Accepted (26 November 2026). Amended the same day at the start of
M3: the interface technology is decided, and three points below changed (see
"Amendments"). Amended on 27 November 2026 by ADR 0013 (install and update
from GitHub releases).

## Context

ADR 0010 makes each dashboard its own executable. That suits a user who
receives one dashboard. A user who has several wants them in one place: see
which are installed, start and stop them, open the running ones, without
hunting for files and console windows.

Both uses must keep working with the same build. A dashboard author must not
choose between "standalone" and "for the hub", and must not write code for
either.

## Decision

### 1. Every dashboard executable is hub-ready by construction

The `eario` facade gives every executable built with `Dashboard::run()` a
small command-line contract, with no code from the author:

- **No argument:** today's behaviour. Standalone, opens the browser, stops on
  Quit or Ctrl+C.
- **`--eario-manifest`:** prints a JSON manifest and exits: name, slug,
  version, Eario version, launch protocol version, description.
- **`--eario-child <run-file>`:** starts without a browser or signal handler,
  with no console of its own, and does not depend on the hub staying alive:
  - standard input, output and error are not used; logs go to a file in the
    hub's directory (`hub/logs/<slug>.log`, rotated by size);
  - once listening, it writes the run file atomically: process id, address,
    token, start time, launch protocol version;
  - it stops on `POST /api/shutdown` or on Quit in its interface, and removes
    its run file when it stops.
- **`--eario-install` / `--eario-uninstall`:** registers or removes this
  executable in the user's hub registry, after confirmation in the terminal.

The manifest and the run file are `eario-core` contract types, versioned like
`DashboardSpec`.

### 2. A hub is a separate Eario executable

An `eario-hub` crate builds an executable distributed like a dashboard. It
keeps its data in `<config dir>/eario/hub/`:

```text
hub/
├── registry.json     installed dashboards: slug, path, size, hash, manifest
├── run/<slug>.json   one per running child, written by the child
└── logs/<slug>.log   output of each child
```

It lists the installed dashboards, starts each with `--eario-child`, shows its
state (starting, running, stopped, failed with its last log lines), opens it
in the browser and stops it. Each dashboard keeps its own process, port,
token and preferences.

### 3. Quitting the hub leaves the dashboards running

Quitting the hub does not stop the dashboards it started. On its next start
the hub reads `run/`, and for each file checks that the process id is alive
and that `GET /api/app` answers with the recorded token. It re-attaches to
those that answer and deletes the other files (a dashboard that crashed or
was stopped from its own interface).

A dashboard left running can be stopped from its own interface (Quit), or
from the hub after it restarts. A "stop all" action in the hub covers the
case where the user wants everything closed.

### 4. Updates are in place

The registry holds one entry per slug. Running `--eario-install` from a newer
executable of the same slug replaces the entry after confirmation, and is
refused while that dashboard runs. A file replaced on disk without being
reinstalled fails the hash check, and the hub tells the user to reinstall it.
There is no side-by-side install of two versions.

### 5. The hub lives in the system tray

On Windows the hub is built without a console (`windows_subsystem =
"windows"`) and shows a tray icon with: open the hub, the running dashboards
(click to open), stop all, quit. Children have no console either (point 1),
so no console window appears when the hub is used. Standalone dashboards
keep their console (ADR 0010).

On Linux and macOS the tray is shown where the desktop supports it. Without
tray support the hub stays a terminal process that prints its address, like
a dashboard.

The tray crate is chosen in M3 and justified in its pull request, as for any
dependency. A tray needs an event loop on the main thread on some platforms,
so the hub's Tokio runtime runs on other threads.

### Security

The hub starts processes, which the security rules otherwise forbid to any
endpoint. The exception is bounded:

- The hub's HTTP API starts and stops dashboards **by registry identifier
  only**. No route accepts a path, an executable or arguments.
- A dashboard enters the registry only by an action outside the HTTP API:
  `--eario-install` run by the user, or a hub command-line subcommand. An
  install from the hub interface, if added, needs its own design.
- The hub checks the executable's size and hash against the registry before
  every start.
- Run files hold session tokens. They are in the user's configuration
  directory, readable by the user only where the platform allows. A program
  running as the same user can read them; that threat is already outside the
  model of `architecture/security.md`.
- The hub binds `127.0.0.1` and uses the same Host/Origin check and session
  token as a dashboard. It opens a child in the browser by its address with
  the child's token in the fragment, and never returns a child's token to a
  page of another origin.

AGENTS.md and `architecture/security.md` are updated when the hub is
implemented, in the same change.

## Alternatives

- **One hub executable linking several dashboards at compile time.** One
  process, no launch protocol, but every dashboard must be rebuilt into the
  hub to be added, which is not "installable". It can come later as a
  `Hub::with(dashboard)` builder for an organisation that ships a fixed set.
- **The hub loads dashboard definitions (specifications) instead of
  executables.** Requires the declarative model and the plugin story that ADR
  0010 already set aside, and cannot carry custom Rust logic (M1).
- **Dashboards discovered by scanning a folder.** No install step, but any
  file dropped there becomes runnable from the browser.
- **Children stop when the hub quits** (tied to its standard input). Simpler,
  no re-attach, no run files. Rejected: the user wants closing the hub not to
  close their work.
- **Side-by-side versions.** Easy rollback, but more entries and a version to
  choose at each start. Can be added later; the registry format leaves room
  for it.
- **A console window per dashboard.** Nothing to build, but several windows
  for a user who only wanted one place.

## Consequences

- Standalone stays the default; nothing changes for a user with one file.
- A dashboard built with an older Eario than the hub is recognised by its
  launch protocol version in the manifest; the hub refuses an unknown version
  instead of guessing.
- Several dashboards run as several processes, each with its own memory, and
  may outlive the hub.
- The hub has state on disk (`run/`) that can be stale; it is checked at
  every start, never trusted.
- The hub adds a tray dependency and a main-thread event loop; dashboards do
  not.
- The hub is the natural window for the desktop shell of ADR 0010.

## Amendments (start of M3, 26 November 2026)

- **Interface technology:** a dedicated hub view in the generic Svelte
  frontend, chosen by what `/api/app` reports, with the hub routes as
  `eario-core` contract types. A `DashboardSpec` would have needed hub-only
  component kinds and the first action buttons in the generic
  `ComponentKind`.
- **Install copies the executable** into `hub/bin/<slug>/` instead of
  registering it where it lies (point 4). The download can then be moved or
  deleted; size and hash are those of the copy, so a mismatch means the copy
  was changed after install. Reinstalling replaces the copy, refused while
  that dashboard runs. Costs one copy (about 30 MB) per dashboard.
- **Liveness is checked by HTTP and token only** (point 3): a run file is
  live when `GET /api/app` at its address answers with its token. Only the
  real child holds that token; the process id stays in the file for display.
- **One hub per user:** the hub writes `hub/hub.json` (its address and
  token). A second start that finds a hub answering there opens the browser
  on it and exits.
- `--eario-child <run-file>` stays a flag. Because an executable may turn its
  own arguments into files to load, `Dashboard::run()` in child mode ignores
  the `load_file` entries whose path is exactly one of those two arguments.

## Amendment: the tray is deferred (during M3, 27 November 2026)

Point 5 is not implemented in M3. On every platform the hub is, for now, a
terminal process that prints its address and opens the browser, like a
dashboard; on Windows its console window stays open. Children still have no
console window (point 1).

The tray needs a Win32 message loop on the main thread, and the workspace
forbids `unsafe` in Eario's code. Candidates compared during M3:

| Crate | Message loop without `unsafe` in Eario | Notes |
| --- | --- | --- |
| `tray-icon` (Tauri) | Only with winit or tao | Maintained, MIT OR Apache-2.0 |
| `trayicon` | Only with winit | MIT |
| `tray-item` | Yes (own thread) | Append-only menu, cannot list running dashboards; icon needs an exe resource |
| `winsafe` | No (`DispatchMessage` is `unsafe`) | 0.0.x API |
| `native-windows-gui` | Yes | MIT, light (winapi), but no release since September 2022; needs a comctl32 v6 manifest embedded by a build script |

A working tray with `native-windows-gui` was built and set aside: the
choice is between a frozen dependency and a heavy event-loop dependency
(winit or tao), and neither was accepted for M3. Revisit when the desktop
shell of ADR 0010 is designed (M11), which brings its own event loop, or
when a maintained tray crate offers a safe loop.

## Amendment: install and update from GitHub (27 November 2026)

[ADR 0013](0013-github-install-and-update.md) widens two points, for the hub
only:

- **Install from the HTTP API is allowed**, under ADR 0013's rules: a signed
  GitHub release, previewed and then confirmed by its SHA-256, by routes that
  take an `owner/repo`, a slug or a hash as path segments, never a path, a
  URL, an executable or arguments. `--eario-install` keeps working; it never
  replaces an entry installed from GitHub.
- **Updates keep one previous version** (point 4) for a dashboard installed
  from GitHub, which a rollback restores once. A local install is still
  updated in place by `--eario-install`, with no previous version.
