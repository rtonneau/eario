# Ticket 06: hub-self-update

**Model:** heavy
**Effort:** xhigh

**Acceptance Criteria:**
- [ ] The Eario release public key is compiled from `crates/eario-hub/eario-release.pub`; while it is the placeholder, self-update reports `self_update_not_configured`.
- [ ] `check()` also reports a newer hub release of `rtonneau/eario`; `self_update()` downloads `eario-hub-<triple>[.exe]`, verifies it like a dashboard, renames the running executable aside, places the new one, restores on failure, and restarts the hub; running children are not touched.
- [ ] The `.old` executable is removed at the next start; route `POST /self-update` and CLI `self-update`.
- [ ] Test with the fake server covers success, bad signature and failed placement (restored).

**Files to Touch:**
- `crates/eario-hub/src/self_update.rs`
- `crates/eario-hub/src/supervisor.rs`
- `crates/eario-hub/src/http.rs`
- `crates/eario-hub/src/main.rs`
- `crates/eario-hub/eario-release.pub`
- `crates/eario-hub/tests/hub.rs`

**Verification Step:**

Run:
```bash
cargo test -p eario-hub self_update
```

Expected:
All self-update tests pass.

**Notes:**

Restart: spawn the new hub executable detached after this hub's server has stopped, so the one-hub-per-user check (`hub.json`) does not bounce it. No `unsafe`.
