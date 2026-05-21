# Assumptions - terminal-commander-runtime

- The operator confirmed `main` contains the merged TC01a-TC32 work and requested this chain target `main`.
- `feature/terminal-commander-mvp` may still exist locally/remotely but is prohibited for this chain.
- The previous chain's evidence reports are trusted input, but TC33 must verify actual `main` state before implementation.
- The product target is not merely stored knowledge; it is an active realtime signal abstraction layer for command output and filesystem intelligence.
- Any stale, scaffold-only, mock, test-only, degraded, or partial runtime surface must be source-status labeled.
- MCP is the provider-neutral tool control surface. Provider-specific hooks are optional adapters, not core correctness.
- The chain remains conservative on privilege: no root shell, no setuid, no network listener, no automatic sudo/service installation.
