# Install and update dashboards from GitHub releases

_ADR 0013, recorded 2026-11-27 (session 2026-11-27__dashboard-launcher)._

<!-- Format adapted from mattpocock-skills' domain-modeling (ADR-FORMAT.md): https://github.com/mattpocock/skills.git -->

Status: Accepted (27 November 2026). Amends ADR 0011 (install outside the HTTP API, in-place updates, no side-by-side copies) and the local-first rule of AGENTS.md, for the hub only.

Users should be able to install and update dashboards that live in other GitHub repositories from the hub, as a launcher does. So the hub, and only the hub, may contact GitHub over HTTPS: at every start (a setting turns this off) and on user action. Dashboards themselves stay offline. Rejected: unsigned checksums only, because a compromised repository could then ship any code; and building from source, because the end user has no toolchain (ADR 0010).

## Decisions, as built

- **Signed checksums.** A release is installed only from its assets: an executable `<slug>-<target triple>[.exe]` per target, listed in `SHA256SUMS.txt`, whose minisign signature `SHA256SUMS.txt.minisig` must verify. A release without the signature is refused before its asset is downloaded. The downloaded asset's SHA-256 must be the one the signed list gives.
- **Which key.** The key is the one the release carries as `minisign.pub`, or, without that file, the key the hub already trusts for the repository. Its status, shown in the preview, compares it with the key pinned in the registry (for an installed repository), else with the key of the first catalog, in the user's order, that lists the repository: `pinned` or `catalog` when it matches, `changed` when it differs, `unknown` when nothing vouches for it. A repository that no catalog lists must attach `minisign.pub` to its releases. An install or update pins the key it was verified with.
- **Keys are confirmed by their full SHA-256.** The user accepts an `unknown` or `changed` key by the SHA-256 of its whole key line, never by the minisign key id (8 bytes anyone can copy into another key). A changed key is refused until accepted.
- **A key not trusted never runs the executable.** Only once the signature, the hash and a trusted key agree does the hub run the downloaded file, with `--eario-manifest` alone (no standard input, bounded output, a timeout), and check its slug and launch protocol. With an `unknown` or `changed` key the preview has no manifest; accepting the key hashes the staged file again, then runs it.
- **Two steps.** A preview downloads and verifies into `staging/`, a directory of this process held by a lock; nothing enters `bin/<slug>/` until the user confirms the preview by its SHA-256. One exception: the one-step update (`POST /dashboards/{slug}/update`, `eario-hub update --yes`) applies without that confirmation, and only to an installed dashboard signed by its pinned key (or a key the user already accepted by its SHA-256) and whose new manifest version is strictly newer; a changed key not yet accepted is refused with `hub_key_changed`. The file is then moved into `bin/<slug>/`, hashed again and recorded, under the hub directory's lock. A slug that another source installed is refused.
- **No downgrade.** An update installs a release only when its executable's manifest version is strictly newer than the installed one, by SemVer precedence. The unsigned tag only lets the hub refuse an older release before downloading it. A version that is missing or not SemVer cannot be compared, so the update is refused.
- **One previous version.** An update keeps the version it replaces in `bin/<slug>/` and removes the older one; a rollback restores it once, after checking its size and hash. Update and rollback are refused while the dashboard runs or starts.
- **Outbound network, the hub only.** Every request is HTTPS to `api.github.com`, `github.com`, `objects.githubusercontent.com` or `release-assets.githubusercontent.com` on the default port, each redirect checked before it is followed (at most 5). Reads are capped while they stream: 2 MiB for an API answer or a catalog, the listed size (at most 512 MiB) for an asset. Requests time out (30 s between reads, 60 s for a small file whole). A debug build can be pointed at a fake GitHub on loopback (`EARIO_HUB_GITHUB_API`); release builds ignore it.
- **Check at start, with an opt-out.** The hub checks the latest release of each dashboard installed from GitHub when it starts, in the background, unless `check_updates_at_start` is off, and on "Check now". A check downloads nothing. A repository that cannot be read is reported as "could not check"; it never stops the hub.
- **Routes take path segments only.** The install, update, rollback, catalog and setting routes read no body and no query. Each parameter is one path segment, validated strictly: `{owner}/{repo}` a GitHub repository, `{slug}` a slug, `{sha256}` and `{key_sha256}` 64 lower-case hexadecimal digits, `on` or `off`. They sit behind the Host/Origin check and the session token. No route takes a URL, a path, an executable or arguments. The command line (`eario-hub install`, `update`, `rollback`, `check`, `catalog`, `check-at-start`) offers the same, with a terminal confirmation; `--yes` never accepts a key.
- **Self-update** of the hub, from a release signed with the Eario key compiled into it, is to follow the same verification. It is not built yet.

## Consequences

- The hub can now run code that was not on the user's machine before: published under a key the user trusted. A signature proves who published, not that the code is safe.
- The hub keeps more state: `catalogs.json`, `settings.json`, `updates.json`, `staging/`, a pinned key and a previous version per GitHub entry.
- Releases must follow the asset convention and be signed. Publishers that no catalog lists also attach `minisign.pub`.
- A key accepted but not installed is remembered by the running hub only; a pin is removed with its entry at uninstall, so a reinstall trusts on first use again.
- Unauthenticated GitHub allows about 60 API requests per hour, enough for a check at start; private repositories are not supported.
