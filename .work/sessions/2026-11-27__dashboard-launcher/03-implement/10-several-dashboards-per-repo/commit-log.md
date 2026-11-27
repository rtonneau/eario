# Ticket 10: several-dashboards-per-repo

**Status:** ✅ Done

## Local Test Result

Parent (after the implementer was cut off by the usage limit): `cargo fmt --all -- --check` clean; `cargo test -p eario-hub --all-features -j 4` 89 unit + 39 integration pass; `cargo test --workspace --all-features -j 4` 0 failures; workspace clippy `--all-targets --all-features -D warnings` clean; `cargo +1.95.0 check -p eario-hub --all-features` ok; `cargo xtask gen-types` stable (only this ticket's CatalogEntry/HubCatalogEntry changes); `npm run check` 0/0; vitest 112/112 (local zstd shim, Node 22.14); `cargo xtask frontend` + `cargo xtask e2e` 15/15 passed, no axe violation.

## Review Notes

Parent: base 6d955c1f9158fbfd5acc8b20bc875b88c132fde0; tree clean except session files (ticket 10 added to the plan by the user's decision). Mode delegate (implementer opus/high). Recovery count: 0.

Recovery pass 1 (recovery count 1): the delegated implementer (opus/high) stopped on a 5-hour usage limit after its last edit (a doc link to a private item in remote.rs); it did not fill this log. Parent inspected the partial work instead of restarting: routes `/sources/{owner}/{repo}/dashboards/{slug}/preview|accept-key/{key_sha256}|install/{sha256}` (http.rs), CLI `install --slug` (cli.rs), per-repository pin (`repo_pin`/`pin_for_repo`, remote.rs), catalog dedup by (repo, slug) (`list_catalog`), slug-aware catalog/view (HubSources.svelte, api.ts), tests `two_dashboards_of_one_repository_install_side_by_side_and_update_on_their_own` and `a_dashboard_of_a_repository_that_publishes_several_installs_over_http_by_its_slug` (hub.rs), e2e 'installs two dashboards of one repository, by their slugs', docs (ADR 0013, security.md, overview.md, guide.md, release-dashboard.yml comment). All checks above pass; acceptance criteria appear covered, pending independent review.

Independent review (opus/high, base 6d955c1): all listed properties hold; 2 findings, both P3. (1) remote.rs `refuse_pin_changed` / supervisor.rs `KeyPinnedMeanwhile`: message claimed another key was pinned when the pin was removed by an uninstall → parent reworded the message and doc to cover both (refusal behaviour unchanged; no new test for the wording). (2) HubSources.svelte: an ambiguous catalog install focused the slug field with the repository field empty or wrong → parent fills the repository with the entry's repo; vitest 'fills the repository when a catalog entry publishes several dashboards' fails without the fix, passes with it. Correction pass (parent, inline); not re-reviewed (wording + 3-line UI change, verified red/green). Parent re-ran: fmt, workspace clippy -D warnings, hub tests 89+39, vitest 113/113 (shim), npm run check 0/0, e2e 15/15. Accepted.

## Blockers / Challenges

Implementer cut off by the usage limit before writing its report; parent verified the tree itself.

## Commits

- 97c295c feat: one repository can publish several dashboards, installed by slug (ticket 10)

## Time Spent

1h 26m (ticket-start.js to ticket-complete.js)
