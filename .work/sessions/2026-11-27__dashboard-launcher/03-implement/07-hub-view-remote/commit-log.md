# Ticket 07: hub-view-remote

**Status:** ✅ Done

## Local Test Result

Evidence in `.scratch/tests/2026-11-27__dashboard-launcher/evidence/`, prefix `07-`, one file per command. Every command below ran on the final tree unless said otherwise.

- `cargo xtask e2e` (with `CARGO_BUILD_JOBS=4`): exit 0, 14 passed, 0 failed, no axe violation in either theme (`07-e2e.txt`). That is the 10 earlier specs plus 4 new ones in `hub.spec.ts`, "hub with GitHub", against the real debug hub and the new `fake-github` process:
  - installs from a catalog (catalog key), then updates (pinned key) and rolls back;
  - refuses an update signed with a changed key in the dialog (role=alert, no Update button, Escape cancels, focus back on Update, version unchanged), then a check that could not reach GitHub (`github_rate_limited`, listed and announced);
  - installs from a repository with an unknown key once the user trusts it (key-pending preview, Trust this key, full preview, install);
  - adds and removes catalogs (shape check first), turns the check at start off, and checks after a reload that the hub stored both.
  - `expectAccessible` runs after each new state: 20 states in the new tests, each in the light and the dark theme.
  - Earlier runs: run 1 had 2 failures (a label that matched two inputs; focus not returned because the Update button was disabled during the preview, so the opener was lost). Run 3 had 1 failure (a text locator matched the dialog's new status line too). All three were fixed; see Blockers. No process and no hub directory was left behind (`target/e2e-hub` empty, no `fake-github`, `eario-hub` or example process).
- `npm run check`: 406 files, 0 errors, 0 warnings, exit 0 (`07-npm-check.txt`).
- vitest: **ran locally** through a preload shim, without changing Node or any committed file. Node 22.14 lacks `zlib.createZstdDecompress`, which undici (through jsdom) binds at load. `07-zstd-shim.cjs` (session scratchpad, copied into evidence) defines it as a function that throws. `NODE_OPTIONS="--require <shim>" npx vitest run`: 11 files, 99 tests, 0 failed (`07-vitest-with-zstd-shim.txt`). New: `hub.test.ts` (12), 3 in `api.test.ts`, 19 in `HubView.test.ts` (38 there in all). CI runs vitest on Node 24 without the shim.
- Mutation check (`07-mutate.py`, output `07-mutation-check.txt`): 10 of 10 mutations killed by vitest, each file restored byte for byte (SHA-256 compared). The mutations: Install offered before the key is trusted; accept-key sent with the asset SHA-256; the changed key not an alert; polling going on after a self-update (survived on the first run, because the mock counted no unreachable request; the test now counts at `fetch`, then killed); Escape not cancelling; update controls on a local dashboard; no shape check before adding a catalog; the setting not put back after a refusal; the could-not-check list hidden; the error code dropped.
- `cargo fmt --all -- --check`: exit 0 (`07-fmt.txt`).
- `cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings`: exit 0 (`07-clippy.txt`). It builds the new `fake-github` binary. Only the known manifest warnings are printed (`redundant_homepage`, `missing_lints_inheritance`).
- `cargo test -p eario-hub --all-features -j 4`: exit 0, 82 unit and 36 integration tests, 0 failed (`07-hub-tests.txt`).
- `CARGO_BUILD_JOBS=4 cargo xtask check`: fmt, clippy, the workspace tests (every `test result: ok`, no `FAILED`), `cargo doc` and `npm run check` passed. It stops at `npm run test`: `Failed to start forks worker` for all 11 files, the known Node 22.14 gap; exit 1 (`07-xtask-check-jobs4.txt`). The same vitest run passes with the shim above.
- Not run:
  - `cargo deny check`: no dependency added.
  - The MSRV build: the new binary uses only std and items that earlier tickets already built with 1.95.
  - `cargo xtask gen-types`: no contract type changed.
  - The hub's self-update in the browser: the hub under test holds the placeholder key, so `available_version` is never set. vitest covers the button, the confirmation, the refusal, the restart notice and the stop of polling.
  - macOS and Linux: only Windows x86_64 ran here. CI runs the browser tests on ubuntu.

Correction pass 1 (after independent review 1), on the final tree; evidence prefix `07-fix-`:
- vitest with the zstd shim: 11 files, 106 tests, 0 failed, exit 0 (`07-fix-vitest.txt`). 7 new or changed tests: 4 in `HubView.test.ts`, 3 in `api.test.ts`, plus the existing trust-then-install test, which now waits for Install to be enabled. Before the fix, the 4 new `HubView.test.ts` tests and the 2 new refusal tests in `api.test.ts` failed (runs seen, not saved).
- `npm run check`: 406 files, 0 errors, 0 warnings, exit 0 (`07-fix-npm-check.txt`).
- `cargo test -p eario-hub --all-features -j 4`: exit 0, 83 unit (1 new) and 36 integration tests, 0 failed (`07-fix-hub-tests.txt`). The new test failed with the dedup check disabled (a temporary mutation, then restored; SHA-256 compared).
- `cargo fmt --all -- --check`: exit 0 (`07-fix-fmt.txt`). `cargo clippy --workspace --all-targets --all-features -j 4 -- -D warnings`: exit 0 (`07-fix-clippy.txt`).
- `cargo xtask frontend` (exit 0, `07-fix-frontend-build.txt`), then `CARGO_BUILD_JOBS=4 cargo xtask e2e`: 14 passed, 0 failed, no axe violation, exit 0 (`07-fix-e2e.txt`). No `fake-github`, `eario-hub` or example process was left, and `target/e2e-hub` is empty. A whitespace-only fix to `api.ts` (`app: () =>`) came after the frontend build began. vitest and `npm run check` were run again after it.
- Not run: `cargo xtask check` as a whole (it stops at vitest on Node 22.14, as before), `cargo deny check` (no dependency added), `gen-types` (no contract type changed).

## Review Notes

Parent: base a60ec33295ad447513539820e87a4796faeacab7; tree clean except session files. Mode delegate (implementer opus/high). Recovery count: 0.

Implementer self-check against the Acceptance Criteria (review rounds: 0 so far).

1. **Hub view** (`HubView.svelte`, plus the new `HubSources.svelte`, `HubUpdates.svelte` and `InstallDialog.svelte`; pure texts in the new `src/lib/hub.ts`).
   - **Installed dashboards.** Each card shows its source ("From GitHub: owner/repo" or "Installed from a local executable"). A GitHub entry with `available_version` shows a "vX available" badge and an Update button. Update runs `update-preview`, then the dialog, then `update/{sha256}`, so a changed key gets the same trust step. With `has_previous` there is a Roll back button, behind a confirmation. A local dashboard shows no update control. Update and Roll back are disabled while the dashboard runs or starts, as Uninstall is.
   - **Install from GitHub.** A labelled "Repository (owner/repo)" input with "Add repository" opens the preview. Catalogs are listed with their entries. An entry shows an Install button, or "Installed" when a dashboard comes from that repository: computed from the hub's list, case-insensitive, so it stays right after an install or uninstall. A catalog that could not be read shows its message and code. There are Reload catalogs and Remove (with a confirmation) buttons, and a labelled "Catalog repository (owner/repo)" input with "Add catalog". The `owner/repo` shape check (`looksLikeRepo`) only gives a hint: no request is sent, the input gets `aria-invalid`, and the hub still validates. Catalogs are read once at load and after a change or Reload, never polled (ticket 05's rate-limit note).
   - **Confirmation dialog** (`InstallDialog.svelte`).
     - A native modal `<dialog>` with `showModal()`, so the page is inert. It is named by its heading and described by the key text.
     - Focus goes to Cancel on open, and again to Cancel once the key is trusted (the Trust button is gone, and focus should not land on Install). On close it returns to the control that opened the dialog, captured before that control was disabled during the preview. When that control is gone (an installed entry), focus goes to `main` (`tabindex="-1"`).
     - Escape cancels unless a request is in flight. A close the browser forces counts as a cancel.
     - It shows repository, release tag, dashboard name and version (from the manifest), file name, size (MiB and bytes), SHA-256, the full public key line, the key's SHA-256, and the key status in plain words. `unknown` is a warning that says to compare the key. `changed` has a danger frame, `role="alert"`, the title "Warning: the signing key has changed", a text saying someone else may be publishing in the repository's name, and a Trust button that is not primary.
     - While `manifest` is null there is no Install button, only "Trust this key". That calls `accept-key/{key_sha256}`, then shows the full preview, announced in the dialog's status ("Key trusted. Name, version X."). Install or Update posts the previewed `sha256`.
   - **Updates** (`HubUpdates.svelte`).
     - "Check now", with a busy label.
     - "Last checked <date>" or "Not checked yet".
     - A "Could not check" list with each message and code, from `HubInfo.last_check`. The hub's own `self_update_not_configured` is shown as "This build of the hub does not update itself", not as a problem.
     - Every new `checked_at` is announced, whether it came from "Check now" or from the check at start that a later poll shows.
     - A labelled checkbox "Check for updates at start", loaded from `GET /settings` and set through `/settings/check-at-start/{on|off}`. On a refusal it goes back to the stored value.
   - **Self-update.** With `HubInfo.available_version`, "Update the hub to vX and restart", behind a confirmation. After success the page announces the update and the restart at a new address in a new tab, and replaces its content with a notice. Polling stops and no error is shown for the old address (`session.hubRestart`).
