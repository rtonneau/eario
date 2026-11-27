# Install and update dashboards from GitHub releases

_ADR 0013, recorded 2026-11-27 (session 2026-11-27__dashboard-launcher)._

<!-- Format adapted from mattpocock-skills' domain-modeling (ADR-FORMAT.md): https://github.com/mattpocock/skills.git -->

Status: Proposed. Amends ADR 0011 (install outside the HTTP API, in-place updates, no side-by-side copies) and the local-first rule of AGENTS.md, for the hub only.

Users should be able to install and update dashboards that live in other GitHub repositories from the hub, as a launcher does. So the hub, and only the hub, may contact GitHub over HTTPS: at every start (a setting turns this off) and on user action. It installs only signed release assets: an executable per target triple, listed in a minisign-signed `SHA256SUMS.txt` and verified against a pinned key, taken from a catalog entry or trusted on first use after the user confirms its fingerprint. Install and update may be started from the hub interface (routes take an `owner/repo` or a slug, never a URL or path) or from its command line. One previous version is kept for rollback, and the hub updates itself the same way. Dashboards themselves stay offline. Rejected: unsigned checksums only, because a compromised repository could then ship any code; and building from source, because the end user has no toolchain (ADR 0010).
