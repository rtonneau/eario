# Roadmap

This roadmap follows from the architecture as built, and from its limits as
recorded in [project-status.md](project-status.md) and the
[decision records](adr/README.md). Milestones are ordered by what unblocks
the most; none has a date.

## Two ways to run, one build

Every dashboard built with Eario must work in both ways, from the same
executable and with no code from its author for either:

- **Standalone:** the user starts the file, the dashboard opens in the
  browser (ADR 0010). This stays the default.
- **From a hub:** the user installs the dashboard into an Eario hub, a
  dashboard of dashboards that lists what is installed and starts, opens
  and stops each one (ADR 0011).

From M3 on, a milestone is done only if both ways still work.

## M0. Foundation (done)

Workspace, dashboard specification, data import, application services, local
server with its security layer, generic frontend, data-explorer example,
read-only MCP endpoint, tests, documentation.

## M1. Make it a framework: custom Rust logic (done)

The bootstrap could only combine built-in components. This milestone removed
that limit (ADR 0012). What landed:

- A registry of named Rust computations (`Computation`, `Dashboard::computation`):
  `tables + parameters → table`. A custom statistic is a computation that
  returns a small table. The specification refers to them by name.
- Derived datasets as first-class slots (`Dashboard::derived`), including a
  derived dataset computed from another, read by every component.
- Number, slider, text, select (static or from a column) and checkbox inputs.
- A cache of derived tables, one per slot, keyed by the revision of their
  dependencies (ADR 0002).
- Typed state handles, `State<T>`, in the Rust API.
- Polars re-exported as `eario::polars`.
- A second example, `signal-lab`, that uses custom computations.
- MCP reads derived datasets from the cache only (ADR 0009).

Left out on purpose: progress and cancellation (M7), custom component kinds,
Python computations (M10), persistence of derived results. Remaining limits
are listed in [project-status.md](project-status.md).

## M2. Trust and continuous verification (done)

- First CI run on Linux; fix what it finds.
- Browser end-to-end tests (Playwright, Chromium) covering import,
  selection, filter and chart rendering: `cargo xtask e2e`, CI job.
- `cargo deny` for licences and advisories: `deny.toml`, CI job.
- A minimum supported Rust version (1.95), tested in CI.
- Frontend accessibility check with axe-core in every browser test state,
  both themes, failing on any WCAG 2.1 A/AA violation.
- Benchmarks run on the development machine, with results recorded and dated
  (`cargo xtask bench`, `docs/benchmarks/`).

Left for later: browser tests in Firefox and WebKit, and a benchmark
regression gate.

## M3. Standalone and hub (done, except the tray)

Design in ADR 0011. The hub's interface is a dedicated view of the generic
frontend, chosen by what `/api/app` reports. What landed:

Launch contract, in every dashboard executable, with no code from its author:

- `--eario-manifest`: prints the manifest (name, slug, version, Eario and
  launch protocol versions, description) as JSON. `Dashboard::version`
  declares the version.
- `--eario-child <run-file>`: starts without a browser, signal handler or
  console, logs to a file, writes the run file (process id, address, token)
  once listening, removes it when it stops. Does not depend on the hub
  staying alive.
- `--eario-install` and `--eario-uninstall` (`--yes` skips the
  confirmation): copy the executable into the hub directory and register it,
  or remove both. Reinstalling the same slug replaces the entry, refused
  while it runs.
- Manifest and run file as `eario-core` contract types, tested with real
  example executables in each mode.

The hub (`eario-hub` crate and executable):

- Registry of installed dashboards, with size and SHA-256 of the installed
  copy checked before each start.
- Hub view: list, start, open, stop, stop all, uninstall; state of each child
  (starting, running, stopped, failed with its last log lines).
- Quitting the hub leaves dashboards running; the next hub re-attaches to
  those whose run file answers with its token and removes the others. One hub
  per user.
- Children started by slug only; no route takes a path or arguments
  (security document and AGENTS.md updated for this exception).
- Both examples installed and run from the hub, in tests and in Chromium.

Left out:

- **The system tray, and a hub with no console window.** Deferred by decision
  (ADR 0011, "Amendment: the tray is deferred"): it needs a safe event loop
  that the workspace's no-`unsafe` rule and the available crates do not give
  yet. It moves to M11, with the desktop shell. Today the hub is a terminal
  process on every platform.
- Install from the hub interface, side-by-side versions and a hub update
  action. The first and last were built afterwards, see "Install and update
  from GitHub" below; side-by-side versions were not. Remaining limits are in
  [project-status.md](project-status.md).

