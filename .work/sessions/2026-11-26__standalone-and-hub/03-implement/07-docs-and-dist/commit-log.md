# Ticket 07: docs-and-dist

**Status:** ✅ Done

## Local Test Result

Ran with CARGO_BUILD_JOBS=4 (evidence in `.scratch/tests/2026-11-26__standalone-and-hub/evidence/`):
- `npx -y -p node@24 -- cargo xtask check` (07-check.txt): exit 0. fmt, clippy -D warnings, all Rust tests (eario-core 76, eario-runtime 72, eario 25+5, eario-hub 4+8, eario-data 30+12, launch tests of both examples, xtask 4), doc, svelte-check, Vitest 10 files / 65 tests pass.
- `npx -y -p node@24 -- cargo xtask e2e` (07-e2e.txt): exit 0, 10 Chromium tests pass (4 data-explorer, 2 signal-lab, 4 hub), axe checks included.
- `cargo xtask dist` (07-dist.txt): exit 0, printed `D:\DEV\RUST\eario	arget
elease\data-explorer.exe` (32,182,272 bytes) and `D:\DEV\RUST\eario	arget
elease\eario-hub.exe` (5,824,000 bytes).
- No eario-hub, data-explorer or signal-lab process left (Get-Process: 0).
- Not run: `cargo deny check` (not installed) and `cargo +1.95 test --workspace --all-features` (toolchain not installed); both run in CI. The Verification Step is therefore only partly run.

## Review Notes

Parent: base 056ce0145fd6315c5fadb7c9264669fc86df12e3. No pre-existing changes outside .work/. Mode: delegate.

Criterion 1: updated `docs/project-status.md` (M3 rows, what tests cover vs hand checks, no-tray and the other known limits, hub directory paths), `docs/roadmap.md` (M3 done, tray moved to M11, what was left out), `docs/architecture/overview.md` (eario-hub in the crate graph and table, launch contract, hub directory, data flow diagram, hub HTTP API, frontend hub view), `README.md` (hub section, only what works; tray labelled planned), `docs/development/guide.md` (Node versions and the `npx -p node@24` workaround, `EARIO_HUB_DIR`, e2e building the hub and its temporary hub directory, hub tests row, dist). Criterion 2: `xtask/src/main.rs` dist builds `eario-hub` in release and prints both absolute paths (honours CARGO_TARGET_DIR). Criterion 3: check and e2e pass; deny and MSRV not run locally (above). AGENTS.md checked: lists crates/eario-hub and the boundary line is accurate; not changed. CI uses Node 24 (ci.yml, release.yml). No review rounds. Pre-existing and left alone: project-status says version 0.1.0 while Cargo.toml is 0.2.0; cargo prints manifest lint warnings (homepage redundant, missing [lints]) on every command.

Parent review (inline; documentation and a small xtask change, no independent reviewer needed): read the README, roadmap and xtask diffs against what tickets 01-06 committed. The README lists only working features with the tray labelled planned. The roadmap marks M3 done except the tray, which is moved to M11. `dist` honours CARGO_TARGET_DIR like `e2e`. Checked in code that the hub opens the browser at start (crates/eario-hub/src/lib.rs:105-134, 209), as the README says. Fixed inline: the stale "Version: 0.1.0, unreleased" line in project-status became "0.2.0 (tagged), M3 work unreleased" (tag v0.2.0 exists).
Definition of done: `cargo xtask check` (vitest via Node 24) and `cargo xtask e2e` (10 tests) pass per evidence/07-*. Not run locally: `cargo deny check` (not installed) and MSRV 1.95 tests (toolchain not installed); CI runs both.
Acceptance: criteria met except the two not-run checks, delegated to CI; requested model haiku (fast, medium), actual sonnet (medium), chosen for doc accuracy across many files.

## Blockers / Challenges

None blocking. A shell heredoc containing quotes broke once; the doc edits were redone as scripts in the scratchpad. The release build took about 6 minutes.

## Commits

- 4b6ec26 docs: M3 standalone and hub documented; dist builds the hub (ticket 07)

## Time Spent

17m (ticket-start.js to ticket-complete.js)
