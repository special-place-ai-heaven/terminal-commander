---
goal_id: TC42c
title: Scoped Rule Bindings For Buckets Jobs Probes
chain_id: terminal-commander-runtime
phase: Wave 4 - MCP control surface
status: "Pending"
depends_on: ["TC42b"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-22T17:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "TC42 final report: global ActivationRegistry, per-bucket/per-job scope deferred"
  - "TC42b final report: live rebind for active streams; scope still global"
risk_level: "high"
---

# TC42c - Scoped Rule Bindings For Buckets Jobs Probes

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC42c-scoped-rule-bindings-for-buckets-jobs-probes.md

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
- Source material: TC42 + TC42b final reports and the existing `ActivationRegistry` / `CommandRuntime::rebind_all_jobs` machinery.
- Current known state: `registry_activate` / `registry_deactivate` are global. Activation snapshot applies to every newly-started AND every already-running command. There is no way for an LLM agent to bind a rule to one specific bucket/job/probe without leaking it into unrelated live streams.
- Desired end state: The LLM can activate rule ID X with an explicit scope (global, bucket_id, job_id, probe_id) and the daemon will only merge that rule into the matching job(s). Live streams whose scope does not match are unaffected. Inline rules attached at `command_start_combed` time remain job-local and untouched by scoped or global registry changes. All bind / unbind events are auditable. No raw stdout/stderr ever surfaces.

## Mini-Spec

objective:
- Implement scoped registry rule bindings so activated rules can target a specific bucket, job, probe, or explicit global scope. Live command streams must rebind without restart and must not leak signal across scopes.

non_goals:
- Do not introduce a PTY runtime or stdin lane.
- Do not expose raw stdout/stderr to the LLM.
- Do not implement file/directory/artifact probe features (that is TC43).
- Do not rescan historical frames; "no fake historical matches" stays locked from TC42b.
- Do not change existing MCP tool names. Existing tools may grow new optional scope fields, but no rename, no removal.
- Do not introduce a network listener or a raw-log endpoint.
- Do not silently broaden activation; a request that omits scope MUST still be explicit (default decided in `contracts_or_interfaces`).

allowed_files_or_area:
- crates/daemon/**
- crates/sifters/**
- crates/store/**
- crates/core/** only for narrow DTO/schema additions (e.g. an `ActivationScope` enum + wire reps)
- crates/mcp/** only for registry tool scope metadata + scoped parameter plumbing
- crates/daemon/tests/**
- crates/mcp/tests/**
- docs/runtime/**
- docs/rules/**
- docs/mcp/**
- tests/**/registry*
- tests/**/sifter*
- tests/**/mcp*
- .agent/goals/terminal-commander-runtime/TC42c-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md (insert TC42c row only)
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md (insert TC42c step only)

forbidden_files:
- PTY spawn implementation
- stdin control
- file / directory / artifact probe implementation
- installer / service work
- privileged helper
- network listener
- raw stdout/stderr/log/tail stream endpoint
- direct command spawn from crates/mcp
- shell execution
- unsafe or unbounded regex execution

contracts_or_interfaces:
- A new `ActivationScope` value type captures one of:
  - `Global`
  - `Bucket { bucket_id }`
  - `Job { job_id }`
  - `Probe { probe_id }` (only if probe identity is already addressable from the IPC surface; otherwise this variant must be implemented and accepted, but bound-error-returned with a typed code when the probe id cannot be resolved to a live job)
- `ActivationRegistry` must key activations by `(rule_id, version, scope)`. Snapshotting for a specific running job must return only the entries whose scope matches `(global ∪ matching-bucket ∪ matching-job ∪ matching-probe)`.
- `registry_activate` / `registry_deactivate` IPC params accept an optional `scope` field. Omitted scope MUST default to `Global` (preserves wire compatibility for TC42/TC42b clients) but the request handler MUST emit an audit row that explicitly records `scope=global` so the audit trail is honest about what was requested.
- An unsupported / malformed scope value (e.g. `Scope::Probe { probe_id }` for a probe id the daemon does not know) MUST be rejected with a typed error code (`IpcErrorCode::ScopeInvalid` or equivalent). No silent fallback to global.
- `registry_list_active` MUST return scope per entry. A pre-TC42c client that ignores the field still gets a valid response.
- `CommandRuntime::rebind_all_jobs` must compute per-job `(active-for-this-job ∪ inline)` using the scoped resolver, not a flat global snapshot.
- A scoped activation MUST NOT affect commands started before the activation AND whose job/bucket/probe does not match. A scoped deactivation MUST only affect the matching scope.
- A global activation behaves identically to TC42b: every live job rebinds and every future job picks the rule up at spawn.
- Inline rules attached at `command_start_combed` time remain job-local and survive every rebind (TC42b invariant).
- Rebind work remains bounded, audited, and must not block the request loop for unbounded time. The `command_sifter_rebind` audit row must include the resolved scope so an auditor can prove only matching jobs were touched.
- No raw stream content may surface in any response or audit row introduced by this goal.
- MCP registry tool surface must expose scope clearly in inputs AND outputs and must return bounded metadata only.

invariants:
- The product is a realtime signal channel and abstraction layer for LLM agents, not a raw terminal/log dumping tool.
- MCP-facing code must not be an unrestricted root shell and must not spawn commands directly.
- No network listener, no setuid helper, no polkit/system-service install behavior.
- Responses visible to the LLM must be bounded, structured, and source-status honest.
- Raw terminal/file output is unavailable by default; bounded context is available only through pointers.
- Every severity >= Medium signal event must have a source pointer or a `pointer_unavailable_reason`.
- Do not treat mock, test-only, scaffold-only, degraded, unknown, or disabled behavior as live success.

scope_substitution_policy:
- If a per-probe scope cannot be implemented without touching `crates/probes/**`, accept the `Probe { probe_id }` variant on the wire and resolve it to the owning `JobId` at the daemon layer before storing; do NOT cross into `crates/probes/**`.
- If even that resolution is impossible without probe-API changes, mark `Probe` scope as `not_implemented` in the IPC handler with a typed error; do NOT silently fall back to global. Record the seam in the final report.
- A substitute is only acceptable when it preserves the LLM-visible contract: bounded output, policy gate, auditability, source pointer/context, and no raw stream by default.

implementation_steps:
- Add `ActivationScope` (Rust enum + serde wire form) in `crates/core` (or `crates/daemon::activation` if `crates/core` is too narrow). Variants: `Global`, `Bucket { bucket_id }`, `Job { job_id }`, `Probe { probe_id }`. Wire form snake_case tagged enum.
- Refactor `ActivationRegistry` to key by `(rule_id, version, scope)`. Add `snapshot_for_job(job_id, bucket_id, probe_id) -> Vec<RuleDefinition>` that returns only matching entries. Keep `snapshot()` returning every entry for `registry_list_active`.
- Extend the persistent activation row schema with a `scope_kind` + `scope_value` pair (or equivalent). New migration is allowed; it must be additive and idempotent. Older rows rehydrate as `Global`.
- Extend IPC `RegistryActivateParams` / `RegistryDeactivateParams` with an optional `scope` field defaulting to `Global` on deserialize. Add `RegistryActiveEntry::scope`.
- Add `IpcErrorCode::ScopeInvalid` for unresolvable / malformed scopes.
- Update `handle_registry_activate` / `handle_registry_deactivate` to validate scope, persist the scoped row, update the in-memory registry, and call a scope-aware `rebind_all_jobs` that targets only matching jobs.
- Update `CommandRuntime::start_combed` to merge `(scoped-active-for-this-job ∪ inline)` instead of `(global-active ∪ inline)`. Use the same shared helper so spawn-time and rebind-time semantics stay identical.
- Update `CommandRuntime::rebind_all_jobs` to iterate live jobs and, per job, compute the scoped resolved set. Audit row `command_sifter_rebind` MUST include resolved scope.
- Update MCP tool schemas (`registry_activate`, `registry_deactivate`, `registry_list_active`) so the scope is exposed. Optional input, mandatory output.
- Add daemon-level tests proving:
  - bucket-scoped activation merges into the matching job and not into an unrelated job
  - job-scoped activation behaves the same
  - global activation still works exactly as in TC42b
  - scoped deactivation only affects the matching scope
  - bad scope is rejected with a typed error
- Add an MCP live-daemon e2e proving the LLM-visible contract end-to-end: two running commands emitting the same token, scoped activation on one bucket, prove bucket A emits the signal and bucket B does not, then scoped deactivation, then prove bucket A stops emitting.

acceptance_criteria:
- Two long-running non-shell commands are started, emitting the same matchable token.
- A rule is activated with bucket scope = bucket A. Bucket A starts emitting matching signal on subsequent matching lines. Bucket B does NOT emit that signal.
- A job-scoped activation behaves the same when keyed to a job id.
- A scoped deactivation removes the rule only for the matching scope. Other still-running scopes (or global) are untouched.
- A global activation continues to work, and only when the operator explicitly requests `Global` (or omits scope and accepts the documented default).
- A scope referring to a `bucket_id` / `job_id` / `probe_id` the daemon does not know is rejected with a typed error.
- All TC41, TC42, and TC42b tests still pass without modification (or with only minimal compatibility patches that preserve their original assertions).
- No raw stdout/stderr appears in any MCP or daemon response (verified by the existing `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` grep and by visual inspection of the new tests).
- Audit: every scoped activate / deactivate / rebind lands a row through `PersistentAudit` with a resolvable scope value. No production code path falls back to `InMemoryAudit`.
- `command_status`, `bucket_wait`, `bucket_events_since`, `event_context` keep working unchanged.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `main`.
- File paths changed.
- Verification command output summary.
- Any new public type, API, route, migration, feature flag, environment variable, event, or status enum introduced (including the new `ActivationScope` variants, the new error code, and any new audit shape).
- Explicit source-status notes for live, partial, degraded, disabled, test-only, mock, blocked, unknown, or deleted behavior touched.
- Evidence that bounded-output and pointer invariants remain true for every LLM-visible response touched by this goal.

stop_conditions:
- Current branch is not exactly `main`.
- The goal would require touching `crates/probes/**` to implement scoped binding semantics.
- A required change would expose raw stdout/stderr by default.
- The goal would require introducing a network listener, a privileged helper, or a shell execution path.
- A required interface, route, migration path, branch, or runtime dependency contradicts this mini-spec.
- Verification cannot run on Linux/WSL.
- The goal expands into another goal's scope (PTY, file probe, parallel router multi-bucket bindings beyond per-rule scope, etc.).

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
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
```

Live-daemon scoped binding e2e (targeted):

```bash
cargo test -p terminal-commanderd --test registry_scoped_rebind
cargo test -p terminal-commander-mcp --test registry_scoped_rebind_e2e
```

## Task Prompt

Run TC42c only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing. Do not start TC43 until this goal's final report is reviewed.

## Final Report

(to be filled in after verified work commit lands)
