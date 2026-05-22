---
goal_id: TC42d
title: Explicit Scope And Nextest Gate
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "In progress"
depends_on: ["TC42c"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-22T18:00:00+00:00"
started_at: "2026-05-22T18:05:00+00:00"
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "TC42c final report: optional scope, omitted defaults to Global"
risk_level: "high"
---

# TC42d - Explicit Scope And Nextest Gate

## Branch Guard

Branch must be exactly `main`.

## Mini-Spec

objective:
- Make registry activation / deactivation require an explicit scope. Omitted scope MUST be rejected with a typed error and durably audited; it MUST NOT silently default to Global.
- Install + run `cargo nextest run --workspace` as a first-class verification gate.

non_goals:
- PTY, file probe, raw stream lane, parallel router scope extensions.

allowed_files_or_area:
- crates/core/**
- crates/store/**
- crates/daemon/**
- crates/mcp/**
- crates/daemon/tests/**
- crates/mcp/tests/**
- docs/runtime/**
- docs/rules/**
- docs/mcp/**
- .agent/goals/terminal-commander-runtime/TC42d-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

forbidden_files:
- PTY spawn, stdin control, file/dir/artifact probe, installer/service, privileged helper, network listener, raw stdout/stderr endpoint, direct command spawn from crates/mcp, shell execution.

contracts_or_interfaces:
- `RegistryActivateParams.scope` and `RegistryDeactivateParams.scope` remain `Option<ActivationScope>` on the wire so older clients fail loudly instead of being silently rewritten. Daemon-side: `None` -> `IpcErrorCode::ScopeInvalid` with message "scope is required; pass {kind:'global'} for explicit global".
- Explicit `{kind:"global"}` continues to behave as TC42b global rebind.
- Rejection lands an `ipc_registry_activate` / `ipc_registry_deactivate` row through the standard dispatcher audit path (decision = "error") with a reason captured.
- Old persisted activation rows (pre-TC42c) still rehydrate as Global on bootstrap; no migration required.

acceptance_criteria:
- Missing-scope `registry_activate` is rejected with `ScopeInvalid` and audited.
- Missing-scope `registry_deactivate` is rejected with `ScopeInvalid` and audited.
- Explicit `{kind:"global"}` still works end-to-end.
- TC42c bucket/job isolation still passes.
- TC42b live rebind still passes.
- TC41 command + bucket MCP tests still pass.
- `cargo nextest run --workspace` returns 0; output captured in the final report.
- `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` returns only doc-comment / negative-assertion matches.

stop_conditions:
- Branch not `main`.
- nextest cannot be installed/run on the verification host -> mark Blocked rather than swap in `cargo test`.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
cargo test -p terminal-commanderd --test registry_scope_required
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
```

## Final Report

(to be filled in after verified work commit lands)
