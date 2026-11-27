# Ticket 02: release-verification

**Status:** ✅ Done

## Local Test Result

Evidence in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/` (raw output, one file per command).

- `cargo test -p eario-hub verify`: 20 passed, 0 failed, exit 0 (`02-verify-tests.txt`).
- `cargo fmt --all -- --check`: exit 0 (`02-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: exit 0 (`02-clippy.txt`). The first run failed on `chunks_exact_to_as_chunks` (clippy 1.100); fixed, then re-run clean.
- `cargo test -p eario-hub` (whole crate): 24 unit and 8 integration tests passed, exit 0 (`02-hub-tests.txt`).
- `cargo doc -p eario-hub --no-deps --all-features`: exit 0, no rustdoc warning (`02-doc.txt`).
- MSRV lower bound: `cargo +1.88.0 check -p minisign-verify --locked` exit 0 (`02-msrv-minisign-verify.txt`), and `verify.rs` with its tests built and run under Rust 1.88 in a scratch crate that includes it by path: 20 passed (`02-msrv-verify-module.txt`). 1.88 is older than the 1.95 MSRV. No 1.95 toolchain is installed, so 1.95 itself was not run.
- Mutation check (`02-mutation-check.txt`, script `02-mutate.sh`): 10 mutations, each killed by the tests you would expect. The mutations: accept legacy signatures, skip the signature check, compare keys by fingerprint, accept duplicates, wrong fingerprint byte order, no four-line check, accept `ED` keys, no size cap, a single-space separator, catalog key ignored.
- Not run: `cargo deny check` (not installed; licence and source checked by hand, see Review Notes). Not run: `cargo xtask check` for the whole workspace (vitest cannot run on the local Node 22.14) and `cargo xtask e2e` (no frontend or example change).
- Not caused by this change: cargo 1.100 prints the manifest warnings `redundant_homepage` and `missing_lints_inheritance` for every crate.

## Review Notes

Parent: base bd0a4ae0582f9bacc8bfb357e42583e0f6cf09b4; tree clean except session files. Mode delegate (implementer opus/xhigh). Recovery count: 0.

Acceptance Criteria (all in `crates/eario-hub/src/verify.rs`):

1. **SUMS parser.** It accepts `<64 hex>  <name>` and `<64 hex> *<name>`, either hex case (stored in lower case), `\n` or `\r\n` line ends, and an optional final newline. It refuses, with the line number: a malformed line, a blank line, a BOM, the BSD format, GNU-escaped names, and names outside `[A-Za-z0-9._+-]` (including `.`, `..` and paths). Codes: `release_malformed_sums`, `release_duplicate_asset` (names the line and the asset), `release_empty_sums`. One bad line refuses the whole list. The list is parsed only after its signature has verified.
2. **Signature.** `verify_sums(repo, key, sums, Option<signature>)` uses minisign-verify with legacy signatures refused. Each refusal has its own test and code:
   - wrong key: `release_wrong_key`;
   - a key with the same key id but a different key: `release_bad_signature`;
   - changed list (byte, name, CRLF, newline, added or removed line): `release_bad_signature`;
   - changed trusted comment or signature bytes: `release_bad_signature`;
   - missing signature (`None`): `release_missing_signature`;
   - malformed signature: `release_malformed_signature`;
   - legacy or unknown algorithm: `release_unsupported_signature`;
   - oversized file: `release_file_too_large` (list 64 KiB, signature 16 KiB).
   
   The only way to get a `Checksums` is through `verify_sums`. Interop: vectors in the minisign tool's format, taken from the minisign-verify 0.3.0 tests, verify.
3. **Fingerprint and key status.**
   - `PublicKey::from_base64(repo, text)` accepts only the 56-character key line with algorithm `Ed`. Anything else gives `release_invalid_key`.
   - `fingerprint()` is the minisign key id in 16 upper-case hex digits. It matches what minisign prints: `E7620F1842B4E81F` for the minisign-verify vector, and `027D40E88081F840` / `250D566D7ADA1D2B` from the `.pub` files the minisign crate printed.
   - `key_status(key, pinned, catalog)` reuses `eario_core::KeyStatus` and compares whole keys. pinned_matches maps to `Pinned`, new_key (first use) to `Unknown`, key_changed to `Changed`. A key that matches the catalog key when nothing is pinned is `Catalog`; one that does not match it is `Changed`. A pinned key always wins over the catalog.
4. **Asset names.** `asset_name(slug, target)` returns `<slug>-<triple>[.exe]` for `Target::{WindowsX86_64, LinuxX86_64, MacosAarch64, MacosX86_64}` and refuses a non-slug (`hub_invalid_slug`, the same code as `HubError::InvalidSlug`). `Target::CURRENT` is a `const` set from `cfg!`. It is `None` on any other platform. A test checks it against `std::env::consts::{OS, ARCH, EXE_SUFFIX}`.

