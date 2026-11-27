# Ticket 03: github-client

**Status:** ✅ Done

## Local Test Result

Evidence in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/` (raw output, one file per command, prefix `03-`).

- Verification Step, `cargo test -p eario-hub github`: 23 passed, 0 failed, exit 0 (`03-github-tests.txt`). Every test runs against the loopback fake server or makes no request at all; none reaches the internet.
- Verification Step, `cargo deny check` (cargo-deny 0.22.1): `advisories ok, bans ok, licenses ok, sources ok`, exit 0 (`03-deny.txt`). The only warnings are `duplicate` ones (base64, constant_time_eq, foldhash, getrandom, hashbrown, quick-xml, syn, windows-sys), and all of them were already there: `base64` 0.22/0.23 were both in the `HEAD` lock file.
- `cargo fmt --all -- --check`: exit 0 (`03-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: exit 0 (`03-clippy.txt`).
- `cargo test -p eario-hub` (whole crate): 47 unit tests and 8 integration tests passed, exit 0 (`03-hub-tests.txt`).
- `cargo doc -p eario-hub --no-deps`, with and without `--all-features`: exit 0 both times, no rustdoc warning (`03-doc.txt`).
- MSRV: I installed the Rust 1.95.0 toolchain (`rustup toolchain install 1.95.0 --profile minimal`). In a scratch target dir, `cargo +1.95.0 build -p data-explorer -p signal-lab --locked`, then `cargo +1.95.0 test -p eario-hub --all-features --locked`: 47 unit and 8 integration tests passed, exit 0 (`03-msrv-1.95-test.txt`). The new crates need at most Rust 1.85 (`ureq` and `ureq-proto` declare 1.85; `rustls` and `rustls-native-certs` 1.71; `utf8-zero` declares none). Cargo refused 1.88 on the existing workspace crates (`03-msrv-1.88-check.txt`), which is expected.
- Dependency tree (`03-dependency-tree.txt`): the lock file gains only `ureq` 3.5.2, `ureq-proto` 0.7.1 and `utf8-zero` 0.8.1. `webpki-roots`, `aws-lc-rs` and `aws-lc-sys` are not in the graph. `ring` comes from the tree that was already there.
- Not run: `cargo xtask check` for the whole workspace (vitest cannot run on the local Node 22.14), and `cargo xtask e2e` (no frontend or example changed).
- Not caused by this change: cargo 1.100 prints the manifest warnings `redundant_homepage` and `missing_lints_inheritance`.

## Review Notes

Parent: base 4a3ea5ab9b5389fab22edd364b88f47ab8d98dcc; tree clean except session files. Mode delegate (implementer opus/high). Recovery count: 0.

Acceptance Criteria:

1. **`GitHub` trait** (`crates/eario-hub/src/github.rs`). Every method blocks and is documented to be called through `spawn_blocking`. The trait is `Send + Sync` and dyn-compatible.
   - `latest_release(repo) -> Release { tag, assets: [ReleaseAsset { name, size, download_url }] }` reads `GET /repos/{repo}/releases/latest`. It refuses a draft or pre-release even though that endpoint never returns one (`github_no_release`).
   - `catalog(repo) -> eario_core::Catalog` uses the contents endpoint `GET /repos/{repo}/contents/catalog.json` with `Accept: application/vnd.github.raw+json`. That reads the default branch through `api.github.com` alone, so `raw.githubusercontent.com` is not needed and is not allowed. The entries are only parsed; checking `format_version`, `is_repo` and the keys is left to ticket 04.
   - `release_file(repo, release, name, limit) -> Option<Vec<u8>>` returns `None` when the release has no asset of that name.
   - `checksum_files(repo, release) -> ChecksumFiles { sums, signature }` is a provided method. It uses `verify::MAX_SUMS_BYTES` and `verify::MAX_SIGNATURE_BYTES`. A missing `SHA256SUMS.txt.minisig` comes back as `signature: None`, which ticket 04 passes straight to `verify_sums`.
   - `download(repo, asset, &mut dyn Write) -> Download { size, sha256 }` streams into the caller's writer (a temporary file in ticket 04) and hashes as it goes. It never holds the asset in memory.
