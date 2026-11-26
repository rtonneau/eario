# 0009. A read-only MCP endpoint over the application services

Status: Accepted. Implemented as experimental.

## Context

External AI agents should be able to interact with a running Eario
application through the Model Context Protocol, using the same services as
the frontend.

## Decision

- `eario-mcp` uses `rmcp`, the official Rust SDK, and exposes the Streamable
  HTTP transport at `/mcp` on the application's own server.
- It is optional twice: the Cargo feature `mcp` on `eario`, and
  `Dashboard::mcp(true)` in the application. Without both, the route does not
  exist.
- The route sits behind the same Host and Origin checks and the same session
  token as the HTTP API. Agents send the token as a bearer token.
- The transport is configured stateless with JSON responses. The current
  specification revision removes sessions from this transport, and no
  long-lived stream delays shutdown.
- Tools call `eario_runtime::App` and `eario_data::query`. The crate contains
  schemas and formatting, no logic.

### Tools

| Tool | Returns |
| --- | --- |
| `describe_dashboard` | Title, pages, components with their bindings, current state |
| `list_datasets` | Source, size, worksheets and columns of each loaded dataset |
| `preview_dataset` | A window of rows, at most 100 per call, with an offset |
| `dataset_statistics` | Descriptive statistics of the numeric columns |
| `get_component_data` | What a component currently shows, filter and selections applied |

All are annotated read-only. Results are bounded: tables by page, charts by
their point limit.

### Separation of concerns

| Concern | Where |
| --- | --- |
| Application commands | `eario_runtime::App` |
| Transport | `rmcp` `StreamableHttpService`, mounted by `eario_mcp::router` |
| Tool schemas | Request structs in `eario-mcp` (`schemars`) |
| Permission enforcement | The token layer of `eario-runtime`; read-only tools only |
| Session management | None: stateless |

## Connecting an agent

Start the application with MCP enabled and read the port and token from the
address it prints (`http://127.0.0.1:PORT/#token=TOKEN`). Configure the agent
with an HTTP MCP server at `http://127.0.0.1:PORT/mcp` and the header
`Authorization: Bearer TOKEN`. Both change at each start unless a port is
fixed with `Dashboard::port` or `EARIO_PORT`.

## Alternatives

- **stdio transport.** Usual for local MCP servers, but the agent would have
  to start the dashboard as its child, while here the user starts the
  dashboard and the agent joins a running session.
- **A separate port for MCP.** No benefit, and a second listener to secure.
- **Hand-written JSON-RPC.** Avoids a dependency that moves fast, at the cost
  of tracking the specification ourselves.

## Consequences

- The token gives an agent exactly what the browser has, restricted to five
  read tools. Data returned to the agent may then leave the machine,
  depending on the agent. Enabling MCP is therefore the developer's and the
  user's decision, and the README says so.
- The token and port change at each start, which is inconvenient for a
  permanently configured agent.

## Open questions

- **Mutating tools** (set a filter, select columns, create a chart). They
  need a permission model first: per-tool consent from the user in the
  dashboard, visible indication that an agent acted, and an audit trail.
  None are exposed until that exists.
- **Resources.** Datasets and the specification could be MCP resources
  rather than tool results.
- **A stable way to hand the token to an agent** without copying it by hand.
- **Eario as an MCP client**, connecting to external servers: a separate
  subject, not started.
