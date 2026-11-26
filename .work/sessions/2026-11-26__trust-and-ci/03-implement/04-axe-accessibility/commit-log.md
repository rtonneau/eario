# Ticket 04: axe-accessibility

**Status:** ✅ Done

## Local Test Result

`cargo xtask e2e`: exit 0, `6 passed (5.9s)`. Log: `.scratch/tests/2026-11-26__trust-and-ci/04-e2e-3.log`. The gate is live: before the fix the same suite failed on `scrollable-region-focusable` (serious, `.scroll-x`), log `04-e2e-1.log`. `npm run check`: 0 errors, 0 warnings. `npm run test`: 44 passed.

## Review Notes

Base: ae1aaf8. Pre-existing, not owned: `.work/work-map.*`.

- `@axe-core/playwright` ^4.14.1 (MPL-2.0, dev only, never shipped in the executable) added.
- `e2e/axe.ts` `expectAccessible(page, state)`: tags wcag2a, wcag2aa, wcag21a, wcag21aa; run in the light and the dark theme; fails listing rule, impact, help URL and targets. 10 calls cover every visited state of the 6 tests (empty, imported, chart, filtered, table pages 1 and 2, signal analysis, recomputed histogram, group summary before and after).
- The theme is set through `data-theme` on the root, not through the theme selector. The selector saves the preference through `directories`, which on Windows uses the Known Folder API, so the tests could not redirect it and would overwrite the developer's real preference.
- Fix: the three horizontally scrolling tables (DataTable, Statistics, DatasetInfo) are now `role="region"` with an `aria-label` and `tabindex="0"`, so a keyboard user can scroll them. The existing global `:focus-visible` outline (accent token) shows focus. Svelte's `a11y_no_noninteractive_tabindex` is ignored on those three lines, each with a comment explaining why; no axe rule is disabled.
- Only DataTable was flagged (the others did not overflow in the fixtures); all three were fixed because they have the same structure.
- Guide updated (testing table, accessibility paragraph).

## Blockers / Challenges

The second run still failed because `npm run e2e` used examples that embedded the old frontend build. The full `cargo xtask e2e` rebuilds both, and it passed.

## Commits

- 6557279 test: check accessibility with axe-core in the browser tests (ticket 04)

## Time Spent

3m (ticket-start.js to ticket-complete.js)
