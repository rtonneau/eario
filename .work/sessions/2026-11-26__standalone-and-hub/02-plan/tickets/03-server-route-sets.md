# Ticket 03: server-route-sets

**Model:** medium
**Effort:** high

**Acceptance Criteria:**
- [ ] `eario-runtime` exposes a way to serve a route set other than a dashboard's `App` (for the hub) behind the same Host/Origin check, session token, shutdown handling and embedded frontend; dashboards use the same path internally.
- [ ] `/api/app` lets the frontend tell a dashboard from a hub (a contract type change with regenerated bindings), and the dashboard response is otherwise unchanged.
- [ ] Existing HTTP security tests still pass; a new test shows a custom route set rejects a wrong Host, a foreign Origin and a missing token.

**Files to Touch:**
- `crates/eario-runtime/src/server.rs`
- `crates/eario-runtime/src/http.rs`
- `crates/eario-runtime/src/http_tests.rs`
- `crates/eario-runtime/src/lib.rs`
- `crates/eario-core/src/payload.rs`
- `frontend/src/lib/generated/`
- `frontend/src/lib/session.svelte.ts`

**Verification Step:**

Run:
```bash
cargo test -p eario-runtime -p eario && cd frontend && npm run check && npm run test
```

Expected:
All tests pass with 0 svelte-check warnings; the new route-set security tests pass.

**Notes:**

Keep it a refactor for dashboards: same routes, same responses apart from the added discriminant. AGENTS.md says new routes go through `http::router` so they inherit the checks; the hub's routes must reuse that middleware, not copy it.
