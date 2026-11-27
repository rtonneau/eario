# Security of the local server

An Eario application is a web server on the user's own machine. That is
convenient and also a known source of vulnerabilities: any web page the user
visits can try to send requests to `127.0.0.1`. This document states what
Eario defends against and how. The code is in
`crates/eario-runtime/src/http.rs`, `server.rs` and `assets.rs`; the tests
are in `http_tests.rs`. The hub, which starts dashboards and installs them
from GitHub releases, is in `crates/eario-hub/src/`: `supervisor.rs` and
`http.rs` (services and routes), `remote.rs` (staging, install, update),
`self_update.rs` (the hub's own update), `verify.rs` (signatures and keys)
and `github.rs` (the only network code), tested in
`crates/eario-hub/tests/hub.rs`.

## Threat model

Protected asset: the data the user loaded, and control of the application.

| Threat | Defence |
| --- | --- |
| Another machine on the network connects to the server | The listener binds `127.0.0.1` only. There is no option to bind elsewhere. |
| A malicious web page sends requests to the server (cross-site request forgery) | Every API route requires a session token the page cannot know. Requests carrying a foreign `Origin` header are refused. |
| A malicious page reads responses through DNS rebinding | Requests whose `Host` header is not `127.0.0.1:<port>` or `localhost:<port>` are refused. |
| The dashboard is framed by another page (clickjacking) | `X-Frame-Options: DENY` and `frame-ancestors 'none'`. |
| Injected content loads remote code or exfiltrates data | A Content Security Policy restricts scripts, styles, images and connections to the application's own origin. Nothing is loaded from a remote host. |
| A crafted path reads files from disk | Static files are looked up by key in the embedded file table; the file system is never consulted. Uploaded file names are reduced to their last segment and are only used for display and format detection. |
| A huge upload exhausts memory | Uploads are limited (256 MiB by default, configurable); JSON bodies to 2 MiB. |
| A request runs a program or script | No endpoint or MCP tool executes commands or code supplied at run time. The hub starting an installed dashboard, running a verified release for its manifest, and running and starting its own verified new version, is the one bounded exception, below. |
| A request makes the hub start or install a program of its choice | Hub routes take only path segments, each validated strictly: a slug, an `owner/repo`, a SHA-256, `on` or `off`; never a URL, a path, an executable or arguments, and they read no body or query. The hub starts only its own copy in `bin/<slug>/`, with `--eario-child <run file>` and nothing else, and installs only a signed GitHub release. |
| A spoofed or compromised release ships other code | The release's `SHA256SUMS.txt` must carry a valid minisign signature (`SHA256SUMS.txt.minisig`; a release without one is refused before its asset is downloaded), and the asset's SHA-256 must be the signed one. Nothing enters `bin/<slug>/` before every check has passed. A signature proves who published, not that the code is safe: the user decides whom to trust. |
| Someone substitutes their own signing key | A key is trusted only when it is pinned for the installed repository, listed by a catalog for it, or accepted by the user by the SHA-256 of the whole key line (not the copyable minisign key id). A different key is `changed` and refused until accepted. A release signed by one key but carrying another's `minisign.pub` fails its signature. |
| A release under a key not trusted yet runs on the machine | Until its key is trusted, the downloaded file is never run, not even for its manifest; the preview shows no name or version. Accepting the key hashes the staged file again before running it with `--eario-manifest` alone (no input, bounded output, a timeout). |
| An old, vulnerable release is served again (downgrade, replay) | An update installs only a strictly newer manifest version (SemVer); an older or equal tag is refused before the download, and the signed executable's own version after it. Versions that cannot be compared are refused. Only a rollback, to the one previous version kept, installs an older version. The hub's own update takes only a version strictly newer than its own, as its signed executable prints it with `--version`. |
| A spoofed or compromised release replaces the hub itself | The hub's own releases are verified with the Eario release key compiled into it (`crates/eario-hub/eario-release.pub`), its only trust anchor for itself: never a key the release carries, a catalog's, or one trusted on first use, and no route or command takes another. A build whose key file is the placeholder does not update itself (`self_update_not_configured`). The asset must also have the signed SHA-256 before it is run, with `--version` alone. |
| A failed self-update leaves no working hub | The verified file is copied beside the running executable and hashed again. On Unix the executable gets a second name, `<name>.old` (a hard link), and the copy is renamed over it, atomically. On Windows, and on a Unix file system without hard links, two renames swap it in; if the second fails, the first is undone, and only a crash or power loss between the two leaves `<name>.old` and `<name>.new` for the user to rename. On any failure the copy is removed. The replaced executable is kept as `<name>.old` until the next start. A hub that cannot write beside its executable refuses before downloading. One self-update runs at a time, to its end even if its request is dropped. If the new hub cannot be started (tried again for about 10 s while the system reports the file busy), the old one records it in `self-update.log` in the hub directory before it exits: the new version is in place and starts when run again. |
| A redirect leads the download to another host | Every URL, the first and each redirect, must be HTTPS on the default port to one of `api.github.com`, `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`; each redirect is checked before it is followed, at most 5. |
| An oversized or endless download fills the disk or memory | Reads are capped while they stream, whatever the server declares: 2 MiB for an API answer or a catalog, the size the release lists for an asset and at most 512 MiB. Requests time out (30 s between reads, 60 s for a small file whole). |
| A malicious page drives the install routes | The routes sit behind the token and Origin check. An install needs two requests, the preview and then the install repeating the previewed SHA-256, and a third, with the key's SHA-256, for a key not trusted yet. The one-step update route skips the SHA-256 confirmation only for an installed dashboard signed by its pinned key or a key already accepted by its SHA-256, never a changed key not yet accepted, and whose new version is strictly newer. |
| A stale or swapped preview is installed | An install or update takes only the preview staged for that repository with that SHA-256 (`hub_no_preview`, `hub_preview_mismatch`); the moved file is hashed again, and a file gone from `staging/` is `hub_preview_gone`. A preview is used up by its install, and an update refuses a dashboard that changed since its preview. |
| The hub's copy of a dashboard is replaced on disk | Size and SHA-256 are checked against the registry before every start; a mismatch is refused and the user is told to reinstall. |
| A stale run file, left by a dashboard that crashed, misleads the hub | A run file counts only while the server at its loopback address answers `GET /api/app` with its token. Stale ones are deleted when the hub starts and whenever it lists, starts, stops or uninstalls that dashboard, and only while they still hold the token that failed to answer. |
| A page obtains a dashboard's token through the hub | The open route is behind the hub's own token and Origin check, and returns only loopback addresses. |
| An AI agent reads or changes more than intended | The MCP endpoint is off unless the developer enables it, requires the session token, and offers only read-only, bounded tools. |

Not defended against: a program already running as the same user. It can
read the same files as the application and its memory; no local web server
can prevent that.

## The session token

At start-up the server generates 32 random bytes from the operating system
and prints an address of the form `http://127.0.0.1:PORT/#token=...`.

- The token is in the URL *fragment*. Browsers do not send fragments to
  servers and do not include them in `Referer` headers.
- The frontend reads the token, keeps it in `sessionStorage` for the tab, and
  removes it from the address bar.
- API requests carry it in the `x-eario-token` header (or as a bearer token,
  for MCP clients). Comparison is constant-time.
- A page from another origin cannot read the token and cannot set a custom
  header on a cross-origin request without a CORS preflight, which the server
  never approves (it sends no CORS headers).

Consequence: opening `http://127.0.0.1:PORT/` by hand in another tab shows a
message asking for the full address. This is deliberate.

The token is valid for the lifetime of the process. Each start generates a
new one, and usually a new port.

## Session isolation

One process serves one session. Several tabs holding the token share it. There
is no multi-user mode and no authentication beyond the token; both are
non-goals for now.

## Shutdown

`POST /api/shutdown` (behind the token) or Ctrl+C triggers a graceful
shutdown: the server stops accepting connections and lets requests in flight
finish, for at most five seconds.

## The hub's exception: starting and installing dashboards

The hub (`eario-hub`, ADR 0011) starts processes, which no other endpoint
may do, and installs and updates dashboards from signed GitHub releases
(ADR 0013), the only network access of Eario. The exception is bounded:

- Registry entries are added or changed in two ways only. By
  `--eario-install`, run by the user from the dashboard's own executable in
  a terminal (or with `--yes`), which copies the executable into
  `bin/<slug>/` of the hub directory and records the copy's size and
  SHA-256; it never replaces an entry installed from GitHub. Or by the
  install, update and rollback routes and commands below, from a verified
  GitHub release. Entries are removed by `--eario-uninstall` or by the hub's
  uninstall route, which can only remove an entry and the hub's own
  `bin/<slug>/` directory, never a path read from the registry. Besides
  these, the API deletes only run files in `run/` whose dashboard no longer
  answers with their token.
- Every hub route takes its parameters as path segments and refuses with
  400 anything else: a slug that `eario_core::is_slug` rejects
  (`hub_invalid_slug`), an `{owner}/{repo}` that `eario_core::is_repo`
  rejects (`github_invalid_repo`; a segment that decodes to `/` or `\`
  fails), a `{sha256}` or `{key_sha256}` that is not 64 lower-case
  hexadecimal digits (`hub_invalid_sha256`), a switch that is not `on` or
  `off` (`hub_invalid_switch`). So no parameter can name a file, a URL or a
  host. No hub route reads a request body or a query string. The registry is
  read again on each request.
- Before each start the hub checks that the registry entry names a file
  directly in its own `bin/<slug>/` directory, that its size and SHA-256 are
  those recorded, and that its manifest's launch protocol version is the
  hub's. Otherwise it refuses, and the message says how to reinstall.
- The process gets exactly `--eario-child <hub>/run/<slug>.json`, the hub
  directory in `EARIO_HUB_DIR`, no standard input or output, and no console
  window on Windows (`CREATE_NO_WINDOW`, own process group) or its own
  process group on Unix. A slug that answers already is not started again.
- Stopping a dashboard that answers sends `POST /api/shutdown` with its
  token. The hub kills only processes it started itself: at once when a
  process cannot be asked (its start is in progress and a stop cancels it,
  it did not answer within the start timeout, or it no longer answers when
  stopped, started again or uninstalled), and otherwise when it has not exited once the
  stop timeout has passed. The hub never signals a process by its id.
- The hub binds `127.0.0.1`, sits behind the same Host/Origin check and
  session token as a dashboard, and records its address and token in
  `hub.json` so one hub runs per user; a second start asks the recorded
  hub's `GET /api/alive`, which answers at once. The open route returns a dashboard's
  address with its token in the fragment, only to a caller holding the
  hub's token.

### Installs and updates from GitHub

The routes, relative to `/api` (the module documentation of
`crates/eario-hub/src/http.rs` gives each answer):

- `GET /catalogs`, `POST /catalogs/{owner}/{repo}/add` and `/remove`: the
  catalogs the hub reads (`catalogs.json`; `rtonneau/eario-catalog` until
  removed). A repository is added only once its `catalog.json` was read.
- `POST /sources/{owner}/{repo}/preview`: downloads the latest release's
  asset for this machine into `staging/` and verifies it; installs nothing.
- `POST /sources/{owner}/{repo}/accept-key/{key_sha256}`: accepts the
  previewed key, which must have that SHA-256, then reads the manifest.
- `POST /sources/{owner}/{repo}/install/{sha256}`: installs the preview of
  that SHA-256.
- `POST /check`: reads the latest release of each dashboard installed from
  GitHub; downloads nothing.
- `POST /dashboards/{slug}/update-preview`, `/update/{sha256}`, `/update`
  (both steps at once, refused when the key changed) and `/rollback`.
- `GET /settings`, `POST /settings/check-at-start/{on|off}`.
- `POST /self-update`: replaces the hub's executable with its verified
  newer release, answers, then restarts the hub.

The command line (`eario-hub install`, `update`, `rollback`, `check`,
`catalog`, `check-at-start`, `self-update`) calls the same services after a
terminal confirmation; `--yes` confirms an install but never accepts a key,
which takes a terminal answer or `--accept-key <key sha256>`.
`eario-hub self-update` refuses while a hub runs.

**Trust order.** A release is verified with the key it carries as
`minisign.pub`, or, without that file, the key the hub trusts for the
repository. The key is trusted when it equals the key pinned in the registry
(an installed repository; the pin decides), else the key of the first
catalog, in the user's order, that lists the repository, else when the user
accepted it by its SHA-256. A key that differs from the pinned or catalog
key is `changed`; one that nothing vouches for is `unknown`; both are
refused until accepted. A repository that no catalog lists must attach
`minisign.pub`. An install or update pins the key it was verified with. The
checks run in this order: signature of `SHA256SUMS.txt`, SHA-256 of the
downloaded asset, trusted key, and only then the manifest (`--eario-manifest`
alone, no standard input, output bounded to 64 KiB, killed after 20 s, no
console window): its slug must be the asset name's prefix (or the installed
slug), its launch protocol the hub's, and for an update its version strictly
newer. A slug installed from another source is refused.

**The hub's own update.** One key verifies a release of the hub: the
minisign public key file `crates/eario-hub/eario-release.pub`, compiled
into it. Nothing else is trusted for the hub, not even on first use: the
release's `minisign.pub` and the catalogs are ignored, and no route or
command can give another key (tests substitute one only through the
`fake-github` feature, which a release build never enables). While the file
is the placeholder, self-update is refused and the check reports
`self_update_not_configured` without a request. The latest release of
`rtonneau/eario` must have a newer tag, then the signature of
`SHA256SUMS.txt` must verify and list `eario-hub-<target triple>[.exe]`,
whose download, into `staging/`, must have the signed SHA-256. Only then
is it run, with `--version` alone (no standard input, output bounded to
64 KiB, killed after 20 s, no console window), and the version it prints
must be strictly newer than the running hub's. It is then copied beside
the running executable as `<name>.new`, hashed again, and swapped in,
keeping the running executable as `<name>.old`: on Unix a hard link to it,
then one rename of the new file over it; on Windows (or without hard
links) two renames, the first undone if the second fails. The update runs
to its end and asks for the restart itself, even when its request is
dropped, and a second one waits for it. The hub answers, stops its server,
removes `hub.json`, starts the new executable with no arguments, no
standard streams, no console window and a process group of its own, and
exits; the dashboards, separate processes, keep running. A new executable
that cannot be started is recorded in `self-update.log` in the hub
directory, since a restarted hub has no terminal. The next start removes
`<name>.old`. A build without the release key lists no newer version of
the hub, whatever an update check stored.

**Outbound network.** Only `eario-hub` makes outbound requests, and only to
GitHub: HTTPS on the default port to `api.github.com`, `github.com`,
`objects.githubusercontent.com` and `release-assets.githubusercontent.com`,
each redirect checked before it is followed (at most 5), with the system's
certificate store. Reads are bounded while they stream (2 MiB for JSON, the
listed size and at most 512 MiB for an asset) and time out (30 s between
reads, 60 s for a small file whole). Requests are made at the hub's start
(for the hub's own releases, `rtonneau/eario`, only once it holds the
release key), unless `check_updates_at_start` is off, and on a user action; a failure is
reported ("could not check"), never fatal. Dashboards make no request. Debug
builds can replace GitHub by a server on loopback with
`EARIO_HUB_GITHUB_API`, for tests; release builds ignore it.

**Lock and staging.** Every read-modify-write of the registry, the catalogs
or the settings, and every move into `bin/`, runs under the lock file
`hub.lock` of the hub directory, shared by the hub, the `eario-hub` command
line and `--eario-install`; it is never held while waiting for the user or
for GitHub. Each process stages its downloads in `staging/<pid>-<n>/`,
locked while the process lives; a hub that starts removes only the areas no
one holds. At most 4 previews wait at once, one per repository. A verified
file is moved into `bin/<slug>/` under a name of its own with one rename,
hashed again, then recorded with an atomic write. An update keeps the
version it replaced in `bin/<slug>/` and removes the older one; a rollback
checks the kept copy's size and SHA-256, and refuses a `previous` name that
is not a plain asset name, before restoring it.

Run files, `hub.json` and the registry are in the user's configuration
directory and hold session tokens. On Unix the hub directory and its
subdirectories are created `0700`, and these files and the logs `0600`;
directories that exist already keep their mode. A program running as the same user can
read or change them, as it can read the application's memory; that is
outside this threat model. The SHA-256 check detects a copy changed after
install; it does not protect against such a program, which could also
rewrite the registry.

## Development mode

When the frontend is served by the Vite development server, requests come
from another origin. Debug builds accept one additional origin given in
`EARIO_DEV_ORIGIN`. Release builds ignore the variable.

## Checklist for changes

- New API routes go through `http::router`, inside the token layer.
- Never derive a file-system path from request data.
- Never add an endpoint that runs a command or evaluates code. The hub's
  routes are the one exception (ADR 0011, ADR 0013): keep their parameters
  to strictly validated path segments, keep installs to signed GitHub
  releases, and do not add a path, a URL, an executable or arguments
  without an ADR.
- The hub's own update trusts the compiled Eario release key alone. Never
  read a key for it from a release, a catalog, a request or the
  environment.
- Requests to a non-loopback host are made only by `eario-hub`'s `github.rs`, to the
  GitHub hosts above. Another host, or a request from another crate, needs
  an ADR.
- A new MCP tool that mutates state or is expensive needs the permission
  design of [ADR 0009](../adr/0009-mcp-integration.md) first.
- Loading any remote resource from the frontend requires an ADR; the default
  answer is no.
