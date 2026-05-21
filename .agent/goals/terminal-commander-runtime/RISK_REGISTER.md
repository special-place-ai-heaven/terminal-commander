# Risk Register - terminal-commander-runtime

| Risk | Level | Mitigation goal |
|---|---:|---|
| Main branch work can damage stable trunk | High | Every goal has Branch Guard and narrow allowed files; operator explicitly chose main. |
| Runtime product drifts into docs-only scaffold | High | TC33/TC34 source-status audit and contract lock before implementation. |
| MCP server becomes a command-spawning shell | Critical | TC37-TC41 require daemon-owned policy/audit and MCP forwarding only. |
| Raw output leaks to LLM | Critical | Every goal preserves bounded-output/no-raw-stream invariants. |
| UDS peer credentials are skipped silently | High | TC37 requires implementation or explicit blocker. |
| Audit remains in-memory while runtime goes live | High | TC35 is before IPC/MCP runtime. |
| Dynamic registry updates do not affect live probes | High | TC42 explicitly hot-binds rules. |
| PTY prompts leak secrets | High | TC44 requires secret prompt detection and no secret echo. |
| Parallel probes create unbounded queues | High | TC45/TC47 require metrics, limits, and backpressure evidence. |
| Provider smoke relies on vendor-specific hooks | Medium | TC46 uses generic MCP stdio first; provider configs are examples. |
