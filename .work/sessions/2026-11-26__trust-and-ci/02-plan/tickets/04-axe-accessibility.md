# Ticket 04: axe-accessibility

**Model:** medium
**Effort:** high

**Acceptance Criteria:**
- [ ] `@axe-core/playwright` is added; a helper runs AxeBuilder with the tags `wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa` and fails on any violation, listing rule, target and help URL.
- [ ] The check runs after each visited state of the e2e tests, in the light and dark themes.
- [ ] Every violation found is fixed in the generic frontend (colours via `app.css` tokens), with no rule disabled.
- [ ] `cargo xtask e2e` and `npm run check` pass.

**Files to Touch:**
- `frontend/package.json`, `frontend/package-lock.json`
- `frontend/e2e/axe.ts` and the existing e2e specs
- `frontend/src/**` (fixes)
- `docs/development/guide.md`

**Verification Step:**

Run:
```bash
cargo xtask e2e
```

Expected:
All tests pass, including the accessibility checks in both themes.

**Notes:**

If a fix needs a product decision (for example a brand colour change), stop and ask. ECharts canvases may need an `aria-label`/`role="img"`.
