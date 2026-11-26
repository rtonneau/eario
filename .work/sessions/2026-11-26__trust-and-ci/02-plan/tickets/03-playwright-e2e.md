# Ticket 03: playwright-e2e

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `@playwright/test` is added as a frontend dev dependency; `frontend/playwright.config.ts` targets Chromium only; tests live in `frontend/e2e/`.
- [ ] A fixture starts an example executable with `EARIO_PORT=0` (or a free port) and `EARIO_NO_BROWSER=1`, waits for the printed address, opens it, and stops the process after the test.
- [ ] Tests on data-explorer: import a fixture through the file picker; select a dataset; page through the table; apply a filter and see the row count change; a chart is drawn (canvas of non-zero size).
- [ ] Test on signal-lab: changing an M1 input changes the derived view.
- [ ] `cargo xtask e2e` builds the frontend and the examples, installs Chromium if missing, and runs the suite.
- [ ] New CI job `End-to-end (Chromium)` on ubuntu runs the suite, reusing the frontend-dist artifact, and uploads the Playwright report on failure.
- [ ] Vitest does not pick up `e2e/` files; `npm run check` reports no warning.

**Files to Touch:**
- `frontend/package.json`, `frontend/package-lock.json`
- `frontend/playwright.config.ts`
- `frontend/e2e/*.ts`
- `frontend/vite.config.ts` (Vitest exclude)
- `xtask/src/main.rs`
- `.github/workflows/ci.yml`
- `docs/development/guide.md`

**Verification Step:**

Run:
```bash
cargo xtask e2e
```

Expected:
Playwright reports every test passed in chromium.

**Notes:**

Check how an example reads EARIO_PORT and whether port 0 is accepted. If not, pick a free port in the fixture. Read the actual signal-lab spec to choose the input. Use role and label selectors. Wait on `role="status"` announcements, never on sleeps.
