# 0002. State and computation on the backend, a generic frontend

Status: Accepted

## Context

Dashboard authors should write Rust only. Datasets may hold millions of rows,
which must not be shipped to the browser. Other clients (an MCP agent, later
a desktop shell) must see the same application.

## Decision

- The backend owns the application state. `eario_runtime::App` holds the
  specification, the datasets and the state map, and exposes every operation
  as a method. HTTP handlers and MCP tools only translate.
- The frontend is generic. It renders any `DashboardSpec` and holds a mirror
  of the state. It contains no logic of a particular dashboard, so a new
  dashboard never requires rebuilding it.
- Communication is request/response over HTTP with JSON. A change returns a
  `StateUpdate` listing the components to refresh; the frontend then fetches
  the data of each one.
- Invalidation is computed from the specification: a component is stale when
  something it depends on changed.
- `App` methods are synchronous. Adapters call them with
  `tokio::task::spawn_blocking`, keeping CPU work off the async runtime.
- The contract types live in `eario-core`. TypeScript bindings are generated
  from them (ts-rs) and checked in CI.

## Alternatives

- **State in the browser**, backend as a stateless query service. Simpler
  backend, but a second tab or an agent would see a different application,
  and saving a session would require the browser to send everything back.
- **WebSocket or server-sent events from the start.** Needed for progress
  and for pushing changes made by another client; not needed for anything
  implemented, and they complicate shutdown and testing.
- **Rust compiled to WebAssembly in the browser** (Leptos, Dioxus, Yew). One
  language end to end, but a heavier toolchain for dashboard authors, large
  datasets in the browser, and no mature equivalent of ECharts. The
  constraint "authors write only Rust" is met without it.
- **A hand-written TypeScript copy of the contract.** No extra dependency,
  but it drifts silently.

## Consequences

- One round trip to change state, then one per stale component. Negligible on
  the loopback interface.
- Two tabs share one session, but a change made in one is not pushed to the
  other until it reloads or interacts.
- Imported tables and filtered views are recomputed at each query.
  Acceptable: filtering a few million rows takes milliseconds. Only derived
  datasets (ADR 0012) are cached.

## Path to the deferred capabilities

- **Cached intermediate results (done for derived datasets).** `App` keeps
  one cache entry per derived slot, the latest result, keyed by the revisions
  of the datasets and state entries it depends on, directly or through other
  derived datasets. A dependency that changes while a computation runs
  prevents that result from being cached. A filtered view of an imported
  table is still recomputed at each query: `App::resolve_view` is where a
  per-`DataView` cache would go if it is ever needed.
- **Background computation, progress, cancellation:** a task registry in
  `App` plus a server-sent events channel, used for imports and analyses that
  take longer than an interaction. `StateUpdate` already carries a revision
  that such events can reference.
- **Typed state (done).** State values are JSON, validated against the input
  that owns them. `eario::State<T>` wraps a `StateKey` with a type marker in
  the Rust API, and widgets and computations (`ctx.param(&state)`) take it.
  The wire format did not change.

## Open questions

- Whether pushed updates should be server-sent events or a WebSocket.
- Whether `App` should stay synchronous once background tasks exist.
