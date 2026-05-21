# Decisions - terminal-commander-runtime

## Branch

Target branch is `main` because the operator confirmed the MVP feature branch has been merged and requested this next chain on main.

## Product term

Use `realtime signal channel` / `LLM abstraction layer` / `tool control surface`; avoid over-indexing on the metaphor "glove".

## Core product invariant

```text
All signal. No raw static/noise by default.
```

## Runtime priority

The next phase prioritizes real-deployment blockers and the live MCP path:

1. Runtime audit and contract alignment.
2. Durable audit logging.
3. Daemon runtime state.
4. UDS IPC.
5. command_start_combed runtime wiring.
6. bucket_wait/event_context live channel.
7. rmcp stdio and MCP tools.
8. registry hot activation.
9. file and PTY probe surfaces.
10. parallel routing, provider smoke, load gate, beta review.
