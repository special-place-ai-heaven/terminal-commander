---
goal_id: TC48
title: Beta Gate Evidence Review And Backlog Rerank
chain_id: terminal-commander-runtime
phase: Wave 9 - Beta readiness
status: "Pending"
depends_on: ["TC47"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "production", "release"]
worktree_hint: ""
created_at: "2026-05-21T18:55:35+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "GitHub main repository: https://github.com/special-place-administrator/terminal-commander"
  - "README.md on main: local MCP-operated terminal/file signal-combing layer; raw output in, vetted signal out; context by pointer"
  - "Uploaded BACKLOG.md: P0 blockers rmcp stdio adapter, PTY spawn, UDS IPC, persistent audit log writes"
  - "Uploaded EVIDENCE_REPORT.md: TC01a-TC32 evidence and crate/test inventory"
  - "Uploaded FINAL_REPORT.md: completed chain, scope substitutions, and open runtime gaps"
risk_level: "medium"
---

# TC48 - Beta Gate Evidence Review And Backlog Rerank

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-runtime/TC48-beta-gate-evidence-review-and-backlog-rerank.md

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
- Review the runtime chain evidence, correct source-status drift, rerank the backlog, and decide whether Terminal Commander is beta-ready as a realtime MCP signal abstraction layer.

non_goals:
- Do not implement new product features.
- Do not mark beta-ready if any core live path is mock/scaffold-only.
- Do not delete unresolved risk records.

