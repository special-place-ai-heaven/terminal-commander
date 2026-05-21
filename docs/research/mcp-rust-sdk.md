# MCP Rust SDK: rmcp

Topics: B1, B2
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: high

## Recommendation

Use `rmcp`, the official Rust SDK for the Model Context Protocol,
published by `modelcontextprotocol/rust-sdk` on GitHub.

## Primary evidence

### Repository

- URL: https://github.com/modelcontextprotocol/rust-sdk
- Owner: `modelcontextprotocol` (official org).
- Default branch: `main`.
- License: dual MIT / Apache-2.0 (project is in the middle of a
  transition to Apache-2.0; new contributions are Apache-2.0, older
  code stays MIT until relicensing consent is obtained).
- Quote from LICENSE: "The MCP project is undergoing a licensing
  transition from the MIT License to the Apache License, Version 2.0
  (\"Apache-2.0\")."
- Source: https://github.com/modelcontextprotocol/rust-sdk/blob/main/LICENSE

### Crate

- Crate name: `rmcp`.
- Workspace layout: `crates/rmcp` + `crates/rmcp-macros` + `examples/*`
  + `conformance` (resolver 2).
- Versions available on crates.io include `0.16.0` (user-pinned) and
  the current latest `1.7.0` (released 2026-05-13).
- Source for 1.7.0 release: https://github.com/modelcontextprotocol/rust-sdk/releases
- Source for the 0.16.0 install reference still being in the main
  README: https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md

### Rust toolchain

- 0.16.0 tag: `rust-toolchain.toml` `channel = "1.90"`,
  edition = `"2024"`.
- main (1.7.0): `rust-toolchain.toml` `channel = "1.92"`,
  edition = `"2024"`.

### Transport support

The rmcp transport module exposes (per
`crates/rmcp/src/transport.rs`):

- `io` (includes `stdio`) - feature `transport-io`
- `async_rw` - feature `transport-async-rw`
- `child_process` - feature `transport-child-process`
- `sink_stream` - duplex object streams (e.g. WebSockets)
- `streamable_http_server` - feature
  `transport-streamable-http-server-session`
- `streamable_http_client` - feature
  `transport-streamable-http-client`
- `worker` - feature `transport-worker`
- `auth` - OAuth flow helpers

This confirms stdio + streamable HTTP transport for server-side
implementations. SSE is reachable through `client-side-sse` feature on
the client side. Server-side SSE is no longer a separate transport in
the spec line targeted (2025-11-25); streamable HTTP supersedes it.

Source: https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/src/transport.rs

### Feature flags

From the workspace Cargo.toml (main branch):

```text
default = ["base64", "macros", "server"]
```

Notable features:

- `client`, `server` - core protocol implementations
- `macros` - `#[tool]`, `#[prompt]` proc macros
- `schemars` - JSON Schema generation
- `transport-io` (stdio)
- `transport-async-rw`
- `transport-child-process`
- `transport-streamable-http-server`
- `transport-streamable-http-client`
- `server-side-http`
- `client-side-sse`
- `auth`, `auth-client-credentials-jwt`
- `elicitation`
- `tower`, `uuid`, `base64`

Source: https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/Cargo.toml

### Direct dependencies (highlights)

- `tokio = "1"` with `["sync", "macros", "rt", "time"]`
- `serde = "1.0"` with `["derive", "rc"]`
- `serde_json = "1.0"`
- `async-trait = "0.1.89"`
- `thiserror = "2"`
- `futures = "0.3"`
- Optional: `hyper = "1"`, `reqwest = "0.13.2"`, `axum = "0.8"`
  (dev-dependencies), `oauth2` (under `auth` features),
  `jsonwebtoken = "10"`, `schemars = "1.0"`.

## MCP protocol version targeted

rmcp targets MCP protocol revision `2025-11-25`. The specification
homepage is https://modelcontextprotocol.io/specification/latest and
the schema file is at
https://github.com/modelcontextprotocol/specification/blob/main/schema/2025-11-25/schema.ts.

Quote from spec: the protocol uses JSON-RPC 2.0 messages, stateful
connections, and server/client capability negotiation.

Source: https://modelcontextprotocol.io/specification/latest

## Server-side quick usage

From the main README:

```rust
let service = Calculator.serve(stdio()).await?;
service.waiting().await?;
```

For streamable HTTP server, rmcp exposes `StreamableHttpService` under
the `transport-streamable-http-server` feature.

Source: https://github.com/modelcontextprotocol/rust-sdk/blob/main/README.md

## Alternative crates considered

### rust-mcp-sdk

- URL on crates.io: https://crates.io/crates/rust-mcp-sdk
- Separate community implementation of MCP for Rust.
- Not the official SDK. Risk of API drift relative to the spec.

Trade-off: rejected as primary; mention only as fallback if rmcp
introduces a blocking issue.

### 4t145/rmcp (community fork)

- URL: https://github.com/4t145/rmcp
- Pre-dates the official SDK. The official SDK moved the `rmcp` crate
  name into `modelcontextprotocol/rust-sdk`. This fork is now legacy
  and may not be the same crate published at `rmcp` on crates.io.

Trade-off: ignore; the crates.io `rmcp` slot is owned by the official
SDK.

### agenterra-rmcp

- URL: https://crates.io/crates/agenterra-rmcp
- A specialized fork.

Trade-off: ignore for MVP.

## Failure-mode plan: hand-rolled JSON-RPC

If rmcp cannot meet a requirement (e.g. a transport or extension not
supported), the fallback is hand-rolling MCP at the JSON-RPC layer.
The spec is JSON-RPC 2.0 over stdio (newline-delimited) or streamable
HTTP. Required pieces:

- newline-delimited JSON-RPC reader/writer over stdin/stdout (any
  `AsyncRead`/`AsyncWrite` pair); for HTTP, an axum/hyper handler that
  reads JSON-RPC frames per the streamable HTTP transport.
- request envelope (`jsonrpc`, `id`, `method`, `params`) and
  response/error envelopes per JSON-RPC 2.0.
- `initialize`, `initialized`, `notifications/*`, `tools/list`,
  `tools/call`, `resources/list`, `resources/read`, `prompts/list`,
  `prompts/get`, plus capability negotiation per the spec.
- capability negotiation matrix matching protocol version
  `2025-11-25`.
- explicit session id handling for streamable HTTP server transport.

This is a contingency; do not implement it unless rmcp blocks a
required feature. Sketch only.

## Confidence

High for choice of rmcp. The crate exists, is official, is on
crates.io, is actively maintained, and matches the platform
constraints. The only open user decision is rmcp 0.16.0 vs 1.7.0
(see `msrv.md`).

## HALT-worthy findings

None. rmcp confirmed present at both 0.16.0 (user-pinned) and 1.7.0
(current crates.io latest).

## SOURCE_MAP reclassification

- rmcp exists, edition 2024, streamable HTTP server session: was
  user-pinned evidence, confirmed by current crates.io and GitHub.
- MCP spec revision `2025-11-25`: evidence-backed via
  modelcontextprotocol.io specification page.
