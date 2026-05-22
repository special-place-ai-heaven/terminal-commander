---
goal_id: TC44
title: Posix Pty Spawn And Stdin Control
chain_id: terminal-commander-runtime
phase: Wave 6 - Interactive terminal capability
status: "Completed"
depends_on: ["TC43"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-22T19:35:00+00:00"
completed_at: "2026-05-22T20:55:00+00:00"
completion_commit: "4cc8dcc"
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
  - "https://raw.githubusercontent.com/special-place-administrator/terminal-commander/main/crates/probes/src/pty.rs"
risk_level: "high"
---

# TC44 - Posix Pty Spawn And Stdin Control

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC44-posix-pty-spawn-and-stdin-control.md

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
- Add POSIX/WSL PTY spawn and controlled stdin so interactive commands can be observed and steered while still emitting only filtered signal to the LLM.

non_goals:
- Do not support Windows-native ConPTY in this goal.
- Do not pass secrets blindly from the LLM.
- Do not bypass policy for sudo/password prompts.
- Do not expose raw PTY screen buffers by default.

allowed_files_or_area:
- crates/probes/src/pty.rs
- crates/probes/src/lib.rs
- crates/probes/tests/**
- crates/daemon/src/pty_command.rs
- crates/daemon/src/ipc/**
- crates/daemon/src/state.rs
- crates/daemon/src/policy.rs
- crates/daemon/src/router.rs
- crates/daemon/src/runtime.rs
- crates/daemon/src/lib.rs
- crates/daemon/tests/**
- crates/mcp/src/**
- crates/mcp/tests/**
- crates/core/src/** only for narrow DTO/schema additions required by PTY DTOs and prompt events
- docs/runtime/**
- docs/mcp/**
- docs/security/**
- Cargo.toml / Cargo.lock / per-crate Cargo.toml only for the justified PTY dependency below
- .agent/goals/terminal-commander-runtime/TC44-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

Note: `crates/daemon/src/command.rs` is intentionally NOT in the normal allowed edit set. PTY lifecycle must be a separate runtime path (mirroring TC43's `WatchRuntime`). If implementation proves `command.rs` is required, stop and report the exact seam instead of editing it silently.

forbidden_files:
- Windows-native ConPTY implementation
- network listeners
- secret storage
- automatic password entry without explicit policy
- LLM-supplied password forwarding
- raw PTY screen buffer endpoint
- direct command spawn from crates/mcp
- direct file reads from crates/mcp
- shell execution feature expansion
- directory/artifact probe expansion
- TCP/HTTP/WebSocket listener
- audit rows containing stdin text, secret text, hashes of stdin text, or raw prompt payloads

contracts_or_interfaces:
- PTY spawn is platform-gated to Linux/WSL POSIX where supported. Unsupported hosts return a typed error (`UnsupportedPlatform` or equivalent), not a fake-success.
- `command_write_stdin` (or equivalent IPC method) must target a running interactive PTY job, must be bounded in byte count, and must be audited.
- Prompt detection may emit structured prompt events (e.g. `password_prompt`, `sudo_prompt`, `yes_no_prompt`). Secret-bearing prompts MUST be marked secret on the event shape; the event MUST NOT carry the typed secret value.
- `command_write_stdin` MUST be denied by default when the target PTY job is in an active secret-prompt state. The denial returns a typed `IpcErrorCode::SecretInputDenied` (or equivalent). No automatic password entry. No LLM-supplied password forwarding under any path.
- Audit rows for stdin / prompt activity carry only bounded metadata: `job_id`, `byte_count`, `prompt_kind`, `decision`, `reason`. Audit rows MUST NOT contain stdin text, secret text, hashes of stdin text, or raw prompt payloads.
- ANSI/CR normalization (`AnsiNormalizer` from `crates/probes::pty`) remains active before frames reach the sifter runtime. No raw screen buffer is surfaced to the LLM by default.

dependency:
- Pre-approved PTY dependency: `pty-process = "=0.5.3"`.
- Use the Tokio/async-capable path if the crate exposes it cleanly.
- License basis: MIT.
- Rationale: directly matches the P0 backlog wording "pty-process spawn path"; current enough (0.5.3 dated 2025-07-11 per docs.rs); designed to spawn commands attached to a PTY.
- Fallback policy: if `pty-process` 0.5.3 fails workspace / lint / platform constraints, STOP and report; do NOT silently substitute. The only sanctioned next candidate is `portable-pty` (docs.rs 0.9.0, MIT, cross-platform PTY API), and only after explicit review and goal-file amendment.

dependency_evidence:
- `pty-process` 0.5.3 docs.rs source exists and describes "spawn commands attached to a pty"; changelog shows 0.5.3 on 2025-07-11.
- Fedora packaging records `rust-pty-process` 0.5.3 with MIT license and a `tokio` feature.
- `portable-pty` remains fallback-only; docs.rs shows `portable-pty` 0.9.0 (MIT, cross-platform). No silent switch.

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
- Inspect existing AnsiNormalizer and PromptDetector.
- Resolve POSIX PTY crate/dependency with current compatibility and licensing evidence.
- Implement PTY command spawn with stdout/stderr/screen normalization into frames.
- Implement command_write_stdin and prompt event handling.
- Expose command_write_stdin through daemon IPC and MCP only after tests pass.
- Add tests using a pseudo-interactive script, yes/no prompt, and password-like prompt detection without entering a real secret.

acceptance_criteria:
- Interactive PTY command can emit signal events through buckets.
- LLM can write bounded stdin to an interactive job through MCP.
- Secret prompt events are detected and do not leak secret text.
- Unsupported platforms are explicitly blocked, not treated as live success.

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
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
# targeted PTY IPC tests
cargo test -p terminal-commanderd --test pty_ipc -- --nocapture
# targeted live-daemon MCP PTY e2e
cargo test -p terminal-commander-mcp --test pty_tools_live_e2e -- --nocapture
# privilege model guards on the MCP crate
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
# prove MCP does not read files directly
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
# audit/grep evidence that stdin text is not persisted or returned: the new
# audit rows for stdin/prompt activity must carry only bounded metadata
# (job_id, byte_count, prompt_kind, decision, reason); a manual review of
# the test output captures the audit row shapes.
```

Verification host evidence:
- WSL2 / Linux POSIX. The PTY tests MUST execute on a real POSIX host; do not "pass" them on Windows-native by skipping.

## Scope Amendment (TC44 prep)

This amendment aligns the original TC44 mini-spec with the actual repo layout as of TC43, codifies the pre-approved PTY dependency, and locks the secret-prompt boundary. Same precedent as TC41 / TC42 / TC43.

Drift corrected:

- `crates/daemon/src/ipc.rs` does not exist on `main`; daemon uses `crates/daemon/src/ipc/` as a module directory. Allowed area now points at `crates/daemon/src/ipc/**`.
- `tests/**/pty*` / `tests/**/stdin*` are not real paths; tests live in `crates/<crate>/tests/`. Allowed area now lists `crates/daemon/tests/**`, `crates/mcp/tests/**`, and `crates/probes/tests/**`.
- The original `allowed_files_or_area` omitted `crates/daemon/src/state.rs`, `crates/daemon/src/policy.rs`, `crates/daemon/src/lib.rs`, and a dedicated `crates/daemon/src/pty_command.rs`, all of which are needed to wire a PTY lifecycle into the daemon the same way `command.rs` wires a non-PTY process probe lifecycle and `file_watch.rs` wires a file-watch lifecycle. They are now explicit.
- `crates/probes/src/process.rs` removed from the allowed area: TC44 is the PTY-specific path; the non-PTY argv runtime is locked.
- `Cargo.toml` / `Cargo.lock` / per-crate `Cargo.toml` are now explicitly allowed only for the justified PTY dependency below.
- Per-crate test directories under `crates/probes/tests/**` are now explicit.

Dependency:

- Pre-approved: `pty-process = "=0.5.3"` (MIT). Tokio/async path preferred when cleanly exposed.
- Fallback: if `pty-process` 0.5.3 fails workspace/lint/platform constraints, STOP and report. The only sanctioned next candidate is `portable-pty` (0.9.0, MIT) and only after explicit review.
- Evidence sources are recorded under `dependency_evidence` above (docs.rs source + changelog, Fedora packaging, `portable-pty` fallback metadata).

Explicit non-allowance:

- `crates/daemon/src/command.rs` is intentionally not in the normal allowed edit set. PTY lifecycle must be a separate runtime path. If implementation proves `command.rs` is required, stop and report the exact seam instead of editing it silently.

Secret-prompt boundary (locked):

- Prompt detection may emit structured `password_prompt` / `sudo_prompt` / `yes_no_prompt` events. Secret-bearing events MUST be marked secret. Events MUST NOT contain typed secret values.
- `command_write_stdin` MUST be denied by default when the target PTY job is in an active secret-prompt state. Denial code: `IpcErrorCode::SecretInputDenied` (or equivalent typed code; closed-set).
- No automatic password entry. No LLM-supplied password forwarding under any code path.
- Audit rows for stdin / prompt activity carry only bounded metadata: `job_id`, `byte_count`, `prompt_kind`, `decision`, `reason`. Audit MUST NOT contain stdin text, secret text, hashes of stdin text, or raw prompt payloads.
- Future operator/console secret entry requires a separate explicit policy goal — TC44 does NOT add it.

Forbidden list tightened:

- Windows-native ConPTY, network listeners, secret storage, automatic password entry, LLM-supplied password forwarding, raw PTY screen buffer endpoint, direct command spawn from `crates/mcp`, direct file reads from `crates/mcp`, shell execution feature expansion, directory/artifact probe expansion, TCP/HTTP/WebSocket listener, audit rows containing stdin/secret/hash/raw-prompt bytes.

Verification additions:

- `git branch --show-current`, `git status --short`, `cargo test --workspace`, targeted `cargo test -p terminal-commanderd --test pty_ipc -- --nocapture`, targeted `cargo test -p terminal-commander-mcp --test pty_tools_live_e2e -- --nocapture`, `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`, `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src`, manual audit-row-shape review, and explicit WSL/Linux PTY evidence are now part of the verification command set so the gates are explicit and reproducible.

## Task Prompt

Run TC44 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report

Objective:
- Add POSIX/WSL PTY spawn and controlled stdin so interactive commands can be observed and steered while emitting only filtered signal to the LLM, without touching `command.rs`, without exposing raw screen buffers, and without ever forwarding an LLM-supplied secret.

Changes (verified work commit `4cc8dcc`):
- `crates/probes/Cargo.toml`: `pty-process = "=0.5.3"` (MIT) added under `[target.'cfg(unix)'.dependencies]`, async feature on. Pinned per the TC44 prep amendment; no fallback to `portable-pty` was needed.
- `crates/probes/src/pty.rs`: new `PtyProbe` runtime (mod `runtime`, cfg(unix) only). Owns the pty exclusively in its streaming task; the borrow-checker headache from sharing `pty_process::Pty` is resolved by a tokio mpsc channel for stdin writes. Reads run with a 50ms timeout per loop iteration so the loop also services cancellation and queued writes. PTY output flows through the existing `AnsiNormalizer` (ANSI/CR normalization) and `PromptDetector`. The detector now also inspects the pending partial-line buffer via the new `AnsiNormalizer::peek_pending`, so `[sudo] password for dev: ` style prompts (no trailing newline) flip the secret flag before the LLM can write. New types: `PtyProbeConfig`, `PtyProbeMetrics`, `PtyProbeError`, `WriteStdinError`, `PtyProbe`, plus `DEFAULT_PTY_GRACE` and `MAX_PTY_STDIN_BYTES = 4096`.
- `crates/daemon/src/pty_command.rs` (new): `PtyRuntime` lifecycle. Mirrors `CommandRuntime` / `WatchRuntime` shape but lives entirely in this module so `command.rs` stays untouched. Audit rows are `pty_command_start` / `pty_command_write_stdin` / `pty_command_stop` / `pty_sifter_rebind`. The pre-spawn shell-interpreter deny list (`SHELL_INTERPRETERS_DENY` from `command.rs`) is reused; the existing `PolicyAction::CommandStart` gate (sudo/doas/su/... default-deny) applies before spawn. `write_stdin` returns `PtyRuntimeError::SecretInputDenied` when the probe has flagged a secret prompt; the audit metadata is exactly `{byte_count, prompt_kind: "secret"}` — never the bytes.
- `crates/daemon/src/state.rs`: `DaemonState.pty: Arc<PtyRuntime>` wired alongside `CommandRuntime` and `WatchRuntime` at bootstrap (cfg(unix)).
- `crates/daemon/src/ipc/protocol.rs`: 4 new IPC methods (`PtyCommandStart`, `PtyCommandWriteStdin`, `PtyCommandStop`, `PtyCommandList`), new error code `IpcErrorCode::SecretInputDenied`, caps `MAX_PTY_ARGV_ITEMS` and `MAX_PTY_STDIN_BYTES`. Param/response DTOs carry only bounded metadata; no raw screen buffer field exists.
- `crates/daemon/src/ipc/server.rs`: 4 handler fns. `validate_scope_against_live_jobs` now resolves `Bucket`/`Job`/`Probe` scope ids against the union of `command.live_jobs()`, `watch.live_watches()`, and `pty.live_jobs()`. `handle_registry_activate` / `handle_registry_deactivate` rebind all three runtimes; the returned `jobs_rebound` is the sum.
- `crates/mcp/src/tools.rs`: 4 new MCP tools forwarding through the daemon UDS (`pty_command_start` / `pty_command_write_stdin` / `pty_command_stop` / `pty_command_list`). Flat MCP DTOs. The MCP crate still does not touch the filesystem and still does not spawn commands. Tool catalogue grew 22 -> 26 live; `not_implemented` count remains zero.
- `crates/mcp/tests/mcp_live_daemon.rs` + `crates/mcp/tests/mcp_stdio.rs`: tool-set assertions updated to the 26-tool catalogue.

Files changed:
- `crates/probes/Cargo.toml`
- `crates/probes/src/lib.rs`
- `crates/probes/src/pty.rs`
- `crates/daemon/src/pty_command.rs` (new)
- `crates/daemon/src/lib.rs`
- `crates/daemon/src/state.rs`
- `crates/daemon/src/ipc/mod.rs`
- `crates/daemon/src/ipc/protocol.rs`
- `crates/daemon/src/ipc/server.rs`
- `crates/daemon/tests/pty_ipc.rs` (new, 6 tests)
- `crates/mcp/src/tools.rs`
- `crates/mcp/tests/mcp_live_daemon.rs`
- `crates/mcp/tests/mcp_stdio.rs`
- `crates/mcp/tests/pty_tools_live_e2e.rs` (new, 3 tests)
- `Cargo.lock`
- `.agent/goals/terminal-commander-runtime/TC44-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, rustc 1.95.0, cargo-nextest 0.9.136, pty-process 0.5.3):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **334/334 passing, 0 skipped**
- PASS: `cargo test -p terminal-commanderd --test pty_ipc -- --nocapture` — 6 tests
- PASS: `cargo test -p terminal-commander-mcp --test pty_tools_live_e2e -- --nocapture` — 3 tests
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc/negative-assertion matches
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches
- PASS: `rg "stdin_text|secret_text|stdin_bytes_value|raw_prompt" crates/daemon/src crates/daemon/tests crates/mcp/src crates/mcp/tests` — NO STDIN-LEAK PATHS (audit evidence)

Evidence — explicit acceptance confirmations:

- **PTY command starts through daemon/MCP.** Both `crates/daemon/tests/pty_ipc.rs::pty_command_start_then_stop_returns_metrics` and `crates/mcp/tests/pty_tools_live_e2e.rs::pty_command_full_lifecycle_through_mcp` spawn a `python3` PTY child end-to-end and verify the bounded response shape (job_id + bucket_id + probe_id + cursor) plus the post-stop counters.
- **PTY output creates bucket signal events.** The probe feeds normalized lines through `SifterRuntime::evaluate`; matching drafts land via `WatchEventSink → Router::bucket_append`. `pty_command_start_then_stop_returns_metrics` asserts `frames_total >= 2`.
- **Non-secret stdin works.** `pty_command_full_lifecycle_through_mcp` writes `"hello\n"` over `pty_command_write_stdin` and asserts `stdin_bytes_written > 0` on stop.
- **Secret-prompt stdin is denied and audited safely.** `pty_write_stdin_denied_during_secret_prompt` asserts `IpcErrorCode::SecretInputDenied` and inspects the persisted audit row: decision = `deny`, metadata contains `"prompt_kind":"secret"`, metadata does NOT contain the test password string `"should-not-be-sent"`. `pty_secret_prompt_denies_stdin_through_mcp` proves the same path surfaces through the MCP tool layer.
- **Shell PTY attempt is denied and audited.** `pty_command_rejects_shell_interpreter` (daemon) and `pty_shell_interpreter_denied_through_mcp` (MCP) prove the `SHELL_INTERPRETERS_DENY` list rejects `bash` before any spawn happens; the audit row is `pty_command_start` with decision = `deny` and reason naming the interpreter.
- **No raw PTY output appears in MCP responses.** Every PTY response DTO carries only ids + bounded counters + boolean flags. The PTY probe never exposes its byte buffer to the IPC layer; events reach the LLM only through the existing structured `EventDraft` lane, after the sifter has matched.
- **Existing command, bucket, registry, file-watch tests still pass.** Full nextest run is 334/334; TC41 / TC42 / TC42b / TC42c / TC42d / TC43 suites are untouched.
- **No `crates/daemon/src/command.rs` edits.** `git diff HEAD~2..HEAD -- crates/daemon/src/command.rs` returns nothing in this work commit. PTY lifecycle is the new `pty_command.rs` module; the `SHELL_INTERPRETERS_DENY` constant is imported, not re-declared.
- **No raw file/log/tail stream endpoint.** Confirmed by both grep guards above.

Source-status:
- `pty-process 0.5.3` dependency: **live (TC44)** under cfg(unix). Pinned via `=0.5.3`; license MIT.
- `PtyProbe` + `AnsiNormalizer::peek_pending` + `WriteStdinError::SecretInputActive`: **live (TC44)**.
- `PtyRuntime` + `pty_command_*` IPC methods + MCP tools: **live (TC44)**.
- `IpcErrorCode::SecretInputDenied`: **live (TC44)**.
- Existing `SHELL_INTERPRETERS_DENY` reuse, `PolicyAction::CommandStart` gate: **unchanged**.
- `command.rs` (TC38 process probe runtime): **unchanged**.
- Windows-native ConPTY: **deferred** per `non_goals`. The PTY runtime is cfg(unix) only; on non-Unix builds the `state.pty` field is absent and the daemon would surface `IpcErrorCode::UnsupportedPlatform` (already a closed-set variant). WSL2 is the supported Windows story.

Commits:
- Goal file prep amendment: `f097d51`
- Verified work commit: `4cc8dcc`
- Goal status commit: this commit

Known gaps / blockers:
- None.

Next goal:
- TC45-parallel-probe-router-and-multi-bucket-bindings.md — do NOT start until this TC44 report is reviewed.
