# Development guide

## Prerequisites

- A stable Rust toolchain installed with [rustup](https://rustup.rs). The
  repository's `rust-toolchain.toml` selects `stable` with rustfmt and clippy.
- [Node.js](https://nodejs.org) with npm, to build the frontend. The project
  is developed with Node.js 25; CI uses Node.js 24. The frontend tests
  (Vitest) need Node.js 22.19 or later (undici 8.13.1). On a machine with
  an older Node.js, run the commands through a newer one without
  installing it: `npx -y -p node@24 -- cargo xtask check`.

No other tool is needed. The commands below work the same on Windows and
Linux and do not require a POSIX shell.

## First build

```text
cargo xtask setup       # npm ci in frontend/
cargo xtask frontend    # build the frontend into crates/eario-runtime/frontend-dist
cargo run -p data-explorer -- fixtures/csv/measurements.csv
```

If you skip the frontend build, Rust still compiles: `eario-runtime`'s build
script writes a placeholder page and prints a warning. The dashboard itself
needs the real build.

The first Rust build is long, mostly because of Polars (with its `lazy`
feature).

### Building with little memory or disk

Compiling the Polars crates in parallel can exhaust memory (rustc aborts with
`memory allocation ... failed`, or the host crashes). Limit the parallel jobs:

```text
CARGO_BUILD_JOBS=4 cargo xtask check
```

In PowerShell, set the variable first: `$env:CARGO_BUILD_JOBS = "4"`.

The workspace `Cargo.toml` also sets

```toml
[profile.dev.package."*"]
debug = "line-tables-only"
```

Full debug symbols for every dependency cost about 1 GB per test binary once
Polars `lazy` is on. On Windows that filled the disk and ended in linker
errors (`LNK1140`, `LNK1318`). Line tables are enough for readable
backtraces, and your own crates keep full debug information.

## Commands

| Command | What it does |
| --- | --- |
| `cargo xtask setup` | Installs frontend dependencies from the lockfile. |
| `cargo xtask frontend` | Builds the frontend. |
| `cargo xtask gen-types` | Regenerates `frontend/src/lib/generated/` from the Rust contract types. |
| `cargo xtask fixtures` | Regenerates the files in `fixtures/`. |
| `cargo xtask large-csv` | Writes `fixtures/generated/large.csv`, one million rows, not tracked by git. |
| `cargo xtask test` | Runs Rust and frontend tests. |
| `cargo xtask check` | Runs every CI check except the browser tests, `cargo deny` and the minimum-version build. |
| `cargo xtask dist` | Builds the frontend, then `data-explorer` and `eario-hub` in release mode, and prints the path of each executable. |
| `cargo xtask e2e` | Builds the frontend, the examples, the hub and the fake GitHub, then runs the browser tests in Chromium. |
| `cargo xtask bench` | Runs the benchmarks and records the results in `docs/benchmarks/<date>.md`. |

Environment variables read by an Eario application:

| Variable | Effect |
| --- | --- |
| `EARIO_PORT` | Listen on this port instead of one chosen by the system. |
| `EARIO_NO_BROWSER` | When set to anything but `0`, do not open a browser. |
| `EARIO_LOG` | Log filter, for example `debug` or `eario_runtime=debug`. Default `warn`. |
| `EARIO_DEV_ORIGIN` | Extra allowed origin. Debug builds only. |
| `EARIO_HUB_DIR` | Hub directory, for `--eario-install`, `--eario-uninstall`, `--eario-child` and `eario-hub`. Default: `hub` in Eario's configuration directory (`%APPDATA%\eario\config\hub` on Windows, `~/.config/eario/hub` on Linux). Use a temporary one when trying the hub, so your own installation stays untouched. |

## Working on the frontend

In debug builds the backend reads the frontend files from disk on each
request, so after `cargo xtask frontend` a browser reload is enough; the Rust
side need not be rebuilt.

For hot reloading, run the backend on the port the Vite proxy expects and
allow the Vite origin.

PowerShell:

```powershell
$env:EARIO_PORT = "4780"; $env:EARIO_NO_BROWSER = "1"; $env:EARIO_DEV_ORIGIN = "http://localhost:5173"
cargo run -p data-explorer
```

Linux:

```sh
EARIO_PORT=4780 EARIO_NO_BROWSER=1 EARIO_DEV_ORIGIN=http://localhost:5173 cargo run -p data-explorer
```

Then, in another terminal:

```text
cd frontend
npm run dev
```

Open `http://localhost:5173/#token=<token>`, with the token from the address
the backend printed.

Frontend commands, run in `frontend/`:

| Command | What it does |
| --- | --- |
| `npm run dev` | Vite development server with a proxy to the backend. |
| `npm run build` | Production build. |
| `npm run check` | `svelte-check`: TypeScript and Svelte diagnostics. |
| `npm run test` | Vitest, in jsdom. |

## Writing a computation

A computation is a named Rust function from tables and parameters to a table.
Register it, then declare a derived dataset that binds its inputs and
parameters. The complete example is
[`examples/signal-lab`](../../examples/signal-lab/src/main.rs); this is the
shape:

```rust
use eario::polars::prelude::*;
use eario::prelude::*;

fn above() -> Computation {
    let threshold = State::<f64>::new("threshold");
    Computation::new("above", move |ctx| {
        let limit = ctx.param(&threshold)?; // typed by the handle
        let rows = ctx.input("table")?.clone().lazy();
        Ok(rows.filter(col("value").gt(lit(limit))).collect()?)
    })
    .input("table") // names the function reads, declared up front
    .param("threshold")
    .describe("The rows whose `value` exceeds `threshold`.")
}

fn dashboard() -> Dashboard {
    let data = DatasetId::new("data");
    let large = DatasetId::new("large");
    let threshold = State::<f64>::new("threshold");

    Dashboard::new("Threshold")
        .computation(above())
        .derived(&large, "above", |derived| {
            derived.input("table", &data).param("threshold", &threshold)
        })
        .page("Rows", |page| {
            page.row([file_import(&data).span(4), slider("Threshold", &threshold, 0.0..=100.0, 50.0)])
                .row([data_table(&large)])
        })
}
```

What to know:

- Every component reads `large` as it reads an imported dataset: a table, a
  chart, statistics, a column select, a filter. A derived dataset can read
  another one; the computations run in dependency order.
- `.input` and `.param` on the `Computation` declare names; `derived.input`,
  `.param` and `.literal` bind them. `App::new` (so `build()` and `run()`)
  rejects an unknown computation, an undeclared or missing binding, an import
  into a derived slot, a cycle, and a duplicate dataset or computation name.
  Each error names the dataset and the computation.
- A computation returns `Result<DataFrame, ComputeError>`. `ComputeError::new`
  takes a message written for the user; a Polars error converts with `?`. An
  error or a panic shows as an alert naming the computation and does not stop
  the application.
- A parameter whose state is still unset (`null`) makes the component say
  which input to set. Give inputs a default to avoid that.
- Results are cached by the revisions of the inputs, so a computation must
  depend only on its declared inputs and parameters. It runs on a blocking
  thread and has no progress reporting or cancellation yet.
- `ctx.param(&state)` finds the parameter bound to that state in the derived
  dataset being computed. If the computation is reused with literals or with
  other states, read by name: `ctx.param::<f64>("threshold")`. A `&&str` or
  `&Cow<str>` name does not compile as is: pass `*name` or `name.as_ref()`.
- A select or column select bound to a derived dataset must not offer values
  of a dataset that reads that select's own state.
- To test a computation without a server, build a `ComputeContext::new(inputs,
  params)` (from `eario::data`) and call `Computation::run`; use `bind_state`
  when the computation reads parameters by handle. `signal-lab` does this.
- MCP tools never run a computation (ADR 0009): a derived dataset becomes
  readable by an agent once the dashboard has displayed it.

After changing the frontend, rebuild it with `cargo xtask frontend`:
`cargo xtask check` does not, and the embedded bundle is not tracked.

## The frontend/backend contract

The types exchanged over HTTP are defined once, in `eario-core`. With the
`ts` feature they derive `ts_rs::TS`, and `cargo xtask gen-types` writes one
TypeScript file per type into `frontend/src/lib/generated/`. These files are
committed so the frontend builds without running Cargo. CI regenerates them
and fails on a difference.

Do not edit the generated files. Change the Rust type and regenerate.

## Testing

| Level | Location | Covers |
| --- | --- | --- |
| Rust unit tests | beside the code | Specification validation and serialization, queries, type inference, preferences, builder. |
| Import tests | `crates/eario-data/tests/import.rs` | Every file in `fixtures/`, including malformed ones. |
| HTTP tests | `crates/eario-runtime/src/http_tests.rs` | Every endpoint, invalidation, errors, and the security layer, through the router. |
| End-to-end tests | `crates/eario/tests/end_to_end.rs` | A dashboard built with the public API, served on a real socket, including MCP. |
| Frontend unit tests | `frontend/src/lib/*.test.ts` | API client, chart option, formatting, the hub's texts for keys and update checks. |
| Frontend rendering test | `frontend/src/App.test.ts` | A whole dashboard rendered from a specification, with the backend mocked. |
| Hub tests | `crates/eario-hub/tests/hub.rs`, `crates/eario-hub/tests/catalog_example.rs`, `examples/*/tests/launch.rs`, `crates/eario-runtime/src/hub_dir.rs` | The launch contract and the hub with the real example executables in a temporary hub directory: install, start, open, stop, re-attach, changed executable, slug-only routes; install and update from a fake GitHub; `catalog_example.rs` reads `docs/catalog/catalog.json` with the hub's catalog type. |
| Browser tests | `frontend/e2e/*.spec.ts` | The real examples and the real hub in Chromium, each state checked with axe-core: import through the file picker, chart drawn in its canvas, filter, table pagination, inputs that recompute a derived dataset in Rust, and the hub view (list, start, open, stop, stop all, uninstall; against a fake GitHub: install from a catalog and from a repository with an unknown key, update, rollback, a changed key, a check that could not reach GitHub, catalogs and the check-at-start setting). |

### Browser tests

```text
cargo xtask e2e
```

builds the frontend, then `data-explorer`, `signal-lab` and `eario-hub` in
debug mode, installs Playwright's Chromium if it is missing, and runs the
tests in `frontend/e2e` (`npm run e2e`). Each test starts its own example process with
`EARIO_NO_BROWSER=1` on a port chosen by the system. It reads the address
(with the session token) that the process prints, so the tests go through the
same host, origin and token checks as a user. `--prebuilt-frontend` keeps the
frontend already in `crates/eario-runtime/frontend-dist`; CI uses it.
`EARIO_E2E_BIN_DIR` points the tests at other executables.

The hub tests never touch your hub directory. `cargo xtask e2e` empties and
recreates `<target>/e2e-hub` and passes it as `EARIO_E2E_HUB_ROOT`; each hub
test makes its own directory in it, sets `EARIO_HUB_DIR` to it for the hub
and for the `--eario-install --yes` runs of the examples, and removes it
afterwards. If a run was interrupted and a hub or dashboard process still
holds the directory, the next `cargo xtask e2e` fails with a message asking
you to stop it.

The hub tests never reach GitHub either. `cargo xtask e2e` also builds
`fake-github` (`crates/eario-hub/src/bin/fake-github.rs`, feature
`fake-github`, built apart so the hub under test is built as it ships).
Each hub test starts one and points the debug hub at it with
`EARIO_HUB_GITHUB_API`. It serves the default catalog and, for the tests
that install, signed releases of `data-explorer` and `signal-lab` made from
the executables in `EARIO_E2E_BIN_DIR`, with test keys from fixed seeds. A
test changes what it serves through its control address (a newer build, a
build signed with another key, a refused release), never through the hub.
Every example's manifest says the workspace version, and so do the releases
the fake serves, so a test that updates first sets the installed version
back to a pre-release of it in the hub directory's `registry.json`, as
`crates/eario-hub/tests/hub.rs` does.

Wait on what the user sees (roles, labels, `role="status"` text), never on
a fixed delay. `cargo xtask check` does not run these tests, because they
need a browser; CI runs them in the "End-to-end (Chromium)" job, on ubuntu.
On Linux, Chromium's system libraries come from
`npx playwright install --with-deps chromium`.

Every visited state is also checked for accessibility with axe-core
(`frontend/e2e/axe.ts`, `expectAccessible`): every WCAG 2.1 A and AA rule, in
the light and the dark theme. Any violation fails the test, and no rule is
disabled. The helper sets the theme on the root element directly, so the
tests never write the user's saved preferences. Call `expectAccessible` after
each new state a test reaches.

### Fixtures

`fixtures/` holds small synthetic files generated by `cargo xtask fixtures`:
a CSV and a TSV with numeric, text and missing values, an empty CSV, a CSV
with a ragged line, an XLSX workbook with three worksheets (one with mixed
types and a blank header, one empty), and a text file with an `.xlsx`
extension. Two more, `time-series.csv` and `groups.csv`, feed the `signal-lab`
example. Regenerating produces identical files.

Never add real data.

## Minimum Rust version

Eario supports Rust 1.95 and later: `rust-version` in `[workspace.package]`,
inherited by every crate. 1.95 is the floor set by the dependencies
(`sysinfo` declares it). The CI job "Rust (minimum version)" builds the
workspace and runs its tests with exactly that toolchain; clippy and rustfmt
run on stable only.

To test it locally:

```text
rustup toolchain install 1.95 --profile minimal
cargo +1.95 test --workspace --all-features
```

The version is raised only on purpose, in a pull request that says why: a
dependency that needs a newer compiler, or a language feature worth it. A
dependency upgrade that needs a newer compiler fails the job, which forces
that decision.

## Dependency checks

```text
cargo install --locked cargo-deny   # once
cargo deny check
```

checks every dependency against `deny.toml`. CI runs it in the
"Dependencies" job.

- **Licences:** an explicit allow-list. It holds the permissive licences in the
  tree, plus MPL-2.0 (file-level copyleft, compatible with distributing an
  executable). A dependency under any other licence fails the check. Add a
  licence only in a pull request that names the crate and says why it is
  acceptable.
- **Advisories:** a RustSec vulnerability fails the check. An unmaintained
  crate fails only when a workspace crate depends on it directly. A yanked
  version is a warning. An `ignore` entry needs a reason that explains why the
  vulnerable code is never reached, and a condition for removing it.
- **Sources:** crates.io only; no git dependency or other registry.
- **Bans:** duplicate versions of a crate are warnings, not errors.

## Benchmarks

```text
cargo bench -p eario-data
```

covers CSV import, XLSX import, filtering, statistics, chart preparation and
table-page serialization on generated tables.

```text
cargo xtask bench
```

runs the same benchmarks, then writes `docs/benchmarks/YYYY-MM-DD.md`
(UTC date; a second run on the same day replaces that day's file). The file
holds each benchmark's mean and 95 % confidence interval, plus what the
numbers depend on: CPU, cores, memory, operating system, compiler, and the
commit (marked when the working tree had uncommitted changes). Results are
recorded from a developer machine, never from CI: shared runners are too
noisy to compare. Compare files only when they come from the same machine.
Close other heavy programs while it runs.

## Release build

```text
cargo xtask dist
```

produces `target/release/data-explorer` and `target/release/eario-hub` (`.exe`
on Windows) with the frontend embedded, and prints their paths. To build
your own application the same way, build the frontend once, then
`cargo build --release` your crate.

## Publishing a release

`.github/workflows/release-dashboard.yml` is a reusable workflow
(`workflow_call`) that makes a release the hub can install from and update
(ADR 0013). For each target (`x86_64-pc-windows-msvc`,
`x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin`) it
builds each executable given, names it `<slug>-<target triple>` (`.exe` on
Windows), writes `SHA256SUMS.txt` with `sha256sum`, signs it with minisign,
verifies the signature with the committed public key, and attaches the files,
with that key as `minisign.pub`, to the release of the tag. The header of the
workflow lists its inputs and shows a caller. Before building, it refuses a
tag that is not `v<SemVer version>`, a package whose version is not the
tag's, and a public key file that is missing, malformed or the placeholder.
On the runners that can run them, it checks that each dashboard's
`--eario-manifest` gives its slug and the tag's version, which needs
`.version(env!("CARGO_PKG_VERSION"))` in the dashboard. The release stays a
draft unless the caller sets `publish: true`; a hub sees only published
releases. A release may carry several dashboards, which a hub installs one at a
time by slug.

A dashboard in another repository calls the workflow the same way. How to make
the key pair (`minisign -G`), store the secrets `MINISIGN_SECRET_KEY` and
`MINISIGN_PASSWORD`, write the caller, name the tag and get listed in a catalog
is in [Publishing a dashboard](publishing-a-dashboard.md); this section covers
only Eario's own release.

Eario's own `.github/workflows/release.yml` calls it on a `v*` tag for
data-explorer, signal-lab and the hub, which take the workspace version, so
the tag must be `v<workspace version>`. It builds the frontend first
(`cargo xtask frontend`), signs with the key whose public key file is
`crates/eario-hub/eario-release.pub`, the key compiled into the hub, and
leaves a draft to review and publish. While that file is the placeholder,
the release stops before building. To release Eario: replace the
placeholder with the release key's `minisign.pub` (the hub's unit test
refuses a file that is neither), set the secrets, set the version in
`Cargo.toml`, tag `v<version>`, and publish the draft once its files are
attached. A hub installs the examples from that release one at a time, by
slug (`eario-hub install rtonneau/eario --slug data-explorer`, or a catalog
entry with a `slug`); it finds its own executable by name.

### What only the maintainer can do

Nothing below can be done from the repository's source alone, and none of it
has been done yet. In this order:

1. Make the Eario release key pair, as in
   [The signing key](publishing-a-dashboard.md#the-signing-key). Keep
   `minisign.key` and a backup outside the repository.
2. Replace `crates/eario-hub/eario-release.pub` with that key's `minisign.pub`
   and commit it. The hub's unit test accepts only the placeholder or a valid
   key.
3. In the Actions secrets of `rtonneau/eario`, set `MINISIGN_SECRET_KEY` and
   `MINISIGN_PASSWORD`.
4. Create the public repository `rtonneau/eario-catalog`, the catalog a hub
   reads by default. Put in its root a `catalog.json` copied from
   [`docs/catalog/catalog.json`](../catalog/catalog.json), with each
   `public_key` replaced by the key line (the second line) of the release key's
   `minisign.pub`.
5. Raise `version` in `[workspace.package]` of `Cargo.toml` (the tag `v0.2.0`
   already exists), commit, tag `v<version>` and push the tag. The `Release`
   workflow builds, signs and attaches the files to a draft.
6. Review the draft, then publish it. Try it with a throwaway hub directory:
   `EARIO_HUB_DIR=<empty directory> eario-hub install rtonneau/eario --slug
   data-explorer`.

The workflows have never run on GitHub, so step 5 is also their first run: read
its log before publishing.

## Publishing the crates (not done yet)

`crates/eario-runtime/frontend-dist` is ignored by git but listed in the
crate's `include`, so `cargo package` ships the built frontend and users of
the published crate do not need Node.js. The release workflow must build the
frontend before packaging. Nothing has been published to crates.io.
