# Ticket 02: launch-contract

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `Dashboard::version(impl Into<String>)` sets the manifest version; both examples call it with `env!("CARGO_PKG_VERSION")`.
- [ ] With no `--eario-*` argument `run()` behaves exactly as before.
- [ ] `--eario-manifest` prints the manifest JSON on stdout and exits 0 without building the app or reading any file (an example given extra file arguments still succeeds).
- [ ] `--eario-install [--yes]` copies the running executable into `hub/bin/<slug>/`, records slug, file, size, SHA-256 and manifest in the registry; asks for confirmation showing old and new version; `--yes` skips it; refuses without a terminal and without `--yes`; refuses while that slug's run file is live; a reinstall replaces the copy.
- [ ] `--eario-uninstall [--yes]` removes the entry and the copy, with the same confirmation and live-run refusal.
- [ ] `--eario-child <run-file>` drops the `load_file` entries whose path is exactly `--eario-child` or the run-file path, opens no browser, installs no Ctrl+C handler, logs to `hub/logs/<slug>.log` (1 MB rotation, one `.log.1` kept), writes the run file atomically once listening, removes it on Quit or `POST /api/shutdown`.
- [ ] Errors name the file or slug and have stable codes; nothing is printed to stdout in child mode.

**Files to Touch:**
- `crates/eario/src/dashboard.rs`
- `crates/eario/src/launch.rs`
- `crates/eario/src/error.rs`
- `crates/eario/src/lib.rs`
- `crates/eario-runtime/src/server.rs`
- `crates/eario-runtime/src/lib.rs`
- `examples/data-explorer/src/main.rs`
- `examples/data-explorer/tests/launch.rs`
- `examples/data-explorer/Cargo.toml`
- `examples/signal-lab/src/main.rs`
- `examples/signal-lab/tests/launch.rs`

**Verification Step:**

Run:
```bash
cargo test -p eario -p eario-runtime -p data-explorer -p signal-lab
```

Expected:
All pass, including tests running the real `data-explorer` and `signal-lab` executables (`CARGO_BIN_EXE_*`) with a temporary `EARIO_HUB_DIR`: manifest JSON parsed with the right slug and version; install `--yes` creates the copy and registry entry with matching hash; install without `--yes` and without a terminal refused; child mode writes a run file whose token answers `/api/app`, then `POST /api/shutdown` removes it; uninstall removes entry and copy.

**Notes:**

Logging in child mode: install a file-writing `tracing` subscriber; a small size-rotating writer is enough, no new dependency needed. "Without a terminal" = stdin is not a terminal (`std::io::IsTerminal`). The copy's directory is the child's working directory. `println!` stays reserved for standalone start-up and shutdown lines; the manifest JSON and install prompts are the documented exceptions of the launch contract.
