# Ticket 02: msrv

**Status:** ✅ Done

## Local Test Result

`CARGO_TARGET_DIR=target/msrv cargo +1.95 test --workspace --all-features`, after `rust-version` was declared: exit 0, 16 `test result: ok`, 0 FAILED. Logs: `.scratch/tests/2026-11-26__trust-and-ci/02-msrv-1.95.log` (first run) and `02-msrv-final.log`. `cargo metadata` shows rust_version 1.95 for all 9 workspace packages.

## Review Notes

Base: 81a802a (after ticket 01). Pre-existing changes not owned: `.work/work-map.*` (user's); `xtask/src/main.rs`, `frontend/package*.json`, `frontend/playwright.config.ts`, `frontend/e2e/` are ticket 03 work in progress and are not committed here.

The lowest working version is 1.95. The search starts at the floor: `sysinfo` declares `rust-version = 1.95`, and cargo refuses an older compiler for it, so no lower version can build. 1.94 was not installed to show that failure: it follows from the declared rust-version. 1.95 builds and passes every test.

- `[workspace.package] rust-version = "1.95"`, with `rust-version.workspace = true` in the 9 crates.
- CI job `Rust (minimum version)`: reads the version from Cargo.toml (single source), installs it, sets `RUSTUP_TOOLCHAIN` (which overrides rust-toolchain.toml), downloads frontend-dist, runs build and test with `--all-features`. Not run in CI yet.
- Guide section "Minimum Rust version" with the policy for raising it.

## Blockers / Challenges

None.

## Commits

- e5cb117 ci: declare and test the minimum Rust version, 1.95 (ticket 02)

## Time Spent

4m (ticket-start.js to ticket-complete.js)
