# Implementation Plan

**Session:** standalone-and-hub
**Date:** 2026-11-26T21:54:21.821Z
**Estimated effort:** 3-4 days

## Strategy

Build from the contract outwards. First the `eario-core` types and the shared hub-directory code in `eario-runtime` (registry, run files, hashing, liveness probe), then the launch contract every dashboard gets through `Dashboard::run()`, verified on the real example executables. Then generalize the runtime server so a second route set (the hub) is served behind the same security layer, build the hub crate on it, add its frontend view, then the Windows tray and console-free build. Documentation and `dist` close the milestone. The hub's security exception is documented in the same ticket that introduces it.

## Tickets Overview

- **Ticket 1:** Contract types (`Manifest`, `RunFile`, hub API payloads, protocol version) in `eario-core`, and the hub directory module in `eario-runtime` (directory resolution, registry, run files, SHA-256, liveness probe).
- **Ticket 2:** Launch contract in every dashboard: `Dashboard::version`, `--eario-manifest`, `--eario-install`/`--eario-uninstall`, `--eario-child`, tested on the real examples.
- **Ticket 3:** Runtime server entry point serving another route set behind the same Host/Origin and token checks and the embedded frontend, with `/api/app` telling a dashboard from a hub.
- **Ticket 4:** The `eario-hub` crate and executable: supervisor, HTTP API by slug, re-attach, one hub per user, terminal mode; security document and AGENTS.md updated for the exception.
- **Ticket 5:** The hub view in the generic frontend, with Vitest and a Playwright spec with axe checks in both themes.
- **Ticket 6:** Windows tray and console-free hub; tray crate chosen and justified.
- **Ticket 7:** Documentation, `cargo xtask dist`, final checks.

## Sequencing Rationale

Types first, because every later ticket serializes them and the bindings must be regenerated once. The launch contract (2) needs only the shared hub-directory module and is independently useful: dashboards built after it are hub-ready. The server generalization (3) is a refactor that must leave dashboards unchanged before the hub (4) relies on it. The frontend view (5) needs the hub API. The tray (6) wraps a working hub and carries the riskiest dependency, so it comes last among the code tickets and can stop without blocking the rest. Docs (7) describe what landed.

## Risks & Mitigation

- **Risk:** the tray crate needs `unsafe` in our code or a heavy event-loop dependency (winit, tao) → **Mitigation:** research in ticket 6 first; if heavy, stop and ask the user before adding it; tickets 1-5 stand alone.
- **Risk:** the hub starts processes, an exception to the security rules → **Mitigation:** routes take a slug only, size and hash checked before every start, protocol version checked, registry written only outside the HTTP API; HTTP tests that no route accepts a path or arguments; security document updated in the same ticket.
- **Risk:** filtering `load_file` entries in child mode misses an author's argument handling → **Mitigation:** the hub passes only the two contract arguments; tests on both examples; documented in ADR 0011.
- **Risk:** children outlive the hub and leave stale run files → **Mitigation:** run files trusted only after `GET /api/app` answers with their token; stale ones deleted at hub start; tested.
- **Risk:** real-executable tests are slow or flaky on CI → **Mitigation:** bounded timeouts, temporary `EARIO_HUB_DIR` per test, ports chosen by the system.
- **Risk:** standalone behaviour regresses → **Mitigation:** the contract only acts on `--eario-*` arguments; existing tests and e2e specs must keep passing at each ticket.

## Assumptions

- `sha2` is already compiled in every build (through `rust-embed-utils`), so using it adds no new crate to the tree; it is still declared and justified in `[workspace.dependencies]`.
- `CARGO_BIN_EXE_<name>` is only available to tests of the package that owns the binary, so real-executable tests live in the example packages (or the hub package for the hub binary).
- Windows children spawned with `CREATE_NO_WINDOW` get no console; on Unix `process_group(0)` detaches them from the hub's terminal signals. Both are in `std`, without `unsafe`.
- CI runs on Linux, where the tray is not built.
