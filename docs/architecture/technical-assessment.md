# Technical assessment of building blocks

Written during the bootstrap (November 2026). Versions are those resolved in
`Cargo.lock` and `frontend/package-lock.json` at that time. "Verified" means
the library was compiled into Eario and exercised by its tests; other
statements come from the package metadata on crates.io and from reading the
published source, and say so.

## Selected

| Library | Version | Licence | Role | Notes |
| --- | --- | --- | --- | --- |
| polars | 0.55 | MIT | Tabular engine | Verified. Only the eager API and the CSV reader are enabled (`csv`, `fmt`). See below. |
| calamine | 0.38 | MIT | XLSX reading | Verified. Pure Rust, reads from memory, exposes typed cells. |
| axum | 0.8 | MIT | HTTP server | Verified. Minimal feature set (`http1`, `json`, `query`, `tokio`). |
| tokio | 1 | MIT | Async runtime | Verified. |
| rust-embed | 8 | MIT | Frontend embedding | Verified. Embeds in release, reads from disk in debug. |
| rmcp | 3.11 | Apache-2.0 | MCP server | Verified. The official Rust SDK. Its Streamable HTTP service mounts on an Axum router. |
| ts-rs | 12 | MIT | TypeScript bindings | Verified. Optional feature, used only by `cargo xtask gen-types`. |
| serde, serde_json | 1 | MIT OR Apache-2.0 | Serialization | Verified. |
| thiserror | 2 | MIT OR Apache-2.0 | Error types | Verified. |
| tracing, tracing-subscriber | 0.1, 0.3 | MIT | Logging | Verified. |
| webbrowser | 1 | MIT OR Apache-2.0 | Opening the browser | Used by `server::run`; not exercised by automated tests. |
| directories | 6 | MIT OR Apache-2.0 | Per-user config location | Verified on Windows. Pulls `option-ext` (MPL-2.0, see Licensing). |
| getrandom | 0.4 | MIT OR Apache-2.0 | Session token | Verified. |
| mime_guess | 2 | MIT | Content types of assets | Verified. |
| Svelte | 5 | MIT | Frontend framework | Verified, with runes. |
| Vite | 8 | MIT | Frontend build | Verified. |
| Apache ECharts | 6 | Apache-2.0 | Charts | Verified in Chrome. Imported per module to limit the bundle. |
| criterion | 0.8 | MIT OR Apache-2.0 | Benchmarks | Development only. Benchmarks compile; no result is recorded. |
| rust_xlsxwriter | 0.104 | MIT OR Apache-2.0 | Generating XLSX fixtures | Development only. |

### Polars

Suitability: the natural choice for tables in Rust. Arrow memory layout,
columnar operations, parallel execution, cheap clones.

Costs observed:

- It is by far the largest part of the build. A clean debug build of the
  workspace takes minutes, most of it Polars.
- The API changes between minor versions. Three calls written from memory of
  older versions did not compile against 0.55 and had to be corrected from
  the source. Expect maintenance at each upgrade; keeping Polars confined to
  `eario-data` limits it.
- Enabling `lazy` together with `strings` and without temporal features
  failed to compile in 0.55.2 (a missing `polars-time` import in
  `polars-expr`). The lazy engine was not needed, so it is disabled.

Consequence: grouping, joins and lazy query plans will need more Polars
features, a longer build and a check that the feature combination compiles.

### rmcp

Version 3.11 targets the 2026-07-28 revision of the MCP specification, which
removes sessions from the Streamable HTTP transport. Eario configures the
service as stateless with JSON responses, which also avoids long-lived
streams that would delay shutdown. The SDK is released often; pin and upgrade
deliberately.

## Evaluated and not adopted now

| Library | Version | Licence | Assessment |
| --- | --- | --- | --- |
| charming | 0.6.0 | MIT OR Apache-2.0 | A typed Rust builder for ECharts options with HTML, server-side (Deno) and WASM renderers. Its last release on crates.io dates from June 2025 while ECharts has since moved to 6. Adopting it would make its types the chart specification of Eario and tie the specification to one renderer and one upstream. Eario sends renderer-independent `ChartData` instead and builds the ECharts option in one frontend module. Charming remains a candidate for server-side figure export. See ADR 0004. |
| rustview | 0.1.8 | MIT OR Apache-2.0 | A Streamlit-like framework: the application is a function re-run on each interaction. Published in March 2026, a few hundred downloads, API declared unstable by its authors. Its immediate-mode model has no serializable dashboard specification, which Eario requires. Not reusable as a component; useful as a reference for API ergonomics. |
| csv | 1.4 | Unlicense OR MIT | Not needed: Polars reads delimited text with type inference. Would be the base for a custom text format reader. |
| rayon | 1.12 | MIT OR Apache-2.0 | Not a direct dependency. Polars uses it internally; Eario's own code has no parallel loop yet. |
| tower | 0.5 | MIT | Development dependency only (driving the router in tests). Axum middleware covers current needs. |
| rusqlite | 0.43 | MIT | Deferred with session persistence (ADR 0006). |
| clap | 4.6 | MIT OR Apache-2.0 | Deferred. The examples take one optional argument and `xtask` a command name. |

## Data mining libraries

No analysis beyond descriptive statistics is implemented, so none of these is
a dependency. Facts below are from crates.io metadata; none was compiled or
tested for this assessment, and numerical correctness was not evaluated.

| Library | Version | Licence | What the metadata says |
| --- | --- | --- | --- |
| linfa | 0.9.0 | MIT OR Apache-2.0 | Family of crates (clustering, linear models, PCA, ...) built on ndarray. |
| smartcore | 0.7.0 | Apache-2.0 | Single crate covering classification, regression and clustering. Apache-2.0 only, compatible with Eario's dual licence for a dependency. |
| statrs | 0.19.1 | MIT | Distributions and statistical functions. |
| ndarray, nalgebra | | | Array and linear algebra foundations the above rely on. |

Selection is to be done per algorithm when `eario-analysis` is created, by
testing against reference results (for example scikit-learn or R output on
the same data), not by popularity. This is recorded as an open task in the
roadmap.

## Licensing

Eario is `MIT OR Apache-2.0`. The dependency tree (`cargo tree`) consists of
MIT, Apache-2.0, dual MIT/Apache-2.0, Unicode-3.0, Zlib, BSD and Unlicense
licences, plus one MPL-2.0 crate: `option-ext`, pulled by `directories`.
MPL-2.0 is a file-level copyleft: using the crate unmodified as a dependency
does not affect Eario's licence, but distributors must keep its source
available. If that is unwanted, the per-user configuration path can be
computed without `directories`.

Frontend dependencies bundled into the executable: Svelte (MIT) and Apache
ECharts with its dependency zrender (Apache-2.0 and BSD-3-Clause).

A licence check in CI (for example `cargo deny`) is a roadmap item; the review
above was done by hand.

## Platforms

Developed and tested on Windows 11 with Rust 1.99 and Node.js 25. CI is
configured to run on Windows and Linux; its first run will be the first Linux
verification. macOS has not been tried.
