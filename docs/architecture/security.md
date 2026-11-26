# Security of the local server

An Eario application is a web server on the user's own machine. That is
convenient and also a known source of vulnerabilities: any web page the user
visits can try to send requests to `127.0.0.1`. This document states what
Eario defends against and how. The code is in
`crates/eario-runtime/src/http.rs`, `server.rs` and `assets.rs`; the tests
are in `http_tests.rs`.

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
| A request runs a program or script | No endpoint or MCP tool executes commands or code supplied at run time. |
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

## Development mode

When the frontend is served by the Vite development server, requests come
from another origin. Debug builds accept one additional origin given in
`EARIO_DEV_ORIGIN`. Release builds ignore the variable.

## Checklist for changes

- New API routes go through `http::router`, inside the token layer.
- Never derive a file-system path from request data.
- Never add an endpoint that runs a command or evaluates code.
- A new MCP tool that mutates state or is expensive needs the permission
  design of [ADR 0009](../adr/0009-mcp-integration.md) first.
- Loading any remote resource from the frontend requires an ADR; the default
  answer is no.
