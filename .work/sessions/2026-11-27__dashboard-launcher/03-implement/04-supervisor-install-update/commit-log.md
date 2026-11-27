# Ticket 04: supervisor-install-update

**Status:** ✅ Done

## Local Test Result

Evidence in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/` (raw output, one file per command, prefix `04-`).

- Verification Step, `cargo test -p eario-hub --all-features`: exit 0. 67 unit tests, 23 integration tests (`tests/hub.rs`, 14 of them new), 0 failed (`04-verification-step.txt`). The ticket's literal `cargo test -p eario-hub` (no `--all-features`) also exit 0, same counts, because the crate's self dev-dependency turns `fake-github` on for its tests (`04-verification-step-no-features.txt`).
- The 14 new integration tests run against the fake GitHub on loopback. The asset is the real `data-explorer` debug executable (54 MB), signed at test time with `fake_github::TestKey` (synthetic Ed25519 seeds, never stored). They cover:
  - install from a catalog key (status `catalog`; the installed copy then starts and runs);
  - trust on first use (`unknown`, refused until accepted by the key's SHA-256; a wrong hash and a bare key id are refused);
  - key change refused (`hub_key_changed`) and then accepted; a release signed by key B that carries key A's `.pub` is refused (`release_wrong_key`);
  - slug clash with a local install and with another repository; the same repository in other case counts as installed;
  - tampered asset, missing `.minisig` (refused before the download), changed `SHA256SUMS.txt`;
  - manifest slug mismatch; a signed file that is not an executable;
  - zero and two matching assets (refused before any download);
  - two updates keep exactly one previous version and remove the older; rollback restores it and the restored copy starts; a changed previous copy and a path in `previous.file_name` are refused;
  - update, apply and rollback refused while the dashboard starts or runs;
  - downgrade refused by the tag before the download, and by the signed manifest after it; equal versions; non-SemVer versions;
  - `check()` result, stored and read by `list()`, and offline "could not check" (`github_offline`);
  - check at start off (no request, no `updates.json`), on, and offline at start;
  - catalogs add/remove/list;
  - the real `eario-hub` CLI beside a running hub: usage error exit 2, `--yes` required without a terminal, install, check, update refused while running, update, rollback, `check-at-start off`, `catalog remove`. A second CLI test: `--yes` never trusts a key; `--accept-key` with a wrong hash is refused; with the right hash it installs.
- Mutation check (`04-mutate.py`, output in `04-mutation-check.txt` and `04-mutation-check-tofu.txt`): 10 of 10 mutations killed by the test you would expect. The mutations:
  - unknown key installed without acceptance;
  - changed key installed without acceptance;
  - no hash comparison;
  - no manifest slug check;
  - no slug-clash check;
  - an equal or older version accepted;
  - the older previous version not removed;
  - `make_idle` dropped from `apply_update`;
  - check at start ignoring the setting;
  - the previous copy restored without its check.
  The first run skipped the TOFU mutation, because rustfmt had reflowed its line. Re-run with the reflowed pattern: killed. Every file was restored (diff stat unchanged).
- `cargo test --workspace --all-features -j 4`: exit 0, every binary ok (`04-workspace-tests.txt`). The first attempt without `-j` failed at compile time with `memory allocation of 2129936 bytes failed` and `link.exe` exit 1102, an out-of-memory error (free virtual memory 9.4 GB). See Blockers.
- `cargo fmt --all -- --check`: exit 0 (`04-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: exit 0 (`04-clippy.txt`), re-run after the last code change. `cargo clippy -p eario-hub --all-targets -- -D warnings` (without the feature on the command line) also clean.
- `cargo check -p eario-hub --lib --bins` (the hub as shipped, without `fake-github`): exit 0 (`04-check-without-feature.txt`). `cargo tree -e normal -i ring` shows that `ring` still reaches the hub only through rustls.
- `RUSTDOCFLAGS="-D warnings" cargo doc -p eario-hub -p eario-core -p eario-runtime --no-deps --all-features`: exit 0 (`04-doc.txt`).
- `cargo deny check`: `advisories ok, bans ok, licenses ok, sources ok`, exit 0 (`04-deny.txt`). The lock file gains only two dependency edges in `eario-hub` (`eario-hub` itself as a dev-dependency, and `ring`), no new package.
- MSRV: `cargo +1.95.0 check -p eario-hub -p eario-core -p eario-runtime --all-targets --all-features --locked` in a separate target dir: exit 0 (`04-msrv-1.95-check.txt`).
- `cargo xtask gen-types`: exit 0 (`04-gen-types.txt`). A second run leaves the generated directory byte-identical (same SHA-256 over all files). The bindings are uncommitted: modified `HubInfo`, `InstallPreview`, `KeyStatus`; new `DashboardUpdate`, `HubCatalog`, `HubCatalogEntry`, `HubSettings`, `UpdateCheck`.
- `npm run check` in `frontend/`: 399 files, 0 errors, 0 warnings, exit 0 (`04-npm-check.txt`), after adding `last_check: null` to the `HubView.test.ts` fixture.
- `eario-hub help` and a usage error: `04-cli-help.txt`.
- Not run:
  - `npm test` (vitest): the local Node 22.14 cannot start it, as in tickets 01 to 03.
  - `cargo xtask e2e`: no view or example changed, only generated types, a test fixture, and one field (`last_check`) added to the hub's list answer.
  - `cargo xtask check` as one command: its parts were run one by one above, except vitest.
  - macOS and Linux: only Windows x86_64 was run here.
