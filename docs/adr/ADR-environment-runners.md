# ADR: Environment runners (parent daemon + optional remote runner)

Status: Accepted; only point 1 was ever built. The partly built runner path
described below (the router, the WSL forward and its lookup) was removed on
2026-10-05 because nothing could reach it: the `environment` field stays on the
IPC params for wire compatibility, and a non-local value is now refused with an
error that names `wsl_argv` / `wsl_shell` access routes and `target_id` instead.
The notes below describe the code as it stood before that removal.
- Implemented as written: point 1 (parent adapter and daemon stay on the
  harness host; MCP never runs inside WSL).
- Partly built, not usable: points 2 and 3. The IPC params
  `CommandStartParams` and `PtyCommandStartParams` carry
  `environment: Option<EnvironmentSpec>` (`crates/ipc/src/protocol.rs`; the
  field is `environment`, not `environment_id`), and `CommandStartCombed`
  and `PtyCommandStart` with a non-local value go through `EnvironmentRouter`
  (`crates/daemon/src/ipc/server.rs` and the since-removed environment router module). The WSL
  forward is a stub: from a Windows parent `forward_to_runner` returns
  "frame relay (planned)" (in the since-removed WSL forward module), and a non-Windows parent
  refuses; `SshHost` returns "not implemented". No runner bootstrap exists
  and no test routes a non-local environment; every in-repo caller passes
  `environment: None`. The MCP tools expose no environment parameter.
- Superseded by `system_discover` access routes (July 2026, commits
  cd0d906 and d135e27, `crates/daemon/src/environment/probe.rs`): an MCP
  client reaches WSL by running the route's `wsl_argv` or `wsl_shell`
  template through `run_and_watch`/`exec` on the parent daemon. See
  `docs/mcp/TOOL_CONTROL_SURFACE.md` section 1.
- Point 4: `TC_WSL_DISTRO` is read by the npm launcher and forwarded by the
  supervisor allowlist (`crates/supervisor/src/ensure.rs`); it does not
  select a runner daemon. `TC_USE_LEGACY_WSL_BRIDGE` still exists
  (`packages/terminal-commander/bin/terminal-commander-mcp.js`), past the
  "one release cycle" below.
The decision text below is unchanged.
Date: 2026-05-23
Builds-on: [`ADR-native-tier1-runtime`](./ADR-native-tier1-runtime.md) — the
native tier-1 decision establishes that the runtime is native Rust on every
supported OS with no WSL stdio bridge. This ADR adds the *cross-environment*
execution model layered on top of that native parent.
Related: `docs/audits/2026-05-24-windows-mcp-connect-closed-findings.md`.

## Context

The native tier-1 ADR settles where the runtime lives: a native
`terminal-commander-mcp` + `terminal-commanderd` on the harness host, with MCP
talking to the daemon over local IPC (Unix domain socket on Unix, named pipe on
Windows). It does not settle how the operator reaches *another* environment —
e.g. running a probe inside a WSL distro, or later over SSH — from that single
native MCP surface.

Terminal Commander is an MCP control glove: one MCP surface for the LLM,
structured signal only. Forcing the entire MCP stdio session into WSL (the
deprecated bridge) contradicts that doctrine and blocks non-WSL Windows users.
We need a way to *target* another environment without relocating the control
plane into it.

## Decision

1. **Parent stays on the harness host.** The native MCP adapter and daemon run
   where the harness runs. MCP never runs inside WSL. This is the native
   tier-1 parent; this ADR does not change it.

2. **Runner is an optional second daemon in another environment.** A full
   `terminal-commanderd` instance may run in a different environment (WSL distro
   first; SSH later). The parent bootstraps the runner if missing and forwards
   probe operations to it over a control channel.

3. **Environment is selected per operation, not per session.** Probe-start IPC
   and MCP tools carry an optional `environment_id`. Absent it, the operation
   runs in the parent's own environment. The MCP session, signal channel, and
   control plane always stay on the parent.

4. **WSL runner selection.** `TC_WSL_DISTRO` selects which WSL distro hosts the
   runner. This is distinct from the deprecated legacy bridge
   (`TC_USE_LEGACY_WSL_BRIDGE=1`), which relocates the whole stdio session and
   is retained for one release cycle only.

## Consequences

- The probe-start contract (IPC + MCP tool params) gains an optional
  `environment_id` field; absence means "parent environment."
- The parent needs a runner-bootstrap path and a control channel to forward
  probe operations and stream signal back from the runner.
- Windows-native ConPTY and named-pipe IPC (from the native tier-1 work) are the
  parent's transport; the runner reuses the same daemon binary in its own
  environment.
- `TC_WSL_DISTRO` is a runner selector only. It must not re-enable the legacy
  stdio bridge.

## References

- `docs/adr/ADR-native-tier1-runtime.md`
- `docs/runtime/REALTIME_SIGNAL_CHANNEL.md`
- `docs/research/mcp-transport-pattern.md`
