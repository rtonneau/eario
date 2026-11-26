# Security policy

## Supported versions

Eario has no stable release yet. Security fixes are made on the `main` branch.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it through
GitHub's private vulnerability reporting: on the repository page, open the
**Security** tab and choose **Report a vulnerability**.

Include what is affected, how to reproduce it, and the impact you expect. You
should receive an acknowledgement within a week. Please allow time for a fix
before disclosing the issue publicly.

## Scope

An Eario application is a local program serving one user on one machine. The
protections it implements, and the threats it does not address, are described
in [docs/architecture/security.md](docs/architecture/security.md). In short:

In scope:

- A web page in the user's browser reading data from, or acting on, a running
  Eario application (cross-site requests, DNS rebinding).
- A request reaching files outside the embedded frontend (path traversal).
- An import crashing the application or exhausting memory beyond the
  configured upload limit.
- An MCP tool exposing more than its documented read-only, bounded result.

Out of scope:

- An attacker who already runs code as the same user on the same machine.
- Applications whose developer changed the defaults to expose the server
  beyond the loopback interface.
