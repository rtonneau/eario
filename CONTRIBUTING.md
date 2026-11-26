# Contributing to Eario

Thank you for considering a contribution. Eario is at an early stage: the
most useful contributions right now are bug reports, small focused fixes, and
discussion of the open questions listed in the
[decision records](docs/adr/README.md).

## Before you start

- For anything larger than a fix, open an issue first so the design can be
  discussed before code is written.
- Read [AGENTS.md](AGENTS.md). It states the conventions of the project for
  human and AI contributors alike: crate boundaries, coding rules, testing
  requirements and the definition of done.
- Read the [development guide](docs/development/guide.md) to set up your
  machine.

## Workflow

1. Fork the repository and create a branch from `main`.
2. Make the change, with tests.
3. Run `cargo xtask check`. It runs what CI runs.
4. Open a pull request. Describe what changed, why, and what you verified.

Keep pull requests small and about one thing.

## What we look for in a pull request

- It follows the crate boundaries and conventions of [AGENTS.md](AGENTS.md).
- New behaviour is tested; a bug fix has a test that fails without the fix.
- Public items are documented.
- A new dependency is justified: what it replaces, its licence, how well it
  is maintained.
- Documentation in `docs/` matches the change. Features that are not
  implemented are not described as working.

## Test data

Fixtures must be synthetic. Do not commit laboratory data, personal data or
anything confidential, even as an example. Add generated fixtures to
`xtask/src/fixtures.rs` so they can be reproduced.

## Reporting bugs

Open an issue with the version or commit, your operating system, what you
did, what you expected and what happened. If a file triggers the bug, attach
a synthetic file that reproduces it rather than your real data.

Security issues are reported privately: see [SECURITY.md](SECURITY.md).

## Licensing

By contributing you agree that your contribution is dual licensed under the
MIT license and the Apache License 2.0, as described in the
[README](README.md#license).
