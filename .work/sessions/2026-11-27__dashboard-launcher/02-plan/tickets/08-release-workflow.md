# Ticket 08: release-workflow

**Model:** medium
**Effort:** medium

**Acceptance Criteria:**
- [ ] `.github/workflows/release-dashboard.yml` (`workflow_call`): inputs package, slug, targets; builds Windows x86_64, Linux x86_64, macOS aarch64 and x86_64; names assets `<slug>-<triple>[.exe]`; writes `SHA256SUMS.txt`; signs it with minisign from a secret; attaches all to the release.
- [ ] `release.yml` uses it for data-explorer, signal-lab and eario-hub.
- [ ] An `xtask` or documented command generates a key pair locally; nothing secret is committed.

**Files to Touch:**
- `.github/workflows/release-dashboard.yml`
- `.github/workflows/release.yml`
- `xtask/src/main.rs`

**Verification Step:**

Run:
```bash
cargo xtask check
```

Expected:
Passes; workflow YAML reviewed (actionlint if installed). A real run needs the user's secrets and a tag.

**Notes:**

The workflow cannot be run end to end in this session; state that in the summary.
