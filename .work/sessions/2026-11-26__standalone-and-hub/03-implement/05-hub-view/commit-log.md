# Ticket 05: hub-view

**Status:** ✅ Done

## Local Test Result

Verification Step `cargo xtask check && cargo xtask e2e`, run in two parts (logs in `.scratch/tests/2026-11-26__standalone-and-hub/evidence/`):

- `CARGO_BUILD_JOBS=4 npx -y -p node@24 -- cargo xtask check` (Node 24 from the npm cache on PATH for this command only, so vitest can start; nothing installed globally, package.json untouched): exit 0. fmt, clippy `-D warnings`, `cargo test --workspace --all-features` (every result ok, including the eario-hub tests), `cargo doc`, `npm run check` (0 errors, 0 warnings) and vitest (10 files, 65 tests passed, 19 new in `HubView.test.ts`, 2 new in `api.test.ts`). Log: `05-check.log`.
- `CARGO_BUILD_JOBS=4 cargo xtask e2e` (Node 22.14, no override): exit 0, 10 passed (6 existing data-explorer and signal-lab tests, 4 new hub tests, `expectAccessible` in both themes after each new state). Log: `05-e2e.log`. Afterwards no `eario-hub`, `data-explorer` or `signal-lab` process was left and `target/e2e-hub` was empty.
- Plain `npm run check` on Node 22.14 also passes; plain vitest on 22.14 cannot start (undici), as the parent said.

## Review Notes

Parent: base fbc19a4c5f7835c46640369fd4413a010cb3d629. No pre-existing changes outside .work/. Mode: delegate.

Checked against the Acceptance Criteria:

1. Hub view: `HubView.svelte` is shown by `App.svelte` when `session.hub` is set (the answer of `/api/app` of kind `hub`). Each dashboard card has name, version ("Version unknown" when none), state, description, and Start, Open, Stop, Uninstall (each enabled only for the states where the hub accepts it: Start/Uninstall for stopped and failed, Open for running, Stop for running and starting), plus Stop all and Quit hub. A failed dashboard shows its `log_lines` in a focusable `pre`. No hub logic beyond calling the API.
2. Only `api.ts` calls `fetch` (new `hubStart/Stop/Open/Uninstall/StopAll`, slug URL-encoded; test shows `../stop-all` stays one path segment). Colours come from tokens. Every control has an accessible name (`aria-label="Start <name>"` etc.). State changes go to a `role="status"` region (a change seen by the poll is announced too); refusals and an unreachable hub are `role="alert"`. Polling every 2 s (`POLL_INTERVAL_MS`), skipped while the page is hidden, immediate refresh on `visibilitychange`, stopped on unmount; a list that left before an action came back is dropped so it cannot undo the action.
3. Vitest: `HubView.test.ts` and `api.test.ts`; `npm run check` clean.
4. `frontend/e2e/hub.spec.ts`: listing, start then Open (new tab shows the Data Explorer h1, axe on that tab too) then Stop, Stop all with two running dashboards, uninstall (dismiss keeps it, accept removes it; a running dashboard's Uninstall is disabled). Fixture `launchHub` in `e2e/app.ts`: per test a temporary hub directory, both examples installed with `--eario-install --yes` and `EARIO_HUB_DIR` (one after the other: parallel installs raced on the registry and lost one entry), hub started with `EARIO_NO_BROWSER`. Teardown in `finally`, pass or fail: POST stop-all and shutdown through the API, wait up to 15 s for the hub to exit then kill it, kill any pid left in `run/*.json`, remove the directory with retries (Windows locks).
5. `cargo xtask e2e` builds `eario-hub` with the examples, empties and recreates `<target>/e2e-hub` and passes it as `EARIO_E2E_HUB_ROOT`; the fixture makes each test's hub directory in it (system temp directory when the variable is not set). The examples are installed by the fixture per test (not by xtask) so each test can uninstall freely.

Changes beyond the file list, all small and needed: `Header.svelte` got optional props (`themeChoice`, `quitLabel`, `quitQuestion`) so the hub reuses it without the theme selector (the hub has no `/api/preferences` route, so a selector would only produce errors) and with its own quit wording; `App.svelte` shows "The hub has stopped ... dashboards keep running" after Quit hub; `session.svelte.ts` sets the tab title. Fixed after the first e2e run: state text was split across a hidden "State:" prefix and the label in one element, so exact text lookups failed; the label is now its own span.

Not done: no e2e for a failed start (no example fails to start; the log display is covered by vitest). Docs (`docs/development/guide.md` mentions of `cargo xtask e2e` and the browser tests) not updated: ticket 07 owns docs. Nothing staged or committed.

Independent review (fresh reviewer, sonnet high, base fbc19a4, round 1): 0 P0/P1, 1 P2, 2 P3.
- P2 hub e2e tests relied on Playwright's default 30 s test timeout while waits allow 30 s per state and tests took up to 21.6 s → fixed inline: `test.describe.configure({ timeout: 120_000 })` for the hub suite.
- P3 `e2e/app.ts` killDashboards kills by run-file pid (pid "for display") → kept as last-resort test cleanup; accepted risk documented in a comment.
- P3 focusable log `pre` had no accessible name (label on the wrapper group) → fixed inline: `role="region"` + `aria-label` on the `pre`; vitest queries updated.
- Reviewer gaps: no Linux run (CI ubuntu is first); failed-start card not driven through axe/e2e (no example fails to start; content covered by vitest); popup blocking undetectable with noopener. Accepted.
Parent re-verification after the fixes: `npm run check` 0/0; vitest under Node 24 via npx: 65 passed (evidence/05-parent-vitest.log); `cargo xtask e2e`: 10 passed, no stray process (evidence/05-parent-e2e.log). Re-review skipped: three localized edits, all re-verified by the suites they affect.
Acceptance: all criteria met; requested model sonnet (medium, high), actual sonnet.

## Blockers / Challenges

None of substance. Vitest cannot start on the local Node 22.14; worked around for verification only with `npx -y -p node@24 -- ...` (see Local Test Result). Two e2e failures on the way, both mine and fixed: parallel `--eario-install` calls into one hub directory lost a registry entry (now sequential, which also means two concurrent installs into one directory are not safe: a product observation for ticket 04, not changed here), and exact text lookup on a state label (see Review Notes).

## Commits

- e6576cd feat: hub view in the generic frontend, with browser tests (ticket 05)

## Time Spent

22m (ticket-start.js to ticket-complete.js)
