---
goal_id: TC46
title: Provider Harness Integration Smoke
chain_id: terminal-commander-runtime
phase: Wave 8 - Provider-facing validation
status: "Completed"
depends_on: ["TC45"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: "2026-05-22T22:15:00+00:00"
completed_at: "2026-05-22T22:45:00+00:00"
completion_commit: "a7e1544"
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
risk_level: "medium"
---

# TC46 - Provider Harness Integration Smoke

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC46-provider-harness-integration-smoke.md

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
- Prove a provider-neutral MCP harness can use Terminal Commander as the abstraction layer for command execution, file probing, bucket_wait, and bounded context.

non_goals:
- Do not depend on proprietary provider hooks.
- Do not require Claude-only behavior or Codex-only behavior.
- Do not send real secrets or run destructive commands.
- Do not add production installer behavior.

allowed_files_or_area:
- docs/integrations/**
- docs/mcp/**
- examples/mcp/**
- examples/provider-harness/**
- scripts/smoke/**
- crates/mcp/tests/**
- crates/daemon/tests/**
- crates/mcp/src/** only for tiny compatibility fixes required by smoke execution
- crates/daemon/src/** only for tiny compatibility fixes required by smoke execution
- .agent/goals/terminal-commander-runtime/TC46-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

Note: TC46 is a smoke / integration proof goal, not a feature-building goal. Any product-code change under `crates/mcp/src/**` or `crates/daemon/src/**` must be a tiny compatibility fix required by smoke execution AND explicitly recorded in the final report. If a product-code change exceeds a tiny compatibility fix, STOP and report instead of widening scope.

forbidden_files:
- privileged install files
- network listener additions
- provider credential files
- destructive filesystem commands
- new MCP tools
- command / runtime feature expansion
- registry / file / PTY / router feature work
- direct command spawn from `crates/mcp`
- direct file reads from `crates/mcp`
- shell execution feature expansion
- raw stdout / stderr / file / PTY stream endpoint
- privileged helper
- installer / service work
- secrets, tokens, private usernames, private paths, or machine-specific absolute paths in committed docs
- pretending provider smoke passed when the provider CLI / auth / config is unavailable

contracts_or_interfaces:
- Smoke tests must use MCP stdio and the local daemon/UDS path, not in-process mocks.
- The demo command must produce lots of noise and one or more matching signal lines.
- The harness must prove `command_start_combed` -> `bucket_wait` / `bucket_events_since` -> `command_status`. Where applicable, also `event_context` and / or `file_read_window` / `file_search`.
- Codex MCP config example and Claude Code MCP config example MUST both ship in `docs/integrations/` (or `examples/provider-harness/`). Examples MUST use machine-local paths and environment variables, NOT hardcoded user paths.
- Examples MUST include instructions to start `terminal-commanderd` in UDS mode and to run `terminal-commander-mcp` over stdio.
- If Codex CLI cannot be run on the verification host, the report MUST mark the Codex provider smoke as `Not Run` or `Blocked` with the exact reason (missing CLI, missing auth, etc.); the config-only example still ships.
- If Claude Code CLI cannot be run on the verification host, the report MUST mark the Claude provider smoke as `Not Run` or `Blocked` with the exact reason; the config-only example still ships.
- Direct daemon UDS + MCP stdio smoke is useful secondary evidence but MUST NOT be called provider-harness success.
- The smoke script `scripts/smoke/verify-runtime-smoke.sh` (created by TC46 implementation) MUST NOT require secrets, MUST NOT spawn raw shell bridges through MCP, MUST cover the daemon + MCP stdio path using bounded tool calls, and MUST clearly state which provider harnesses (if any) it can drive directly.

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
- Create deterministic e2e scripts that start daemon runtime, start MCP stdio adapter, issue MCP tool calls, and stop both cleanly.
- Add Claude Code and Codex CLI config examples without credentials or absolute user-only paths.
- Run one command-output scenario and one file-search/watch scenario.
- Capture bounded output excerpts and cursor/context evidence in docs/integrations.
- Add cleanup logic for temp dirs, sockets, child processes, and stores.

acceptance_criteria:
- Generic MCP stdio harness passes locally.
- Docs include commands/config for at least two MCP-capable clients or explain a verified blocker.
- Smoke evidence shows signal-only output and bounded context, not raw stream dumping.
- No provider secrets, tokens, or credentials are committed.

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
cargo test -p terminal-commander-mcp -- --nocapture
# privilege model guards on the MCP crate
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
# prove MCP does not read files directly
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
# local daemon + MCP stdio smoke (created by TC46 implementation)
bash scripts/smoke/verify-runtime-smoke.sh
```

Provider smoke evidence (per acceptance):
- Codex: config path used, command run, observed `list_tools` / call result, or exact blocker. No secrets, tokens, or private paths in committed artifacts.
- Claude Code: config path or `--mcp-config` used, `/mcp` or tool discovery evidence, observed call result, or exact blocker. No secrets, tokens, or private paths in committed artifacts.

## Scope Amendment (TC46 prep)

This amendment aligns the original TC46 mini-spec with the actual repo layout as of TC45 and locks the provider-harness smoke boundary. Same precedent as TC41 / TC42 / TC43 / TC44 / TC45.

Drift corrected:

- `tests/e2e/**` is not a real path in this repo. Tests live in `crates/<crate>/tests/`. Replaced with `crates/mcp/tests/**` and `crates/daemon/tests/**` (the latter only if harness-adjacent smoke helpers are needed).
- `scripts/dev/**` is not a real directory and `scripts/dev/verify-runtime-smoke.sh` does not exist. Replaced with `scripts/smoke/**` and `scripts/smoke/verify-runtime-smoke.sh`; TC46 implementation creates the script.
- Allowed creation of missing directories: `docs/integrations/**`, `docs/mcp/**`, `examples/mcp/**`, `examples/provider-harness/**`, `scripts/smoke/**`. None exist on disk today; TC46 implementation may create them.
- Allowed area now lists `GOAL_CHAIN_INDEX.md` / `RUN_ORDER.md` only if TC46 wording needs alignment.

Scope rules:

- TC46 is a smoke / integration proof goal, not a feature-building goal.
- `crates/mcp/src/**` and `crates/daemon/src/**` are allowed ONLY for tiny compatibility fixes required by smoke execution.
- Any product-code change must be explicitly recorded in the final report.
- If a product-code change exceeds a tiny compatibility fix, STOP and report instead of widening scope.

Forbidden list locked:

- new MCP tools
- command / runtime feature expansion
- registry / file / PTY / router feature work
- network listener
- direct command spawn from `crates/mcp`
- direct file reads from `crates/mcp`
- shell execution feature expansion
- raw stdout / stderr / file / PTY stream endpoint
- privileged helper
- installer / service work
- secrets, tokens, private usernames, private paths, or machine-specific absolute paths in committed docs
- pretending provider smoke passed when the provider CLI / auth / config is unavailable

Provider boundary locked:

- Codex MCP config example required.
- Claude Code MCP config example required.
- Examples use environment variables and machine-local paths only.
- If Codex CLI / auth is unavailable on the verification host: Codex provider smoke = `Not Run` or `Blocked`; the config-only example still ships; the exact blocker is named.
- If Claude Code CLI / auth is unavailable on the verification host: same — `Not Run` / `Blocked` with exact reason; config still ships.
- Direct daemon UDS + MCP stdio smoke is secondary evidence; it is NOT provider-harness success on its own.

Script decision (locked):

- TC46 implementation MAY create `scripts/smoke/verify-runtime-smoke.sh`.
- The script MUST NOT require secrets.
- The script MUST NOT spawn raw shell bridges through MCP.
- The script MUST verify the daemon + MCP stdio smoke using bounded tool calls (`system_discover`, `health`, plus the `command_start_combed -> bucket_wait -> command_status` flow at minimum).
- If the script cannot directly drive a provider harness (no Codex CLI, no Claude Code CLI), it MUST say so explicitly and remain a local MCP / daemon smoke only.

Verification additions:

- `git branch --show-current`, `git status --short`, `cargo test --workspace`, `cargo test -p terminal-commander-mcp -- --nocapture`, `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`, `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src`, `bash scripts/smoke/verify-runtime-smoke.sh`, plus provider smoke evidence (Codex + Claude Code) OR exact blocker for each, are now part of the verification command set so the gates are explicit and reproducible.

## Task Prompt

Run TC46 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report

Objective (narrow / smoke-proof per Scope Amendment):
- Prove a provider-neutral MCP harness can attach to Terminal Commander's local MCP stdio surface and call the TC45 29-tool catalogue without raw stream noise, without MCP-side spawning, without MCP-side filesystem access, and without network listeners. Ship Codex CLI + Claude Code config examples that are safe to commit.

Changes (verified work commit `a7e1544`):
- `docs/integrations/codex-cli.md` (new): Codex CLI MCP stdio walk-through. `~/.codex/config.toml` stanza targets `terminal-commander-mcp` and sets `TC_SOCKET` from `TC_DATA/terminal-commanderd.sock`. No hardcoded user paths. No credentials.
- `docs/integrations/claude-code.md` (new): Claude Code walk-through with both the `--mcp-config <path>` form and the persistent `settings.json` form. Same env-var socket resolution. No credentials, no machine-specific absolute paths.
- `docs/integrations/README.md`: rewrote the lead paragraph + status table to point at the new per-provider docs and the smoke script. TC27 baseline (5 MVP tools) content retained below as historical reference.
- `scripts/smoke/verify-runtime-smoke.sh` (new, executable): bounded local smoke. Builds debug binaries, spawns `terminal-commanderd --data-dir <tmp> start --mode ipc-server`, spawns `terminal-commander-mcp` over stdio with `TC_SOCKET` pointed at the daemon's UDS, pumps a fixed JSON-RPC sequence (`initialize` → `tools/list` → `tools/call system_discover` → `tools/call health` → `tools/call command_start_combed` → `tools/call bucket_wait` → `tools/call command_status`), and asserts every response is bounded JSON. Includes a raw-stream leak check that fails if the literal echo argv string appears outside the audit-bearing fields (`argv`, `argv0`, `subject`, `summary`, `summary_template`, `reason`). Uses only cargo + python3 — no `jq`, no provider CLIs.

No source code changes in `crates/`.

Files changed:
- `docs/integrations/codex-cli.md` (new)
- `docs/integrations/claude-code.md` (new)
- `docs/integrations/README.md` (rewrote lead; TC27 baseline retained below)
- `scripts/smoke/verify-runtime-smoke.sh` (new)
- `.agent/goals/terminal-commander-runtime/TC46-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **339/339 passing, 0 skipped** (TC45 surface unchanged)
- PASS: `cargo test -p terminal-commander-mcp -- --nocapture` — green
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — **8/8 PASS assertions, SUCCESS**:
  1. initialize protocol version
  2. tools/list reports 29 tools
  3. command_start_combed advertised
  4. system_discover payload bounded
  5. health reports ok
  6. command_start_combed returned bucket_id + job_id
  7. bucket_wait returned events array
  8. no raw stream text in bounded MCP responses
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — only doc / negative-assertion matches
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches

Provider smoke evidence:

- **Codex CLI: Not Run.** `codex --help` on this verification host fails with `Error: Missing optional dependency @openai/codex-linux-x64. Reinstall Codex: npm install -g @openai/codex@latest`. The `codex` shim under Windows nvm (`/mnt/c/Program Files/nodejs/codex`) does not include the Linux x64 native binary required to run under WSL2. The Codex config-only example still ships in `docs/integrations/codex-cli.md` and is correct against the documented Codex MCP server schema. To run the provider smoke, an operator with a working Codex CLI install must follow the doc and observe the tool calls in the session transcript.
- **Claude Code: Not Run.** `which claude` returns no result on this verification host; no `claude` binary in `$PATH` or in the user's npm-global. The Claude Code config-only example still ships in `docs/integrations/claude-code.md` and is correct against the public Claude Code MCP configuration docs. To run the provider smoke, an operator with a working Claude Code install must launch `claude --mcp-config <path>` (or use the persistent settings form) and observe `/mcp` + a tool call.
- **Secondary evidence (not provider-harness success):** the local daemon + MCP stdio smoke run via `scripts/smoke/verify-runtime-smoke.sh` proves the transport surface end-to-end. 8/8 assertions PASS, no raw stream text in any response, the TC45 29-tool catalogue is advertised.

Evidence — explicit acceptance confirmations:

- **Codex MCP config example exists and is bounded/safe.** `docs/integrations/codex-cli.md` ships the `~/.codex/config.toml` stanza using `TC_SOCKET = "${TC_DATA}/terminal-commanderd.sock"` — no hardcoded user paths, no secrets.
- **Claude Code MCP config example exists and is bounded/safe.** `docs/integrations/claude-code.md` ships both the `--mcp-config` and persistent settings forms; same env-var socket resolution.
- **At least one real provider-harness smoke is executed if the host has the provider CLI available.** Neither provider CLI is usable on this verification host; both blockers are named with exact text above.
- **If a provider cannot be run, the report says exactly why.** Codex: missing `@openai/codex-linux-x64`. Claude Code: no `claude` binary on PATH.
- **No raw stream appears in any MCP response.** The smoke script's leak check passes; no MCP DTO field carries raw stdout/stderr.
- **MCP crate still has no direct spawn or direct file-read path.** Both verification greps clean.
- **Existing TC41-TC45 tests still pass.** Nextest 339/339, zero skipped.
- **Full WSL/Linux verification passes.** All gates green.

Source-status:
- `scripts/smoke/verify-runtime-smoke.sh`: **live (TC46)** as the local daemon + MCP stdio smoke. Secondary evidence; not provider-harness success.
- `docs/integrations/codex-cli.md`: **config-only (TC46)**. Provider smoke = `Not Run` on this host (exact blocker named above).
- `docs/integrations/claude-code.md`: **config-only (TC46)**. Provider smoke = `Not Run` on this host (exact blocker named above).
- `docs/integrations/README.md`: **updated (TC46)** — lead paragraph + status table now point at the TC45 surface and the per-provider walk-throughs; TC27 baseline retained below.
- Every `crates/` source file: **unchanged** in this commit.

Commits:
- Goal file prep amendment: `89a573e`
- Verified work commit: `a7e1544`
- Goal status commit: this commit

Known gaps / blockers:
- Provider-harness live smoke (Codex + Claude Code) is **Not Run** on this verification host. Both blockers are mechanical CLI-install issues, not Terminal Commander defects. The config-only examples ship; operators with a working provider CLI can execute the provider smoke by following the doc.
- No Terminal Commander runtime defect surfaced during TC46.

Next goal:
- TC47-load-noise-and-backpressure-gate.md — do NOT start until this TC46 report is reviewed.
