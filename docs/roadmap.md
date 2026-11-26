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

## M3. Standalone and hub

Comes early so that every dashboard built from now on can be installed in a
hub without being rebuilt. Design in ADR 0011 (accepted).

First, decide the open question of ADR 0011: the hub's interface as a
dedicated frontend view or as a `DashboardSpec` of Eario components.

Launch contract, in every dashboard executable:

- `--eario-manifest`: prints the manifest (name, slug, version, Eario and
  launch protocol versions) as JSON.
- `--eario-child <run-file>`: starts without a browser or console, logs to a
  file, writes the run file (process id, address, token) once listening,
  removes it when it stops. Does not depend on the hub staying alive.
- `--eario-install` and `--eario-uninstall`: register the executable in the
  user's hub registry, after confirmation; reinstalling a newer file of the
  same slug replaces the entry, refused while it runs.
- Manifest and run file as `eario-core` contract types, with tests that run
  a real example executable in each mode.

The hub (`eario-hub` crate and executable):

- Registry of installed dashboards, with the executable's hash checked
  before each start.
- Interface: list, start, open, stop, stop all, uninstall; state of each
  child (starting, running, stopped, failed with its last log lines).
- Quitting the hub leaves dashboards running; at start the hub re-attaches
  to live run files and removes stale ones.
- System tray on Windows, no console window for the hub or its children;
  tray where supported on Linux and macOS, terminal otherwise.
- Children started by registry identifier only; no route takes a path or
  arguments. Security document and AGENTS.md updated for this exception.
- Both examples installable and runnable from the hub, verified end to end.

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
  next to its tray icon.
- Signing and installers, for the hub and for standalone dashboards.
- Updating an installed dashboard from the hub.

## Further out

Visual dashboard editing, user-created layouts, scientific extensions,
larger-than-memory datasets. A hub built at compile time from a fixed set of
dashboards, for an organisation that ships them together (ADR 0011).

## Not planned

Multi-user authentication, cloud synchronization, a plugin marketplace,
mobile applications, loading arbitrary code at run time.
