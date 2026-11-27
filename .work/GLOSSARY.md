# eario glossary

Eario is a local-first Rust framework for interactive dashboards and data exploration: a dashboard is described in Rust, compiled into one executable, and served to a generic browser frontend from the loopback interface.

<!-- Format adapted from mattpocock-skills' domain-modeling (GLOSSARY-FORMAT.md): https://github.com/mattpocock/skills.git -->

<!--
How to add a term (the moment it is resolved, not in a batch at the end):

**Term**:
One or two sentences: what it IS, not what it does.
_Avoid_: the other words people use for it

- Be opinionated: one word per concept; list the others under _Avoid_.
- Only terms specific to this project. General programming concepts and
  implementation details don't belong here: this is a glossary, not a spec.
- Group terms under "### <area>" subheadings once natural clusters emerge.
-->

## Language

### Custom logic

**Computation**:
A named Rust function registered with the dashboard that turns input tables and parameters into one table. The specification refers to it by name only.
_Avoid_: callback, plugin, transform, custom statistic (a custom statistic is a computation that returns a small table)

**Derived dataset**:
A dataset slot whose table is the output of a computation rather than an import. Components read it exactly like an imported dataset.
_Avoid_: computed dataset, virtual dataset, view

**Compute context**:
What a computation receives: its named input tables and its named parameters, typed on access.
_Avoid_: inputs, args

**Parameter**:
A named value a computation reads, bound either to a state entry or to a literal in the specification.
_Avoid_: argument, option

### Standalone and hub

**Standalone**:
A dashboard executable started by the user directly: it opens the browser and keeps its console. The default way to run.
_Avoid_: direct mode, single mode

**Hub**:
A separate Eario executable that lists the installed dashboards and starts, opens and stops each one as a child. One hub runs per user at a time.
_Avoid_: launcher, manager, dashboard of dashboards (in code)

**Child**:
A dashboard process started by the hub in child mode: no browser, no console, logs to a file, may outlive the hub.
_Avoid_: instance, worker, subprocess

**Launch contract**:
The command-line behaviour every executable built with `Dashboard::run()` has without code from its author: `--eario-manifest`, `--eario-child <run-file>`, `--eario-install`, `--eario-uninstall`.
_Avoid_: launch protocol (that is the version number of the contract), CLI

**Slug**:
The identity of a dashboard in the hub: its title reduced to lower-case letters, digits and dashes. The same slug names its preferences directory.
_Avoid_: id, name, key

**Manifest**:
What a dashboard executable says about itself: name, slug, version, Eario version, launch protocol version, description.
_Avoid_: metadata, descriptor

**Registry**:
The hub's list of installed dashboards, one entry per slug, each pointing at the hub's own copy of the executable with its size and hash.
_Avoid_: catalogue, index, library

**Run file**:
The file a child writes once it is listening, holding its process id, address, token and start time, and removes when it stops. The hub trusts it only after the child answers with that token.
_Avoid_: lock file, pid file, state file

### Remote install and update

**Source**:
Where a registry entry's executable came from: `local` (installed with `--eario-install`) or a GitHub repository (`owner/repo`) whose releases the hub reads.
_Avoid_: origin (clashes with the HTTP Origin header), remote, provider

**Catalog**:
A `catalog.json` file in a GitHub repository listing dashboards the hub can install: repository, name, description and, optionally, the author's public key. The hub knows a default one and any the user adds.
_Avoid_: registry (that is what is installed), store, marketplace, index

**Release asset**:
One executable attached to a GitHub release of a dashboard, named `<slug>-<target triple>[.exe]`, listed with its SHA-256 in the release's signed `SHA256SUMS.txt`.
_Avoid_: artifact, package, binary

**Pinned key**:
The ed25519 (minisign) public key the hub accepts for a source: taken from the catalog entry, or from the release's `minisign.pub` at first install after the user confirms the full key and its SHA-256 (never the minisign key id, which can be forged). A release signed by another key is refused until the user accepts the change.
_Avoid_: trusted key, certificate

**Previous version**:
The one older copy of a dashboard the hub keeps in `bin/<slug>/` after an update, so the user can roll back once.
_Avoid_: backup, old version, side-by-side install

**Self-update**:
The hub replacing its own executable with a verified newer release of `rtonneau/eario`, then restarting; the running children are left alone.
_Avoid_: auto-update (nothing is applied without the user's click)
