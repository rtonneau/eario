# Ticket 02: release-verification

**Model:** heavy
**Effort:** xhigh

**Acceptance Criteria:**
- [ ] `SHA256SUMS.txt` parser accepts `<64 hex>  <name>` lines (and `*name`), rejects duplicates and malformed lines with coded errors.
- [ ] Signature verification of the SUMS file with a minisign public key; wrong key, tampered file and missing signature are refused, each with a test.
- [ ] Key fingerprint (minisign key id) for display; key comparison decides `pinned_matches`, `new_key` (first use) or `key_changed`.
- [ ] `asset_name(slug, target)` gives `<slug>-<triple>[.exe]` for the four supported triples, and the hub's own triple is known at compile time.

**Files to Touch:**
- `crates/eario-hub/src/verify.rs`
- `crates/eario-hub/Cargo.toml`
- `Cargo.toml`

**Verification Step:**

Run:
```bash
cargo test -p eario-hub verify
```

Expected:
All verification tests pass.

**Notes:**

Candidate: `minisign-verify` (MIT, no dependencies) for verifying. Test vectors signed with `minisign` as a dev-dependency, or generated once and committed as synthetic fixtures. Use the `sha2` crate already in the workspace if present.
