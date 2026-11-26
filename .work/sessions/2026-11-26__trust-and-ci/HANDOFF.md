# Handoff: trust-and-ci

**Session:** 2026-11-26__trust-and-ci
**Saved:** 2026-11-26T19:59:39.739Z
**Current phase:** grill
**Active ticket:** none

## Where I Stopped

Grill round 1 of M2 (Trust and continuous verification) answered. Paused on the user's choice to wait until M1 (PR #1, branch feat/custom-rust-logic) is merged into main.

## Reasoning So Far

Sized Standard. Facts gathered: CI (.github/workflows/ci.yml) runs frontend, Rust on ubuntu+windows, and contract jobs, all green (run 45779398561). No `rust-version` in any Cargo.toml (rust-toolchain.toml = stable, edition 2024). Benches exist only in crates/eario-data/benches/tabular.rs, never run. Frontend tests are Vitest in jsdom; no real-browser test. The server URL carries the token as `/#token=`; EARIO_PORT and EARIO_NO_BROWSER exist for driving a server from tests. Starting on main now and stacking on the M1 branch were both rejected: the user wants M1 merged first, so the e2e tests can cover M1's inputs from a stable base.

## Next Step

Check that PR #1 is merged (`git fetch && git log origin/main`), pull main, then continue the grill with round 2: MSRV policy (lowest version that builds vs stable minus N), cargo-deny licence allow-list and advisory handling, how dated benchmarks are recorded (file location, xtask command, which machine), and whether the e2e suite also runs in `cargo xtask check` locally.

## Open Questions

Blocked on the merge of M1 (PR #1). Round 2 questions are listed under Next Step.

## Settled Decisions (do not re-litigate)

- Scope: all five remaining M2 bullets in one session: Playwright e2e, cargo deny, MSRV in CI, automated a11y check, dated benchmarks.
- Base: wait for M1 to merge, then branch from main.
- Playwright: Chromium only, ubuntu only, in CI.
- Accessibility: axe-core inside the Playwright suite; fail CI on any WCAG 2.1 A/AA violation, and fix what it finds.

## Uncommitted Work

- **Git status (project):** clean
- **Git status (session dir):** ?? .work/sessions/2026-11-26__trust-and-ci/.session-config.json, ?? .work/sessions/2026-11-26__trust-and-ci/01-grill/resume.md
- **Why not committed:** Only gps session state exists so far. There is no code change yet, because the session is waiting on M1.

## Machine State (auto-filled)

- **Ticket queue:** no tickets yet
- **Recent commits:** no commits yet
