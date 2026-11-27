# Ticket 04: supervisor-install-update

**Model:** heavy
**Effort:** xhigh

**Acceptance Criteria:**
- [ ] Catalogs: add/remove by repo, default `rtonneau/eario-catalog`, stored in the hub directory; list entries with their install state.
- [ ] `preview(repo)` downloads to a staging file, verifies signature and hash, runs `--eario-manifest` on the verified file, checks slug and protocol, and returns an `InstallPreview`; `install(repo, sha256)` succeeds only for a staged file with that hash.
- [ ] Trust on first use: a repo without a catalog key pins the key seen at install; a different key later is refused (`key_changed`) until the user accepts it.
- [ ] A slug already installed from another source is refused.
- [ ] `check()` reports available versions per GitHub entry and stores the last result; it runs at hub start unless the preference `check_updates_at_start` is false.
- [ ] `update(slug)` is refused while running, keeps one previous version, removes the older one; `rollback(slug)` restores the previous version.
- [ ] `eario-hub` CLI subcommands: `catalog add|remove|list`, `install <owner/repo>`, `check`, `update <slug>`, `rollback <slug>`, each confirming in the terminal unless `--yes`.

**Files to Touch:**
- `crates/eario-hub/src/supervisor.rs`
- `crates/eario-hub/src/remote.rs`
- `crates/eario-hub/src/lib.rs`
- `crates/eario-hub/src/main.rs`
- `crates/eario-hub/tests/hub.rs`

**Verification Step:**

Run:
```bash
cargo test -p eario-hub
```

Expected:
All tests pass, including install, update, rollback, key change, slug clash, tampered asset and running-refusal cases against the fake server with a real example executable.

**Notes:**

Business logic lives here, not in `http.rs`. The staging directory is inside the hub directory; clean it at start. Network and Polars-free CPU work run on `spawn_blocking`.
