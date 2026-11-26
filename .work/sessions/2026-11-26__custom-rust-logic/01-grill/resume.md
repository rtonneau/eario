# Session: custom-rust-logic

**Date:** 2026-11-26T10:28:36.281Z
**Status:** Grill phase complete

## Problem Statement

Roadmap M1: a dashboard can only combine built-in components and cannot run custom Rust code (ADR 0001, project-status "most important gap"). Developers need to register their own Rust computations, show their output through the existing components, drive them with more input kinds, and read state through typed handles.

## Context & Constraints

- **Current behavior:** `ComponentKind` is a closed set; every component reads an imported `DatasetSlot` through a `DataView` (dataset + optional filter). Inputs are `ColumnSelect` and `FilterInput` only; state is a JSON map keyed by `StateKey`, starting as `null`. Every query recomputes from the dataset (no cache). Polars is built with `csv` and `fmt` features only and is not re-exported for authors.
- **Pain point:** no extension point for custom logic; no numeric/text/choice inputs to parameterize anything; untyped state in the builder API.
- **Dependencies:** keep `DashboardSpec` serializable (no closures in it, ADR 0001); keep `App` the single source of behaviour with thin HTTP/MCP adapters; keep crate boundaries (`eario-core` serde-only); MCP stays read-only and bounded (ADR 0009); CPU work through `spawn_blocking`; no `unwrap`/`unsafe`; regenerate TS bindings after contract changes.
- **Tech stack:** Rust workspace (eario, eario-core, eario-data with Polars 0.55, eario-runtime with Axum/Tokio, eario-mcp), Svelte 5 + TypeScript frontend, ts-rs bindings, ECharts.

## Success Metrics

- A developer registers `Computation::new("name", f)` with declared inputs/params, declares a derived dataset bound to views, state handles and literals, and every existing component (table, chart, statistics, dataset info, column select, filter) reads it like an imported dataset.
- `App::new` rejects specs with an unknown computation, undeclared or missing input/param bindings, import/load into a derived slot, or cycles between derived datasets; each with an actionable error and a stable `code()`.
- Derived datasets are computed lazily, cached per slot by the revisions of their dependencies (verified by a test counting calls), recomputed eagerly when inputs are bound to them, and a failing or panicking computation yields an alert naming it without stopping the app.
- Number, slider, text, select (static or from a column's distinct values, capped) and checkbox inputs work end to end with defaults and server-side validation (range, integrality, options).
- `State<T>` handles replace `&StateKey` in the builder API; `ctx.param::<T>` reads them typed; unset parameters give an `Empty` reason.
- A new two-page example (signal analysis + group summary) runs on synthetic fixtures; `cargo xtask check` passes.

## Architecture & Approach

- **Core contract (`eario-core`):** `DatasetSlot` gains `source: DatasetSource` = `Imported | Computed { function, inputs: BTreeMap<name, DataView>, params: BTreeMap<name, ParamBinding> }` with `ParamBinding = State(StateKey) | Literal(serde_json::Value)`. `SPEC_VERSION` becomes 2. New `ComponentKind` input variants: `NumberInput`, `Slider`, `TextInput`, `Select` (options `Static(Vec<String>)` or `Column { dataset, column }`), `Checkbox`, each with label, state and default (numeric ones with min/max/step and integer flag). `Dependency` resolution follows derived datasets to their inputs. `validate()` checks structural rules (cycles, derived-slot targets of imports, bindings resolve to declared slots/states).
- **Registry (`eario-runtime` / `eario-data`):** `Computation { name, inputs, params, description, func: Arc<dyn Fn(&ComputeContext) -> Result<DataFrame, ComputeError> + Send + Sync> }`; `ComputeContext` exposes `input(name) -> Result<&DataFrame>` and `param::<T: DeserializeOwned>(name)` (also accepting `&State<T>`). `AppOptions` carries the registry; `App::new` validates the spec against it.
- **Evaluation:** per-dataset and per-state revisions in `Session`; `resolve_view` resolves a derived dataset by computing its inputs recursively, then the function on a blocking thread already provided by adapters; cache one entry per derived slot keyed by the dependency revisions. Panics caught with `catch_unwind(AssertUnwindSafe(..))`; errors mapped to `AppError::Computation { name, message }` and shown as alerts. Derived datasets with bound inputs are recomputed inside the change call and reconciled like imports.
- **Typed state (`eario` facade):** `State<T>` wrapping `StateKey` with `PhantomData<T>`; widget constructors take typed handles (`column_select` → `State<String>`, `filter_input` → `State<Filter>`, `checkbox` → `State<bool>`, numeric inputs generic over a sealed `NumberType` for i64/f64/u32). `Dashboard::computation(..)` and `Dashboard::derived(&id, name, |d| d.input(..).param(..))`.
- **Polars:** `eario::polars` re-export, workspace Polars gains the `lazy` feature.
- **MCP:** lists derived slots; reads only a cached table; never triggers a computation.
- **Frontend:** Svelte renderers for the five inputs, `registry.ts` entries, alert for computation errors; regenerated bindings.
- **Example:** `examples/signal-lab` (name decided at plan time) with Signal analysis (moving average via slider, histogram bins via number input, derived-of-derived) and Group summary (select category, per-group aggregates, outlier checkbox), on new synthetic fixtures.

## Assumptions & Trade-offs

- Not doing: progress, cancellation, background tasks or push updates (ADR 0002 stays deferred); custom component kinds; Python computations (ADR 0007); persistence of derived results.
- A "custom statistic" is a computation returning a small table, no separate concept.
- Breaking builder API change (`State<T>` instead of `&StateKey`) is acceptable: nothing is published.
- Cache holds the latest result per derived slot only; memory grows with the number of derived slots, not with history.
- Enabling Polars `lazy` lengthens the first build; accepted for authoring power.
- Eager recomputation of input-bound derived datasets moves CPU work into the state-change call; accepted for state consistency.

## Open Questions

- None blocking. Exact name of the new example crate and distinct-value cap for column-driven selects are chosen during planning.

## Notes

- Glossary terms recorded: Computation, Derived dataset, Compute context, Parameter.
- ADR 0012 records the decision; ADR 0001's "Custom computations" open question is closed by it and must be updated, along with project-status, roadmap and the development guide.
