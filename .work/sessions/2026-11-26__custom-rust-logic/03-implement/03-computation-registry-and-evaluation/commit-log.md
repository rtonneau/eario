# Ticket 03: computation-registry-and-evaluation

**Status:** ✅ Done

## Local Test Result

Ran `cargo test -p eario-runtime`, then `cargo xtask check` (with `CARGO_BUILD_JOBS=4`, see Blockers). No source changed between the two runs. Cited output: `.scratch/tests/2026-11-26__custom-rust-logic/evidence/03-verification.txt`.

- `cargo test -p eario-runtime`: `test result: ok. 45 passed; 0 failed`.
- `cargo xtask check`: fmt, clippy `-D warnings`, `cargo test --workspace --all-features` (every binary `ok`, eario-runtime 45 passed, eario-core 57, eario-data 26 + import 12, eario 7 + end_to_end 4), `cargo doc`, svelte-check (`257 FILES 0 ERRORS 0 WARNINGS`), vitest (`Tests 25 passed`); `exit=0`.
- `cargo xtask gen-types` produced no change to the committed-to-be bindings (`DatasetInfo.ts`, `DerivedStatus.ts`).
- Mutation check of the new lock test: holding a session read lock across `compute::run` made `computation_runs_without_the_session_lock_and_a_stale_result_is_not_cached` fail after 5 s (`not resumed: the session stayed locked`). I reverted the mutation.

Correction pass: ran `cargo test -p eario-runtime && cargo xtask check` as one chain, with `CARGO_BUILD_JOBS=4`, on the parent's Cargo.toml profile. Output is appended to the same evidence file.
- `cargo test -p eario-runtime`: `test result: ok. 46 passed; 0 failed`.
- `cargo xtask check`: fmt, clippy `-D warnings`, workspace tests (every binary `ok`, eario-runtime 46 passed), `cargo doc`, svelte-check (`0 ERRORS 0 WARNINGS`), vitest (`Tests 25 passed`); `exit=0`.
- F1 mutation check: with the state-writer edges dropped from `dependency_order`, the new test fails with `left: (String("energy_keV"), Null)`, `right: (String("energy_keV"), String("energy_keV"))`. I reverted the mutation.

## Review Notes

**Parent recovery record (pass 1 of 1).** Base dba50dccfdf9f3b99764e15a4f19206874c5044e. First implementer (opus, xhigh) was cut off when the host process crashed (exit 0xC0000409); it left uncommitted partial edits (eario-core lib/payload, eario-data Cargo.toml/dataset/lib + new compute.rs, eario-runtime app/error/http/http_tests/lib + new compute.rs, Cargo.lock, generated DatasetInfo.ts + new DerivedStatus.ts) with no verification. Recovery: fresh implementer continues from these edits, no reset.


**Recovery implementer.** The edits left in the tree compiled and their tests passed as found. I reviewed them against each criterion and added one test (below). I kept everything else as found.

Where things live, and why:
- `Computation`, `ComputeContext`, `ComputeError` and `ComputationRegistry` are in `eario-data::compute`, re-exported from the `eario_data` root. Their signatures carry Polars `DataFrame`. AGENTS.md keeps Polars types in `eario-data` and keeps `eario-core` serde-only, so they can't go lower. Putting them in `eario-runtime` would tie authors' functions, and later Python implementations (ADR 0007), to the HTTP/Tokio crate. `eario-data` now depends directly on `serde`, which was already a workspace dependency (Cargo.lock adds an edge, not a crate), for `DeserializeOwned` in `param::<T>`.
- Evaluation is in `eario-runtime::compute` (private): `DerivedSlot` (bindings, transitive dependencies on imported datasets and state, a per-slot `evaluating` mutex), `CacheEntry`, `derived_slots` (binding checks, inputs-first order) and `run` (`catch_unwind(AssertUnwindSafe(..))`, no `unsafe`). `App` (`app.rs`) owns the per-dependency `revisions`, the cache and the decision of when to compute.

Acceptance Criteria checked:
1. Builder `Computation::new(name, f).input().param().describe()` and `AppOptions::computations`: covered by `eario-data` compute tests and the doctest.
2. `ctx.input` / `ctx.param::<T>`. A null parameter gives `ComputeError::UnsetParameter`, and the component shows `ComponentData::Empty` with a reason that names the input setting the value: `unset_parameter_asks_for_a_value_instead_of_failing`.
3. `App::new` rejects an unknown computation and undeclared or missing inputs or parameters. Each error names the dataset and the computation and has a stable `code()`: compute.rs `unknown_computation_is_rejected` and `undeclared_and_missing_bindings_are_rejected_with_their_names`.
4. Lazy, recursive evaluation with the session lock released:
   - `derived_datasets_are_computed_when_read` (laziness, chained `count` reading `selected`).
   - New `computation_runs_without_the_session_lock_and_a_stale_result_is_not_cached`: `set_state` takes the write lock while a computation is blocked, and a result whose dependencies changed during the computation is returned but not cached.
   - `concurrent_readers_share_one_computation`.
5. One cache entry per slot keyed by dependency revisions, with counted invocations: `cached_tables_are_reused_until_a_dependency_changes`.
6. An error or a panic gives `AppError::Computation { name, message }` (HTTP 422, which the frontend shows through the existing `role="alert"` component error), caches nothing, and leaves the app usable: `failing_computation_…`, `panicking_computation_…`, `failure_during_a_change_…`.
7. Import, load and `select_sheet` on a derived slot are rejected with `derived_dataset` (409): `nothing_can_be_imported_or_loaded_into_a_derived_dataset`.
8. Eager recomputation and reconciliation of bound inputs, with `StateUpdate` listing them: `bound_inputs_are_reconciled_when_their_derived_dataset_changes`, `cached_tables_…` (invalidated lists `s` and `sfilter`), and `derived_dataset_of_literals_reconciles_its_inputs_at_start_up`.
9. `App::datasets()` describes derived slots as `computed` or `not_computed` (`DatasetInfo.derived: Option<DerivedStatus>`, bindings regenerated).

