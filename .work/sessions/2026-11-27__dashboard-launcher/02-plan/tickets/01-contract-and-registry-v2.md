# Ticket 01: contract-and-registry-v2

**Model:** medium
**Effort:** high

**Acceptance Criteria:**
- [ ] `RegistryEntry` gains `source` (`local` or `github { repo }`), `pinned_key` (optional) and `previous` (optional size, sha256, manifest, file name); `REGISTRY_FORMAT_VERSION` is 2.
- [ ] A format-1 `registry.json` reads as local entries with no key and no previous version; an unknown future format is refused with a coded error.
- [ ] `eario-core` gets `is_repo` (strict `owner/repo`: GitHub name characters, lengths), `Catalog`/`CatalogEntry`, the update fields on `HubDashboard` (source, available version, has previous) and an `InstallPreview` payload (repo, version, asset, size, sha256, key fingerprint, key status).
- [ ] `cargo xtask gen-types` output committed; `npm run check` passes.

**Files to Touch:**
- `crates/eario-core/src/launch.rs`
- `crates/eario-runtime/src/hub_dir.rs`
- `crates/eario/src/launch.rs`
- `frontend/src/lib/generated/`

**Verification Step:**

Run:
```bash
cargo test -p eario-core -p eario-runtime -p eario && cargo xtask gen-types && git diff --exit-code frontend/src/lib/generated
```

Expected:
All tests pass, including a format-1 migration test and `is_repo` cases; no diff after regenerating.

**Notes:**

Keep `eario-core` serde-only. `--eario-install` writes `source: local`. Repository names: `owner` 1–39 chars `[A-Za-z0-9-]`, `repo` 1–100 chars `[A-Za-z0-9._-]`, not `.` or `..`.
