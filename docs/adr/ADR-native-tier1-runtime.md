# ADR: Native tier-1 runtime for Windows and Linux

Status: Accepted
Date: 2026-05-24
Supersedes: parts of SPEC.md §3 (non-goals) and earlier statements in
`docs/research/_USER_DECISIONS.md` that excluded Windows native shipping.
Related: `docs/audits/2026-05-24-windows-mcp-connect-closed-findings.md` §13.

## Context

The codebase contained three contradictory positions on Windows:
SPEC §3 listed Windows-native shipping as a non-goal;
`crates/daemon/src/ipc/mod.rs` said `Windows native: NOT SUPPORTED`;
crate code, the resolver, and packaging docs treated Windows-x64 as a
live target. The user-reported failure mode
(`MCP error -32000: Connection closed`) cannot be diagnosed cleanly
while these positions stand.

## Decision

1. Tier-1 targets are Windows-x64 native and Linux-x64 native. WSL
   Ubuntu is supported through the Linux-x64 artifact; it is not a
   separate target and the runtime does not require a WSL bridge.
2. macOS (arm64 + x64) is a build-only tier-3 target. Binaries are
   produced when cross-compile is straightforward, but QA, smoke
   tests, and doctor coverage are not in scope.
3. The runtime is native Rust (edition 2024, toolchain 1.95) on
   every supported OS. Once installed, neither the MCP adapter nor
   the daemon depends on Node, Python, WSL, PowerShell, or shell
   scripts.
4. The install step may use any mechanism that delivers one-command
   set-and-forget UX (npm postinstall downloader, winget, scoop,
   cargo install, or curl one-liner). The current `npm install -g`
   front door is acceptable.

## Consequences

- SPEC.md §3 non-goals must be updated (this plan's Task 1).
- `crates/daemon/src/ipc/mod.rs` module doc must be updated (this
  plan's Task 5).
- Future plans address replacing the Node session supervisor (this
  plan's Task 8) and migrating the install vehicle (Phase 4 plan).
- The legacy WSL bridge path (`TC_USE_LEGACY_WSL_BRIDGE=1`,
  `lib/wsl/spawn.js`) is now legacy/optional, not the documented
  Windows path.
