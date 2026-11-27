# Ticket 03: server-route-sets

**Status:** ✅ Done

## Local Test Result

Verification Step: `cargo test -p eario-runtime -p eario && cd frontend && npm run check && npm run test`, run in parts (CARGO_BUILD_JOBS=4).
- `cargo test -p eario-core --features ts -p eario-runtime -p eario`: all pass (eario-core 25 + 3, eario-runtime 75, eario 72 + 7). Output: `.scratch/tests/2026-11-26__standalone-and-hub/evidence/03-cargo-test.txt`. New: four `custom_route_set_*` tests in http_tests.rs (wrong token/no token 401; wrong Host and foreign Origin 403; own Origin accepted; shutdown and frontend shared), `app_response_is_tagged_by_kind_and_keeps_the_info_fields` in payload.rs, and a `kind == "dashboard"` assertion in `app_info_starts_with_empty_state`. All existing http security tests pass unchanged.
- `cargo xtask gen-types` created `frontend/src/lib/generated/AppResponse.ts` (no other binding changed).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`: clean.
- `npm run check` (frontend): 0 errors, 0 warnings.
- `npm run test`: cannot start on local Node 22.14.0 (vitest worker fails loading undici, which needs Node >= 22.19: "Cannot read properties of undefined (reading 'bind')"). Output: evidence/03-npm-test.txt. Not run, so the one-line change to `src/App.test.ts` (the mocked `/api/app` answer now carries `kind: 'dashboard'`) is unverified by vitest; `npm run check` type-checks it.
- `cargo xtask e2e` not run (no Playwright run in this ticket).

## Review Notes

Parent: base 268164b93d283c4f62776d1492d881ac466c45d2. No pre-existing changes outside .work/. Mode: delegate.

Checked against the criteria:
1. Route sets: `http::router(api_routes, security, shutdown, extra_routes)` now takes any `/api` route set (state already applied) and adds `POST /api/shutdown`, the 404 fallback, the token layer, the Host/Origin layer and the embedded frontend. `http::dashboard_api(app, max_upload)` builds the dashboard routes; `server::start` is `start_routes(dashboard_api(..), config)`. Public entry for ticket 04: `eario_runtime::start_routes(api_routes: Router, config: ServerConfig) -> RunningServer` (`address()`, `token()`, `url()`, `shutdown_handle()`, `wait()`). Routes are relative to `/api` and must include their own `/app`; they must not define `/shutdown`. `RunningServer::shutdown_handle()` / `ShutdownHandle` is new so a Ctrl+C handler can stop a server whose `wait()` has consumed it (`run` uses it). `HttpState` no longer holds the `App`; handlers take `State<App>`.
2. Discriminant: new `AppResponse` enum in eario-core (`#[serde(tag = "kind")]`: `dashboard(AppInfo)` | `hub(HubInfo)`). `AppInfo` and `HubInfo` are unchanged; the dashboard response is the same JSON plus `"kind": "dashboard"`. The hub's `/api/app` in ticket 04 should answer `AppResponse::Hub(HubInfo)`. `App::info()` still returns `AppInfo` (MCP unchanged). Frontend: `api.app()` returns `AppResponse`; `session.svelte.ts` stores a hub answer in a new `session.hub` and returns; `App.svelte` is untouched, so a hub answer shows the loading state until ticket 05 adds the hub view.
3. Tests: see Local Test Result.
Also updated the `GET /api/app` row in `docs/architecture/overview.md`. No changes made after a review round (none held).

Independent review (fresh reviewer, sonnet high, base 268164b, round 1): 0 P0/P1/P2, 2 P3. No route-set bypass found: route set and extra routes are merged before the token layer; Host/Origin wraps everything; a route set's own fallback is replaced.
- P3 `server.rs:6` module doc line over 100 characters → fixed inline (rewrapped).
- P3 `start_routes` docs did not say a `/shutdown` route panics at assembly → fixed inline (doc states it).
- Reviewer gaps: vitest not run (Node 22.14); missing evidence for fmt/clippy/doc/svelte-check; partial cargo scope; e2e not run → parent ran `cargo fmt --check`, workspace clippy `-D warnings`, `cargo test --workspace --all-features` (all pass), `cargo doc` with `-D warnings`, `npm run check` (0/0): evidence/03-parent-reverify.log; `cargo xtask e2e`: 6 passed (evidence/03-e2e.log), so the session's handling of the new `/api/app` shape works in Chromium for dashboards. Remaining gap: vitest (App.test.ts change) unverified locally, carried to ticket 07/CI.
Acceptance: criteria met; requested model sonnet (medium, high), actual sonnet.

## Blockers / Challenges

Local Node 22.14 cannot run vitest (undici needs >= 22.19); recorded above, Node not touched. Design choice worth the parent's eye: the tag lives on a new `AppResponse` enum rather than on `AppInfo`, so the hub's `HubInfo` needs no tag field and the dashboard payload struct is untouched.

## Commits

- 8703711 refactor: serve any API route set behind the dashboard security layer (ticket 03)

## Time Spent

15m (ticket-start.js to ticket-complete.js)