2. **Rules.**
   - Only `api.ts` calls `fetch`. The new routes are in `api` and take path segments only. A repository becomes two encoded segments, so `a/../b` cannot reach another route.
   - New colour tokens in `app.css` (`--color-warning`, `--color-warning-surface`, `--color-backdrop`); no colour in a component.
   - Every input is labelled.
   - State changes are announced. The page has one `role="status"`, which the sub-components write through an `announce` callback. The dialog has its own status, because the modal makes the page's inert. Refusals are `role="alert"` with message and code (`errorWithCode`); the existing actions now show the code too.
   - Busy and disabled states cover every request.
   - Text from releases and catalogs is rendered as text only.
3. **Tests.** Vitest beside the code: `hub.test.ts`, `api.test.ts`, `HubView.test.ts`. Playwright: see Local Test Result.
4. **The fake GitHub as a process** (`crates/eario-hub/src/bin/fake-github.rs`, `required-features = ["fake-github"]`, `test = false`, `doc = false`; the crate gets `default-run = "eario-hub"`).
   - It serves a scripted scenario through the existing `FakeGitHub`:
     - the default catalog, listing `lab/data-explorer` with its signing key;
     - `lab/catalog`, listing `lab/signal-lab`;
     - given the bin directory: signed `v0.0.0` releases of data-explorer (catalog key) and signal-lab (a key no default catalog lists), each with its `minisign.pub`.
   - Keys come from fixed test seeds.
   - It prints `fake GitHub is running at <api>, control at <control>`, and exits when its standard input closes.
   - Control is a second listener of the fake process, never the hub: `POST /data-explorer/newer`, `/new-key`, `/offline` (rate limit).
   - `fake_github.rs` is unchanged.
   - `cargo xtask e2e` builds it in a separate `cargo build -p eario-hub --features fake-github --bin fake-github`, so the hub under test is built without the feature.
   - In `frontend/e2e/app.ts`, every hub test now starts one: `launchHub` with catalogs only, so the view never reaches the real GitHub, and the new `launchRemoteHub` with releases. Each fixture sets `EARIO_HUB_GITHUB_API`.
