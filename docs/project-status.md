# Project status

Last updated: 27 November 2026, at the end of milestone M3 (standalone and hub), except the tray, which was deferred.
Version: 0.2.0 (tagged), M3 work unreleased. Not published on crates.io. The source is public
at https://github.com/rtonneau/eario.

This document, with [architecture/overview.md](architecture/overview.md) and
[roadmap.md](roadmap.md), is meant to let someone resume the work without the
conversation that produced it.

## What works

Verified by automated tests unless noted.

| Capability | State |
| --- | --- |
| Dashboard defined through the Rust builder (`eario::Dashboard`) | Works |
| Serializable, validated dashboard specification | Works |
| CSV and TSV import with type inference and missing values | Works |
| XLSX import, several worksheets, worksheet selection, mixed types, blank headers | Works |
| Clear errors for empty, malformed, unsupported files and unknown worksheets | Works |
| Dataset metadata, paged table, column selection | Works |
| Row filter on one numeric or text column | Works |
| Scatter and line charts with backend downsampling | Works; drawn in Chromium by the browser tests |
| Descriptive statistics (count, missing, mean, std, min, median, max) | Works |
| Invalidation of only the components depending on a change | Works |
| Local server: loopback only, Host and Origin checks, session token, upload limit | Works |
| Frontend embedded in the executable | Works; release executable checked by hand outside the repository |
| Light and dark themes, system default, stored between runs | Works; both themes checked with axe-core in the browser tests; storing checked by hand |
| Clean shutdown from the interface and from Rust | Works |
| Named Rust computations (`Computation`) registered with `Dashboard::computation` | Works |
| Derived datasets (`Dashboard::derived`): a dataset slot filled by a computation from other datasets and state entries, read by every component like an imported one | Works; derived-of-derived works |
| Cache of derived tables, one per slot, keyed by the revisions of its dependencies | Works; verified by counting computation runs |
| Eager recomputation of a derived dataset that has inputs bound to it, so their state is reconciled | Works |
| Computation errors and panics shown as an alert naming the computation, application kept running | Works |
| Number, slider, text, select (static or from a column's distinct values, at most 1,000) and checkbox inputs, with defaults and server-side validation | Works; number input and select also driven in Chromium; slider, text and checkbox in jsdom and through the HTTP API only |
| Typed state handles `State<T>`, read in computations with `ctx.param(&state)` | Works |
| Polars re-exported as `eario::polars` (with the `lazy` feature) for computations | Works |
| `signal-lab` example: two pages, custom computations, synthetic fixtures | Works; driven in Chromium by the browser tests |
| Read-only MCP endpoint (five tools); derived datasets read from the cache only | Works; experimental |
| TypeScript bindings generated from Rust contract types | Works |
| Browser end-to-end tests (Playwright, Chromium) on the real `data-explorer` and `signal-lab` executables: file picker import, chart canvas, filter, table pagination, inputs recomputing derived datasets | Works; `cargo xtask e2e`, CI job on ubuntu |
| Accessibility check (axe-core, WCAG 2.1 A and AA) of every state the browser tests visit, in both themes; zero violations | Works |
| Dependency policy with `cargo deny`: licence allow-list, RustSec advisories, crates.io only | Works; CI job |
| Minimum supported Rust version 1.95, built and tested in CI | Works; tested locally with 1.95 |
| Launch contract in every dashboard executable: `--eario-manifest` (JSON manifest), `--eario-install` and `--eario-uninstall` (with confirmation, or `--yes`), `--eario-child <run-file>` (no browser, no signal handler, log file, run file); no argument still runs standalone | Works; real example executables run in each mode by the tests |
| `Dashboard::version`, declared in the manifest | Works |
| Hub directory (`EARIO_HUB_DIR`, otherwise `hub` in Eario's configuration directory): registry with size and SHA-256 of the installed copy, run files, logs; install copies the executable into `bin/<slug>/` | Works; unit tests, and tests that run the example executables |
| Liveness of a dashboard decided by its token: `GET /api/app` answers with the token of its run file | Works |
| `eario-hub` executable: lists installed dashboards, starts, opens, stops, stops all, uninstalls; one hub per user (`hub.json`); quitting the hub leaves dashboards running and the next hub re-attaches to those that answer, removing stale run files | Works; `eario-hub` integration tests with real executables, browser tests |
| Hub API by slug only (no route takes a path or arguments), behind the same Host, Origin and token checks as a dashboard; `GET /api/alive` for another hub | Works; HTTP tests |
| Hub view in the generic frontend, chosen by what `/api/app` reports: states starting, running, stopped, failed, last log lines | Works; Vitest, and Chromium with axe-core in both themes for the main flows (`hub.spec.ts`) |
| Dated benchmark results (`cargo xtask bench`, `docs/benchmarks/`) | Works; first result 2026-11-26 on the development machine |

## Verification performed

M3 (standalone and hub), Windows 11. Details per ticket are in the session
commit logs; summary:

- Covered by automated tests: the launch contract and the contract types
  (`eario-core`, `eario`); the hub directory, registry, run files and
  liveness (`eario-runtime`); the supervisor and the hub HTTP API with its
  security checks, driving the real `data-explorer`, `signal-lab` and
  `eario-hub` executables in a temporary hub directory (`eario-hub`, 4 unit
  and 8 integration tests): install, list, start, open, stop, stop all,
  uninstall, re-attach after a hub restart, stale run files, changed or
  missing executable, slug-only routes, start failure, stop timeout, second
  hub; the hub view (Vitest); and in Chromium with axe-core, in both themes,
  listing, start and open and stop, stop all, and uninstall after
  confirmation (`frontend/e2e/hub.spec.ts`, run by `cargo xtask e2e`).
- Run at the end of M3 (Windows 11, Node.js 24 through `npx -y -p node@24`):
  `cargo xtask check` passes (eario-core 76, eario-runtime 72, eario 25 plus
  5 end-to-end tests, eario-hub 4 plus 8, eario-data 30 plus 12, signal-lab
  and data-explorer launch tests, 65 Vitest tests, svelte-check clean);
  `cargo xtask e2e` passes, 10 browser tests in Chromium with no
  accessibility violation (4 data-explorer, 2 signal-lab, 4 hub);
  `cargo xtask dist` builds `target/release/data-explorer.exe` (32.2 MB) and
  `target/release/eario-hub.exe` (5.8 MB) and prints both paths.
- Checked by hand only: one smoke run of `eario-hub.exe` (ticket 04): start,
  address printed, stop. The tray (ticket 06) was researched, not shipped; see
  ADR 0011.
- Not run locally for M3: `cargo deny check` (not installed on the development
  machine) and the tests with Rust 1.95 (toolchain not installed there). CI
  runs both.
- Never run on Linux or macOS: the Unix-only code (file modes `0700`/`0600`,
  process group of a child) is compiled and tested for the first time by CI on
  ubuntu.

On Windows 11, Rust 1.99.1, Node.js 25.6.1:

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace --all-features`: every test binary passes
  (eario-core 63, eario-data 30 plus 12 import tests, eario-runtime 53,
  eario 18 plus 5 end-to-end tests, eario-mcp 4, signal-lab 9, among others),
  plus the documentation tests. The eario-core count includes the tests
  generated by ts-rs that write the TypeScript bindings.
- `cargo doc --workspace --no-deps --all-features`: no warning.
- `npm run check` (svelte-check): 0 errors, 0 warnings.
- `npm run test` (Vitest): 44 tests pass.
- `npm run build`: succeeds; one 652 kB JavaScript bundle.
- `cargo build --release -p data-explorer`: succeeds; 29 MB executable. Copied
  alone to an empty directory, it served the dashboard with a CSV file and
  stopped cleanly.
- Manual checks with headless Chrome screenshots: XLSX and CSV loaded, chart
  drawn, filter applied through the API and reflected after reload, light and
  dark themes.
- MCP checked with `curl` against a running application: `initialize`,
  `tools/list`, `tools/call`, and 401 without the token.
- `cargo xtask check`, which chains all of the above, passes (run with
  `CARGO_BUILD_JOBS=4`, see the development guide). The M1 figures above were
  taken at the end of M1; the manual checks and measurements below date from
  the bootstrap and were not repeated.
- M2, on the same machine: `cargo xtask e2e`, 6 browser tests pass in
  Chromium with no accessibility violation in either theme; `cargo deny check`
  passes (two quick-xml advisories ignored with a documented reason in
  `deny.toml`: the affected copy is only reached through Polars' cloud
  storage code, which Eario never runs); `cargo +1.95 test --workspace
  --all-features` passes; `cargo xtask bench` wrote
  [benchmarks/2026-11-26.md](benchmarks/2026-11-26.md).
- CI (`.github/workflows/ci.yml`) has run green on ubuntu and windows
  (run 45779398561, before M2). The M2 jobs (End-to-end, Rust minimum
  version, Dependencies) first run on the M2 pull request.
- `cargo package -p eario-runtime --list` includes the built frontend.
- One informal measurement with the release executable and the generated
  one-million-row CSV (34 MB, five columns), through the HTTP API on the
  development machine: import 0.14 s; chart reduced to 5000 points 0.05 s;
  statistics 0.10 s; filter keeping 327,000 rows then a table page 0.02 s;
  about 68 MB of memory. This is a single run, not a benchmark.

## Not verified

- **The hub on Linux and macOS.** Not built or run by hand; Linux is covered by CI only.
- **macOS.** Never built or run. Linux is covered by CI only (build, tests,
  browser tests), not by manual use.
- **The release workflows.** `release.yml` and the reusable
  `release-dashboard.yml` (four targets, signed `SHA256SUMS.txt`) were
  checked with actionlint and shellcheck, and their shell steps were run
  locally with throwaway keys, but they have never run on GitHub: that needs
  the release key's secrets and a tag. The x86_64 macOS executable is
  cross-compiled and never run. Eario's release stops before building while
  `crates/eario-hub/eario-release.pub` is the placeholder.
- **Opening the default browser** (`webbrowser::open`). The code path exists;
  all runs used `EARIO_NO_BROWSER`.
- **Ctrl+C handling.** Implemented; only shutdown through the API and from
  Rust was exercised.
- **Some interactions in a real browser**: drag and drop of a file, chart
  zoom and pan, the slider, text and checkbox inputs, and worksheet selection
  are not driven by the browser tests. Firefox and Safari are never tested.
- **The Vite development server workflow** described in the development
  guide. The backend side (`EARIO_DEV_ORIGIN`) is implemented but the
  combination was not run.
- **Offline operation** was checked by inspection, not by disconnecting: the
  page references only its own origin and the Content Security Policy forbids
  anything else.
- **Benchmarks on any machine but the development one.** One dated result
  exists; there is no comparison between runs yet and no regression gate.
- **Datasets above one million rows**, wide tables, and large XLSX files.
- **Publishing to crates.io.** The file list of the package was checked; a
  full `cargo package` verification build and a publish dry run were not.

## Known limitations

- Components are a closed set: custom logic goes through computations that
  produce derived datasets, not through new component kinds (ADR 0001,
  ADR 0012).
- State is lost when the application stops; only the theme is stored.
- One filter, on one column. No sorting, grouping, joins or calculated columns.
- No date or time types: Excel dates appear as serial numbers, CSV dates as
  text (ADR 0003).
- Import has no settings beyond the worksheet: first row is the header,
  comma or tab separator from the extension, UTF-8.
- Uploads are held in memory; the limit is 256 MiB by default.
- Line charts are downsampled by even sampling, which can miss narrow peaks
  (ADR 0004).
- Computations have no progress reporting and no cancellation, and there is
  no background task (ADR 0002). A computation runs to its end on a blocking
  thread; a long one delays the request that needs it.
- The cache holds the latest result per derived slot, not a history. Memory
  grows with the number of derived slots. Imported tables and filtered views
  are still recomputed at each query.
- MCP never computes. A derived dataset that the dashboard has not displayed
  yet, or whose cache is stale, is reported as not computed (ADR 0009).
- Reading a parameter by handle (`ctx.param(&state)`) works only where that
  state is bound to a parameter of the derived dataset being computed;
  otherwise it fails with `unbound_state`. A computation reused with literals
  or different bindings must read its parameters by name.
- `ctx.param` does not accept `&&str` or `&Cow<str>` names: write `*name` or
  `name.as_ref()`.
- A column select bound to a derived dataset that reads the select's own state
  is not rejected, although it has the same trap that the `Select` now
  rejects (its options depend on the value it writes). A `Select` offering the values of such a dataset is rejected at start-up
  (`select_depends_on_its_state`).
- The retry paths taken when a table or a revision changes between two lock
  acquisitions (stale version during a select check or a reconciliation) have
  no test, because they need a controlled interleaving of two requests.
- The built frontend in `crates/eario-runtime/frontend-dist` is not tracked
  and not rebuilt by `cargo xtask check`. After a change in `frontend/`, run
  `cargo xtask frontend`, or the new inputs do not appear in the browser.
- A change made in one tab is not pushed to the other open tabs; they see it
  at their next interaction or reload.
- Closing the browser tab does not stop the application.
- The port and token change at each start, which is awkward for a
  permanently configured MCP client.
- The interface is in English only.
- **No tray.** The hub is a terminal process that prints its address and
  opens the browser, on every platform. On Windows its console window stays
  open (children have none). The tray was deferred (ADR 0011, "Amendment: the
  tray is deferred"); it moves to roadmap M11, next to the desktop shell.
- No install from the hub interface: a dashboard enters the hub only through
  its own `--eario-install`.
- No side-by-side versions: one registry entry per slug; reinstalling replaces
  it and is refused while the dashboard runs.
- No update action in the hub: updating means running `--eario-install` from
  the newer executable.
- Two hubs launched at nearly the same instant can both run; the single
  instance check reads `hub.json` and is not a lock.
- Concurrent `--eario-install` runs into one hub directory, or an install
  racing a hub uninstall, can lose one registry update (the registry is
  written atomically but read-modify-write is not locked). Seen in a test
  fixture of the hub view.
- A dashboard that misses one 2-second liveness probe while the hub lists
  dashboards loses its run file and shows as stopped, though it may still run.
- The hub has no theme preference (no `/api/preferences`).
- The failed-start state of the hub view is covered by Vitest only, not by a
  browser test.
- Hub directory: `%APPDATA%\eario\config\hub` on Windows, `~/.config/eario/hub`
  on Linux (from `directories::ProjectDirs`), unless `EARIO_HUB_DIR` is set.

## Naming

- `eario` was free on crates.io on 26 November 2026. It is not reserved.
- `rtonneau/eario` was created on GitHub, public, on 26 November 2026.
- An unrelated project uses the name: `ThomasDev-1/Eario`, a small Windows
  tray utility for audio routing. Different domain, not on crates.io. No
  trademark search was made.

## Repository state

- Branch `main`, pushed to `origin` (`github.com/rtonneau/eario`).
- Not tracked: `target/`, `node_modules/`,
  `crates/eario-runtime/frontend-dist/` (build output), `fixtures/generated/`.

## Where to start next

Milestones M1 (custom Rust computations), M2 (trust and continuous
verification) and M3 (standalone and hub, without the tray) are done; the next ones are in the
[roadmap](roadmap.md). Entry points for what exists:

- `eario_data::compute` (`Computation`, `ComputeContext`, `ComputeError`) and
  `eario_runtime::compute` (dependency order, evaluation, panic catching).
- `eario_runtime::App`: `derived_table`, the cache, `reconcile_derived` and
  the cached-only reads used by MCP.
- `eario_core::DatasetSource` and `ComponentKind` for the contract.
- `eario_runtime::hub_dir` (launch contract, hub
  directory) and the `eario-hub` crate (`Supervisor`, hub API).
- `examples/signal-lab` as the model of a dashboard with computations.

The limits above, and ADR 0012, say what M1 did not do.
