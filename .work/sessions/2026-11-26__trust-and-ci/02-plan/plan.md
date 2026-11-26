# Implementation Plan

**Session:** trust-and-ci
**Date:** 2026-11-26T20:07:29.452Z
**Estimated effort:** 1–2 days

## Strategy

Start with the Rust-side checks, which are independent and quick: cargo-deny, then the minimum Rust version. Then the browser layer: the Playwright harness and its flows first, then axe on top of it, which may require frontend fixes. Then the benchmark command and its first dated result. Last, the milestone documentation. Each ticket documents its own command in the development guide; the last ticket updates status, roadmap and AGENTS.md together.

## Tickets Overview

- **Ticket 1:** cargo-deny configuration and CI job.
- **Ticket 2:** the minimum Rust version, found, declared and tested in CI.
- **Ticket 3:** Playwright e2e harness, flows on data-explorer and signal-lab, `cargo xtask e2e`, and a CI job.
- **Ticket 4:** axe-core accessibility check in every e2e state, both themes, with the frontend fixes it requires.
- **Ticket 5:** `cargo xtask bench` writing a dated results file; first result committed.
- **Ticket 6:** M2 documentation: project status, roadmap, AGENTS.md, README.

## Sequencing Rationale

Tickets 1 and 2 touch only Cargo and CI and unblock nothing else, so they land first and small. Ticket 4 needs the harness of ticket 3. Ticket 5 is independent but sits after the CI work so its commit refers to a stable tree. Ticket 6 describes the final state, so it comes last.

## Risks & Mitigation

- **Risk:** a dependency floor above 1.95 (for example a crate without a declared rust-version) → **Mitigation:** move up one minor version at a time until build and tests pass, and record the version found.
- **Risk:** cargo-deny reports an existing advisory → **Mitigation:** update the crate if possible; otherwise add a documented, scoped `ignore` entry with the reason.
- **Risk:** flaky e2e tests (startup timing, canvas rendering) → **Mitigation:** wait on the printed address and on accessible roles/status messages, never on fixed sleeps; check the canvas through its rendered size or an ECharts instance, not pixels.
- **Risk:** axe finds many violations (colour contrast in a theme) → **Mitigation:** fix them via the tokens of `app.css`; if a fix needs a product decision, stop and ask.
- **Risk:** Playwright Chromium install on Windows locally → **Mitigation:** `cargo xtask e2e` runs `npx playwright install chromium` first.

## Assumptions

- `cargo-deny` and the extra rustup toolchains can be installed on the development machine.
- The examples print their address (with the token) on stdout at startup, as `server::run` does today.
- Network access is available in CI to install Chromium and cargo-deny; the application itself stays offline.
