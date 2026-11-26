# 0010. One executable per dashboard, loopback server, browser interface

Status: Accepted

## Context

The end user may not program. They receive a file, start it, and work with
their data locally. They must not install Rust, Node.js, Python or a web
server.

## Decision

- Each dashboard is its own executable: the developer's crate, linked with
  Eario and its embedded frontend.
- Starting it binds `127.0.0.1` on a port chosen by the operating system,
  prints the address and opens the default browser on it.
- The interface runs in the user's browser.
- The application stops when the user chooses Quit in the interface or
  presses Ctrl+C. Shutdown is graceful, bounded to five seconds.
- The security of the local server is described in
  [architecture/security.md](../architecture/security.md).

The application logic does not depend on how it is displayed:
`Dashboard::build()` returns the `App` and the server configuration, and
`eario_runtime::start()` serves it without opening a browser or handling
signals. `eario_runtime::run()` adds those for the ordinary case.

## Alternatives

- **A desktop shell from the start** (Tauri or a WebView). A window instead
  of a browser tab, and no token in a URL, but a second toolchain, platform
  WebView differences and a larger surface to maintain before the core is
  proven. Deferred, not excluded.
- **One generic executable loading dashboard definitions.** No compilation
  for the end user's developer, but it requires the declarative model and a
  plugin story that do not exist yet.
- **A fixed default port.** Bookmarkable, but collides when two dashboards
  run. A port can be fixed explicitly.

## Consequences

- The window is a browser tab: closing the tab does not stop the application.
  The process keeps running until Quit or Ctrl+C. On Windows a console window
  stays open, which also shows the address.
- A release executable is about 29 MB on Windows, mostly Polars.
- Each dashboard is distributed separately and updated by replacing the file.
- Unsigned executables trigger operating-system warnings; signing is the
  distributor's responsibility for now.

## Path to a desktop shell

A shell crate would call `Dashboard::build()` and `start()`, then open a
WebView on `RunningServer::url()`. Nothing in `App` or the HTTP layer needs
to change. Serving a native window and a browser at once is then the same
server with two clients.

## Open questions

- Stopping the application when the last tab closes (a heartbeat), without
  killing it on a page reload.
- Hiding the console window on Windows while keeping a way to see the address
  and stop the process.
- Signing, installers and auto-update.
- Linux and macOS have not been run yet (CI will be the first Linux run).
