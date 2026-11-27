# Ticket 08: release-workflow

**Status:** ✅ Done

## Local Test Result

Evidence is in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/`, files `08-*`. All runs are on the development machine: Windows 11, Git Bash, Node 22.14.0.

- `CARGO_BUILD_JOBS=4 cargo xtask check` (`08-xtask-check.txt`) stops at its last step, vitest, with the known Node 22.14 gap: 11 "Failed to start forks worker" errors, as in earlier tickets. Every step before it passed:
  - fmt;
  - clippy `--workspace --all-targets --all-features -D warnings`;
  - `cargo test --workspace --all-features`: 397 passed, 0 failed, including the new `verify::tests::the_list_the_release_workflow_writes_and_signs_is_read` and the 36 tests of `crates/eario-hub/tests/hub.rs`;
  - `cargo doc` with warnings denied;
  - svelte-check (0 errors, 0 warnings) and `tsc -p e2e`.

  `xtask check` runs these with `CARGO_BUILD_JOBS=4`, so clippy and the tests were not run a second time with `-j 4`.
- `cargo fmt --all -- --check` (`08-fmt.txt`): clean.
- vitest with `NODE_OPTIONS="--require …/07-zstd-shim.cjs"` (`08-vitest-shim.txt`): 11 files, 107 tests pass.
- `CARGO_BUILD_JOBS=4 cargo xtask e2e` (`08-e2e.txt`): 14 passed. This includes the 4 "hub with GitHub" specs, which now use the workspace version.
- Workflows:
  - actionlint 1.7.7 (`08-actionlint.txt`): 0 errors on release.yml, release-dashboard.yml and ci.yml. The binary is pinned and its SHA-256 was checked against the release's checksums file; it is in the scratch dir.
  - actionlint's own shellcheck integration hung on Windows and was stopped. Instead, I extracted every `run:` script of release-dashboard.yml with PyYAML 6.0.3 and checked them with shellcheck 0.10.0, `-s bash` (`08-shellcheck.txt`): clean.
  - PyYAML parses both workflows.
- I ran the workflow's shell steps locally, extracted from the YAML:
  - Plan step (`08-plan-simulation.txt`), 23 cases:
    - Eario as committed is refused with the placeholder message.
    - With a real key file it gives the expected outputs.
    - These are each refused: tag ≠ version, non-SemVer tag, an unknown or duplicate target, no target, a bad or duplicate slug, a missing package or binary, an executables input that is not JSON / an empty array / has a non-string value, a bad probe, a key file with a blank line or a short key line, a missing key file, and a run that is not for a tag.
    - A CRLF key file is accepted.
    - A jq shim drops the CR that the Windows jq.exe adds; ubuntu's jq does not add it.
  - Build step (`08-build-simulation.txt`): a cargo shim built nothing, and the step ran against the 0.2.0 debug executables from e2e.
    - The manifest of data-explorer and signal-lab and the hub's `--version` pass.
    - A wrong version, a wrong slug, and a wrong hub version each fail with a clear error.
    - `probe: none` skips the check, and a foreign target gives a notice.
  - Release job (`08-release-simulation.txt`), run with minisign 0.12 (Windows build, pinned; its zip verified with minisign's published key) and a throwaway password-protected key:
    - Writing SHA256SUMS.txt in text mode, signing with the password on stdin, `minisign -V -H` against the public key, `sha256sum --check` and the 4-line `RU` check all pass.
    - The secret key file is gone after the step.
    - A wrong password, an empty secret, verification against another key and a missing asset each fail.
  - CLI facts I checked on minisign 0.12 itself: `-S` pre-hashes by default (`-l` makes a legacy signature); `-V -H` refuses a legacy signature; a password piped on stdin works, and a `-W` key ignores it.
- The new unit-test vector is what `sha256sum --text` printed for four sample files, signed by minisign 0.12 with a throwaway key whose secret was deleted at once (`08-sha256sum-sample.txt`, `08-minisign-vector.txt`). The hub's `verify_sums` accepts it.
- Not run: the workflow on GitHub. That needs the release key's secrets and a tag. `rustup toolchain install` without arguments, apt's `minisign` on ubuntu-latest, and the cross build to x86_64-apple-darwin on macos-latest were therefore not exercised.

## Review Notes

Parent: base c12dd2da172bd308dfd95346e17ec5a52cba955e; tree clean except session files. Mode delegate (implementer opus/high; raised from medium because it handles a signing secret). Recovery count: 0.

Acceptance criteria:
- **release-dashboard.yml** (`workflow_call`), a new file.
  - **Inputs:**
    - `executables`: a JSON array of `{package, binary?, slug?, probe?}`, in place of single package/slug inputs; see below.
    - `targets`: default all four.
    - `public-key`: the committed `.pub` file, default `minisign.pub`.
    - `attach-public-key`: default true.
    - `publish`: default false, which leaves a draft.
    - `tag`, `working-directory`, `node-version`, `npm-lock-file`, `before-build`.
  - **Secrets:** `MINISIGN_SECRET_KEY` (required) and `MINISIGN_PASSWORD` (optional).
  - **Jobs:**
    - `plan` (ubuntu) validates the tag (`v<SemVer>`), the public key file, the targets, the executables, and that each package's version equals the tag's (via `cargo metadata`).
    - `build` runs a matrix built from the plan: windows-latest, ubuntu-latest, and macos-latest for both macOS triples. x86_64-apple-darwin is cross-compiled, because GitHub's Intel macOS runners are deprecated or retired. It runs `cargo build --release --locked --target T -p P --bin B` once per package, so features are not unified across examples. It renames each executable to `<slug>-<triple>[.exe]`. On native targets it runs `--eario-manifest` (checks the slug and the tag version) or `--version` (expects `<slug> <version>`, exactly what the hub's self-update reads).
    - `release` (ubuntu, the only job with `contents: write`):
      - checks that the asset set is exactly slugs × targets, then writes `SHA256SUMS.txt` with `sha256sum --text`;
      - installs minisign from apt, writes the secret to a 0600 file in `RUNNER_TEMP` with a shell builtin, signs with the password on stdin, and deletes the key by a trap and an `if: always()` step;
      - verifies with `minisign -V -H` against the committed key, plus a 4-line / `RU` check and `sha256sum --check`;
      - creates a draft with `--verify-tag`, or uploads with `--clobber`, and publishes only after every file is attached, if `publish`.
  - **Hygiene:** no `${{ }}` inside any `run:` script; everything goes through `env:`. Checkouts use `persist-credentials: false`. Actions are pinned to major versions, as ci.yml does (`actions/checkout@v4`, `setup-node@v4`, `upload/download-artifact@v4`, `Swatinem/rust-cache@v2`).
- **release.yml** calls it on `v*` tags:
  - executables: data-explorer, signal-lab, and eario-hub with `probe: version`;
  - `public-key: crates/eario-hub/eario-release.pub`;
  - `before-build: cargo xtask frontend` with Node 24 and the npm cache;
  - `publish: false`, a draft, as before;
  - secrets passed explicitly, not `inherit`.
  - While the key file is the placeholder, `plan` refuses with a clear message before anything is built. This is the release-time check that no hub release is signed while the placeholder is in place. The placeholder file is unchanged.
- **Versions.**
  - The three examples now take `version.workspace = true`, and Cargo.lock was updated: 0.0.0 → 0.2.0 for data-explorer, hello-dashboard and signal-lab. **Commit Cargo.lock with this change: the workflow builds with `--locked`.**
  - data-explorer and signal-lab already used `.version(env!("CARGO_PKG_VERSION"))`. hello-dashboard declares no `.version` and is not released; I left it unchanged.
  - Tests that relied on 0.0.0 now derive the version:
    - `crates/eario-hub/tests/hub.rs`: `EXAMPLE_VERSION`, `TAG`, `EARLIER_1/2` (pre-releases of the version), and `AHEAD = "500.0.0"`, with tag `v900.0.0` for the "newer tag, older executable" case. Its assertion that each example's manifest version equals `CARGO_PKG_VERSION` checks that the manifest version comes from the workspace.
    - `fake-github.rs`: `TAG = concat!("v", env!("CARGO_PKG_VERSION"))`.
    - The e2e specs: `EXAMPLE_VERSION` is read from `[workspace.package]` in `Cargo.toml`, in `frontend/e2e/app.ts`.
- **Key generation** is documented, not an xtask subcommand, so there is no new dependency. `docs/development/guide.md` gains a section "Publishing a release": `minisign -G -p minisign.pub -s minisign.key` (`-W` for no password), the rsign2 equivalent, `gh secret set`, never committing the `.key`, and the steps to release Eario. Nothing secret is committed; the throwaway keys were deleted.
- **Optional unit test:** `verify.rs` gains `the_list_the_release_workflow_writes_and_signs_is_read`, built on real `sha256sum --text` output and a real minisign 0.12 signature.
- **Docs:** the guide's e2e paragraph now names the workspace version. `project-status.md` "Not verified" describes what was checked and what was not. ADR 0013's consequences mention the workflow and the ambiguity below.

Changes after self-review:
- The key check moved before the targets and `cargo metadata`, so the placeholder refusal comes first.
- `cargo build` reads `/dev/null`.
- Empty lines of the executables output are skipped.
- An `ls` became `find -printf`.
- The trailing commas were dropped from the manifest grep patterns.
- The quotes around `"version"` keep `eario_version` from matching.

Independent review 1 (opus/high, base c12dd2d): 3 findings. P2 the tag input did not change the built commit → plan/build/release now check out the tag. P3 a published release was rewritten in place → the attach step refuses unless the release is a draft. P3 signed builds restored shared caches → rust-cache and the npm cache removed; the `npm-lock-file` input (listed above) was removed with it. Correction pass 1 (parent, inline). actionlint 1.7.7 clean. Re-review (sonnet/medium): no findings. User decision (27 Nov 2026) on the implementer's note that one repository hosts one dashboard: let a repository host several (optional slug in catalog entries and previews) → new ticket 10, shipped before 09. Accepted; workflow not run on GitHub (needs secrets and a tag).

## Blockers / Challenges

- **Design finding for the parent and ticket 09.** The hub can install from a repository only when its release has one executable per target. `remote.rs::find_asset` refuses two (`hub_ambiguous_asset`), and `refuse_installed` allows one dashboard per repository. Eario's release carries data-explorer, signal-lab and the hub, so its examples cannot be installed from `rtonneau/eario` by a hub; they are downloads only. The hub's self-update is not affected: it asks for `eario-hub-<triple>` by name. This is documented in the workflow header, release.yml, the guide and ADR 0013. Making the examples installable would need a slug-aware first install or separate repositories, which is out of this ticket's scope.
- **Input shape.** The AC says "inputs package, slug". One release has one `SHA256SUMS.txt`, and Eario's release has three executables, so the reusable workflow takes `executables` as a JSON list rather than single package/slug inputs. Calling it three times would have overwritten the signed list.
- **Example version 0.0.0 → 0.2.0.** About 60 hard-coded `0.0.0` versions in hub.rs and 10 in hub.spec.ts depended on the old version. All now derive the version from the workspace.
- **Local tool quirks:**
  - actionlint's shellcheck integration hung on Windows; I stopped it and ran shellcheck on the extracted scripts instead.
  - The Windows jq.exe emits CRLF, so the local simulation uses a CR-stripping shim. The workflow runs jq on ubuntu only.
  - The Windows minisign writes CRLF signature files; the unit-test vector uses the LF form that Linux writes.
- **Unverifiable here:**
  - the apt minisign version on ubuntu-latest: Ubuntu 24.04 should ship 0.11, which pre-hashes by default and has `-V -H`. The `-V -H` and `RU` checks fail the job if it does not.
  - `rustup toolchain install` with no argument, which needs rustup 1.28 or later and should be on the runners;
  - cross-compiling x86_64-apple-darwin on Apple silicon;
  - `gh release view` finding a draft by tag on a re-run.
- vitest under Node 22.14 fails to start workers inside `xtask check`, the known gap; it passes with the shim.

## Commits

- 20a7ec8 ci: reusable signed release workflow for dashboards; Eario releases the examples and the hub through it (ticket 08)

## Time Spent

54m (ticket-start.js to ticket-complete.js)
