---
bump: minor
floor: minor
---
- The hub installs dashboards from signed GitHub releases: from a catalog or by `owner/repo`, with a preview showing the release, its SHA-256 and the signing key, and a confirmation.
- Releases must carry a minisign-signed `SHA256SUMS.txt`; the hub pins each repository's key, refuses a changed key until the user accepts it by its SHA-256, and never runs a file signed by a key it does not trust.
- The hub checks for updates at start (a setting turns this off) and on "Check now"; Update installs only a strictly newer version and keeps the previous one for a single Rollback.
- Catalogs (`catalog.json` in a GitHub repository) list installable dashboards; the default is `rtonneau/eario-catalog`. One repository can publish several dashboards, installed by slug.
- The hub updates itself from releases signed with the Eario release key ("Update and restart"); running dashboards keep running.
- New `eario-hub` commands: `catalog`, `install`, `check`, `update`, `rollback`, `check-at-start` and `self-update`.
- `--eario-install` refuses to replace a dashboard installed from GitHub.
- A reusable GitHub Actions workflow builds, signs and publishes dashboard releases for Windows, Linux and macOS; see `docs/development/publishing-a-dashboard.md`.
