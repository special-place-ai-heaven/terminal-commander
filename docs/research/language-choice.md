# Language Choice for Terminal Commander

Topic: A1
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: high

## Recommendation

Rust, edition 2024, on a modern stable toolchain. See `msrv.md` for the
exact MSRV floor and `async-runtime.md` for the runtime decision.

## Evidence

### README is Rust-shaped

The repository README at the project root names the future crates with
the prefix `terminal-commander-`:

```text
crates/
  terminal-commander-core/
  terminal-commanderd/
  terminal-commander-mcp/
  terminal-commander-probes/
  terminal-commander-sifters/
  terminal-commander-cli/
```

This naming convention is the Rust workspace + cargo crate convention.
A multi-crate workspace under `crates/` with kebab-case names is
idiomatic Rust. No other language community uses this layout by default
(Go uses `cmd/` + `internal/`, TypeScript uses `packages/`, etc.).

Source: `C:\AI_STUFF\PROGRAMMING\terminal-commander\README.md`,
section "Repository conventions to establish", lines 348-377.

### MCP SDK availability

The official Model Context Protocol Rust SDK exists, is actively
maintained, and is the canonical implementation choice for an MCP
server in Rust. See `mcp-rust-sdk.md` for full details. The pre-confirmed
user evidence pins crate `rmcp` at version 0.16.0 with edition 2024 and
includes a streamable HTTP server session module.

Source: user's vault note `wiki/sources/RMCP rust-sdk.md`,
plus GitHub repository `modelcontextprotocol/rust-sdk` at
https://github.com/modelcontextprotocol/rust-sdk

### Problem fit

Terminal Commander is a local daemon that runs continuously, watches
PTY/file streams, runs sifters against high-volume byte streams, and
exposes an MCP server. The workload is:

- long-lived process,
- low per-event overhead,
- structured concurrency over many sources,
- predictable memory profile,
- single static binary deployable into developer machines and WSL.

Rust matches all of these.

## Alternatives considered

### Go

Trade-offs:

- Simpler to start, decent stdlib, good for daemons.
- `github-mcp-server` is implemented in Go and works in production.
- However, the official MCP SDK ecosystem in Rust is more cleanly
  typed; Go has competing community SDKs but no single canonical one.
- The downstream crate names already exist as Rust crates in the README,
  switching to Go would invalidate that.

Source for github-mcp-server in Go:
https://github.com/github/github-mcp-server

### TypeScript / Node.js

Trade-offs:

- The reference filesystem MCP server is TypeScript.
- TypeScript is fine for IO-light MCP wrappers.
- Per-event PTY/file streaming with sub-millisecond sifters and
  long-lived watchers is a poor fit for Node's event loop and GC
  characteristics under sustained pressure.
- Deployment for a local daemon is heavier (node_modules vs single
  static binary).

Source for filesystem MCP server in TypeScript:
https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem

## Confidence

High. README naming convention + user-pinned rmcp crate +
problem fit jointly point at Rust without ambiguity. No HALT-worthy
issue found.

## SOURCE_MAP reclassification

Language choice = Rust: was inferred, now evidence-backed via README
crate names + user-pinned rmcp 0.16.0.
