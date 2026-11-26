# Session: trust-and-ci

**Date:** 2026-11-26T19:48:49.843Z
**Status:** Grill phase complete

## Problem Statement

Milestone M2 (Trust and continuous verification) has five open items. No test runs the frontend in a real browser. Dependencies are not checked for licences or advisories. No minimum Rust version is declared or tested. Accessibility is not checked automatically. The benchmarks have never been run, and no result is recorded.

## Context & Constraints

- **Current behavior:** CI (`.github/workflows/ci.yml`) has three jobs, all green: frontend (check, Vitest in jsdom, build), Rust on ubuntu and windows (fmt, clippy, tests, docs, build without features), and contract bindings. `rust-toolchain.toml` pins `stable` (local 1.99.1), with edition 2024 and no `rust-version`. Criterion benches exist in `crates/eario-data/benches/tabular.rs`. The server can be driven from tests with `EARIO_PORT` and `EARIO_NO_BROWSER`, and the token travels as `/#token=`. M1 is merged (`f88fb91`): the examples are `data-explorer`, `hello-dashboard` and `signal-lab`.
- **Pain point:** the frontend was only checked by hand, with headless screenshots. Dependency licences and advisories are not checked. A user on an older toolchain gets no guarantee. The performance claims have no dated evidence.
- **Dependencies:** keep the existing CI jobs and `cargo xtask check` (it stays fast and does not run e2e). The frontend stays generic. Server security (Host/Origin check, token) is unchanged: the tests go through it.
- **Tech stack:** `@playwright/test` and `@axe-core/playwright` (new frontend dev dependencies), `cargo-deny` through `EmbarkStudios/cargo-deny-action`, criterion 0.8, the xtask crate, GitHub Actions.

## Success Metrics

- `cargo xtask e2e` builds the frontend and an example, starts real servers, and passes Playwright tests in Chromium. They cover import through the file picker, table selection and pagination, a filter, a chart drawn in its canvas, and one M1 input that changes a derived view. A new ubuntu CI job runs the same suite.
- axe-core runs on every page state the e2e tests visit, in light and dark themes, and reports zero WCAG 2.1 A/AA violations. Violations found now are fixed in the frontend.
- `cargo deny check` passes locally and in a CI job. A dependency under a licence outside the allow-list, or with a RustSec vulnerability, fails it.
- `rust-version` is declared in `[workspace.package]` as the lowest toolchain (from 1.95 up) on which the workspace builds and its tests pass. A CI job builds and tests with exactly that version.
- `cargo xtask bench` writes `docs/benchmarks/YYYY-MM-DD.md` with the results, CPU, RAM, OS, rustc and commit. One dated file from the development machine is committed.
- `docs/project-status.md`, `docs/roadmap.md` (M2 done), `docs/development/guide.md` and the AGENTS.md command list describe the new commands and checks. Every command listed exists.

## Architecture & Approach

1. **End-to-end tests:** they live in `frontend/e2e/`, with a Playwright config in `frontend/`. A fixture starts an example executable on a free port (`EARIO_PORT`, `EARIO_NO_BROWSER=1`), reads the printed address (it carries the token), and opens it. `data-explorer` serves the import, table, filter and chart flows; `signal-lab` serves the M1 input and derived-view flow. Chromium only. `cargo xtask e2e` runs the frontend build, the example builds and `npx playwright test`. The CI job runs on ubuntu, reuses the `frontend-dist` artifact and installs Chromium with its system dependencies.
2. **Accessibility:** a shared helper runs `AxeBuilder` with the WCAG 2.1 A/AA tags, in both themes, after each visited state. The test fails on any violation. Fixes go into the generic Svelte components and use the colour tokens of `app.css`.
3. **`cargo deny`:** a `deny.toml` at the root. Licences: an explicit allow-list of today's set (the permissive licences plus MPL-2.0). Advisories: a vulnerability is an error; unmaintained or yanked is a warning. Sources: crates.io only. Bans: duplicate versions are a warning. It gets its own CI job.
4. **Minimum Rust version:** try 1.95, then go up until build and tests pass. Declare that version as `rust-version = "1.x"` in `[workspace.package]`, inherited by every crate. A CI job installs that toolchain explicitly, overriding `rust-toolchain.toml`, and runs `cargo build` and `cargo test --workspace --all-features`.
5. **Benchmarks:** `cargo xtask bench` runs `cargo bench -p eario-data`, reads criterion's estimates from `target/criterion`, collects machine information and the git commit, and writes the dated Markdown file. Not run in CI.
6. **Documentation:** the files listed in the Success Metrics. The README lists only what works.

## Assumptions & Trade-offs

- Chromium on ubuntu only. Firefox, WebKit and Windows are not run in e2e: the frontend uses no engine-specific API, and the cost stays low.
- `cargo xtask check` does not run e2e: it stays fast and needs no browser install. The documentation says CI also runs e2e.
- GitHub runners are not a reference machine, so benchmarks are not run in CI.
- MPL-2.0 is accepted: it is file-level copyleft and compatible with distributing a static binary.
- The minimum Rust version is raised only on purpose. A dependency upgrade that needs a newer toolchain fails the CI job and forces that decision.
- Not doing: performance regression gates, visual snapshot tests, coverage reporting.

## Open Questions

None. The remaining choices are implementation details (exact selectors, the final minimum Rust version found by testing, the axe fixes needed).

## Notes

- The session waited for M1 to merge, so the e2e tests can cover the M1 inputs (`signal-lab`).
- The dependency floor today is `sysinfo` (rust-version 1.95).
- The licences in the dependency tree today: MIT, Apache-2.0 (also with LLVM-exception), BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, CC0-1.0, MIT-0, 0BSD, BSL-1.0, Unlicense, MPL-2.0. LGPL-2.1 appears only as an OR alternative.
- New dev dependencies (`@playwright/test`, `@axe-core/playwright`) need a justification in the PR, as AGENTS.md requires.
