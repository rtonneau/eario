# Development guide

## Prerequisites

- A stable Rust toolchain installed with [rustup](https://rustup.rs). The
  repository's `rust-toolchain.toml` selects `stable` with rustfmt and clippy.
- [Node.js](https://nodejs.org) with npm, to build the frontend. The project
  is developed with Node.js 25.

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
| `cargo xtask check` | Runs everything CI runs. |
| `cargo xtask dist` | Builds the frontend, then `data-explorer` in release mode. |

Environment variables read by an Eario application:

| Variable | Effect |
| --- | --- |
| `EARIO_PORT` | Listen on this port instead of one chosen by the system. |
| `EARIO_NO_BROWSER` | When set to anything but `0`, do not open a browser. |
| `EARIO_LOG` | Log filter, for example `debug` or `eario_runtime=debug`. Default `warn`. |
| `EARIO_DEV_ORIGIN` | Extra allowed origin. Debug builds only. |

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
| Frontend unit tests | `frontend/src/lib/*.test.ts` | API client, chart option, formatting. |
| Frontend rendering test | `frontend/src/App.test.ts` | A whole dashboard rendered from a specification, with the backend mocked. |

There is no automated test in a real browser yet. During the bootstrap the
dashboard was checked by hand with headless Chrome screenshots. Adding
Playwright is a roadmap item.

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
table-page serialization on generated tables. No result is recorded in the
repository: numbers depend on the machine.

## Release build

```text
cargo xtask dist
```

produces `target/release/data-explorer` with the frontend embedded. To build
your own application the same way, build the frontend once, then
`cargo build --release` your crate.

## Publishing the crates (not done yet)

`crates/eario-runtime/frontend-dist` is ignored by git but listed in the
crate's `include`, so `cargo package` ships the built frontend and users of
the published crate do not need Node.js. The release workflow must build the
frontend before packaging. Nothing has been published to crates.io.
