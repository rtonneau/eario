# Ticket 05: typed-state-and-builder

**Status:** ✅ Done

## Local Test Result

`CARGO_BUILD_JOBS=4 cargo xtask check`: exit 0. Full output in
`.scratch/tests/2026-11-26__custom-rust-logic/evidence/05-check.txt`.

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace --all-features`: every suite `ok`. These include the new
  `end_to_end::chart_reads_a_derived_dataset_through_the_server`, the new
  `dashboard::tests::{derived_datasets_are_declared_with_their_bindings,
  inputs_of_a_derived_dataset_are_declared_even_if_no_component_reads_them,
  registered_computations_are_given_to_the_application}`, `state::tests::*`,
  `compute::tests::parameter_is_found_through_the_state_it_is_bound_to`, and the doctests of
  `crates/eario/src/lib.rs` (crate example, compile-only), `state.rs` and `compute.rs`.
- `cargo doc --workspace --no-deps --all-features`: no warning.
- `svelte-check`: 0 errors, 0 warnings. `vitest`: 4 files, 25 tests passed.

### Correction pass

`CARGO_BUILD_JOBS=4 cargo xtask check`: exit 0. Full output in
`.scratch/tests/2026-11-26__custom-rust-logic/evidence/05-check2.txt`.

- fmt and clippy (`-D warnings`): clean.
- Every one of the 15 `cargo test` result lines reads `ok`, with 0 failed.
- The new tests pass:
  - `dashboard::tests::{dataset_declared_twice_is_rejected_at_build,
    computation_registered_twice_is_rejected_at_build}`
  - `compute::tests::parameter_is_read_by_an_owned_or_borrowed_string_name`
  - the doctest `state::filtered`
  - the extended `end_to_end::chart_reads_a_derived_dataset_through_the_server`, which now also
    filters the derived input: 40 raw rows, 20 derived rows.
- `cargo doc`: clean. `svelte-check`: 0 errors, 0 warnings. `vitest`: 25 passed.

## Review Notes

Checked against each Acceptance Criterion:

- `State<T>` (`crates/eario/src/state.rs`) wraps a `StateKey` with `PhantomData<fn() -> T>`, so
  it is `Clone`, `Send` and `Sync` whatever `T` is (`Clone` and `Debug` are implemented by hand
  so they do not require `T: Clone`). It exposes `new` and `key()`. It is exported at the crate
  root and in the prelude.
- The widget constructors take typed handles: `column_select`/`numeric_column_select` take
  `&State<String>`, `filter_input` takes `&State<Filter>`, and `scatter_chart`/`line_chart` take
  `&State<String>` for both axes. `Filter` was added to the prelude so `State<Filter>` can be
  written without another import.
- `Dashboard::computation(Computation)` registers into a `ComputationRegistry` that `build()`
  passes in `AppOptions::computations`. `Dashboard::derived(&id, name, |d| ...)` takes a
  `Derived` builder (exported at the crate root, not in the prelude) with `.input(name, view)`,
  `.param(name, &State<T>)` and `.literal(name, impl Into<serde_json::Value>)`, and produces
  `DatasetSource::Computed`. `spec()` declares slots in order of first reference. A derived
  slot is pushed before its inputs, which comes next, so a cycle stops there and is left to
  `validate`. Derived datasets nothing refers to come last. Declaring a dataset again replaces
  the earlier declaration.
- `ctx.param(&State<T>)` returns `T`. Placement decision: the facade cannot add methods to
  `ComputeContext` (it lives in `eario-data`), and `State<T>` must stay in the facade (AC 1). So
  `eario-data` gained a small trait, `compute::Param<T>` (`fn param_ref(&self) -> ParamRef<'_>`,
  where `ParamRef` is `Name(&str) | State(&StateKey)`). `ComputeContext::param` now takes
  `impl Param<T>`. `&str` implements it for every `T`, so `ctx.param("x")` and
  `ctx.param::<u32>("x")` still compile unchanged. The facade implements it for `&State<T>`,
  which fixes `T`. The orphan rule allows that because `&` is fundamental.
  - Semantics: a handle finds the parameter **bound to** that state entry, not a parameter
    named like the state key. This is why `ComputeContext` gained
    `bind_state(state, param)`, and why the runtime's `DerivedSlot::context` records every
    `ParamBinding::State` (a one-line change in `App::derived_table`).
  - Reading a state that no parameter is bound to gives the new
    `ComputeError::UnboundState` (code `unbound_state`).
  - The end-to-end test binds parameter `column` to state `y` on purpose, so the names
    differ.
