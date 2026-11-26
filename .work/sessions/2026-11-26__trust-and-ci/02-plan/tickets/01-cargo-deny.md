# Ticket 01: cargo-deny

**Model:** fast
**Effort:** low

**Acceptance Criteria:**
- [ ] `deny.toml` at the root: a licence allow-list of today's licences, plus MPL-2.0; vulnerabilities are errors; unmaintained and yanked crates are warnings; only the crates.io source is allowed; duplicate versions are warnings.
- [ ] `cargo deny check` passes locally.
- [ ] New CI job `Dependencies (cargo deny)` uses `EmbarkStudios/cargo-deny-action`.
- [ ] The development guide documents `cargo deny check` and the licence policy.

**Files to Touch:**
- `deny.toml`
- `.github/workflows/ci.yml`
- `docs/development/guide.md`

**Verification Step:**

Run:
```bash
cargo deny check
```

Expected:
`advisories ok, bans ok, licenses ok, sources ok`

**Notes:**

Workspace crates are `MIT OR Apache-2.0`; set `private.ignore = true` or a `[licenses.private]` entry so unpublished examples and xtask do not need a licence check. Justify the action in the PR.
