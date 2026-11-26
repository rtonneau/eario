# Ticket 06: new-input-kinds

**Status:** ✅ Done

## Local Test Result

Command (with `CARGO_BUILD_JOBS=4`): `cargo xtask gen-types && cargo xtask check`, exit code 0.
Full output: `.scratch/tests/2026-11-26__custom-rust-logic/evidence/06-verify.txt`.

- gen-types: `export_bindings` 31 passed; regenerated `ComponentKind.ts`, `ComponentData.ts`, new `SelectOptions.ts` (not edited by hand).
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --workspace --all-features`: every suite `ok` (eario-core 63, eario-runtime 51, eario-data 30, eario facade 18, doc-tests including the new `number_input`, `slider`, `select` examples).
- `cargo doc --workspace --no-deps --all-features`: no warning.
- `npm run check` (svelte-check `--fail-on-warnings`): 269 files, 0 errors, 0 warnings.
- `npm run test` (Vitest): 9 files, 43 tests passed.

Mutation checks (run, then reverted):
- Removing the `Select` arm from `compute::bound_states` makes `select_bound_to_a_derived_dataset_is_reconciled_when_it_is_recomputed` fail (`06-mutation.txt`).
- Removing the revert in `Checkbox.svelte` makes "announces a refusal and shows the value the application holds" fail.

### Correction pass

Same command, `cargo xtask gen-types && cargo xtask check`, exit code 0. Full output: `.scratch/tests/2026-11-26__custom-rust-logic/evidence/06-fix-verify.txt`. fmt, clippy (`-D warnings`) and doc are clean; every Rust suite is `ok` (eario-runtime now 52 tests, eario-core 63); svelte-check reports 269 files, 0 errors, 0 warnings; Vitest 9 files, 44 tests passed. gen-types produced no further change to `generated/` (no contract change in this pass).

### Correction pass 2

Same command, `cargo xtask gen-types && cargo xtask check`, exit code 0. Full output: `.scratch/tests/2026-11-26__custom-rust-logic/evidence/06-fix2-verify.txt`. fmt, clippy (`-D warnings`) and doc are clean; every Rust suite is `ok` (eario-runtime 53 tests, eario-core 63); svelte-check reports 269 files, 0 errors, 0 warnings; Vitest 9 files, 44 tests passed. No binding change.

## Review Notes

Checked against the Acceptance Criteria:

- Contract (`eario-core/src/spec.rs`): `ComponentKind::{NumberInput, Slider, TextInput, Select, Checkbox}` with label, state, bounds/step/integer/default, `max_length`, `SelectOptions::{Static(Vec<String>), Column { dataset, column }}` (adjacently tagged like `ParamBinding`). `dependencies()` (a column select depends on its dataset; the others on nothing) and `written_state()` cover all five; `written_state` is now exhaustive. New `ComponentKind::initial_state()` and `number_constraints()`, and `NumberConstraints::check` shared by spec validation (of the default) and `App::set_state`. `validate()` rejects inconsistent inputs with `SpecError::InvalidInput { component, reason }` (min above max, non-positive step, non-whole bound/step/default for integer inputs, default out of range/off-step, text default longer than `max_length`, zero `max_length`, empty or duplicated static options, static default not among options). Spec tests cover each.
- Initial state is the default (`App::new` uses `initial_state`); integer defaults are stored as JSON integers. `App::set_state` validates range, step, integrality (5.0 normalized to 5, 2.5 rejected, values beyond ±2^53 given as floats rejected), text length (characters), option membership, booleans; messages name the state and the bound (`invalid value for state `bins`: 0 is below the minimum 1`).
- Column-driven select: `eario_data::query::distinct_values` returns sorted text values without nulls, keeping at most the cap (corrected in correction pass 2: the text conversion itself covers every distinct value of a non-text column); cap `MAX_SELECT_OPTIONS = 1_000` in `eario_runtime::app`; truncation in `ComponentData::Select { options, truncated }`. Number/slider/text/checkbox answer `ComponentData::Input`. The select is part of `compute::bound_states`, so it is reconciled on import and, when bound to a derived dataset, on recomputation, exactly like `ColumnSelect` (HTTP test with a derived `head` of `bins` rows). Fallback: current value if still offered, else default if offered, else first value, else `null`.
- Facade: `number_input(label, &State<T>, impl RangeBounds<T>, default)`, `slider(label, &State<T>, RangeInclusive<T>, default)`, `text_input`, `select`, `select_from_column(label, &dataset, column, &State<String>)`, `checkbox(label, &State<bool>, default)`; `Widget::step` and `Widget::max_length`. Sealed `NumberType` for `i64`, `u32`, `f64` (exported as `eario::NumberType`); `u32` gives an integer input with min 0 and max `u32::MAX` on any unbounded side. Default step: 1 for whole numbers, a hundredth of the range for decimal sliders, none for decimal number inputs. Default `max_length` 1,000. New constructors in the prelude.
- Frontend: `NumberInput`, `Slider`, `TextInput`, `Select`, `Checkbox` Svelte components, each with a `<label for>`; refusals in `role="alert"`; the slider value in an `<output>` (implicit `role="status"`); the truncation notice in `role="status"`. Colours only through `app.css` tokens (`accent-color: var(--color-accent)`, `.muted`, `.error`). They call `session.setState`/`componentData` only; `api.ts` stays the only `fetch` caller (tests stub `fetch`). Registered in `registry.ts`. Vitest tests for each (plus `input-test-support.ts`).
- HTTP tests: a valid and an invalid value for each input kind (`each_input_accepts_a_valid_value_and_rejects_an_invalid_one`), initial defaults, column options and fallback, the 1,000 cap and truncation, and reconciliation through a derived dataset.

Trade-off to note for review (superseded by the correction pass below): like the column list of a column selection, the distinct values of a column-driven select were listed while the session write lock was held.

### Correction pass

- P2-1: a string sent to a column-driven select is now checked by `App::set_offered_value`. When the dataset has no table it is rejected with `no values are on offer until a table is loaded into dataset `data`` or `... until dataset `head` is computed`. `null` is still accepted. If the table goes stale between computing it and taking the lock, the check runs once more against the new table, then the value is rejected with `the values of `column` changed while ... was checked; choose again`. New HTTP test `select_from_a_column_accepts_no_value_before_its_dataset_has_a_table` covers both the imported and the derived case before any upload. The stale-table retry path has no test of its own: it needs a concurrent change between two lock acquisitions.
- P2-2: distinct values are no longer listed under the session lock. `App::offered_values` lists them first: in `install` from the table being installed, in `reconcile_derived` from the cached table read under a read lock, and in `set_offered_value` likewise. Under the write lock, `App::table_version` (the cache key of a derived slot, or the revision of an imported one) confirms the table is still current. If it is not, reconciliation stops (the change that made the table stale reconciles the inputs itself) and `set_state` retries once. `reconcile_state` takes the precomputed values and does only cheap work. `query::distinct_values` now deduplicates with Polars `unique()` before casting to text. Once full, it skips any value sorting after the last kept one, and a smaller value replaces that last one. The 1,000 cap and the `truncated` flag are kept. New query test: rows in descending order, and a limit of 0.
- P3-1: the step check measures the distance to the nearest accepted value in value units: `|number − (base + round((number − base)/step)·step)| ≤ 4·EPSILON·max(|number|, |base|) + 1e-9·step`. Tests: 600000000.5 with step 1 is rejected, 1000000000.3 with min 1e9 and step 0.1 is accepted, and 1000000000.35 is rejected.
- P3-2: integer inputs now check whole numbers first and reject any magnitude above 2^53, read exactly from the JSON integer (`unsigned_abs`) when there is one. Range checks then compare exactly. Tests: ±2^53 is accepted; 2^53 + 1, `i64::MAX` and `u64::MAX` are rejected, including against a maximum of 2^53.
- P3-3: a bound beyond ±2^53 on an integer input now says `the minimum -10000000000000000 is beyond ±2^53, the whole numbers an input holds exactly` (tested).
- P3-4: `NumberInput`, `TextInput` and `Select` put the field back to the value the application holds after a refusal. `NumberInput` and `TextInput` do the same after a local error. Each has a Vitest case asserting the restored DOM value.
- P3-5: `TextInput` drops `maxlength`, which counts UTF-16 units. It counts characters with `[...text].length`, as the server's `chars().count()` does, and shows the limit as a hint. Vitest: 8 emoji (16 UTF-16 units) are sent; 9 are refused locally and nothing is sent.

Not done here (other tickets): docs (`project-status`, guide, README; ticket 08) and the example (ticket 07). The frontend bundle in `crates/eario-runtime/frontend-dist` was not rebuilt; `cargo xtask check` does not build it.

### Correction pass 2

- Integer step: when the value, base and step are whole numbers within ±2^53 (always so for a validated integer input), `NumberConstraints::check` tests `(value - base) % step == 0` exactly in `i128`. The float tolerance is kept for decimal inputs only. Tests: min 0 with step 1e9 rejects 3000000001 and accepts 3000000000; min 0 with step 3 rejects 9007199254740991 and accepts 9007199254740990. Both of these were accepted before.
- `distinct_values`: the doc no longer says at most `limit` values are held as text. The text conversion covers every distinct value of a non-text column; only the sorted selection is bounded by `limit`. A text column is no longer cast. The earlier claim in these notes is corrected.
- A select offering the values of a derived dataset whose dependencies, direct or transitive, include the state the select writes is now rejected at `App::new` by `compute::derived_slots`. The error is `AppError::SelectDependsOnItsState { state, dataset }`, code `select_depends_on_its_state` (HTTP status 500, like the other spec errors), and the message names the state and the dataset and says how to fix it. Test: `select_offering_a_dataset_that_reads_its_own_state_is_rejected`, with a control case where the select writes another state and is accepted. The same restriction was not added to `ColumnSelect` (ticket 03 behaviour, outside this request).

**Parent review record.** Base b94bb9b247f8400472e31786f5fffac794a9247e. Review 1 (opus, high, fresh): 2 P2 (column select accepted any string without a table; distinct scan under write lock) + 5 P3 (step tolerance, >2^53 integers, bound message, DOM kept refused value, UTF-16 vs chars). All accepted; correction pass 1 fixed them. Re-review 1 (opus, high, fresh): fixes verified; 3 new P3 (exact integer step, distinct_values doc, self-dependent select). User approved a second correction pass (2026-11-26); fixed. Re-review 2 (opus, medium, fresh): no findings; gap: fmt/clippy exit codes not in evidence. Parent ran `CARGO_BUILD_JOBS=4 cargo xtask check`: exit 0, vitest 44 passed (`.scratch/tests/2026-11-26__custom-rust-logic/06-parent-check.txt`). Known untested: stale-version retry paths (need lock interleaving). Accepted.

## Blockers / Challenges

None blocking. Bash heredocs failed in this shell on content with quotes, so edits were made with the editor tool instead.

## Commits

- 07d3bab feat: number, slider, text, select and checkbox inputs (ticket 06)

## Time Spent

1h 37m (ticket-start.js to ticket-complete.js)
