# Ticket 03: computation-registry-and-evaluation

**Model:** heavy
**Effort:** xhigh

**Acceptance Criteria:**
- [ ] `Computation::new(name, f).input(name).param(name).describe(text)` with `f: Fn(&ComputeContext) -> Result<DataFrame, ComputeError> + Send + Sync + 'static`; a `ComputationRegistry` passed through `AppOptions`.
- [ ] `ComputeContext::input(name) -> Result<&DataFrame, ComputeError>` and `param::<T: DeserializeOwned>(name) -> Result<T, ComputeError>`; an unset (null) parameter is a distinct `ComputeError` that components show as `ComponentData::Empty` with a reason.
- [ ] `App::new` rejects a spec naming an unknown computation, binding an undeclared input or parameter, or missing a declared one; errors name the slot and the computation and have a stable `code()`.
- [ ] Derived datasets are computed lazily when a component reads them, recursively for derived inputs, with the session lock released during the computation.
- [ ] One cache entry per derived slot keyed by the revisions of its dependencies; a test counting invocations shows no recomputation when nothing relevant changed and one recomputation after a relevant change.
- [ ] A computation returning an error or panicking yields `AppError::Computation { name, message }` (alert in the frontend), caches nothing, and leaves the app usable.
- [ ] `import_dataset`, `load_dataset` and `select_sheet` on a derived slot are rejected.
- [ ] A derived dataset with inputs (column select, filter) bound to it is recomputed during the change call and those inputs are reconciled; `StateUpdate` lists them.
- [ ] `App::datasets()` describes derived slots (computed or not yet).

**Files to Touch:**
- `crates/eario-runtime/src/app.rs`
- `crates/eario-runtime/src/compute.rs` (new)
- `crates/eario-runtime/src/error.rs`
- `crates/eario-runtime/src/lib.rs`
- `crates/eario-runtime/src/http_tests.rs`
- `crates/eario-core/src/payload.rs` (if a derived-dataset state is added to `DatasetInfo`)
- `frontend/src/lib/generated/`

**Verification Step:**

Run:
```bash
cargo test -p eario-runtime && cargo xtask check
```

Expected:
New tests for laziness, cache hits, invalidation, chained derived datasets, error and panic handling, rejected imports and eager reconciliation pass; all checks pass.

**Notes:**

Per-dataset and per-state revisions are needed for the cache key; keep the global `revision` for `StateUpdate`. Compute outside the lock and install the result only if the dependency revisions still match. Use `catch_unwind(AssertUnwindSafe(..))`, no `unsafe`. Adapters already call `App` through `spawn_blocking`.
