# Goal Chain Overview - terminal-commander-runtime

## Chain name

`terminal-commander-runtime`

## Purpose

This chain pivots Terminal Commander from the tested TC01a-TC32 scaffold/MVP library state into the real runtime product: a provider-neutral MCP realtime signal abstraction layer for LLM agents.

The target product behavior is:

```text
LLM asks through MCP.
Daemon/probes do the terminal and filesystem toil.
Sifters/registry/indexes extract signal.
Buckets expose realtime structured events.
Pointers provide bounded context.
Raw noise stays out of the LLM context window.
```

## Branch policy

```text
target_branch: main
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
```

The chain targets `main` because the operator explicitly confirmed the previous feature branch was merged and requested this next phase on `main`.

## Target output folder

`.agent/goals/terminal-commander-runtime/`

## Total goals

16 goals: TC33 through TC48.

## Run order

1. `TC33` — Code Reality Audit And Runtime Pivot
2. `TC34` — Realtime Signal Channel Contract
3. `TC35` — Persistent Audit Log V0003
4. `TC36` — Daemon Runtime Bootstrap And Config
5. `TC37` — Daemon UDS IPC And Peer Identity
6. `TC38` — Command Start Combed Process Wiring
7. `TC39` — Bucket Wait And Event Context Daemon API
8. `TC40` — Rmcp Stdio Adapter And Tool Discovery
9. `TC41` — Mcp Command And Bucket Tools
10. `TC42` — Registry Hot Activation And Rule Binding
11. `TC43` — File Probe Search Watch And Bounded Read
12. `TC44` — Posix Pty Spawn And Stdin Control
13. `TC45` — Parallel Probe Router And Multi Bucket Bindings
14. `TC46` — Provider Harness Integration Smoke
15. `TC47` — Load Noise And Backpressure Gate
16. `TC48` — Beta Gate Evidence Review And Backlog Rerank

## Dependency graph

Linear by design. Each goal depends on the previous goal unless explicitly marked with no dependency.

```text
TC33 -> TC34 -> TC35 -> TC36 -> TC37 -> TC38 -> TC39 -> TC40 -> TC41 -> TC42 -> TC43 -> TC44 -> TC45 -> TC46 -> TC47 -> TC48
```

## Assumptions register

- The previous TC01a-TC32 chain has been merged into `main`.
- The next phase should run directly on `main`, per operator instruction.
- Existing product invariants from TC02 remain locked: no unrestricted root shell, no setuid/polkit/system-service install, no network listener, default-deny sensitive paths, bounded output, and pointer-or-reason for severity >= Medium.
- The repo uses the short crate layout under `crates/<short>/`.
- Runtime P0 gaps remain until source evidence proves otherwise: rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes.
- Where code state differs from this chain, TC33 must correct downstream goal files before implementation continues.

## Source evidence used

- GitHub main README states the purpose: local MCP-operated terminal/file signal-combing layer; raw terminal/file output in, only vetted signal out, context by pointer.
- GitHub main daemon binary currently carries a scaffold-only entry point.
- GitHub main MCP binary currently carries a scaffold-only entry point while `ToolSurface` exists in library code.
- GitHub main Router is live for in-process dispatch but still contains an audit placeholder seam.
- GitHub main ProcessProbe exists for streamed stdout/stderr and sifter output.
- GitHub main PTY module ships normalization and prompt detection, but full PTY spawn is deferred.
- Uploaded BACKLOG.md lists P0 real-deployment blockers.
- Uploaded EVIDENCE_REPORT.md and FINAL_REPORT.md record completed TC01a-TC32 evidence and substitutions.

## First command

```bash
/goal .agent/goals/{chain}/{goals[0]['id']}-{goals[0]['kebab']}.md
```
