# Ticket 03: github-client

**Model:** heavy
**Effort:** high

**Acceptance Criteria:**
- [ ] `GitHub` trait: latest release (non-draft, non-prerelease) with asset names, sizes and URLs; fetch `catalog.json` of a repo; download an asset to a temporary file.
- [ ] Real client over HTTPS (rustls) refuses non-HTTPS URLs and hosts outside `api.github.com`, `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`, including after redirects; caps downloads (512 MiB) and JSON (2 MiB); sets timeouts.
- [ ] A fake GitHub server on loopback, used by tests, serves releases, catalogs and assets; the base URL override `EARIO_HUB_GITHUB_API` is honoured only in debug builds and only for loopback addresses.
- [ ] Rate-limit, 404 and offline errors map to coded `HubError`s naming the repository.
- [ ] `cargo deny check` passes and the MSRV 1.95 build works.

**Files to Touch:**
- `crates/eario-hub/src/github.rs`
- `crates/eario-hub/src/fake_github.rs` (test support)
- `crates/eario-hub/Cargo.toml`
- `Cargo.toml`
- `deny.toml`

**Verification Step:**

Run:
```bash
cargo test -p eario-hub github && cargo deny check
```

Expected:
Tests pass against the fake server; cargo-deny reports no error.

**Notes:**

Candidate: `ureq` 3 with rustls (blocking; call through `spawn_blocking`). Record the justification of each crate for the PR. No test may reach the real internet.
