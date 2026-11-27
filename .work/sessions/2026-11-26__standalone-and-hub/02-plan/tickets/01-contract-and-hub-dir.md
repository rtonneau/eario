# Ticket 01: contract-and-hub-dir

**Model:** medium
**Effort:** medium

**Acceptance Criteria:**
- [ ] `eario-core` exposes `LAUNCH_PROTOCOL_VERSION` (1), `Manifest` (name, slug, version: Option, eario_version, protocol, description), `RunFile` (protocol, pid, address, token, started_at) and the hub API payloads (dashboard entry with state `starting | running | stopped | failed` and last log lines), all serde + ts-rs, documented.
- [ ] `eario-runtime` has a hub-directory module: resolves `<config dir>/eario/hub/` or `EARIO_HUB_DIR`; reads and atomically writes `registry.json` (with a format version; an unknown version is an error naming the file); reads, writes atomically and removes run files; computes SHA-256 and size of a file; probes liveness (`GET /api/app` with the token at a loopback address, bounded timeout).
- [ ] The title slug function is shared (used by preferences and the manifest) and keeps its current results.
- [ ] Errors are `thiserror` variants with a stable `code()` naming the file.
- [ ] `cargo xtask gen-types` run and bindings committed; `npm run check` clean.

**Files to Touch:**
- `crates/eario-core/src/lib.rs`
- `crates/eario-core/src/launch.rs`
- `crates/eario-runtime/src/hub_dir.rs`
- `crates/eario-runtime/src/preferences.rs`
- `crates/eario-runtime/src/lib.rs`
- `crates/eario-runtime/src/error.rs`
- `crates/eario-runtime/Cargo.toml`
- `Cargo.toml`
- `frontend/src/lib/generated/`

**Verification Step:**

Run:
```bash
cargo test -p eario-core -p eario-runtime && cargo xtask gen-types && git status --short frontend/src/lib/generated
```

Expected:
All tests pass, including round-trip tests of the registry and run file, an atomic-write test, an unknown-format-version test and a probe test against a real `start()`ed server (live with the right token, not live with a wrong token or a closed port); the generated bindings are committed (no diff after regeneration).

**Notes:**

Registry and run files are not frontend contract types; keep them in `eario-runtime` unless the hub API exposes them. The probe needs a small HTTP client: prefer hyper's client over the `hyper`/`hyper-util` already in the tree, or a minimal request over `tokio::net::TcpStream`, over adding reqwest; justify the choice. Use `sha2` from the workspace (already in the dependency tree via rust-embed). Atomic write: write a temp file in the same directory, then rename.
