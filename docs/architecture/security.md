# Security of the local server

An Eario application is a web server on the user's own machine. That is
convenient and also a known source of vulnerabilities: any web page the user
visits can try to send requests to `127.0.0.1`. This document states what
Eario defends against and how. The code is in
`crates/eario-runtime/src/http.rs`, `server.rs` and `assets.rs`; the tests
are in `http_tests.rs`. The hub, which starts dashboards, is in
`crates/eario-hub/src/supervisor.rs` and `http.rs`, tested in
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
| A request runs a program or script | No endpoint or MCP tool executes commands or code supplied at run time. The hub starting an installed dashboard is the one bounded exception, below. |
| A request makes the hub start a program of its choice | Hub routes take a slug only, never a path, an executable or arguments, and read no body or query. The hub starts only its own copy in `bin/<slug>/`, with `--eario-child <run file>` and nothing else. |
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

## The hub's process-start exception

The hub (`eario-hub`, ADR 0011) starts processes, which no other endpoint
may do. The exception is bounded:

- Registry entries are added or changed only outside the HTTP API: by
  `--eario-install`, run by the user from the dashboard's own executable in
  a terminal (or with `--yes`), which copies the executable into
  `bin/<slug>/` of the hub directory and records the copy's size and
  SHA-256. Entries are removed by `--eario-uninstall` or by the hub's
  uninstall route, which can only remove an entry and the hub's own
  `bin/<slug>/` directory, never a path read from the registry. Besides
  these, the API deletes only run files in `run/` whose dashboard no longer
  answers with their token.
- Every hub route that acts on a dashboard takes one path segment, the slug,
  and refuses with 400 anything that `eario_core::is_slug` rejects, so a
  slug cannot name a file. No hub route reads a request body or a query
  string. The registry is read again on each request.
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
  start route is the one exception: keep it to a slug, and do not add a
  path, an executable, arguments or an install route without an ADR.
- A new MCP tool that mutates state or is expensive needs the permission
  design of [ADR 0009](../adr/0009-mcp-integration.md) first.
- Loading any remote resource from the frontend requires an ADR; the default
  answer is no.
