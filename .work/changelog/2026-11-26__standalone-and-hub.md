---
bump: minor
floor: minor
---
- Every dashboard executable now has a launch contract with no code from its author: `--eario-manifest` prints its manifest as JSON, `--eario-install` / `--eario-uninstall` (with `--yes` to skip the confirmation) copy it into the user's hub or remove it, and `--eario-child` runs it for a hub without a browser or console.
- `Dashboard::version` sets the version shown in the manifest and the hub.
- New `eario-hub` executable: lists the installed dashboards, starts, opens, stops, stops all and uninstalls them from a hub view in the browser; checks each copy's size and SHA-256 before starting it; quitting the hub leaves the dashboards running, and the next hub re-attaches to them; one hub per user.
- The hub directory is `hub` in Eario's configuration directory, or the one named by `EARIO_HUB_DIR`.
- `cargo xtask dist` also builds the hub; `cargo xtask e2e` also runs the hub's browser tests.
- `/api/app` now carries a `kind` field (`dashboard` or `hub`).
