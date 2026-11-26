# 0005. Frontend embedded in the executable, prebuilt in the published crate

Status: Accepted

## Context

An end user receives one executable and must need nothing else. A developer
depending on Eario from crates.io must not need Node.js. Contributors need a
reproducible frontend build.

## Decision

- Vite builds `frontend/` into `crates/eario-runtime/frontend-dist/`.
- `eario-runtime` embeds that directory with `rust-embed`. Release builds
  contain the files; debug builds read them from disk at run time, so a
  frontend rebuild does not require recompiling Rust.
- The directory is ignored by git and listed in the crate's `include`, so
  `cargo package` ships the built frontend. The release process builds the
  frontend first.
- On a checkout without a built frontend, the crate's build script writes a
  placeholder page and emits a warning, so `cargo check` and `cargo test`
  work without Node.js.
- The frontend lockfile (`package-lock.json`) is committed and CI installs
  with `npm ci`.

## Alternatives

- **Commit the build output.** Simplest for consumers of the git repository,
  but every frontend change adds a minified bundle to the history and review
  cannot tell whether it matches the source.
- **Build the frontend from `build.rs`.** Every user of the crate would need
  Node.js, and builds would run npm.
- **Ship assets beside the executable.** Breaks the single-file distribution.
- **Download assets at first run.** Violates local-first.

## Consequences

- Someone depending on Eario through a git URL, rather than crates.io, gets
  the placeholder unless they build the frontend. This must be documented
  where git dependencies are mentioned.
- The published crate contains a build artefact. Its provenance is the tagged
  commit and the lockfile; the release workflow is the only place it is built.
- The executable grows by the size of the bundle (about 0.65 MB, mostly
  ECharts).

## Open questions

- Whether the frontend should become its own crate (`eario-frontend`) so its
  version and size are visible separately.
- Reproducibility of the published bundle: byte-identical rebuilds from a
  tag have not been verified.
- How a third-party frontend extension would be embedded alongside.
