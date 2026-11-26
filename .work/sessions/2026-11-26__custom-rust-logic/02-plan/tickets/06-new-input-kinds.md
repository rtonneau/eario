# Ticket 06: new-input-kinds

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `ComponentKind` variants `NumberInput`, `Slider` (label, state, min, max, step, integer, default), `TextInput` (label, state, default, max_length), `Select` (label, state, options `Static(Vec<String>)` or `Column { dataset, column }`, default), `Checkbox` (label, state, default), each with `dependencies()` and `written_state()`.
- [ ] Initial state is each input's default; `App::set_state` validates range, step/integrality, text length, option membership and booleans with actionable errors.
- [ ] Column-driven select options are the distinct values of the column, capped at 1,000, with truncation reported in `ComponentData`; reconciled when the dataset changes.
- [ ] Facade constructors `number_input`, `slider`, `text_input`, `select`, `select_from_column`, `checkbox` take typed handles; numeric ones are generic over a sealed `NumberType` (i64, f64, u32).
- [ ] Svelte components with labels and announced changes, registered in `registry.ts`; colours from `app.css` tokens; Vitest tests for each.
- [ ] Bindings regenerated; HTTP tests cover a valid and an invalid value for each input.

**Files to Touch:**
- `crates/eario-core/src/spec.rs`
- `crates/eario-core/src/payload.rs`
- `crates/eario-runtime/src/app.rs`
- `crates/eario-runtime/src/http_tests.rs`
- `crates/eario/src/widgets.rs`
- `crates/eario/src/lib.rs`
- `frontend/src/lib/components/` (NumberInput, Slider, TextInput, Select, Checkbox .svelte and tests)
- `frontend/src/lib/registry.ts`
- `frontend/src/lib/generated/`

**Verification Step:**

Run:
```bash
cargo xtask gen-types && cargo xtask check
```

Expected:
All checks pass, `npm run check` reports no warning, new Rust and Vitest tests pass.

**Notes:**

Integer inputs reject non-integral JSON numbers. A `u32` handle maps to an integer input with `min >= 0` and `max <= u32::MAX`. Only `src/lib/api.ts` calls `fetch`.
