# 0001. A serializable dashboard specification, built by a Rust API

Status: Accepted

## Context

Three ways of defining a dashboard are wanted over time: Rust code, a
configuration file, and a visual editor. AI agents should also be able to
read and eventually modify a dashboard. The model to favour has not been
chosen, and the first version must not close any of the three.

## Decision

A dashboard is a `DashboardSpec`: plain data (serde), with no closure, no
trait object and no reference to Svelte or the browser. It names pages, rows,
components, dataset slots and state entries.

The Rust API (`eario::Dashboard`, `eario::widgets`) is a builder that
produces this specification. It is the only authoring model implemented.

Reactivity is expressed in the specification itself: each component kind
declares what it reads (`ComponentKind::dependencies()`), and input
components declare the one state entry they write. `validate()` enforces that
every reference resolves and every state entry has exactly one writer.

The specification carries `spec_version`. A specification from a newer
version is rejected.

Executable code is kept out of the specification. Today all behaviour is
built in: a component kind selects a function of the runtime.

## Alternatives

- **Immediate mode** (the application is a function re-run on each
  interaction, as in Streamlit or rustview). Very little API to learn, but
  the dashboard exists only as running code: it cannot be saved, edited
  visually or inspected by an agent, and each interaction re-runs everything
  unless caching is added by hand.
- **Rust closures as components.** Flexible, but not serializable, and the
  frontend would need a matching escape hatch.
- **A file format first** (TOML or JSON written by hand). It would work
  without recompiling, but loses type checking and is a worse authoring
  experience for the Rust developers Eario is for. It stays possible: the
  specification already serializes.

## Consequences

- The set of component kinds is closed. A dashboard can only combine what the
  framework provides. This is the main limitation of the bootstrap.
- A configuration-defined dashboard needs only a loader:
  `serde_json::from_str::<DashboardSpec>` then `App::new`. Not exposed yet
  because nothing uses it.
- A visual editor or an agent would manipulate the same structure.
- Identifiers are generated (`chart-1`) unless set with `.id()`. Anything
  that stores references to components must use explicit identifiers.

## Open questions

- **Custom computations.** How a developer registers a Rust function (a
  computed dataset, a custom statistic) and refers to it from the
  specification. Intended direction: a registry of named functions held by
  `App`; the specification stores the name and serializable parameters, never
  the function. This is the next milestone.
- **Custom components.** Whether third parties can add component kinds, and
  how the frontend learns to render them without being rebuilt.
- **Layout.** Rows on a 12-column grid are enough for predefined dashboards.
  A free-form, resizable layout needs positions and sizes per component.
- **Evolution.** Migration of stored specifications between versions.
