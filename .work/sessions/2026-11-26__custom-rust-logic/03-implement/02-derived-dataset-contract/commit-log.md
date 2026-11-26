# Ticket 02: derived-dataset-contract

**Status:** ✅ Done

## Local Test Result

Ran `cargo xtask gen-types && cargo xtask check` (from `C:\DEV\RUST\eario`). `gen-types` regenerated the bindings (29 export tests ok): `DatasetSlot.ts` changed, `DatasetSource.ts` and `ParamBinding.ts` are new. `cargo xtask check` (fmt, clippy `-D warnings`, Rust tests, docs, svelte-check, vitest) passes: exit 0 on the final re-run; svelte-check 0 errors / 0 warnings; vitest 25 passed; eario-core 56 unit tests passed, including the new ones. A second `gen-types` produced no further diff. Log: `.scratch/tests/2026-11-26__custom-rust-logic/evidence/ticket02-check.log`. The first `check` run failed in svelte-check on `frontend/src/App.test.ts` (missing `source`); fixed and re-run.

## Review Notes

Review rounds: 0 (self-check only).

- AC1: `DatasetSlot.source: DatasetSource` (`Imported` | `Computed { function, inputs: BTreeMap<String, DataView>, params: BTreeMap<String, ParamBinding> }`), `ParamBinding` = `State(StateKey)` | `Literal(serde_json::Value)` (adjacent tagging `type`/`value`, since an internally tagged newtype cannot hold a non-object `Value`). Wire format asserted in a test.
- AC2: `SPEC_VERSION` = 2; `#[serde(default)]` on `source` (default `Imported`); test deserializes a version-1 JSON without `source` and validates it.
- AC3: new `SpecError` variants `UnknownComputedInput`, `UnknownComputedState` (also covers an input filter reading an unwritten state), `DerivedDatasetCycle` (message shows the path `a -> b -> a`), `ImportIntoDerivedDataset`; one unit test each, plus tests for self-cycle, diamond (not a cycle) and filter state.
- AC4: `DashboardSpec::dataset_dependencies(&DatasetId) -> BTreeSet<Dependency>` (transitive, cycle-safe, includes input datasets, their filter states and state-bound params), `DatasetSlot::dependencies()` for the direct level, and `DashboardSpec::dataset()` lookup. Tested for imported, one level, chains, cycle termination.
- AC5: bindings regenerated and frontend type check passes.
- Outside the listed files but required to compile/type-check: `crates/eario-runtime/src/http_tests.rs` (slot literal) and `frontend/src/App.test.ts` (fixture needs `source`, `spec_version` 2). `docs/architecture/overview.md` still shows `DatasetSlot { id, label }`; left for the documentation ticket.
- Registry-dependent checks (unknown function, undeclared input/param names) intentionally not done here (ticket 3).

**Parent review record.** Base 6d0c6664591bd889a9b552827defc6e97e00447f. Delegated implementer (opus, high). Independent reviewer (opus, high, fresh context) on the frozen diff: 0 findings, no verification gaps; noted the overview doc update belongs to ticket 08. Accepted: all AC met, check log shows exit 0.

## Blockers / Challenges

One failure on the first `cargo xtask check`: the frontend test fixture lacked the now-required `source` field; fixed in `frontend/src/App.test.ts`. No other blockers.

## Commits

- e14310a feat: derived dataset contract with computed sources and spec version 2 (ticket 02)

## Time Spent

7m (ticket-start.js to ticket-complete.js)
