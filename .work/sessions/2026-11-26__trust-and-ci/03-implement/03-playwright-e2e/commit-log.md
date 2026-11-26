# Ticket 03: playwright-e2e

**Status:** ✅ Done

## Local Test Result

`cargo xtask e2e` (full: frontend build, examples build, Chromium install, `npm run e2e`): exit 0, `6 passed (2.7s)`. Log: `.scratch/tests/2026-11-26__trust-and-ci/03-e2e-final.log`. Also: `npm run check` (svelte-check + `tsc -p e2e`), 0 errors and 0 warnings; `npm run test` (Vitest), 44 passed, with e2e files not picked up; `cargo fmt --check` and `cargo clippy -p xtask -D warnings` clean; no example process left running after the suite.

## Review Notes

Base: e5cb117. Pre-existing, not owned: `.work/work-map.*`.

Against the acceptance criteria:
- `@playwright/test` ^1.65.1 (Apache-2.0) and `@types/node` ^24 (MIT, to type-check the Node-side fixture; major 24 matches CI's Node) added. Config targets Chromium only; tests in `frontend/e2e/`.
- Fixture `e2e/app.ts`: spawns `<bin>/<example>` with `EARIO_NO_BROWSER=1` and the system-chosen port (default port 0, so `EARIO_PORT` is not needed), parses `is running at <url>#token=` from the output, opens it, and kills the process after the test. 30 s start timeout, with the output in the error.
- data-explorer: file picker import; scatter canvas larger than 100×100 px with an exact accessible name (38 points, 2 rows skipped); filter applied and cleared, with the statistics row count changing and then restored; pagination 1–50 → 51–100 → back, on groups.csv (144 rows).
- signal-lab: the bins number input changes the histogram from 20 to 8 points (Rust computation); the category select on the Group summary page changes the table.
- `cargo xtask e2e [--prebuilt-frontend]`; `EARIO_E2E_BIN_DIR` honours `CARGO_TARGET_DIR`.
- CI job `End-to-end (Chromium)`: ubuntu, frontend-dist artifact, `playwright install --with-deps chromium`, report uploaded on failure. Not run in CI yet.
- Vitest includes only `src/**/*.test.ts`, so it does not pick up e2e. The e2e TS gets its own `e2e/tsconfig.json` (Node types stay out of the browser code); `npm run check` now also runs `tsc -p e2e`.
- Also: Playwright output ignored in `.gitignore`; guide (testing table, Browser tests section, commands table); xtask help for `check` now says what it skips.

## Blockers / Challenges

First run: 5/6 passed. The scatter test expected 40 points, but the chart draws 38, because 2 rows have missing values. The assertion was wrong, not the app; it now asserts the exact name and the skipped-rows note.

## Commits

- ae1aaf8 test: browser end-to-end tests with Playwright (ticket 03)

## Time Spent

6m (ticket-start.js to ticket-complete.js)