allowed_files_or_area:
- BACKLOG.md (create if missing)
- EVIDENCE_REPORT_RUNTIME.md (create if missing)
- RISK_REGISTER.md (create if missing)
- RELEASE_CHECKLIST.md (update if needed)
- ROADMAP.md (update if needed)
- README.md only for source-status summary
- docs/audits/**
- docs/runtime/**
- docs/mcp/**
- docs/security/**
- docs/install/**
- docs/integrations/**
- docs/testing/**
- .agent/goals/terminal-commander-runtime/TC48-*.md
- .agent/goals/terminal-commander-runtime/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-runtime/RUN_ORDER.md

Note: TC48 is the beta gate / evidence review goal. NO product-code changes. NO new MCP tools or runtime capabilities. Three TC48 artifacts (`BACKLOG.md`, `EVIDENCE_REPORT_RUNTIME.md`, `RISK_REGISTER.md`) do not exist on disk today and are explicit TC48 deliverables.

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- scripts/**
- product-code changes anywhere
- new MCP tools
- new runtime capabilities
- command / file / PTY / registry / router feature work
- network listener
- direct command spawn from `crates/mcp`
- direct file reads from `crates/mcp`
- shell execution feature expansion
- raw stdout / stderr / file / PTY stream endpoint
- privileged helper
- installer / service implementation
- secrets, tokens, private usernames, private absolute paths, or machine-specific paths in committed artifacts
- promoting `Not Run` evidence to PASS
- hiding, downgrading, or deleting unresolved risks without traceability
- claiming beta readiness beyond the evidence

contracts_or_interfaces:
- The beta decision must classify the core flow: MCP client -> daemon -> command/file/probe -> sifter registry -> bucket_wait -> event_context.
- Every remaining P0 must be either completed, blocked, or explicitly declared beta-blocking.
- Evidence must include command, file, realtime wait, context, policy denial, audit, and load/noise results if live.

Review requirements (each goal must be confirmed live or honestly reported):
- TC35 persistent audit is live.
- TC37 UDS IPC is live, bounded, local-only, no network listener.
- TC38 command runtime is live, argv-only, shell-guarded.
- TC39 bucket / context APIs are live, bounded, heartbeat-aware, non-streaming.
- TC40 MCP stdio adapter is live.
- TC41 MCP command + bucket tools are live.
- TC42 / TC42b / TC42c / TC42d dynamic registry activation, scoped binding, live rebind, explicit-scope-required are live.
- TC43 file read/search/watch tools are live and bounded.
- TC44 PTY/stdin path is live with secret-prompt deny.
- TC45 `runtime_state` / `probe_list` / `probe_status` aggregate view is live.
- TC46 provider-harness status reported honestly (Codex + Claude Code provider smokes were `Not Run` on the verification host; local daemon + MCP stdio smoke is SECONDARY evidence only).
- TC47 load / noise / backpressure evidence reported honestly (8/8 stress tests pass; dedicated file-watch + PTY load tests are `Not Run` with exact reasons; `frames_suppressed` daemon-side counter does not exist).

Content rules:
- No secrets, tokens, private usernames, private absolute paths, or machine-specific paths in any committed artifact.
- `Not Run` evidence MUST NOT be promoted to PASS.
- TC46 provider blockers remain recorded verbatim: Codex CLI = `Not Run` (missing `@openai/codex-linux-x64`); Claude Code = `Not Run` (no `claude` binary on PATH). Local daemon + MCP stdio smoke = secondary evidence only.
- TC47 `Not Run` areas remain marked: dedicated file-watch load test, dedicated PTY load test.
- `frames_suppressed` explicit daemon-side counter MUST land in `BACKLOG.md` as P1 unless it already exists.
- Do NOT hide, downgrade, or delete unresolved risks without traceability.
- Do NOT claim beta readiness beyond the evidence.

Beta recommendation rule:
- The final report MUST choose one of:
  - `Go`
  - `Conditional Go`
  - `No-Go`
- The recommendation MUST be evidence-backed.
- If provider harnesses were not actually run, beta CANNOT be called fully provider-validated. Use `Conditional Go` if all local MCP/daemon gates pass but provider CLI execution remains `Not Run`.

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
- Read TC33-TC47 final reports and commits.
- Create EVIDENCE_REPORT_RUNTIME.md consolidating runtime chain proof.
- Update RELEASE_CHECKLIST.md and ROADMAP.md with beta gate status.
- Rerank BACKLOG.md P0/P1/P2/P3 based on actual runtime evidence.
- Update README source-status summary if and only if the live runtime status changed.

acceptance_criteria:
- EVIDENCE_REPORT_RUNTIME.md has per-goal commit hashes and verification summaries.
- Backlog P0 contains only true beta blockers.
- Release checklist says beta-ready, blocked, or partial with explicit reasons.
- No code is changed in this review goal.

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
# regression: TC47 load gate must still pass
cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture
# regression: TC46 local daemon + MCP stdio smoke must still pass
bash scripts/smoke/verify-runtime-smoke.sh
# privilege model guards on the MCP crate
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
# prove MCP does not read files directly
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Scope Amendment (TC48 prep)

This amendment tightens the TC48 beta gate contract. Same precedent as TC41 / TC42 / TC43 / TC44 / TC45 / TC46 / TC47.

Goal classification (locked):

- TC48 is the beta gate / evidence review goal.
- NO product-code changes. NO new MCP tools. NO new runtime capabilities.
- Pure documentation + evidence consolidation + backlog rerank.

Deliverables (each is an explicit TC48 artifact):

- `BACKLOG.md` — create if missing.
- `EVIDENCE_REPORT_RUNTIME.md` — create if missing.
- `RISK_REGISTER.md` — create if missing.
- `RELEASE_CHECKLIST.md` — update if needed (file exists today).
- `ROADMAP.md` — update if needed (file exists today).
- `README.md` — beta-status summary only if needed (existing surface-status section).
- `docs/audits/**`, `docs/runtime/**`, `docs/mcp/**`, `docs/security/**`, `docs/install/**`, `docs/integrations/**`, `docs/testing/**` — beta-gate evidence + status updates only.
- `.agent/goals/terminal-commander-runtime/{GOAL_CHAIN_INDEX,RUN_ORDER}.md` — final status sweep only if needed.

Forbidden list tightened:

- product-code changes anywhere
- new MCP tools
- new runtime capabilities
- command / file / PTY / registry / router feature work
- network listener
- direct command spawn from `crates/mcp`
- direct file reads from `crates/mcp`
- shell execution feature expansion
- raw stdout / stderr / file / PTY stream endpoint
- privileged helper
- installer / service implementation
- secrets, tokens, private usernames, private absolute paths, or machine-specific paths in committed artifacts
- promoting `Not Run` evidence to PASS
- hiding, downgrading, or deleting unresolved risks without traceability
- claiming beta readiness beyond the evidence

Review requirements (each must be confirmed live or honestly reported):

- TC35 persistent audit: live.
- TC37 UDS IPC: live, bounded, local-only, no network listener.
- TC38 command runtime: live, argv-only, shell-guarded.
- TC39 bucket / context APIs: live, bounded, heartbeat-aware, non-streaming.
- TC40 MCP stdio adapter: live.
- TC41 MCP command + bucket tools: live.
- TC42 / TC42b / TC42c / TC42d registry activation + scoped binding + live rebind + explicit-scope-required: live.
- TC43 file read/search/watch tools: live + bounded.
- TC44 PTY/stdin: live with secret-prompt deny.
- TC45 `runtime_state` / `probe_list` / `probe_status`: live.
- TC46 provider harness: report honestly — Codex CLI = `Not Run` (missing `@openai/codex-linux-x64`); Claude Code = `Not Run` (no `claude` on PATH); local smoke = secondary evidence only.
- TC47 load / noise / backpressure: 8/8 stress tests pass; dedicated file-watch + PTY load tests are `Not Run` with exact reasons; `frames_suppressed` counter does not exist (backlog P1).

Content rules:

- No secrets, tokens, private usernames, private absolute paths, or machine-specific paths in committed artifacts.
- `Not Run` evidence MUST NOT be promoted to PASS.
- TC46 provider blockers remain recorded verbatim.
- TC47 `Not Run` areas remain marked.
- `frames_suppressed` explicit daemon-side counter MUST land in `BACKLOG.md` as P1 unless it already exists.
- Unresolved risks MUST stay traceable; do not hide / downgrade / delete without record.

Beta recommendation rule (locked):

- Final report MUST choose exactly one of: `Go`, `Conditional Go`, `No-Go`.
- Recommendation MUST be evidence-backed.
- If provider harnesses were not actually executed end-to-end, beta CANNOT be called fully provider-validated. `Conditional Go` is the correct ceiling if all local MCP/daemon gates pass but provider CLI execution remains `Not Run`.

Verification additions:

- `git branch --show-current`, `git status --short`, `cargo test --workspace`, the TC47 regression (`cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture`), the TC46 regression (`bash scripts/smoke/verify-runtime-smoke.sh`), and the two MCP guard greps (`rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp`, `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src`) are now part of the verification command set so the beta gate regression posture is explicit.

## Task Prompt

Run TC48 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report Format

Objective:
- Review the runtime chain evidence, correct source-status drift, rerank the backlog, and decide whether Terminal Commander is beta-ready as a realtime MCP signal abstraction layer.

Changes:
- <focused list of implementation changes>

Files changed:
- <paths>

Verification:
- PASS/FAIL: `<command>` — <summary>

Evidence:
- <source-status notes, test output summaries, route/status evidence, screenshots only if rendered UI changed>

Commit:
- Verified work commit: `<hash or none>`
- Goal status commit: `<hash or none>`

Known gaps / blockers:
- <none or explicit blocker>

Next goal:
- none
