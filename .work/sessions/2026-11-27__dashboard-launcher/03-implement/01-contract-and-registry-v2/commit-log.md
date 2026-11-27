# Ticket 01: contract-and-registry-v2

**Status:** ✅ Done

## Local Test Result

`cargo test -p eario-core -p eario-runtime -p eario -p eario-hub` (hub added because it builds `HubDashboard`/`RegistryEntry`): all test binaries ok, 0 failed (eario-core 25, eario-runtime 74, eario 7, eario-hub included). Output: `.scratch/tests/2026-11-27__dashboard-launcher/evidence/ticket01-test.txt`.
`cargo xtask gen-types`: 43 bindings exported ok; re-running it leaves the generated directory byte-identical (idempotent). New files Catalog, CatalogEntry, InstallPreview, KeyStatus, Source; HubDashboard changed. They are uncommitted, so `git diff --exit-code frontend/src/lib/generated` shows the HubDashboard change until the parent commits; after commit the diff is empty.
`npm run check` in frontend: 394 files, 0 errors, 0 warnings.
`cargo clippy -p eario-core -p eario-runtime -p eario -p eario-hub --all-targets -- -D warnings` and `cargo fmt --check`: clean.
Not verified: `npm test` (vitest) cannot start in this environment (`[vitest-pool] Failed to start forks worker ... reading 'bind'`, also with `--pool=threads`; no test file runs, so unrelated to the change). The only frontend test edit is the `dashboard()` fixture in `HubView.test.ts`, which now carries the three new `HubDashboard` fields; it type-checks under `npm run check`.

## Review Notes

Parent: base 975d200dc80e4baf37b2b017105496cd001c7366; working tree clean before implementation (only this session's files under .work/). Mode delegate (implementer sonnet/high). Recovery count: 0.

Implementer self-check against the Acceptance Criteria (review rounds: 0 so far).
- AC1: `RegistryEntry` has `source: Source` (`#[serde(default)]`, tagged `{"kind":"local"}` / `{"kind":"github","repo":..}`), `pinned_key: Option<String>`, `previous: Option<PreviousVersion {file_name,size,sha256,manifest}>` (both skipped when `None`). `REGISTRY_FORMAT_VERSION` is 2. Test `source_key_and_previous_version_round_trip`.
- AC2: test `a_format_1_registry_reads_as_local_entries` reads a format-1 file as local, no key, no previous; `read_registry` upgrades `format_version` in memory and a write stores 2. `unknown_registry_version_names_the_file` (code `hub_registry_unknown_version`) still passes for version 99.
- AC3: `eario-core` (serde only) adds `Source`, `is_repo`, `Catalog` (+ `CATALOG_FORMAT_VERSION` = 1), `CatalogEntry {repo,name,description,public_key}`, `KeyStatus {catalog,pinned,unknown,changed}`, `InstallPreview {repo,version,asset,size,sha256,key_fingerprint,key_status}`, and `HubDashboard.{source,available_version,has_previous}`. `is_repo` test covers good names, empty parts, extra slashes, `.`/`..`, URLs, whitespace/newline, non-ASCII, `_` in owner and the length limits (39/100, boundary and one over).
- AC4: bindings regenerated (see above); `npm run check` passes.
Scope notes: `--eario-install` writes `source: local`, no key, no previous (`crates/eario/src/launch.rs`). `crates/eario-hub/src/supervisor.rs` was touched only to fill the new `HubDashboard` fields (source and has_previous from the entry, `available_version: None` until the update-check ticket), because the struct literal would not compile otherwise. Design choices for later tickets: the catalog key field is named `public_key`; `Source` is in `eario-core` because `HubDashboard` exposes it. `PreviousVersion.file_name` is a bare file name inside `bin/<slug>/` for the supervisor to validate before joining.

Independent review (sonnet/high, base 975d200): REVIEWED — no findings. Gaps it raised: clippy/fmt evidence missing → parent ran `cargo fmt --all -- --check` and workspace `cargo clippy --all-targets --all-features -D warnings`: clean. gen-types diff re-checked after commit. vitest: local Node 22.14 < 22.19 (environmental, known since the previous session), carried to CI. Accepted: all four criteria met with tests.

## Blockers / Challenges

- `npm test` (vitest) does not start here (worker pool failure), so frontend unit tests were not run.
- Mid-task I ran `git add -N` on the generated directory to read a diff, then undid it with `git reset -q -- frontend/src/lib/generated`; nothing is staged.

## Commits

- 5464f1e feat: registry v2 with sources, pinned keys and previous versions; catalog and install-preview contract types (ticket 01)

## Time Spent

10m (ticket-start.js to ticket-complete.js)
