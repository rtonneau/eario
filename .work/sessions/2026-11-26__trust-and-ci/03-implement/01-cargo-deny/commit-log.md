# Ticket 01: cargo-deny

**Status:** ✅ Done

## Local Test Result

`cargo deny check` (cargo-deny 0.22.1), exit 0: `advisories ok, bans ok, licenses ok, sources ok`, with 8 `warning[duplicate]` (base64, constant_time_eq, foldhash, getrandom, hashbrown, quick-xml, syn, windows-sys). Log: `.scratch/tests/2026-11-26__trust-and-ci/01-deny.log`.

## Review Notes

Base: 111e8e6d885dfa303fc1e87bcc800711034cb1fb. Pre-existing changes not owned by this ticket: `.work/work-map.html`, `.work/work-map.json` (user's, left untouched).

Against the acceptance criteria:
- Licence allow-list: exactly the 14 licences found in the tree (MPL-2.0 commented). All workspace crates declare `MIT OR Apache-2.0`, so `[licenses.private]` is not needed.
- Vulnerabilities are errors; yanked is `warn`; sources: crates.io only (unknown registry and git: deny); duplicates: `warn`.
- **Deviation:** cargo-deny 0.22 has no `warn` level for unmaintained crates, only a scope. It is set to `workspace`: an error for a direct dependency, ignored for a transitive one. The guide states this.
- CI job `Dependencies (cargo deny)` with `EmbarkStudios/cargo-deny-action@v2`, `check --all-features`. Not run in CI yet (it will run on the PR).
- Guide section "Dependency checks" added.

## Blockers / Challenges

The first run failed on RUSTSEC-2026-0194 and RUSTSEC-2026-0195 (quick-xml 0.39.4). Its only path is polars 0.55.2 → polars-io `cloud` (always on through polars-lazy/polars-stream `file_cache`) → object_store 0.13.2 (pins `quick-xml ^0.39`), which parses XML responses of S3 and Azure. Eario does no network I/O and calamine uses the fixed quick-xml 0.42, so the vulnerable code is never reached. No newer polars exists (0.55.2 is the latest). Following the plan's mitigation, both IDs are ignored with a reason and a removal condition.

## Commits

- 81a802a ci: check dependencies with cargo deny (ticket 01)

## Time Spent

5m (ticket-start.js to ticket-complete.js)
