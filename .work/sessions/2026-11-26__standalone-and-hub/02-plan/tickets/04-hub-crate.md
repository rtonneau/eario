# Ticket 04: hub-crate

**Model:** heavy
**Effort:** xhigh

**Acceptance Criteria:**
- [ ] New crate `crates/eario-hub` (library plus `eario-hub` binary) in the workspace, depending on `eario-runtime` and `eario-core` only among Eario crates.
- [ ] HTTP API by slug only: list (state, version, last log lines when failed), start, stop, stop all, uninstall, open (child URL with token, to the hub's own page), quit. No route accepts a path, an executable or arguments; the registry is re-read on each request.
- [ ] Start refuses a size or SHA-256 mismatch and an unknown protocol version with an actionable message; spawns the copy with `--eario-child <hub/run/<slug>.json>`, no console window on Windows (`CREATE_NO_WINDOW`), its own process group on Unix, working directory the copy's directory; becomes `running` once the run file's token answers, `failed` with the last log lines after 30 s or an early exit.
- [ ] At start the hub re-attaches to run files that answer with their token and deletes the others; quitting the hub leaves children running.
- [ ] `hub/hub.json` records the hub's address and token; a second start that finds a live hub opens the browser on it and exits.
- [ ] Stop sends `POST /api/shutdown`; a child this hub process started is killed if it has not exited after a timeout.
- [ ] Linux/macOS (and Windows until ticket 6): terminal process printing its address, Ctrl+C quits the hub only.
- [ ] `docs/architecture/security.md` and `AGENTS.md` describe the bounded process-start exception.

**Files to Touch:**
- `Cargo.toml`
- `crates/eario-hub/Cargo.toml`
- `crates/eario-hub/src/lib.rs`
- `crates/eario-hub/src/main.rs`
- `crates/eario-hub/src/supervisor.rs`
- `crates/eario-hub/src/http.rs`
- `crates/eario-hub/tests/hub.rs`
- `docs/architecture/security.md`
- `AGENTS.md`

**Verification Step:**

Run:
```bash
cargo build -p data-explorer -p signal-lab && cargo test -p eario-hub
```

Expected:
All pass, including tests that install both real examples into a temporary `EARIO_HUB_DIR`, start them through the hub API, see them `running`, open returns a URL whose token answers, stop them; re-attach after the hub restarts; a stale run file is deleted; a modified copy is refused on hash mismatch; a request with a path-like slug is rejected; a second hub instance is detected.

**Notes:**

The example executables are not `CARGO_BIN_EXE_*` of this package: locate them under the target directory (honour `CARGO_TARGET_DIR`) and fail with a clear message telling to build them first, or move these tests into an example package with `eario-hub` as a dev-dependency; pick one and say why. Business logic lives in the library (supervisor); the HTTP handlers are thin adapters. Blocking work (hashing a 30 MB file) through `spawn_blocking`. Update `deny.toml` only if needed.