- Not caused by this change: cargo 1.100 prints the manifest warnings `redundant_homepage` and `missing_lints_inheritance`.

### Correction pass 1 (review 1): verification

Evidence prefix `04-fix-`, same directory. Every command below ran on the final tree, after the last code change.

- `cargo test -p eario-hub --all-features -j 4`: exit 0. 70 unit tests, 26 integration tests (3 new), 0 failed (`04-fix-hub-tests.txt`).
- `cargo test --workspace --all-features -j 4`: exit 0, every binary ok (`04-fix-workspace-tests.txt`). Among them: `data-explorer` launch tests 6 (2 new), `eario` unit 26 (1 new), `eario-runtime` unit 75 (1 new), `eario-core` unit (the preview round trip now covers both shapes). Each new test is listed by name in that file.
- `cargo fmt --all -- --check`: exit 0 (`04-fix-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings`: exit 0 (`04-fix-clippy.txt`).
- `RUSTDOCFLAGS="-D warnings" cargo doc -p eario-hub -p eario-core -p eario-runtime -p eario --no-deps --all-features`: exit 0 (`04-fix-doc.txt`).
- `cargo +1.95.0 check -p eario-hub -p eario --all-targets --all-features --locked` (separate target dir): exit 0 (`04-fix-msrv-1.95-check.txt`). `File::lock`, `File::try_lock` and `TryLockError` are stable since 1.89.
- `cargo check -p eario-hub --lib --bins` (the hub as shipped, without `fake-github`): exit 0 (`04-fix-check-without-feature.txt`).
- `cargo xtask gen-types`, twice: exit 0 both times. The SHA-256 over all generated files is the same after both runs (`04-fix-gen-types.txt`, `04-fix-gen-types-second.txt`). Only `InstallPreview.ts` changes in this pass.
- `npm run check` in `frontend/`: 399 files, 0 errors, 0 warnings, exit 0 (`04-fix-npm-check.txt`). `HubView.test.ts` needed no change: it does not use `InstallPreview`.
- Mutation check (`04-fix-mutation-check.txt`), 8 mutations, each run against the test meant to catch it, every file restored afterwards (diff stat unchanged):
  - both trust checks removed, so the manifest runs before the key is accepted: killed;
  - only `run_manifest`'s own trust check removed: killed;
  - `accept_staged` without the second hash: killed;
  - settings changed without the hub lock: killed;
  - the hub start removes every staging area: killed;
  - `--eario-install` replaces a GitHub entry: killed;
  - the hub lock does not lock: killed;
  - `--eario-install` without the hub lock: survived on the first run. The test checked only the registry, after 1 s, but the debug build takes longer than that to copy and hash the 54 MB executable. The test now waits 2 s and also checks that `bin/<slug>/` does not exist (the copy starts right after the lock point). Re-run of both lock mutations: killed (`04-fix-mutation-check-install-lock.txt`).
- Not run:
  - `npm test` (vitest): Node 22.14, as before.
  - `cargo xtask e2e`: no view changed. The generated `InstallPreview.ts` is used by no component yet. An example's test file changed, not its code.
  - `cargo deny check`: no dependency changed.
  - macOS and Linux. The locks were run on Windows only (`LockFileEx`). On Unix, `flock` locks per open file description, so two handles in one process exclude each other too, which the tests rely on; that was not run here.

## Review Notes

Parent: base 310f2a251da8c2850994fea8e797934dc65ddb03; tree clean except session files. Mode delegate (implementer opus/xhigh). Recovery count: 0.

Implementer self-check against the Acceptance Criteria (review rounds: 0 so far). Business logic is in `crates/eario-hub/src/remote.rs` (blocking work) and in `Supervisor` methods in `supervisor.rs` (locks, process state, `spawn_blocking`). `http.rs` gets no route.

1. **Catalogs.** `catalogs`, `add_catalog`, `remove_catalog` and `catalog_listing` on `Supervisor`.
   - The list is stored in `catalogs.json` (format 1, written atomically). While the file is missing, it is `[rtonneau/eario-catalog]`. Removing the default writes `[]`, so it stays removed.
   - Adding a catalog reads its `catalog.json` once first, so a repository without one is not added (`github_not_found`). Repositories compare case-insensitively.
   - The listing gives every catalog with its entries and `installed_slug`. A catalog that cannot be read is listed with an `ApiError`, which is not fatal.
   - A catalog in a newer format is refused (`hub_catalog_unknown_version`).
2. **Preview, then install.**
   - `preview(repo)` reads the latest release and picks the asset. It verifies `SHA256SUMS.txt.minisig` (missing → `release_missing_signature`, before any download) and downloads the asset into `staging/<pid>-<n>/`.
   - It then compares the SHA-256 with the signed list (`release_hash_mismatch`). Only after that does it run `--eario-manifest`: no stdin, stdout bounded to 64 KiB, stderr discarded, `detach()` (no console window, own process group), the staging directory as working directory, killed after `SupervisorOptions::manifest_timeout` (20 s). It checks the slug (`hub_manifest_mismatch`) and the protocol.
   - `install(repo, sha256)` takes only a staged preview with that SHA-256 (`hub_no_preview`, `hub_preview_mismatch`, `hub_invalid_sha256`) and a trusted key. It re-checks the registry, moves the file into `bin/<slug>/<slug>-<sha12>[.exe]` with one rename, hashes it again, and writes the registry atomically (on failure it removes the moved file). The staging directory is emptied when the hub starts.
3. **Trust on first use.** The key is the one the release carries in `minisign.pub` (a new convention, see below), or, without it, the pinned or catalog key.
   - Status: `catalog` (the catalog key matches), `pinned`, `unknown`, `changed`, and a new `accepted`.
   - `accept_key(repo, key_sha256)` accepts only the previewed key's SHA-256 (`hub_key_mismatch`); a minisign key id is refused. Install and update pin the key the release was verified with.
   - A different key later is `changed` and refused (`hub_key_changed`) until accepted. A release signed by B but carrying A's `.pub` is refused by the signature check (`release_wrong_key`).
4. **Slug clash.** Refused before the download: `hub_slug_taken` (local or another repository, named in the message) and `hub_already_installed` (same repository, any case).
5. **Check.** `check()` reads only `/releases/latest` of each GitHub entry; nothing is downloaded.
   - It stores `UpdateCheck { checked_at, dashboards: [DashboardUpdate { slug, repo, latest_version, available_version, error }] }` in `updates.json`.
   - `list()` reads it: `HubDashboard.available_version` is shown only while it is still newer than the installed version, and the new `HubInfo.last_check` carries the result, so the view can show "could not check".
   - `start()` empties `staging/`, starts serving, then spawns the check unless `settings.json` says `check_updates_at_start: false`. Settings that cannot be read mean no check. A failure is logged, never fatal.
6. **Update and rollback.**
   - `preview_update` / `apply_update(slug, sha256)` / `update(slug)` are refused while the dashboard starts or runs. `make_idle` also kills a child of this hub that no longer answers.
   - The current version becomes `previous`. `sweep` keeps only the current and previous files in `bin/<slug>/`, so the older one is removed.
   - `rollback` validates `previous.file_name` with `verify::is_asset_name` before joining it to `bin/<slug>/`, checks the size and hash (`hub_previous_unusable`), restores, removes the newer copy, and leaves `previous: None`. My reading of "roll back once": update again to get the newer version back.
   - `update_staged` refuses (`hub_changed_meanwhile`) when the entry changed since the preview.
7. **CLI** (`cli.rs`): `catalog list|add|remove`, `install <owner/repo>`, `check`, `update <slug>`, `rollback <slug>`, and `check-at-start [on|off]`.
   - Changes need a terminal yes, or `--yes`; without a terminal they are refused (`hub_confirmation_required`).
   - The preview printout shows repo, dashboard and version, tag, asset and size, SHA-256, full key line, key SHA-256 and status.
   - An unknown or changed key is trusted only by a terminal yes or `--accept-key <key sha256>`, never by `--yes`. This is the "equivalent interactive step" in place of a separate `accept-key` command, which could not work across processes because accepted keys are held in memory.
   - Without arguments: `run()`, unchanged. Exit codes: 2 for usage, 1 for a failure.
   - Text from a release or catalog is printed with control and bidi characters replaced. Otherwise a tag could carry `\r`/ESC and overwrite the printed "Key status" line.

Parent decisions:

1. A key is identified by `PublicKey::sha256()` (SHA-256 of the key line). `InstallPreview.key_fingerprint` is replaced by `public_key` and `key_sha256`. Bindings are regenerated and the fixture is fixed.
2. **Version rule.** Implemented without a new dependency: a hand-written SemVer 2.0 precedence check (`remote::Version`), tested against the spec's ordering example.
   - The rule: an update installs only when the verified executable's manifest version is strictly newer than the installed manifest version. A SemVer tag that is not newer refuses before the download.
   - When either version is missing or not SemVer, `hub_version_not_comparable`: uninstall, then install again.
   - I did not take the suggested fallback ("unless tags differ and the release is the latest"). Tags are unsigned, so differing tags prove nothing; refusing is the only rule that stops a downgrade. The rule is documented in `remote.rs`.
3. A first install needs exactly one asset `<slug>-<triple>[.exe]` with a slug prefix (`hub_no_asset`, `hub_ambiguous_asset`), and the manifest slug must equal that prefix. An update uses the exact `asset_name(slug)`.
4. A missing `.minisig` is refused.
5. `check()` loops over GitHub entries; `UpdateCheck` can take a `hub` field in ticket 06.

Files outside the ticket's list (needed):

- `eario-core` `launch.rs`/`lib.rs`/`payload.rs` (contract, `MANIFEST_ARGUMENT`).
- `eario-runtime/src/hub_dir.rs`: new paths `catalogs_path`, `settings_path`, `updates_path`, `staging_dir`, and public `read_json_at`, `write_json_at`, `move_file`, `create_private_dir`, so the atomic-write code is reused rather than copied.
- `verify.rs`: `KEY_FILE`, `from_pub_file`, `sha256`, `is_sha256`, errors `release_missing_key`, `release_invalid_key_file`, `release_hash_mismatch`.
- `fake_github.rs`: `TestKey` (ring Ed25519, BLAKE2b-512 checked against RFC 7693 vectors, base64), `sums_file`, and `Arc` bodies so 54 MB answers are not copied per request.
- `http.rs`: exhaustive status mapping only.
- `cli.rs` (new).
- `Cargo.toml` and the hub's `Cargo.toml`.
- `HubView.test.ts` and the generated bindings.

**Dependency.** `ring = { version = "0.17", default-features = false }`, optional, enabled only by `fake-github`, to sign test releases. It is already in the tree (rustls' provider), and its licence (Apache-2.0 AND ISC) passes `cargo deny`. Alternatives rejected: the `minisign` crate (pulls scrypt and rpassword, as ticket 02 found) and a hand-written Ed25519 signer. The crate depends on itself as a dev-dependency (`features = ["fake-github"]`), so its tests always have the fake. `fake_github` is now gated on the feature only.

For later tickets and the parent:

- **Ticket 08.** A repository that no catalog lists must attach `minisign.pub` (the `.pub` file) to each release; without it, and without a catalog key, the release is refused (`release_missing_key`).
- **Ticket 08.** The examples declare package version 0.0.0, so a published data-explorer or signal-lab can never be updated in place. Their manifest version should follow the release.
- **Tickets 05, 09 and the glossary.** ADR 0013 and the glossary say "confirms its fingerprint"; it is now the key's SHA-256. `.work/GLOSSARY.md` is not changed (session state).
- **Ticket 05.** Response types: `HubCatalog` list, `InstallPreview`, `UpdateCheck`, `HubSettings`. Route `accept-key/{fingerprint}` should take `key_sha256`. The status mapping added in `http.rs` is a first proposal.
- **Ticket 07.** Show `public_key` and `key_sha256` and the new status `accepted`. Text from releases and catalogs is untrusted.
- **Ticket 09 (docs).** Document the CLI commands, `catalogs.json` / `settings.json` / `updates.json` / `staging/`, the version rule and `minisign.pub`. `docs/` is not changed here (plan: tickets 05 and 09).

Known limitations:

- A pin is removed with its registry entry at uninstall, so a reinstall is trust on first use again.
- `--eario-install` (crate `eario`, not changed) still replaces a GitHub-installed entry of the same slug after its own confirmation, and leaves that entry's previous copy in `bin/<slug>/` until the next uninstall.
- The CLI and a running hub share no lock. Every write is atomic and re-reads first, as `--eario-install` does, but two writers at the same instant can lose one change.
- A process killed during a preview leaves its `staging/<pid>-<n>/` until the next hub start.
- At most 4 previews are kept; a new one drops the oldest.

Independent review 1 (opus/high, base 310f2a2): 4 findings. P0 remote.rs:482-495 — `stage` runs `--eario-manifest` before the key is trusted (unknown or changed key). P2 crates/eario/src/launch.rs:101-140 — `--eario-install` replaces a GitHub-sourced entry silently (violates slug-from-another-source rule). P3 cli.rs:11-13 — no lock between CLI and running hub registry read-modify-write. P3 lib.rs:106 / remote.rs:363 — hub start wipes another process's staging. Parent disposition: all accepted. Correction pass 1 started (recovery count 1) via the implementer: (P0) untrusted key → stop after signature+hash, return a key-pending preview without manifest fields; manifest runs only after accept_key re-hashes the staged file; tests that an untrusted-key asset is never run. (P2) refuse with a coded error naming the repo, test. (P3) advisory lock file around every registry/catalog/settings read-modify-write incl. `--eario-install` (std File::lock, MSRV 1.95 ok). (P3) per-process staging dir held by a lock; start removes only unlocked ones.

Correction pass 1 (implementer), finding by finding:

1. **P0: nothing signed by an untrusted key is run.**
   - Contract: `InstallPreview.name` and `InstallPreview.version` are replaced by `manifest: Option<Manifest>`, the whole manifest the verified executable printed. It is `None` while the key is `unknown` or `changed`. I chose this over a separate type: one shape for both steps; the view tests `manifest === null`; the install still repeats `sha256`. `slug` stays a field of its own, because it does not come from the manifest: it is the asset name's prefix (first install) or the installed slug (update), and the manifest must match it. Bindings regenerated (`InstallPreview.ts` now imports `Manifest`).
   - `remote::stage`: after the signature and SHA-256 checks, an untrusted key returns the staged file with no manifest. `run_manifest` (slug, protocol, and the version rule for an update) runs only for a trusted key, and checks that again itself.
   - `Supervisor::accept_key` now runs on `spawn_blocking`:
     - It takes the staged release out of the state, so the mutex is not held while the file runs.
     - `remote::accept_staged` hashes the staged file again: a different size or SHA-256 is `release_hash_mismatch`, a missing file is the new `hub_preview_gone`. It then sets `accepted`, runs the manifest step, and returns the full preview.
     - On any failure the preview is dropped and the key is not recorded as accepted.
     - A preview whose key is already trusted is returned unchanged.
   - `take_staged`, `install_staged` and `update_staged` refuse through `Staged::manifest()` unless the key is trusted and the manifest was read (`hub_key_not_accepted`, `hub_key_changed`). The sha256 check is unchanged.
   - CLI (`install`, `update`):
     - It shows the key-pending preview: "its name and version are read from the executable once its key is trusted".
     - It then asks to trust the key, or takes `--accept-key`; `--yes` never trusts a key. Without a terminal or `--accept-key`, the key error and its hint, exit 1. Declining prints "Nothing was changed.", exit 0.
     - After the key is accepted: "Key accepted.", the full preview, then the install or update question.
   - Tests:
     - `a_changed_key_is_refused_until_accepted` (was hub.rs:1041) now asserts no manifest before the acceptance, and the manifest's version after it.
     - `an_asset_that_is_another_dashboard_or_not_one_is_refused` (was hub.rs:1172) now has the catalog vouch for the key, so the file is run at once.
     - New `a_release_signed_with_an_untrusted_key_is_not_run_before_it_is_accepted`. The asset is a signed non-executable.
       - Unknown key: the preview comes back key-pending (never `hub_manifest_failed`) and the install is refused (`hub_key_not_accepted`). Accepting it runs the file: `hub_manifest_failed`, and the preview is dropped.
       - Changed key on an update: key-pending, the apply is refused (`hub_key_changed`), accepting gives `hub_manifest_failed`, and the registry and `bin/` are unchanged.
       - Staged file deleted: accepting gives `hub_preview_gone`. Previewed again and accepted: the manifest is read, the update applies, and the new key is pinned.
       - An install whose staged file was deleted: `hub_preview_gone`.
     - Unit tests: `remote::tests::a_release_whose_key_is_not_trusted_is_neither_run_nor_installed` and `cli::tests::a_preview_without_a_manifest_says_why`.
     - The CLI test now checks that the name is printed only after "Key accepted.".
2. **P2: `--eario-install` never replaces a GitHub entry.**
   - New `LaunchError::InstalledFromGitHub { slug, repo }`, code `launch_installed_from_github`: "`<slug>` is installed from GitHub <repo>; uninstall it in the hub first, then install this executable again".
   - It is checked before the question, and again under the lock after it.
   - Tests: unit `launch::tests::a_dashboard_installed_from_github_is_never_replaced`, and `install_never_replaces_a_dashboard_installed_from_github` on the real data-explorer (registry and copy untouched).
   - `--eario-uninstall` still removes a GitHub entry. It is an explicit removal, and the finding did not cover it.
3. **P3: one lock for every read-modify-write.**
   - New `HubDir::lock()`, returning a `HubLock`, and `HubDir::lock_path()` in `eario-runtime/src/hub_dir.rs`. The lock is `hub.lock` in the hub directory, taken with std `File::lock` and released on drop or when the process ends.
   - It is held around:
     - `install_staged`, `update_staged`, `rollback_entry` and `remove_installed` (the registry, with the file moves in `bin/`);
     - `add_catalog` (after its GitHub read) and `remove_catalog`;
     - `set_check_updates_at_start`;
     - `--eario-install` (after the confirmation, through the copy and the write) and `--eario-uninstall`;
     - the creation and the cleaning of staging areas.
   - It replaces the in-process `files` mutex.
   - It is never held while asking the user or reaching GitHub, and is never taken twice by one thread (it is not reentrant, as its documentation says). `updates.json` is written whole, never read and written back, so it needs no lock.
   - The `cli.rs` module documentation now describes the lock.
   - Tests:
     - `hub_dir::tests::the_hub_lock_excludes_every_other_holder_until_dropped`: while the lock is held, `try_lock` from any other handle is `WouldBlock`, which is deterministic; a second writer thread gets the lock only after the first drops it.
     - Cross-process: `a_command_waits_while_another_process_changes_the_hub_directory` (the test holds the lock; `eario-hub check-at-start off` has neither exited nor written anything after 1 s, then finishes once the lock is released) and `install_waits_while_another_process_changes_the_hub_directory` (`data-explorer --eario-install --yes`; no copy and no registry after 2 s).
     - On a correct build these two cannot fail. A very slow child could only make them pass falsely.
4. **P3: one staging area per process.**
   - New `remote::StagingArea`, one per supervisor, so one per process:
     - `staging/<pid>-<n>/`, created under the hub lock, holding a `lock` file that stays locked for the area's lifetime;
     - one directory per preview inside;
     - removed when the supervisor is dropped.
   - `clean_staging` at hub start runs under the hub lock. It removes stray files, areas without a lock file, and areas whose lock it can take; it keeps the others.
   - `hub_preview_gone` when a staged file has gone: at `accept_key`'s second hash, and at the install's move.
   - Tests: unit `a_hub_start_removes_only_the_staging_areas_nobody_holds`; integration `a_starting_hub_keeps_the_previews_of_running_processes`. In that test, another supervisor's key-pending preview survives a hub start, then is accepted and installed. The stale areas, an area without a lock and a stray file are removed, and the area is gone once its supervisor is dropped.

Known limitations after this pass, replacing items 2 to 4 of the list above:
- `--eario-install` now refuses a GitHub entry.
- The CLI and the hub share the lock.
- A hub start keeps the staging area of a running process. A process killed during a preview still leaves its area until the next hub start, which removes it because nobody holds its lock.

Files changed in this pass:
- `crates/eario-core/src/launch.rs`
- `crates/eario-runtime/src/hub_dir.rs`
- `crates/eario/src/error.rs`, `crates/eario/src/launch.rs`
- `crates/eario-hub/src/remote.rs`, `supervisor.rs`, `cli.rs`, `http.rs` (`hub_preview_gone` mapped to 404), `lib.rs` (documentation)
- `crates/eario-hub/tests/hub.rs`
- `examples/data-explorer/tests/launch.rs`
- `frontend/src/lib/generated/InstallPreview.ts` (generated)

Outside the ticket's list and needed: the `crates/eario` and `eario-runtime` files (findings 2 and 3) and the data-explorer test. No route was added; no dependency was added.

Re-review (opus/high, base 310f2a2, after correction pass 1): REVIEWED — no findings; all four review-1 findings resolved in code, every manifest-run path traced to a trusted key. Parent re-ran fmt, `cargo test --workspace --all-features -j 4` (0 failures), gen-types (no diff beyond the ticket's bindings). Accepted. Carried forward: examples declare version 0.0.0 (ticket 08 must version releases); ADR/glossary still say 'fingerprint' (tickets 05/09); `minisign.pub` release asset convention for repos outside a catalog (ticket 08); uninstall drops the pinned key (document in 09); Unix flock untested locally (CI).

## Blockers / Challenges

- The first `cargo test --workspace --all-features` failed while compiling: `memory allocation of 2129936 bytes failed`, then `link.exe` exit 1102 (out of memory). The machine had 9.4 GB of free virtual memory. With `-j 4` it built and every test passed. This is the environment, not the change. CI runners build fewer crates at a time.
- Long heredocs in the Bash tool lose backslashes, as ticket 03 found. Edits were written as Python scripts in the session scratchpad and run from there.
- clippy's `large_enum_variant` failed the first clippy run on `remote::Purpose`. Fixed by boxing the update's `RegistryEntry`, then clean.
- `crates/eario-hub/src/http.rs` was CRLF in the working copy (as was `github.rs` before this ticket). I converted `http.rs` to LF. `.gitattributes` (`eol=lf`) normalizes on commit either way.
- vitest cannot run on the local Node 22.14, and `cargo xtask e2e` was not run (see Local Test Result).

## Commits

- 76e6ad2 feat: hub installs, updates and rolls back dashboards from signed GitHub releases, with catalogs, key pinning and a CLI (ticket 04)

## Time Spent

1h 52m (ticket-start.js to ticket-complete.js)