5. **Version decision.** The manifest version comes from the executable (`env!("CARGO_PKG_VERSION")`, 0.0.0 for every example), so a rebuilt asset cannot be newer by itself. Chosen: the approach of the hub's integration tests. A new build is the same executable with bytes appended (another SHA-256, same manifest). The spec sets the installed version back to `0.0.0-e2e.1` in the test's own `registry.json`, written by atomic rename with retries, before "Check now". I rejected signal-lab against data-explorer (the slugs differ, and the hub requires the manifest slug to match). I also rejected a second example build through a compile-time variable (an example code change for tests only, and a second 50 MB build). Documented in the binary's module docs, `app.ts` and `docs/development/guide.md`.
6. **Docs.** `docs/development/guide.md`: the `xtask e2e` row, the test table rows, and a paragraph on the fake GitHub in the browser tests. `xtask` help text updated. Project status, roadmap and README are left to ticket 09, as in tickets 05 and 06.

Files changed: `crates/eario-hub/Cargo.toml`, `crates/eario-hub/src/bin/fake-github.rs` (new), `xtask/src/main.rs`, `docs/development/guide.md`, `frontend/src/app.css`, `frontend/src/lib/api.ts`, `frontend/src/lib/api.test.ts`, `frontend/src/lib/hub.ts` (new), `frontend/src/lib/hub.test.ts` (new), `frontend/src/lib/session.svelte.ts`, `frontend/src/lib/components/HubView.svelte`, `frontend/src/lib/components/HubView.test.ts`, `frontend/src/lib/components/HubSources.svelte` (new), `frontend/src/lib/components/HubUpdates.svelte` (new), `frontend/src/lib/components/InstallDialog.svelte` (new), `frontend/e2e/app.ts`, `frontend/e2e/hub.spec.ts`. Outside the ticket's list and needed: the hub crate's manifest and binary (the e2e process), `session.svelte.ts`, `app.css` and the guide. No dependency or contract type was added.

