---
goal_id: TC42d
title: Explicit Scope And Nextest Gate
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Completed"
depends_on: ["TC42c"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-22T18:00:00+00:00"
started_at: "2026-05-22T18:05:00+00:00"
completed_at: "2026-05-22T18:35:00+00:00"
completion_commit: "a636697"
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

Objective:
- Require explicit `scope` on `registry_activate` / `registry_deactivate`. Omitted scope rejected with typed error + durable audit row. Install + run `cargo nextest` as first-class verification.

Changes (verified work commit `a636697`):
- `crates/daemon/src/ipc/server.rs` — `handle_registry_activate` / `handle_registry_deactivate` now `ok_or_else` on `params.scope` with `IpcErrorCode::ScopeInvalid` and message `"scope is required; pass {kind:'global'} for explicit global activation"` (deactivation message symmetric). The dispatcher's existing audit hook lands the rejection as `ipc_registry_activate` / `ipc_registry_deactivate` with `decision = "error"`.
- `crates/daemon/tests/registry_ipc.rs`, `crates/daemon/tests/registry_live_rebind.rs`, `crates/mcp/tests/registry_live_e2e.rs`, `crates/mcp/tests/registry_live_rebind_e2e.rs` — every `scope: None` callsite migrated to explicit `Some(ActivationScope::Global)` / `{"kind":"global"}`. Existing TC41/TC42/TC42b/TC42c semantics unchanged; the wire shape just stopped being implicit.
- `crates/daemon/tests/registry_scope_required.rs` (new) — three daemon-level tests: activate-missing-scope rejection + audit row, deactivate-missing-scope rejection + audit row, explicit-global activate+deactivate happy path.
- `crates/mcp/tests/registry_scope_required_e2e.rs` (new) — two MCP-level rejection tests proving the LLM-visible surface surfaces a scope-required error instead of silently widening to Global.

Files changed:
- `crates/daemon/src/ipc/server.rs`
- `crates/daemon/tests/registry_ipc.rs`
- `crates/daemon/tests/registry_live_rebind.rs`
- `crates/daemon/tests/registry_scope_required.rs` (new)
- `crates/mcp/tests/registry_live_e2e.rs`
- `crates/mcp/tests/registry_live_rebind_e2e.rs`
- `crates/mcp/tests/registry_scope_required_e2e.rs` (new)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo nextest run --workspace` — **317/317 passing, 0 skipped** (cargo-nextest 0.9.136 installed via `cargo install cargo-nextest --locked`)
- PASS: `cargo test -p terminal-commanderd --test registry_scope_required` — 3 tests
- PASS: `cargo test -p terminal-commander-mcp --test registry_scope_required_e2e` — 2 tests
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc/negative-assertion matches

Evidence (acceptance criteria):
1. `registry_scope_required::activate_without_scope_is_rejected_and_audited` asserts `IpcErrorCode::ScopeInvalid`, helpful reason, in-memory registry untouched, and the `ipc_registry_activate` audit row with `decision=error`.
2. `registry_scope_required::deactivate_without_scope_is_rejected_and_audited` mirrors the assertion for deactivate.
3. `registry_scope_required::explicit_global_scope_activates_normally` proves `Some(Global)` continues to activate + deactivate cleanly.
4. `registry_scope_required_e2e::mcp_activate_without_scope_returns_error` + `mcp_deactivate_without_scope_returns_error` prove the MCP surface forwards the rejection.
5. TC42c scoped isolation (`registry_scoped_rebind`, `registry_scoped_rebind_e2e`) and TC42b live rebind (`registry_live_rebind`, `registry_live_rebind_e2e`) still pass under the new contract.
6. TC41 command + bucket MCP tests still pass.
7. Bounded-output invariant preserved: no new raw-stream lane, no new tool, no new wire field carrying free-form bytes.

Source-status:
- `registry_activate` / `registry_deactivate` IPC + MCP handlers: **live (TC42d)**, now scope-required.
- `IpcErrorCode::ScopeInvalid`: **live (TC42c + TC42d)** — same variant, additional emission path.
- Persistent rows pre-TC42c: still rehydrate as `Global` via the column default; no migration changes.
- `cargo nextest` verification gate: **live (TC42d)** on this WSL host.

Commits:
- Goal file creation: `f3e68ce`
- Verified work commit: `a636697`
- Goal status commit: this commit

Known gaps / blockers:
- None.

Next goal:
- TC43-file-probe-search-watch-and-bounded-read.md — do NOT start until this TC42d report is reviewed.
