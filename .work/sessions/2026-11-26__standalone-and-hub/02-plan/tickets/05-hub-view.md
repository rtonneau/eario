# Ticket 05: hub-view

**Model:** medium
**Effort:** high

**Acceptance Criteria:**
- [ ] The generic frontend shows a hub view when `/api/app` reports a hub: the installed dashboards with name, version, state, and Start, Open, Stop, Uninstall actions; Stop all; failed starts show the last log lines.
- [ ] Only `src/lib/api.ts` calls `fetch`; colours from tokens; every control labelled; state changes announced (`role="status"`, errors `role="alert"`); polls the hub state every 2 s while visible.
- [ ] Vitest tests for the hub view and its API calls; `npm run check` reports no warning.
- [ ] Playwright spec `frontend/e2e/hub.spec.ts` drives the real hub with both examples installed in a temporary hub directory: start, open (new tab shows the dashboard), stop, stop all, uninstall, with `expectAccessible` after each new state in both themes.
- [ ] `cargo xtask e2e` builds the hub and prepares the temporary hub directory.

**Files to Touch:**
- `frontend/src/App.svelte`
- `frontend/src/lib/api.ts`
- `frontend/src/lib/session.svelte.ts`
- `frontend/src/lib/components/HubView.svelte`
- `frontend/src/lib/components/HubView.test.ts`
- `frontend/e2e/hub.spec.ts`
- `frontend/e2e/app.ts`
- `xtask/src/main.rs`

**Verification Step:**

Run:
```bash
cargo xtask check && cargo xtask e2e
```

Expected:
Both pass; the hub spec runs with no accessibility violation in either theme alongside the existing data-explorer and signal-lab specs.

**Notes:**

No hub logic in the frontend beyond rendering and calling the API; no `ComponentKind` added. Opening a child uses the URL returned by the hub API in a new tab.
