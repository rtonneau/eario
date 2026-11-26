# Ticket 02: msrv

**Model:** medium
**Effort:** medium

**Acceptance Criteria:**
- [ ] The lowest toolchain, from 1.95 up, on which `cargo build --workspace --all-features` and `cargo test --workspace --all-features` pass is found by trying the versions in turn.
- [ ] `rust-version` is declared in `[workspace.package]` and inherited (`rust-version.workspace = true`) by every crate.
- [ ] New CI job `Rust (minimum version)` on ubuntu installs exactly that toolchain and runs the build and tests, overriding `rust-toolchain.toml`.
- [ ] The development guide states the minimum Rust version and the policy for raising it.

**Files to Touch:**
- `Cargo.toml`
- `crates/*/Cargo.toml`, `examples/*/Cargo.toml`, `xtask/Cargo.toml`
- `.github/workflows/ci.yml`
- `docs/development/guide.md`

**Verification Step:**

Run:
```bash
cargo +<msrv> test --workspace --all-features
```

Expected:
All tests pass, and the same command fails or does not compile with the minor version below it (or that version is below the 1.95 floor of the dependencies).

**Notes:**

The job needs the frontend-dist artifact, like the Rust job. Use `RUSTUP_TOOLCHAIN` or `cargo +x.y` so the toolchain file does not override it. Clippy is not run at the minimum version.
