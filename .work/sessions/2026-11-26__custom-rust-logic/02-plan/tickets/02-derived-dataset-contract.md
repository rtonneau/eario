# Ticket 02: derived-dataset-contract

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `DatasetSlot` has `source: DatasetSource` with `Imported` and `Computed { function, inputs: BTreeMap<String, DataView>, params: BTreeMap<String, ParamBinding> }`; `ParamBinding` is `State(StateKey)` or `Literal(serde_json::Value)`.
- [ ] `SPEC_VERSION` is 2; a version-1 spec without `source` still deserializes as `Imported` (serde default).
- [ ] `validate()` rejects: a computed slot whose input names an undeclared slot; a parameter bound to a state nobody writes; a cycle between derived datasets; a `FileImport` targeting a derived slot. Each is a new `SpecError` variant with an actionable message.
- [ ] A helper resolves the transitive dependencies of a derived dataset (its input datasets, their filters and its state-bound parameters) so invalidation follows derived datasets.
- [ ] Bindings regenerated with `cargo xtask gen-types`; frontend type check passes.

**Files to Touch:**
- `crates/eario-core/src/spec.rs`
- `crates/eario-core/src/lib.rs`
- `frontend/src/lib/generated/`
- `crates/eario/src/dashboard.rs` (default `source: Imported` for declared slots)

**Verification Step:**

Run:
```bash
cargo xtask gen-types && cargo xtask check
```

Expected:
Bindings regenerate without uncommitted drift after commit; all checks pass, including new unit tests for each validation error and for the version-1 compatibility case.

**Notes:**

`eario-core` stays serde-only: `serde_json::Value` is already used for state. Registry-dependent checks (unknown function, undeclared input/param names) belong to ticket 3, since `eario-core` does not know the registry.