Not done here, by plan: MCP cache-only reads (ticket 04; MCP currently reaches `App::dataset_frame`, which computes), facade `Dashboard::computation`/`derived` (05), docs (`project-status`, `roadmap`, ticket 08). ~~The global `revision` is incremented once per change call. State entries that reconciliation changes are part of that same revision.~~ Superseded by the correction pass (F2): a reconciliation that changes state is published under a revision of its own.

**Parent review record.** Independent reviewer (opus, high, fresh context, base dba50dc, evidence-only): 2 findings. F1 (P2) compute.rs:198-224 / app.rs:376-383: eager reconciliation orders derived slots only by dataset inputs; a derived dataset reading state written by an input bound to another derived dataset is reconciled against a stale table (AC "those inputs are reconciled" not met, untested). Fix: add state-writer edges to dependency_order or iterate refresh_derived to a fixpoint; add HTTP test. F2 (P3) app.rs:279-293/477-483: reconciled state written after the revision is published, without a new revision; bump revision or document it. Disposition: both accepted as valid. Recovery allowance already used by the post-crash pass, so ticket blocked pending user go-ahead for a correction pass.

**User decision (2026-11-26):** one extra correction pass approved for F1 and F2. Parent added `[profile.dev.package."*"] debug = "line-tables-only"` to Cargo.toml (user-approved fix for disk/memory exhaustion).

**Correction pass.**
- F1 (fixed by adding state-writer edges, not a fixpoint). `compute::dependency_order` now places a derived dataset after its derived inputs and also after every other derived dataset whose bound inputs write a state entry in its transitive dependencies. A change therefore refreshes `cols` before `hist`, and each slot is recomputed at most once per change. A fixpoint would first recompute `hist` from the stale `col`, which can fail and log a spurious warning. Cycles (including a slot whose own bound inputs write a state it reads) are broken by the DFS, walking in declaration order, where it meets a dataset it is already placing. The rule of which components count as bound inputs now lives in one place, `compute::bound_states`, which `App::has_bound_inputs` also uses. New HTTP test `inputs_are_reconciled_through_a_state_another_derived_dataset_reconciles` covers the reported scenario, with `hist` declared before `cols`: after the first import, `col` and `bin` are both `energy_keV`; after importing a file with columns `a,b`, both are `a`, `bin` offers `["a"]`, and the chart on `hist` answers 200 with `type: chart`. With the state edges removed, the test fails (`bin` stays `null`).
- F2 (fixed). `reconcile_derived` increments `revision` under the same write lock when reconciliation changes any state entry. `StateUpdate` is still read after the refresh, so it reports the final revision and state. `App::new` resets `revision` to 0 after the start-up reconciliation, because that is the initial state, not a change (`AppInfo.revision` is "number of state changes since start-up"). `bound_inputs_are_reconciled_when_their_derived_dataset_changes` now expects revision 2 after the first import (the import, then the reconciliation of `s`) and checks that `/api/app` reports the same revision and state as the update. The start-up test still expects revision 0.

**Parent acceptance.** Fresh re-reviewer (opus, high) on the corrected diff: 0 findings; F1 and F2 fixed; gap: could not confirm the passing check ran on the final compute.rs. Parent ran `cargo clean` (55 GB target/) then `CARGO_BUILD_JOBS=4 cargo xtask check` on the current tree: exit 0, eario-runtime 46 passed incl. the F1 test, vitest 25 passed (`.scratch/tests/2026-11-26__custom-rust-logic/03-parent-check.txt`). Rebuilt target/ with line-tables-only dependency debug info ≈ 7 GB. All AC met; accepted.

## Blockers / Challenges

- The previous implementer was killed mid-ticket (host exit 0xC0000409). Its edits were complete enough to compile and pass tests, so I continued from them without a reset.
- The first `cargo xtask check` failed while building dependencies: rustc ran out of memory compiling `polars-ops` (`memory allocation of 2097152 bytes failed`, exit 0xC0000409, the same code as the host crash). The likely cause is many parallel Polars rustc jobs. I re-ran with `CARGO_BUILD_JOBS=4`.
- That re-run failed at link time (`LNK1140` "limit exceeded for program database", `LNK1318` PDB FILE_SYSTEM). The disk was 97% full (17 GB free), and `target/debug/deps` held 22.9 GB of PDBs, about 1 GB per test binary since Polars `lazy`. I deleted the 87 stale PDBs from builds before 13:30 (18.9 GB, build artifacts only) and the partial PDBs of the failed links. The next run passed. Expect this again on this machine as test binaries accumulate. A `[profile.dev.package."*"] debug = "line-tables-only"` setting would be the lasting fix, but it is not in scope here and would need its own decision.
- Correction pass: none blocking. After the parent's profile change and the rebuild, `C:` is again at 97% (18 GB free). `target/` probably still holds the dependency artifacts built with full debug info before the change. A `cargo clean` would reclaim the space, at the cost of a full rebuild. I did not run it.

**Blocked (2026-11-26T13:18:15.639Z):** Review F1 (P2): eager reconciliation misses derived-to-derived state edges; F2 (P3): reconciled state not under a new revision. Recovery allowance used by the post-crash pass.

## Commits

- 830850f feat: computation registry, lazy cached derived datasets and eager reconciliation (ticket 03)

## Time Spent

2h 53m (ticket-start.js to ticket-complete.js)