- `hello-dashboard` uses no state and needed no change; it compiles. `data-explorer`, the crate
  doctest, the dashboard unit tests and `end_to_end.rs` were migrated. The new end-to-end test
  serves two charts over the real socket: one of the imported `samples.xlsx`, one of a derived
  dataset scaling the chosen Y column by a literal factor 2. It checks that every derived point
  is the raw point with y doubled. It then PUTs `y = "dead_time"` and checks that the derived
  chart was recomputed with the new column.
- Files outside the ticket's list, all needed for the typed `ctx.param`:
  - `crates/eario-data/src/{compute.rs,lib.rs}`: the trait, `bind_state`, `UnboundState`, and a
    test.
  - `crates/eario-runtime/src/{compute.rs,app.rs}`: the context carries the state bindings.
  - `crates/eario/Cargo.toml`: `serde_json` moved from dev-dependencies to dependencies for
    `Derived::literal`. It is already a workspace dependency, so `Cargo.lock` is unchanged.
- No contract type changed: `StateKey` stays the wire identifier, and no bindings needed
  regenerating.

### Correction pass

This pass fixes the review findings. It supersedes the "declaring again replaces" behaviour
described above.

1. **Duplicates are errors.**
   - `Dashboard::derived` now keeps every declaration. `spec()` emits one slot per
     declaration, so `DashboardSpec::validate` reports
     `SpecError::DuplicateDataset(id)`. `build()`/`run()` then fail with
     `AppError::Spec` (code `invalid_spec`) and the message "dataset identifier `scaled` is
     declared more than once".
   - `Dashboard::computation` now collects into a `Vec`. `build()` registers each
     computation and fails with the new facade variant `Error::DuplicateComputation(name)`
     (code `duplicate_computation`), whose message names the computation and says to give
     each one its own name.
   - The facade `Error` gained `code()` to carry these stable codes. It delegates to
     `AppError::code`; the other codes are `serve_failed` and `read_file_failed`.
   - The "last wins" assertion was removed. Two rejection tests replace it.
2. **Owned and borrowed string names.** `Param<T>` is now also implemented for `&String`
   and `String`, so `ctx.param(&name)` and `ctx.param(name)` compile again. A new unit test
   covers both.
3. **Documented limit of handle reads.** The docs of `ComputeContext::param` and `State` now
   say that reading by handle works only where that exact state is bound to a parameter of
   the evaluation; otherwise it returns `UnboundState`. A computation reused across derived
   datasets with different bindings, or with literals, must read its parameters by name.
4. **Typed filtered views.**
   - A new free function `filtered(&DatasetId, &State<Filter>) -> DataView`, defined in
     `state.rs`, is exported at the crate root and in the prelude, with a doctest.
   - The `filter_input` doc points to it.
   - `data-explorer`, the dashboard unit tests and `end_to_end.rs` use it. The end-to-end
     derived dataset now reads `filtered(&data, &filter)`, and the test checks that a filter
     PUT over HTTP narrows the derived table.
   - I chose a free function over an extension trait: an extension method named
     `filtered_by` would be shadowed by the inherent `DataView::filtered_by(&StateKey)`.
- Not done, as instructed: no warning for an imported slot that nothing fills.

**Parent review record.** Base 9fbf31a34c0420b2146715c4b4889605e21cf3ac. Review 1 (opus, high, fresh): P2 duplicate derived id / computation silently replaced; P3 `ctx.param(&String)` broken; P3 handle-read binding limit undocumented; P3 untyped `filtered_by`; P3 dead imported slot warning (rejected: pre-existing component behaviour, not introduced here). One correction pass fixed the four accepted items. Re-review (opus, high, fresh) on the corrected diff: all four fixed, no regression; remaining P3: `&&str` / `&Cow<str>` names no longer coerce in `ctx.param` (workaround `*name` / `.as_ref()`), deferred as a follow-up (recorded for /gps finish). Evidence 05-check2.txt: `cargo xtask check` exit 0. Accepted.

## Blockers / Challenges

- `DataView::filtered_by` (in `eario-core`) still takes `&StateKey`. The correction pass
  added the typed `eario::filtered(&dataset, &State<Filter>)` helper on top of it, so
  authors no longer need `.filtered_by(filter.key())`.
- A heredoc-fed Python edit failed in Git Bash (quoting), so the edits went through the Edit
  tool and scratch scripts instead. No other problem.

## Commits

- b855fa8 feat: typed State handles and the computation/derived builder API (ticket 05)

## Time Spent

41m (ticket-start.js to ticket-complete.js)