2. **Real client** `GitHubClient` (ureq 3, rustls with ring).
   - The agent has `max_redirects(0)` and `http_status_as_error(false)`. `open()` follows at most `MAX_REDIRECTS` (5) redirects itself.
   - Each hop's URL is parsed and checked by `Endpoint::allows` before it is requested. The rules: HTTPS only; the host is exactly one of `api.github.com`, `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com` (case-insensitive); port none or 443; no userinfo (`@`).
   - A relative `Location` is accepted only path-absolute (`/x`), joined to the current origin. Others (`//host`, `x`) are refused. `https_only(true)` is also set on the agent.
   - Caps: JSON (API answer, catalog) 2 MiB, asset 512 MiB. Release text files use the caps from `verify`.
   - The body is counted while it streams, whatever `content-length` says. A declared size or `content-length` over the cap is refused before the body is read. An asset must also have exactly the size the release lists (`github_size_mismatch`), so at most the listed size is ever written.
   - Timeouts: connect, response and per-read 30 s. Small files have a 60 s total; downloads have no total limit, only the 30 s per read.
   - User-Agent `eario-hub/<version>` and `X-GitHub-Api-Version: 2022-11-28` on API calls.
   - Tests: redirects to another host, another loopback port, with userinfo, protocol-relative or bare relative are all refused, and the other server records no request. A redirect loop stops at 6 requests. A 302 without `Location` is refused. Download URLs off the endpoint are refused unrequested. The real `Endpoint::GitHub` client refuses a loopback URL without requesting it. `Endpoint::GitHub.allows` has an allowlist table test.
3. **Fake server and override.**
   - `crates/eario-hub/src/fake_github.rs`, `#[cfg(any(test, feature = "fake-github"))]`. It uses only std: one thread per connection, `connection: close`.
   - `release()` mimics GitHub: the API JSON lists assets under `/<repo>/releases/download/<tag>/<name>`, which redirects (302) to `/assets/...`. Every download test therefore goes through a redirect.
   - `catalog()` and `route(path, FakeResponse)` cover the other answers. `FakeResponse` has `ok`, `new(status)`, `redirect`, `rate_limited()` (403 with `x-ratelimit-remaining: 0`), `header`, `without_length` (body ends at close), `claiming_length`, `delayed`. `requests()` lists the paths requested.
   - `Endpoint::from_env()` reads `EARIO_HUB_GITHUB_API` only when `cfg!(debug_assertions)`. It accepts exactly `http://127.0.0.1:<port>` or `http://localhost:<port>` (optional final `/`, port 1–65535, digits only). Anything else is ignored with a `tracing::warn!`.
   - With the override, `Endpoint::Loopback` allows only `http` on that host and port. Every GitHub host is refused (test `the_loopback_endpoint_replaces_every_github_host`), and no proxy is used.
   - The parse is tested through `from_override`. The `cfg!(debug_assertions)` gate cannot be tested in a debug test build; it is one line in `from_env`.
4. **Coded errors.** `GitHubError` (thiserror, stable `code()`) has these codes: `github_invalid_repo`, `_offline`, `_timeout`, `_no_release`, `_not_found`, `_rate_limited` (403 with remaining 0, or 429), `_http_status`, `_forbidden_url`, `_bad_redirect`, `_too_large`, `_size_mismatch`, `_bad_json`, `_write_failed`, `_transport`, `_no_certificates`.
   - Every message about a repository names it.
   - `HubError::GitHub(#[from] GitHubError)` (transparent) delegates `code()` to it.
   - Tests convert rate-limit, 404 (release and catalog), offline (closed port) and timeout (200 ms against a 3 s delay) into `HubError` and check the code and that the repository is in the message.
   - `is_repo` is checked at the start of every method; tests show invalid repositories make no request.
5. **cargo deny and MSRV:** both pass; see Local Test Result.

Dependencies (for the PR):

- **`ureq` 3.5** (`default-features = false, features = ["rustls-no-provider"]`). Replaces writing an HTTP/1.1 + TLS client by hand. It is blocking, which fits `spawn_blocking`, with no async runtime of its own. Licence MIT OR Apache-2.0. Maintained by Martin Algesten and Jacob Hoffman-Andrews; 3.5.2 is current, rust-version 1.85.
  - No default features, so no gzip, no `webpki-roots` (CDLA-Permissive-2.0, which `deny.toml` does not allow), and no built-in provider.
  - It pulls only `ureq-proto` (MIT OR Apache-2.0) and `utf8-zero` (MIT OR Apache-2.0) as new crates; the rest (`http`, `httparse`, `base64` 0.23, `percent-encoding`, `log`, `rustls-pki-types`) was already locked.
  - Caveat: the provider is passed with `TlsConfigBuilder::unversioned_rustls_crypto_provider`, which ureq documents as outside semver. A ureq update that moves rustls to a new minor version would fail to compile, not misbehave, and `Cargo.lock` pins it.
