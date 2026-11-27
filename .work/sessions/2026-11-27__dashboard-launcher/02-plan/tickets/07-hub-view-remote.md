# Ticket 07: hub-view-remote

**Model:** medium
**Effort:** high

**Acceptance Criteria:**
- [ ] Hub view: catalog list with Install buttons, add/remove catalog, "Add repository" input with label, install confirmation dialog (repo, version, size, SHA-256, key fingerprint and key status), update badge and Update button, Rollback, Check now with "could not check" state, setting toggle, hub "Update and restart".
- [ ] Only `api.ts` calls `fetch`; colours from tokens; state changes announced with `role="status"`/`role="alert"`.
- [ ] Vitest for the new view logic; Playwright spec drives the real hub against the fake GitHub server: install, update, rollback, key-change refusal, with `expectAccessible` after each state.

**Files to Touch:**
- `frontend/src/lib/api.ts`
- `frontend/src/lib/components/HubView.svelte`
- `frontend/src/lib/components/` (new hub sub-components)
- `frontend/src/lib/components/HubView.test.ts`
- `frontend/e2e/hub.spec.ts`
- `xtask/src/main.rs`

**Verification Step:**

Run:
```bash
cargo xtask check && cargo xtask e2e
```

Expected:
All checks and browser tests pass with no axe violation.

**Notes:**

The e2e setup needs the fake GitHub server as a runnable binary or test helper; start it from xtask or from Playwright's global setup, with signed fixture assets built from an example.
