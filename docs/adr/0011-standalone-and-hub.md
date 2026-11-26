# 0011. Each dashboard runs standalone or from a hub

Status: Accepted (26 November 2026). The hub's interface technology is still
open; see the last section.

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

## Open question

- **The hub's interface technology:** a dedicated hub view in the generic
  Svelte frontend, or a `DashboardSpec` built from Eario components with new
  component kinds for the list and its actions. To be decided at the start of
  M3, before the interface is built. It does not change the launch contract,
  the registry, the run files or the tray.
