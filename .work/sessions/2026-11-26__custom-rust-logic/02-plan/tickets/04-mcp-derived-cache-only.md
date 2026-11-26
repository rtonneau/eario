# Ticket 04: mcp-derived-cache-only

**Model:** medium
**Effort:** medium

**Acceptance Criteria:**
- [ ] `list_datasets` and `describe_dashboard` show derived slots with their computation name and whether a cached result exists.
- [ ] `preview_dataset`, `dataset_statistics` and `get_component_data` on a derived dataset use the cached table only and return a "not computed yet" result instead of triggering a computation.
- [ ] Tests cover both the cached and the not-yet-computed cases.

**Files to Touch:**
- `crates/eario-mcp/src/lib.rs`
- `crates/eario-runtime/src/app.rs` (cache-only read methods)

**Verification Step:**

Run:
```bash
cargo test -p eario-mcp -p eario-runtime && cargo xtask check
```

Expected:
New MCP tests pass, and an instrumented computation is never invoked through an MCP call; all checks pass.

**Notes:**

Keep the business rule in `App` (a cache-only mode), the MCP tool stays a thin adapter (AGENTS.md "One source of behaviour").
