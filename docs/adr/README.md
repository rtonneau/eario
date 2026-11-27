# Architecture decision records

Each record states one decision: its context, what was decided, the
alternatives considered, the consequences, and what remains open. A decision
is changed by adding a new record that supersedes the old one, not by
rewriting history.

| No. | Decision | Status |
| --- | --- | --- |
| [0001](0001-dashboard-specification.md) | A serializable dashboard specification, built by a Rust API | Accepted |
| [0002](0002-backend-frontend-separation.md) | State and computation on the backend, a generic frontend | Accepted |
| [0003](0003-data-representation.md) | Polars data frames for tables, behind `eario-data` | Accepted |
| [0004](0004-visualization-engine.md) | Apache ECharts, fed by renderer-independent chart data | Accepted |
| [0005](0005-frontend-asset-embedding.md) | Frontend embedded in the executable, prebuilt in the published crate | Accepted |
| [0006](0006-persistence.md) | JSON preferences now; versioned session files later | Accepted (preferences), Proposed (sessions) |
| [0007](0007-python-integration.md) | Python through managed subprocesses and Arrow | Proposed |
| [0008](0008-ai-provider-abstraction.md) | Agent providers behind a trait, tools shared with MCP | Proposed |
| [0009](0009-mcp-integration.md) | A read-only MCP endpoint over the application services | Accepted |
| [0010](0010-deployment.md) | One executable per dashboard, loopback server, browser interface | Accepted |
| [0011](0011-standalone-and-hub.md) | Each dashboard runs standalone or from a hub | Accepted (amended at the start of M3, and by 0013) |
| [0012](0012-computations-and-derived-datasets.md) | Named computations and derived datasets | Accepted |
| [0013](0013-github-install-and-update.md) | Install and update dashboards from signed GitHub releases, from the hub only | Accepted |

"Proposed" means the direction is documented and nothing is implemented.
