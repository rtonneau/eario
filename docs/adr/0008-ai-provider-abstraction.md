# 0008. Agent providers behind a trait, tools shared with MCP

Status: Proposed. Nothing is implemented beyond the MCP endpoint of ADR 0009.

## Context

Eario should offer an optional assistant inside a dashboard. The first
provider targeted is Claude Code; others may be Codex, local models or direct
model APIs. An agent runtime and a model API are different things: a runtime
(Claude Code) runs its own loop and brings its own tools, while a model API
returns one completion and leaves the loop to the caller.

## Two integration directions

1. **An external agent connects to Eario.** The user runs their agent and
   points it at the application's MCP endpoint. Implemented, read-only
   (ADR 0009). Eario runs no model and sends nothing anywhere by itself.
2. **Eario hosts a conversation.** A panel in the dashboard talks to a
   provider. Not implemented; this record is about its boundary.

## Proposed boundary for the hosted assistant

Concepts, kept separate:

| Concept | Responsibility |
| --- | --- |
| Agent provider | Starts sessions with one kind of agent or model. Declares what it can do (streaming, tool calls, runs its own loop) and whether data leaves the machine. |
| Agent session | One running conversation with a provider. Can be cancelled. |
| Conversation | The messages, stored by Eario, independent of the provider. |
| Tool registry | The operations an agent may request. The same definitions as the MCP tools, over `App`. |
| Tool execution | Runs a requested tool through `App`, after policy checks. |
| Authorization | Asks the user before mutating or expensive operations. |
| Data access policy | Which datasets, and how many rows, an agent may see. |
| Proposed changes | Dashboard modifications and generated scripts are proposals the user accepts or rejects, never applied directly. |

An `AgentProvider` trait lives in a future `eario-agent` crate, behind a
Cargo feature, with one module per provider. The core never depends on it.

For agent runtimes, the documented integration mechanism of the provider is
used: for Claude Code, its agent SDK or its non-interactive mode with
structured (JSON) output, with Eario's MCP endpoint supplied as the tool
source. Terminal output is never parsed, and no undocumented interface is
used. The exact mechanism must be checked against the provider's current
documentation when the work starts.

## Security requirements

- Off by default; enabled by the developer and then by the user.
- The user is told, before the first message, whether the provider sends
  data off the machine.
- The assistant has no file-system, shell, environment or network access
  through Eario. It has the tool registry and nothing else.
- Statements produced by a model are displayed distinctly from values
  computed by Eario.
- Generated Python is never executed without the explicit run of ADR 0007.

## Open questions

- Which provider capabilities are common enough to abstract, and which stay
  provider-specific.
- Whether agent runtimes that bring their own tools (file access, shell) can
  be restricted to Eario's tools reliably.
- How credentials for model APIs are stored, if at all.
