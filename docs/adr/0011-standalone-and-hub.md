# 0011. Each dashboard runs standalone or from a hub

Status: Proposed

## Context

ADR 0010 makes each dashboard its own executable. That suits a user who
receives one dashboard. A user who has several wants them in one place: see
which are installed, start and stop them, open the running ones, without
hunting for files and console windows.

Both uses must keep working with the same build. A dashboard author must not
choose between "standalone" and "for the hub", and must not write code for
either.

## Decision

Two parts, both proposed:

1. **Every dashboard executable is hub-ready by construction.** The `eario`
   facade gives every executable built with `Dashboard::run()` a small
   command-line contract, with no code from the author:
   - no argument: today's behaviour (standalone, opens the browser);
   - `--eario-manifest`: prints a JSON manifest (name, slug, version, Eario
     version, launch protocol version, description) and exits;
   - `--eario-child`: starts without a browser or signal handler, prints one
     machine-readable ready line (address and token) on standard output, and
     stops when its standard input closes or the hub calls
     `POST /api/shutdown`;
   - `--eario-install` / `--eario-uninstall`: registers or removes this
     executable in the user's hub registry, after confirmation in the
     terminal.

   The manifest and ready line are `eario-core` contract types, versioned
   like `DashboardSpec`.

2. **A hub is a separate Eario executable** (`eario-hub` crate, built and
   distributed like a dashboard). It reads a registry in
   `<config dir>/eario/hub/registry.json`, lists the installed dashboards
   with their manifest, starts each as a child process with `--eario-child`,
   shows its state, opens it in the browser and stops it. Each dashboard keeps
   its own process, port, token and preferences.

### Security

The hub starts processes, which the security rules otherwise forbid to any
endpoint. The exception is bounded:

- The hub's HTTP API starts and stops dashboards **by registry identifier
  only**. No route accepts a path, an executable or arguments.
- A dashboard enters the registry only by an action outside the HTTP API:
  `--eario-install` run by the user, or a hub command-line subcommand. An
  install from the hub interface, if added, needs its own design.
- The hub records the executable's size and hash at install and refuses to
  start a file that changed since.
- The hub binds `127.0.0.1` and uses the same Host/Origin check and session
  token as a dashboard. It holds each child's token and never shows it to a
  page of another origin.

AGENTS.md and `architecture/security.md` must be updated when this is
implemented.

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

## Consequences

- Standalone stays the default; nothing changes for a user with one file.
- A dashboard built with an older Eario than the hub is recognised by its
  launch protocol version in the manifest; the hub refuses an unknown version
  instead of guessing.
- Several dashboards run as several processes, each with its own memory.
- The hub is the natural window for the desktop shell of ADR 0010.

## Open questions

- Whether the hub stops all children when it quits, or leaves them running.
- Whether the hub's interface is a dashboard built with Eario components or a
  dedicated frontend page.
- Updating an installed dashboard: replace in place, or install side by side
  by version.
- A single tray icon or console for all running dashboards on Windows.