Known limitations:
- The self-update button cannot be exercised in the browser until a real release key is compiled in (ticket 08). It is covered by vitest only.
- Rollback and catalog removal use `window.confirm`, like Uninstall. Install and update use the accessible dialog.
- `self-update.log` (ticket 06) is not shown in the view.

Independent review 1 (opus/high, base a60ec33): 4 findings. P2 HubView.svelte:199-232 concurrent previews swap the dialog under the user. P2 HubSources.svelte:178 duplicate catalog repo crashes each-block keying. P3 InstallDialog.svelte:177-183 double-click on Trust can install. P3 api.ts:109-117 dot segments not refused in repository()/segment(). Parent: all accepted; correction pass 1 (recovery count 1) via a fresh implementer.

Correction pass 1 (implementer opus), per finding:
1. **P2, concurrent previews** (`HubView.svelte`, `HubSources.svelte`). Install and update now share one `preview()` function and one `previewing` flag. While the flag is set, every Install button in a catalog, "Add repository" and every Update button are disabled. `install()`, `update()` and `preview()` also return early when the flag is set or a dialog is open. A preview counter drops an answer that is not the latest one asked for, or that arrives while a dialog is open. The opener is still read before the control is disabled. `HubSources` takes a new `previewing` prop; `installing` is kept only for the "Verifying…" label. Tests: "runs one preview at a time, and confirms only the one the user reviews" holds the previews at the mock hub, submits the repository form by script, and clicks the disabled Update. It checks that only Explorer's preview was sent, that the dialog stays Explorer's, and that Install posts Explorer's SHA-256 and no update is sent. "keeps every Install control disabled while an update is previewed" tests the reverse order. jsdom runs click listeners on a disabled button, so the early returns are exercised too.
2. **P2, duplicate catalog repository** (`remote.rs` `list_catalog`, `HubSources.svelte`). `list_catalog` keeps the first entry for each repository: the comparison uses `same_repo` and ignores case, and it is the entry `catalog_key` takes the key from. Each later entry is skipped with `tracing::warn!(catalog, entry, ...)`. Rust test `a_catalog_lists_each_repository_once_keeping_its_first_entry` (stub `GitHub`, entries repeated in another case, one invalid entry). Both each-blocks in `HubSources` are now keyed by `${index}:${repo}`, which cannot collide. That covers the hub's own catalog list too, since a hand-edited `catalogs.json` could repeat one. Vitest: "lists a catalog that names a repository twice without breaking the page" (a duplicated entry and a duplicated catalog).
3. **P3, double-click on Trust** (`InstallDialog.svelte`). When the manifest appears, the Install or Update button stays disabled for `CONFIRM_DELAY_MS` (600 ms, exported from the module script). The timer is cleared when the dialog goes. The click handler also checks the flag, because a click dispatched by script reaches a disabled button in jsdom. Vitest with fake `setTimeout`: "keeps Install disabled for a moment after the key is trusted" checks that a click at once sends nothing, that the button is still disabled at 599 ms and enabled at 600 ms, and that a click then installs. The e2e trust test passes unchanged: Playwright waits for the button to be enabled.
4. **P3, dot segments** (`api.ts`). `segment()` throws `ApiFailure` (code `invalid_path_segment`, status 0) for `''`, `.` and `..` before any request. Its message names the value. `repository()` splits at the first `/` and throws `"<repo>" is not a repository of the form owner/repo.` when the owner or the name is empty, `.` or `..`. A further `/` is still encoded into the name, and the hub refuses it. The comment no longer claims that encoding alone keeps the route. The throw is synchronous. Every caller already runs inside an async function or a try block, so it becomes a rejection shown where the user asked. The upload's `filename` query value uses `encodeURIComponent` directly: it is not a path segment. Tests: the repository refusals (`nothing`, `lab/`, `/repo`, `''`, `lab/..`, `../repo`, `lab/.`, `./repo`); the segment refusals across slug, component id, state key, SHA-256 and key routes, with the code and message; names with dots that are accepted (`...`, `lab/signal.lab`, an upload file named `.`). The old expectation `nothing` → `/api/sources/nothing//preview` was replaced by a refusal.

