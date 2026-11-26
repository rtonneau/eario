# Implementation Plan

**Session:** custom-rust-logic
**Date:** 2026-11-26T10:44:05.792Z
**Estimated effort:** 3-4 days

## Strategy

Build bottom-up along the crate dependency direction, so each ticket compiles and passes `cargo xtask check` on committed work: Polars re-export first (authors' toolbox), then the serializable contract for derived datasets (`eario-core`), then the runtime registry and evaluation with its cache (`eario-runtime`), then the MCP cache-only rule, then the typed builder API (`State<T>`, `Dashboard::computation`/`derived`) in the facade, then the five new input kinds written directly against typed handles, then the new two-page example with its synthetic fixtures, and last the documentation pass. No blocker: ADR 0012 is accepted.

## Tickets Overview

- **Ticket 1:** Enable Polars `lazy` and re-export it as `eario::polars`.
- **Ticket 2:** Contract for derived datasets: `DatasetSource`, `ParamBinding`, `SPEC_VERSION` 2, structural validation, regenerated bindings.
- **Ticket 3:** Computation registry, `ComputeContext`, lazy evaluation with a revision-keyed cache, failure handling and eager recomputation for bound inputs in `App`.
- **Ticket 4:** MCP lists derived datasets and reads them from the cache only.
- **Ticket 5:** Typed state handles `State<T>` and the `Dashboard::computation` / `Dashboard::derived` builder API; existing examples migrated.
- **Ticket 6:** Number, slider, text, select and checkbox inputs, end to end (core, runtime validation, facade constructors, Svelte renderers).
- **Ticket 7:** Two-page example (signal analysis, group summary) with synthetic fixtures.
- **Ticket 8:** Documentation: ADR 0001, project status, roadmap, guide, README, architecture overview.

## Sequencing Rationale

Dependencies point `eario` → `eario-mcp` → `eario-runtime` → `eario-data` → `eario-core`, so contract changes land before the runtime that uses them, and the runtime before the facade. Typed handles (5) come before the new inputs (6) so the input constructors are written once, typed. The example (7) needs every capability; docs (8) describe what was actually built.

## Risks & Mitigation

- **Risk:** Polars `lazy` lengthens build time or pulls unwanted features → **Mitigation:** enable only `lazy` on top of existing features with `default-features = false`; record the build-time impact in the PR.
- **Risk:** Eager recomputation and the cache create lock contention or deadlocks on `Session` → **Mitigation:** never hold the session lock while a computation runs; snapshot inputs and revisions under the lock, compute outside, install result under the write lock only if revisions still match.
- **Risk:** `catch_unwind` misses panics or leaves poisoned state → **Mitigation:** computations get only owned/cloned frames and no lock; test with a panicking computation.
- **Risk:** Breaking builder API churn across examples and docs → **Mitigation:** migrate all examples in ticket 5 and keep the doctest in `crates/eario/src/lib.rs` compiling.
- **Risk:** Column-driven select on a large table ships too many options → **Mitigation:** cap distinct values at 1,000 and report truncation in the component data.

## Assumptions

- Nothing is published on crates.io, so breaking builder changes are acceptable.
- `cargo xtask check` and the frontend toolchain (`cargo xtask setup`) work on this machine.
- Polars 0.55 `lazy` feature compiles with the current toolchain (stable 1.99).
- The new example crate is `examples/signal-lab`; the select distinct-value cap is 1,000.
