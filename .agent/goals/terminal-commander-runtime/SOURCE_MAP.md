# Source Map - terminal-commander-runtime

## Repository

- `https://github.com/special-place-administrator/terminal-commander`
- Branch for this chain: `main`

## Uploaded evidence

- `BACKLOG.md` — P0/P1/P2/P3 deferrals after TC32.
- `EVIDENCE_REPORT.md` — per-goal commits and test-count summary.
- `FINAL_REPORT.md` — executed chain status, substitutions, and recommendations.

## Current code surfaces to inspect first

- `crates/daemon/src/main.rs` — daemon binary entry point.
- `crates/mcp/src/main.rs` — MCP binary entry point.
- `crates/mcp/src/lib.rs` — ToolSurface and current MCP-shaped methods.
- `crates/daemon/src/router.rs` — in-process Router and audit placeholder seam.
- `crates/probes/src/process.rs` — process probe stream capture.
- `crates/probes/src/pty.rs` — ANSI/PTY normalization and prompt detection.
- `crates/probes/src/file.rs` — file probe behavior.
- `crates/probes/src/directory.rs` — directory/artifact probe behavior.
- `crates/store/src/lib.rs` and `crates/store/src/registry.rs` — event store and registry persistence.
- `docs/mcp/README.md`, `docs/runtime/**`, `SPEC.md`, `ARCHITECTURE.md`, `README.md` — contract and source-status docs.

## Evidence rules

- Prefer local source and local verification over GitHub rendering.
- If GitHub/raw rendering appears inconsistent with local build output, trust the local branch and record the discrepancy in TC33.
- Do not mark a runtime path live because a test-only in-process path exists.