### Install and update from GitHub (done, with ADR 0013)

Built on M3, on the branch `feat/hub-github-install`, and counted here under
M11's "Updating an installed dashboard from the hub". What landed:

- Install a dashboard from a GitHub release, by catalog or `owner/repo`, with
  a preview and a confirmation by the executable's SHA-256; update, check at
  start and rollback to the previous version. In the hub view and in the
  `eario-hub` command line.
- Releases signed with minisign (`SHA256SUMS.txt.minisig`), a key per
  repository pinned on first use, a changed key refused until accepted by its
  SHA-256, and no downgrade.
- Catalogs (`catalog.json` of a repository), and several dashboards in one
  repository, installed by slug.
- The hub updates itself from a release signed with the Eario release key
  compiled into it.
- A reusable release workflow, `release-dashboard.yml`, for any dashboard
  repository, and Eario's own release through it. Guide for authors:
  [publishing-a-dashboard.md](development/publishing-a-dashboard.md).

What remains of it:

- The first real release: the maintainer's steps (release key, secrets, the
  catalog repository, a tag) are in the
  [development guide](development/guide.md#what-only-the-maintainer-can-do).
  The workflows and the self-update have never run against GitHub.
- Private repositories and access tokens, which also lift the unauthenticated
  rate limit.
- A tray for the hub (M11), and showing `self-update.log` in the hub view.
- Signing and notarization of the executables for macOS and Windows; today a
  release is signed with minisign only.
- Side-by-side versions of one dashboard.

## M4. Data: import settings and transformations

- Import settings chosen by the user: header row, separator, decimal comma,
  encoding, column types (ADR 0003).
- Dates and times as real types, in CSV and Excel.
- Sorting and column selection in tables; several filters combined.
- Grouping and aggregation, calculated columns, joins. Requires more Polars
  features: verify each feature combination builds.
- Data quality view: missing values, duplicates, constant columns.
- Parquet and JSON readers through `TableReader`.

## M5. Charts

- Histogram, bar, box plot, heatmap; several series per chart.
- Logarithmic axes, axis units, titles.
- Better downsampling: min/max or LTTB for lines, density for scatter
  (ADR 0004).
- Selection in a chart that filters other components.
- Move chart description to an `eario-charts` crate if it has grown enough.

## M6. Sessions

- Save and reopen a session: sources, import options, state (ADR 0006).
- Missing or changed source files handled explicitly.
- Recent files and sessions in the preferences.
- Recording of versions and parameters for reproducibility.
- Loading a dashboard specification from a file.

## M7. Long-running work

- Background tasks with progress and cancellation, pushed to the frontend
  (ADR 0002).
- Imports of large files off the request path, with progress.
- Updates pushed to every open tab.

## M8. Analysis

- An `eario-analysis` crate, starting with correlation, linear regression and
  curve fitting.
- Each algorithm selected by comparison with reference results; candidates
  listed in the [technical assessment](architecture/technical-assessment.md).
- Non-tabular data objects (arrays, spectra) once an analysis needs them.

## M9. Agents

- MCP tools that change state, behind per-tool user consent and an audit
  trail (ADR 0009).
- MCP resources for datasets and the specification.
- A hosted assistant panel with Claude Code as the first provider, in an
  optional `eario-agent` crate (ADR 0008).

## M10. Python

- The optional Python extension: managed subprocess, Arrow IPC (ADR 0007).

## M11. Distribution

- First release on crates.io, with the prebuilt frontend (ADR 0005).
- Project scaffolding (`eario-cli`), generating a dashboard that is
  hub-ready.
- A desktop shell (ADR 0010), hosting the hub as its main window
  next to its tray icon. The tray, deferred from M3, is built here, with a
  console-free hub on Windows.
- Signing and installers, for the hub and for standalone dashboards: the
  operating system's signing (Authenticode, Apple signing and notarization) and
  installers. The minisign signature of releases is done.
- Updating an installed dashboard from the hub: done, see "Install and update
  from GitHub" under M3. Private repositories, a tray and the first real
  release run remain.

## Further out

Visual dashboard editing, user-created layouts, scientific extensions,
larger-than-memory datasets. A hub built at compile time from a fixed set of
dashboards, for an organisation that ships them together (ADR 0011).

## Not planned

Multi-user authentication, cloud synchronization, a plugin marketplace,
mobile applications, loading arbitrary code at run time.