Other decisions:

- **Out-of-list file.** `crates/eario-hub/src/lib.rs` gained `pub mod verify;` and a doc bullet, which the module needs in order to compile. It is `pub` so the unused-until-ticket-4 API raises no dead-code error and the composing tickets can call it. Cargo updated `Cargo.lock` itself. docs/ is not touched: nothing user-visible yet.
- **Dependency `minisign-verify = "0.3"`.**
  - What it replaces: writing minisign/Ed25519/BLAKE2b verification ourselves.
  - Licence: MIT, which is on the `deny.toml` allow list. Source: crates.io.
  - It has no dependencies and no features to narrow, and no `unsafe` (its base64 module has `forbid(unsafe_code)`; grep finds `unsafe` nowhere else).
  - Its Ed25519 verify checks S < L and rejects identity or undecodable keys.
  - Maintenance: the author is Frank Denis, author of minisign and libsodium. Releases: 0.3.0 on 2026-09-25, 0.2.5 on 2026-03-03, 0.2.4 on 2025-05-21. About 24.5M downloads.
  - MSRV: no `rust-version` declared, edition 2018, builds on 1.88. Cargo's MSRV-aware resolver locked 0.3.0 as 1.95-compatible.
- **Dependencies not added.**
  - The `base64` crate: minisign-verify does not expose the key id, so a small private decoder reads the fixed 56-character key line.
  - The `minisign` crate as a dev-dependency: it pulls scrypt and rpassword. Instead, the vectors were generated once in scratch (`02-vectors/`, minisign 0.10.0, output in `evidence/02-vectors-output.txt`) and committed as constants. No secret key was ever written.

Security points for the parent (product decisions, not changed here):

1. **The fingerprint does not authenticate.** The AC asks for the minisign key id as the fingerprint, but a key id is chosen freely at key generation. Test `a_key_with_the_same_fingerprint_is_still_another_key` builds such an impostor key. Pinning is safe, because `key_status` compares whole keys. But the trust-on-first-use confirmation, and ticket 5's `accept-key/{fingerprint}` route, would have the user confirm only the key id. An attacker who controls a release can present another key with the author's published key id. Suggestion: tickets 4, 5 and 7 show the full key line too, and accept-key confirms the previewed key by its full text or its hash, not by key id alone.
2. **Downgrade.** The signature binds the content, not the version, so an old signed list with its assets can be republished under a newer tag. Ticket 4 reads the manifest of the verified file; whether it should refuse a version older than the installed one is a ticket-4 decision.
3. **First install with an unknown slug.** `asset_name` needs the slug, and a first install from `owner/repo` does not know it yet. Ticket 4 must find the asset (for example by the `-<triple>[.exe]` suffix) and then check the manifest's slug.
4. **Missing signature.** Tickets 3 and 4 should pass `None` when the release has no `.minisig` asset, so the refusal is `release_missing_signature`.

Review rounds: 0.

Independent review (opus/high, base bd0a4ae): REVIEWED — no findings; reviewer re-ran the 20 verify tests. Parent re-ran fmt and workspace clippy -D warnings: clean. Parent decisions on the implementer's forward notes, carried into tickets 4, 5 and 7: (1) key confirmation uses the SHA-256 of the full key line, shown with the full key, not the minisign key id; (2) an update to a version not newer than the installed one is refused; older versions only via Rollback; (3) first install by repo finds the single asset ending in `-<triple>[.exe]` and requires the manifest slug to equal its prefix; (4) a release without `SHA256SUMS.txt.minisig` is refused. Gaps: cargo-deny and the 1.95 toolchain not installed (CI checks both). Accepted.

## Blockers / Challenges

- clippy 1.100's `chunks_exact_to_as_chunks` failed the first clippy run. The fix uses `as_chunks`/`as_chunks_mut` (stable since 1.88, under the MSRV); clippy then passed.
- No Rust 1.95 toolchain is installed, so 1.88 (older) served as a lower bound. CI's MSRV job remains the authority.
- cargo-deny is not installed. The licence, source and absence of dependencies were checked by hand.
- The first version of the mutation script reported every mutation as a build error, because its grep matched cargo's `error: test failed`. The script was fixed and re-run; the reported results come from the fixed script.

## Commits

- 57995d9 feat: verify signed SHA256SUMS releases with pinned minisign keys (ticket 02)

## Time Spent

30m (ticket-start.js to ticket-complete.js)