Files changed in this pass: `crates/eario-hub/src/remote.rs`, `frontend/src/lib/api.ts`, `frontend/src/lib/api.test.ts`, `frontend/src/lib/components/HubView.svelte`, `frontend/src/lib/components/HubView.test.ts`, `frontend/src/lib/components/HubSources.svelte`, `frontend/src/lib/components/InstallDialog.svelte`.

Re-review (sonnet/high, base a60ec33, after correction pass 1): all four corrections verified; one new P3: session.svelte.ts:248 synchronous throw from segment() escapes componentData's $effect (endless loading for an id '.'/'..'). Parent fixed inline (call started inside Promise.then) with a regression test in src/App.test.ts that fails without the fix. Not re-reviewed again (3-line change; parent verified red/green). Parent re-ran: npm run check 0/0; vitest 107/107 (with the local zstd shim; Node 22.14); cargo xtask frontend + e2e 14/14 passed; fmt and workspace clippy -D warnings clean. Accepted.

## Blockers / Challenges

- vitest cannot start on Node 22.14 (undici binds `zlib.createZstdDecompress`). It was run with a `--require` preload shim through `NODE_OPTIONS`; neither Node nor any committed file was changed. Plain `cargo xtask check` still stops at vitest.
- jsdom has `<dialog>` without `showModal`/`close`. `HubView.test.ts` defines both on the prototype, opening and closing as a browser does.
- The first vitest run requested the catalogs twice: the shared `session` kept the previous test's list, so the page rendered once before `load()`. The tests now reset `session.hub` (test-only cause).
- e2e run 1:
  - `getByLabel('Repository (owner/repo)')` also matched "Catalog repository (owner/repo)". Fixed with `exact: true`.
  - Focus did not return to Update after Escape. Chromium blurs a focused button when it becomes disabled, and the button is disabled during the preview, so the dialog saw `body` as the opener. The opener is now captured when the request starts; an e2e assertion covers it.
- e2e run 3: a text locator matched the dialog's new status line too; made exact, and the status text is asserted on its own.
- Long Bash heredocs failed to parse again (as in tickets 03 to 06); edits were made with the edit tool or with Python scripts in the scratchpad.
- `cargo xtask check` was run with `CARGO_BUILD_JOBS=4` (memory, as in tickets 04 to 06).

## Commits

- 6dccf8b feat: hub view for catalogs, GitHub installs with key trust, updates, rollback and self-update; e2e against a fake GitHub (ticket 07)

## Time Spent

1h 32m (ticket-start.js to ticket-complete.js)
