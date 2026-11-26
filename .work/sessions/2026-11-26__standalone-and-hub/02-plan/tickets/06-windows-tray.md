# Ticket 06: windows-tray

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] On Windows the hub is built with `windows_subsystem = "windows"` (no console window) and shows a tray icon with: open the hub, the running dashboards (click to open), stop all, quit.
- [ ] The tray runs on the main thread, the Tokio runtime on other threads; quitting from the tray leaves children running.
- [ ] The tray dependency is target-specific to Windows, declared in `[workspace.dependencies]` with the narrowest features, justified (what it does, licence, maintenance), passes `cargo deny check`; no `unsafe` in Eario's code.
- [ ] Linux and macOS builds are unchanged (terminal mode) and CI does not need GTK or appindicator.

**Files to Touch:**
- `Cargo.toml`
- `crates/eario-hub/Cargo.toml`
- `crates/eario-hub/src/main.rs`
- `crates/eario-hub/src/tray.rs`
- `deny.toml`

**Verification Step:**

Run:
```bash
cargo build -p eario-hub --release && cargo clippy -p eario-hub --all-targets -- -D warnings && cargo deny check
```

Expected:
Builds and passes; started by hand on Windows, the hub shows no console window, its tray menu opens the hub, lists a running dashboard, stops all and quits (manual check recorded in the commit log).

**Notes:**

Research first. If every workable crate needs a heavy event-loop dependency (winit, tao) or `unsafe` in our code, stop and ask the user before adding it (agreed in the grill). The tray menu labels come from the same hub supervisor state as the HTTP API.
