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
- Every query recomputes from the dataset. Acceptable now: filtering a few
  million rows takes milliseconds.

## Path to the deferred capabilities

- **Cached intermediate results:** cache the filtered table per `DataView`,
  keyed by the revision of its dependencies. `App::resolve_view` is the single
  place to add it.
- **Background computation, progress, cancellation:** a task registry in
  `App` plus a server-sent events channel, used for imports and analyses that
  take longer than an interaction. `StateUpdate` already carries a revision
  that such events can reference.
- **Typed state:** state values are JSON, validated against the input that
  owns them. Typed handles (`State<T>`) in the Rust API can be layered on
  `StateKey` without changing the wire format.

## Open questions

- Whether pushed updates should be server-sent events or a WebSocket.
- Whether `App` should stay synchronous once background tasks exist.
