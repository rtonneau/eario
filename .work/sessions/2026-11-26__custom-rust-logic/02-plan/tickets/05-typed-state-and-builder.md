# Ticket 05: typed-state-and-builder

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `State<T>` in the facade wraps a `StateKey` with `PhantomData<T>`, is `Clone`, and exposes `key()`; exported in the prelude.
- [ ] Existing widget constructors take typed handles: `column_select`/`numeric_column_select` take `&State<String>`, `filter_input` takes `&State<Filter>`, charts take `&State<String>` for axes.
- [ ] `Dashboard::computation(Computation)` and `Dashboard::derived(&DatasetId, name, |d| d.input(name, view).param(name, &State<T>).literal(name, value))` produce the spec of ticket 2 and the registry of ticket 3.
- [ ] `ComputeContext::param` accepts a `&State<T>` and returns `T` typed.
- [ ] `hello-dashboard`, `data-explorer` and the crate-level doctest compile with the new API; `end_to_end.rs` gains a test serving a dashboard with a derived dataset.

**Files to Touch:**
- `crates/eario/src/state.rs` (new)
- `crates/eario/src/lib.rs`
- `crates/eario/src/widgets.rs`
- `crates/eario/src/dashboard.rs`
- `crates/eario/tests/end_to_end.rs`
- `examples/hello-dashboard/src/main.rs`
- `examples/data-explorer/src/main.rs`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
All checks pass, including the new end-to-end test where a chart reads a derived dataset through the real server.

**Notes:**

`StateKey` stays the wire-level identifier in `eario-core`; no contract change in this ticket. Re-export `Computation`, `ComputeContext`, `ComputeError` in the prelude.