- **`rustls` 0.23.46** (`default-features = false, features = ["ring", "std", "tls12"]`). Direct only to name `rustls::crypto::ring::default_provider()`. Already in the tree with `ring` (via polars → object_store → reqwest/hyper-rustls). Licence Apache-2.0 OR ISC OR MIT; `ring` is Apache-2.0 AND ISC, both allowed. ring rather than aws-lc-rs, which needs cmake and nasm on Windows; aws-lc-rs is not in the graph.
- **`rustls-native-certs` 0.8**. Root certificates from the system store (schannel on Windows, Security.framework on macOS, openssl-probe paths on Linux), instead of `webpki-roots`. Already in the tree through hyper-rustls. Licence Apache-2.0 OR ISC OR MIT.
- `serde`, `serde_json` (were dev-dependencies) and `sha2` (workspace) became normal dependencies of `eario-hub`. None is new to the workspace.
- `deny.toml`: no change needed, because no new licence or source.

Changes outside the ticket's file list:

- `crates/eario-hub/src/lib.rs`: module declarations and a doc bullet.
- `crates/eario-hub/src/supervisor.rs`: the `HubError::GitHub` variant and its `code()`, which AC 4 needs.
- `crates/eario-hub/src/http.rs`: the exhaustive status match needs the new variant. `InvalidRepo` maps to 400, any other GitHub failure to 502. Ticket 05 may refine this.
- `Cargo.lock`: updated by cargo.

Notes for later tickets:

- **Ticket 07 process.** The fake server is a public module behind the `fake-github` feature. No `[[bin]]` was added, because its configuration format belongs to ticket 07. A bin or example with `required-features = ["fake-github"]` can wrap `FakeGitHub` there. The hub binary is built without the feature.
- **Ticket 04.**
  - `download` writes into a `Write` and may leave a partial file on error; the caller must discard its temporary file on any `Err`.
  - The hash returned is that of the bytes written, to compare with `Checksums::sha256`.
  - The release `tag` is passed through unvalidated; ticket 04 decides how to turn it into a version.
- **Proxy.** For the real endpoint, ureq's `HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY` from the environment are kept. Traffic is TLS end to end through `CONNECT`, and each URL is still checked before it is requested.
- **Docs.** `docs/` is not changed, because nothing is user-visible yet (tickets 05 and 09).

Independent review (opus/high, base 4a3ea5a): 2 findings, both P3. (1) github.rs: no resolve/send timeout → parent added `timeout_resolve` and `timeout_send_request`, doc updated. (2) http.rs: local GitHubError::Write/Certificates mapped to 502 → now 500. Correction pass 1 (parent, inline). Re-review (sonnet/medium): no findings. Parent re-ran fmt, workspace clippy -D warnings, `cargo test -p eario-hub --all-features` (47+8 pass), `cargo deny check` (ok). Reviewer gaps accepted: no live-GitHub run by design; the release-only gate checked by reading. Ticket 04 owns the temporary download file. Accepted.

## Blockers / Challenges

- ureq's default `rustls` feature brings `webpki-roots` (CDLA-Permissive-2.0, not allowed by `deny.toml`). Fixed by using `rustls-no-provider`, then giving ureq a ring provider and the system roots (`rustls-native-certs`) explicitly. Both crates were already in the dependency tree.
- No Rust 1.95 toolchain was installed. I installed 1.95.0 (minimal profile) with rustup to run the MSRV build and tests; 1.88 is refused by the existing workspace crates. The first 1.95 run of the hub's integration tests failed only because the examples had not been built in the scratch target dir. After `cargo +1.95.0 build -p data-explorer -p signal-lab`, all 8 passed.
- The resolver picked `ureq` 3.5.2 and `ureq-proto` 0.7.1 instead of the 3.5.0 copy I had first read in the registry. I compared the feature table, `src/tls` and `src/config.rs` of both versions: no difference.
- A heredoc of the test module failed in the Bash tool (quote parsing). I wrote the tests through a scratch file instead. No effect on the result.

## Commits

- 136a926 feat: bounded GitHub client with per-hop host allowlist and a fake loopback server (ticket 03)

## Time Spent

35m (ticket-start.js to ticket-complete.js)
