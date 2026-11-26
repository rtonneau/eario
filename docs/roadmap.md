# Roadmap

This roadmap follows from the architecture as built, and from its limits as
recorded in [project-status.md](project-status.md) and the
[decision records](adr/README.md). Milestones are ordered by what unblocks
the most; none has a date.

## M0. Foundation (done)

Workspace, dashboard specification, data import, application services, local
server with its security layer, generic frontend, data-explorer example,
read-only MCP endpoint, tests, documentation.

## M1. Make it a framework: custom Rust logic

The bootstrap can only combine built-in components. This milestone removes
that limit, which is the largest one.

- A registry of named Rust functions in `App`: computed datasets
  (`tables + parameters → table`) and custom statistics. The specification
  refers to them by name (ADR 0001).
- Derived datasets as first-class slots, so a view can read the output of a
  computation.
- More input components: number, text, slider, select, checkbox.
- A cache of derived tables keyed by the revision of their dependencies
  (ADR 0002).
- Typed state handles in the Rust API.
- A second example that uses a custom computation.

## M2. Trust and continuous verification

- First CI run on Linux; fix what it finds.
- Browser end-to-end tests (Playwright) covering import, selection, filter
  and chart rendering.
- `cargo deny` for licences and advisories.
- A minimum supported Rust version, tested in CI.
- Frontend accessibility review with an automated checker.
- Benchmarks run on a reference machine, with results recorded and dated.

## M3. Data: import settings and transformations

- Import settings chosen by the user: header row, separator, decimal comma,
  encoding, column types (ADR 0003).
- Dates and times as real types, in CSV and Excel.
- Sorting and column selection in tables; several filters combined.
- Grouping and aggregation, calculated columns, joins. Requires more Polars
  features: verify each feature combination builds.
- Data quality view: missing values, duplicates, constant columns.
- Parquet and JSON readers through `TableReader`.

## M4. Charts

- Histogram, bar, box plot, heatmap; several series per chart.
- Logarithmic axes, axis units, titles.
- Better downsampling: min/max or LTTB for lines, density for scatter
  (ADR 0004).
- Selection in a chart that filters other components.
- Move chart description to an `eario-charts` crate if it has grown enough.

## M5. Sessions

- Save and reopen a session: sources, import options, state (ADR 0006).
- Missing or changed source files handled explicitly.
- Recent files and sessions in the preferences.
- Recording of versions and parameters for reproducibility.
- Loading a dashboard specification from a file.

## M6. Long-running work

- Background tasks with progress and cancellation, pushed to the frontend
  (ADR 0002).
- Imports of large files off the request path, with progress.
- Updates pushed to every open tab.

## M7. Analysis

- An `eario-analysis` crate, starting with correlation, linear regression and
  curve fitting.
- Each algorithm selected by comparison with reference results; candidates
  listed in the [technical assessment](architecture/technical-assessment.md).
- Non-tabular data objects (arrays, spectra) once an analysis needs them.

## M8. Agents

- MCP tools that change state, behind per-tool user consent and an audit
  trail (ADR 0009).
- MCP resources for datasets and the specification.
- A hosted assistant panel with Claude Code as the first provider, in an
  optional `eario-agent` crate (ADR 0008).

## M9. Python

- The optional Python extension: managed subprocess, Arrow IPC (ADR 0007).

## M10. Distribution

- First release on crates.io, with the prebuilt frontend (ADR 0005).
- Project scaffolding (`eario-cli`).
- A desktop shell (ADR 0010).
- Signing and installers.

## Further out

Visual dashboard editing, user-created layouts, scientific extensions,
larger-than-memory datasets.

## Not planned

Multi-user authentication, cloud synchronization, a plugin marketplace,
mobile applications, loading arbitrary code at run time.
