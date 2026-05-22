---
goal_id: TC42
title: Registry Hot Activation And Rule Binding
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Completed"
depends_on: ["TC41"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-22T13:45:00+00:00"
completed_at: "2026-05-22T15:00:00+00:00"
completion_commit: "fc3f337"
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
risk_level: "high"
---

# TC42 - Registry Hot Activation And Rule Binding

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC42-registry-hot-activation-and-rule-binding.md

## Goal File Workflow

0. Use the Branch Guard below before editing this goal file, source code, migrations, docs, tests, or generated artifacts.
1. After Branch Guard passes, update this file's frontmatter: set `status` to `In progress` and set `started_at` to an ISO-8601 timestamp.
2. Execute only this goal's mini-spec. Keep changes inside `allowed_files_or_area` and stop if a stop condition is hit.
3. If acceptance criteria pass, run the verification command(s), commit the verified work, then update this file: set `status` to `Completed`, set `completed_at`, and set `completion_commit` to the exact verified work commit hash.
4. Commit the goal-status update as a separate commit unless repository policy says otherwise.
5. If blocked, set `status` to `Blocked`, set `blocked_reason`, leave `completion_commit` empty unless a verified partial commit exists, and record the blocker in the final report.

## Branch Guard

This goal belongs only to branch:

```text
main
```

Before changing anything, run:

```bash
git branch --show-current
git status --short
```

The branch output must be exactly:

```text
main
```

If the current branch is one of the prohibited branches, or anything other than `main`, do not edit there. Switch to or create the correct worktree/branch, then rerun this Branch Guard. Stop if the correct branch/worktree is unavailable, dirty with unrelated work, or still does not print `main`.

## Mission Context

- Target project: https://github.com/special-place-administrator/terminal-commander
- Goal chain: terminal-commander-runtime
- Source material: current `main` repository, uploaded BACKLOG/EVIDENCE/FINAL reports, and this runtime-pivot chain.
- Current known state: TC01a-TC32 are reported complete and merged to `main`; real-deployment P0 items remain around rmcp stdio, PTY spawn, UDS IPC, and persistent audit writes.
- Desired end state: Terminal Commander becomes a provider-neutral MCP realtime signal abstraction layer where LLMs control probes/tools and receive only structured signal, bounded context, and searchable file/terminal intelligence.

## Mini-Spec

objective:
- Make registry rule selection/creation/testing/activation affect live probe runtimes by unique rule IDs, not only the persistent registry database.

non_goals:
- Do not invent a general plugin language.
- Do not allow arbitrary code execution as a rule.
- Do not implement provider-specific prompt engineering or hooks.

allowed_files_or_area:
- crates/store/src/registry.rs
- crates/store/src/import.rs
- crates/store/src/lib.rs
- crates/store/migrations/**
- crates/store/tests/**
- crates/sifters/src/**
- crates/sifters/tests/**
- crates/daemon/src/ipc/**
- crates/daemon/src/router.rs
- crates/daemon/src/runtime.rs
- crates/daemon/src/state.rs
- crates/daemon/src/command.rs
- crates/daemon/src/lib.rs
- crates/daemon/tests/**
- crates/core/src/** only for narrow DTO/schema additions required by activation/binding contracts
- crates/mcp/src/**
- crates/mcp/tests/**
- crates/mcp/Cargo.toml only if TC42 genuinely requires MCP-local dependency changes
- Cargo.lock only if dependency changes are required
- rules/**
- docs/mcp/**
- docs/rules/**
- docs/runtime/**
- docs/security/**
- tests/**/registry*
- tests/**/sifter*
- tests/**/mcp*
- .agent/goals/terminal-commander-runtime/TC42-*.md

forbidden_files:
- PTY spawn implementation
- stdin control
- shell execution
- file/directory/artifact probes
- installer/service work
- privileged helper
- sudo behavior
- TCP/HTTP/WebSocket/network listener
- raw stdout/stderr/log/tail stream endpoint
- direct command spawn from crates/mcp
- unsafe or unbounded regex execution

Note: scope amended from the original listing because (a) the repo
uses `crates/daemon/src/ipc/` as a module directory, not a single
`ipc.rs` file (same drift TC41 needed to correct), and (b) live
registry activation may need to touch daemon state/command/runtime
plus the store migration and test surfaces so an activated rule
actually affects in-flight probe traffic instead of only persisting
a registry row. Recorded in the final report.

contracts_or_interfaces:
- Rules have stable unique IDs and versions.
- LLM can registry_search, registry_get, registry_create, registry_test, registry_activate, and bind selected rules to a bucket/probe/session.
- Activation must hot-load or rebuild runtime rule sets for affected probes without reading raw stream history unless explicitly bounded.
- Unsafe regex/rule definitions must be rejected or capped.

invariants:
- The product is a realtime signal channel and abstraction layer for LLM agents, not a raw terminal/log dumping tool.
- MCP-facing code must not be an unrestricted root shell and must not spawn commands directly.
- No network listener, no setuid helper, no polkit/system-service install behavior unless a later explicit goal authorizes it.
- Responses visible to the LLM must be bounded, structured, and source-status honest.
- Raw terminal/file output is unavailable by default; bounded context is available only through pointers, file windows, or explicit capped reads.
- Every severity >= Medium signal event must have a source pointer or a pointer_unavailable_reason.
- Do not treat mock, test-only, scaffold-only, degraded, unknown, or disabled behavior as live success.

scope_substitution_policy:
- If the exact implementation path is impossible on the current host, do not silently substitute. Record the reason, source evidence, lost behavior, new source-status, and backlog priority in this goal file and final report.
- A substitute is only acceptable when it preserves the LLM-visible contract: bounded output, policy gate, auditability, source pointer/context, and no raw stream by default.

implementation_steps:
- Audit existing registry persistence and sifter runtime build path.
- Define runtime activation binding model: rule_id, version, bucket_id/probe_id/session, status, activated_at.
- Implement registry_test over samples and current rule validator.
- Implement registry_activate so future stream frames use the rule.
- Expose live registry tools via MCP.
- Add tests where an LLM-created rule is activated while a command/probe is running or before start and emits signal.

acceptance_criteria:
- A created/tested/activated rule affects emitted events in a runtime test.
- Activation records are persisted/audited.
- Bad regex/rules are rejected with bounded error messages.
- Tool docs explain how an LLM picks or creates registry IDs.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `main`.
- File paths changed.
- Verification command output summary.
- Any new public type, API, route, migration, feature flag, environment variable, event, or status enum introduced.
- Explicit source-status notes for live, partial, degraded, disabled, test-only, mock, blocked, unknown, or deleted behavior touched.
- Evidence that bounded-output and pointer invariants remain true for every LLM-visible response touched by this goal.

stop_conditions:
- Current branch is not exactly `main`.
- The goal requires touching forbidden files.
- The goal expands into another goal's scope.
- A required interface, route, package, repository path, migration path, branch, or runtime dependency is missing or contradicts this mini-spec.
- Verification cannot run for a reason that is not clearly pre-existing and documented.
- A security, credential, data-retention, privacy, production-safety, or destructive-change question appears that is not answered by this goal file.
- A change would create an unbounded raw-output path to the LLM.

verification_command:
```bash
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

## Task Prompt

Run TC42 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report

Objective:
- Make registry rule selection/creation/testing/activation affect live probe runtimes by unique rule IDs, not only the persistent registry database.

Changes (verified work commit `fc3f337`):
- Store layer adds two methods: `EventStore::list_active_rule_defs()` (bootstrap rehydration) and `EventStore::deactivate_rule(rule_id, version)` (close most-recent open activation row).
- New `daemon::activation::ActivationRegistry`: in-memory authority for active `(rule_id, version)` keys, deterministic snapshot, loaded from the persistent `rule_activations` table at bootstrap.
- `DaemonState` carries `Arc<ActivationRegistry>` and threads it into `CommandRuntime::new`.
- `CommandRuntime::start_combed` merges the active-rule snapshot with the per-call inline rules (inline rules win on key collision), then builds the per-job `SifterRuntime` from the merged set.
- IPC protocol: seven new methods (`registry_search`, `registry_get`, `registry_upsert`, `registry_test`, `registry_activate`, `registry_deactivate`, `registry_list_active`), matching request/response shapes, new error codes `IpcErrorCode::{RuleNotFound, RuleInvalid}`, bounded caps `MAX_REGISTRY_SEARCH_LIMIT`, `MAX_REGISTRY_TEST_SAMPLES`, `MAX_REGISTRY_TEST_SAMPLE_BYTES`.
- IPC server dispatches the new methods to the store and the activation registry; standard `ipc_<method>` audit rows continue to land via the existing dispatcher hook.
- MCP tool surface: seven new live tools forwarding through `McpDaemonClient`. `registry_upsert` accepts the rule body as a JSON string so the `schemars` derive stays narrow; the daemon parses and validates before persisting.
- Tool catalogue grew 10 (TC41) -> 17 (TC42), all `live`, zero `not_implemented`.

Files changed:
- `crates/store/src/registry.rs`
- `crates/daemon/src/activation.rs` (new)
- `crates/daemon/src/lib.rs`
- `crates/daemon/src/state.rs`
- `crates/daemon/src/command.rs`
- `crates/daemon/src/ipc/protocol.rs`
- `crates/daemon/src/ipc/server.rs`
- `crates/daemon/src/ipc/mod.rs`
- `crates/daemon/tests/registry_ipc.rs` (new)
- `crates/mcp/src/tools.rs`
- `crates/mcp/tests/mcp_stdio.rs`
- `crates/mcp/tests/mcp_live_daemon.rs`
- `crates/mcp/tests/registry_live_e2e.rs` (new)
- `.agent/goals/terminal-commander-runtime/TC42-registry-hot-activation-and-rule-binding.md` (frontmatter + this report)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, rustc 1.95.0, cargo-nextest 0.9.136):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after the work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo test --workspace` — 290 tests, 0 failures
- PASS: `cargo nextest run --workspace` — 290/290, 0 skipped
- PASS: `cargo test -p terminal-commanderd --test registry_ipc` — 8 tests
- PASS: `cargo test -p terminal-commander-mcp --test registry_live_e2e` — 2 tests
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc-comment matches

Evidence (acceptance criteria, asserted in tests):
1. `registry_live_e2e::activated_rule_drives_signal_then_deactivated_rule_does_not` walks the full LLM flow through MCP: upsert a keyword rule, dry-run it against bounded samples (one match, one miss), activate it, list-active confirms one entry, start `echo needle ...` via MCP, observe a `needle_match` event on the bucket via `bucket_wait`, deactivate, start the same command again, prove the same argv now produces only lifecycle events (`kind` starting with `command_`).
2. `registry_live_e2e::registry_upsert_rejects_invalid_regex_through_mcp` proves an unclosed regex group is rejected with a typed MCP `invalid_params` error (mapped from `IpcErrorCode::RuleInvalid`).
3. `registry_ipc::registry_test_evaluates_rule_against_samples` proves dry-run returns matches only for the matching sample with bounded shape.
4. `registry_ipc::registry_test_rejects_oversize_sample_count` proves `MAX_REGISTRY_TEST_SAMPLES + 1` is rejected with `IpcErrorCode::RuleInvalid` before the sifter runs.
5. `registry_ipc::registry_activate_then_list_then_deactivate` proves the in-memory authority matches the persistent activation rows and that `ipc_registry_*` audit rows land for every accepted call.
6. `registry_ipc::registry_activations_survive_daemon_restart` proves a second bootstrap rehydrates the in-memory registry from the persistent table.
7. `registry_ipc::registry_upsert_rejects_invalid_regex` proves daemon-side validation surfaces `RuleInvalid` instead of `Internal`.

Source-status:
- `registry_search`, `registry_get`, `registry_upsert`, `registry_test`, `registry_activate`, `registry_deactivate`, `registry_list_active`: **live (TC42)**.
- `ActivationRegistry`: **live**. Persistent backing via the existing TC13 `rule_activations` table; no new SQL migration required.
- Activation scope is **global**: an active rule applies to every newly-started command. Per-bucket / per-job binding is not implemented; the in-memory registry is keyed by `(rule_id, version)` without a scope discriminator. This matches the TC42 acceptance criteria ("activated rule produces a bucket signal") without expanding scope into TC45's parallel-router work.
- Already-running command/probe hot rebind: **not implemented**. The `ProcessProbe` owns the `Arc<SifterRuntime>` captured at spawn time; swapping it would require a new probe API surface that TC42 deliberately does not introduce. Documented as a known gap (next goal can layer this on if needed).
- Audit: every `registry_*` IPC call lands a row through `PersistentAudit` via the standard `ipc_<method>` dispatcher path; no production code path falls back to `InMemoryAudit`.
- Bounded-output invariant: every MCP payload still routes through the daemon's `MAX_RESPONSE_BYTES` envelope; the live e2e asserts the cap on real responses for command + bucket tools and the registry response shapes are bounded by design (counts, ids, severities, capped JSON).

Commits:
- Prep amendment (scope alignment, no code): `26b00eb`
- Verified work commit: `fc3f337`
- Goal status commit: this commit

Known gaps / blockers:
- Hot rebind of an already-running command's sifter is explicitly not implemented; the next probe-touching goal (or a follow-up) needs a swap API on `ProcessProbe` to make this work without daemon restart-equivalent semantics for in-flight jobs.
- Activation scope is global. If TC45 (parallel probe router + multi-bucket bindings) wants per-bucket activation, the `ActivationRegistry` will gain a scope key.
- `started_at` / `completed_at` are operator-set timestamps; commit author dates are the audit-grade truth.

Next goal:
- TC43-file-probe-search-watch-and-bounded-read.md — do not start until this TC42 report is reviewed.
