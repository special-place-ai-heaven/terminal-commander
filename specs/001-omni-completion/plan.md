# Implementation Plan: Omni Completion Program

**Branch**: `001-omni-completion` (spec dir; per-slice review branches created at implement time) | **Date**: 2026-06-16 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-omni-completion/spec.md`

## Summary

Close the remaining gap between TC v0.1.49 (39 MCP tools; shell_exec, trust
campaign, supervisor self-heal landed) and a 100% self-reliant omni LLM terminal
tool. Delivered as six independently-shippable priority slices (P1 sessions ->
P2 parse -> P3 platform -> P4 privileged[plan-only] -> P5 remote -> P6 certify),
each expanding capability through a policy-gated seam without weakening any
existing guard. The five open field-ledger fixes (TC-B1/B3/E1/E2/E4) fold into
the earliest slice touching their code path. Each completed slice commits to its
own review branch and pauses before merge/push.

## Technical Context

**Language/Version**: Rust (workspace; MSRV `rust-version = 1.92.0`); Node >=18
for the npm distribution wrapper.

**Primary Dependencies**: rmcp 1.7.x (stdio MCP); rusqlite + refinery (WAL
SQLite); `pty-process =0.5.3` (POSIX PTY); existing sifter/bucket/probe crates.
New (per slice): `portable-pty` (P3 Windows ConPTY), `notify` (P3 file backend),
`vte` already locked for ANSI test corpus and reusable for TC-B1 stripping.

**Storage**: SQLite under the daemon data dir (audit, registry, event store).
New: a small persisted receipt store for TC-B3.

**Testing**: `cargo nextest run --workspace` (unit+integration), `cargo test
--doc`, profiles `security`/`load`/`e2e`; contract fixtures under
`tests/fixtures/contracts/mcp-tools/`; live e2e under `crates/mcp/tests/*_live_e2e.rs`.

**Target Platform**: Linux + WSL2 (tier-1 today); macOS and native Windows
promoted to tier-1 in P3. Local-only IPC: UDS (Unix) / named pipe (Windows).

**Project Type**: Two-process daemon + thin stdio MCP adapter (Rust workspace)
with a Node distribution wrapper. Not web/mobile.

**Performance Goals**: Bounded structured output (never raw stream); advertised
wait/byte caps honored to the wire (SC-011); compact mode ~5x non-payload byte
reduction (SC-008); event-driven file signals replace ~120ms polling on native FS
(P3).

**Constraints**: All execution flows MCP adapter -> local IPC -> daemon; adapter
never spawns. Default-deny capability caps. No public TCP listener. ASCII-only
docs. Tool-count anchors + discovery fixture updated atomically per new tool.

**Scale/Scope**: ~46-53 final MCP tools (from 39). 25+ rule packs (from 8). 6
slices; P4 is plan-only this run (threat review gated).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

Evaluated against `.specify/memory/constitution.md` v1.0.0:

| Principle | Slice interaction | Verdict |
|---|---|---|
| I. Two-Process Boundary (NN) | All new tools live in the adapter as thin forwarders; every side effect (sessions, suggest, privileged, remote) runs in the daemon. Adapter no-spawn grep-test extended, never relaxed. | PASS |
| II. Policy-Before-Spawn / Default-Deny / Opt-In (NN) | New caps `allow_session`, `allow_privileged`, `allow_remote` default false (scaffold already on `PolicyCaps`). New `PolicyAction` variants gated. No generic sudo; privileged is a closed allow-list helper. Shell-line never on argv path. | PASS |
| III. Combed, Bounded Output (NN) | Sessions, shell, privileged, and remote output all flow through the sifter; compact mode (TC-E1) still bounded; tails stay capped. | PASS |
| IV. Local-Only Privilege Boundary | Remote (P5) is SSH local-forward to a remote socket; no public TCP. Peer identity preserved per transport. | PASS |
| V. Audit Every Gated Action | Session start, privileged op, remote target use audited with redacted subject (reuse the TC49 two-layer shell redactor). | PASS |
| VI. No-Mock + Verification Gate (NN) | Every slice carries source-status labels and passes fmt+clippy(-D)+nextest; MCP tools get through-daemon integration tests. | PASS |
| VII. Honest Degradation / Suggest-Never-Auto-Activate | TC-E2 honors wait caps; TC-B3 makes restart polls honest; `registry_suggest_*` returns proposals only, never activates (FR-008). | PASS |

No violations. Complexity Tracking table below is empty (no justified
deviations). The NON-NEGOTIABLE principles (I, II, III, VI) are upheld by
construction in every slice.

## Project Structure

### Documentation (this feature)

```text
specs/001-omni-completion/
├── plan.md              # This file
├── research.md          # Phase 0 decisions (per-slice technical choices)
├── data-model.md        # Phase 1 entities
├── quickstart.md        # Phase 1 validation guide (O-gate smokes)
├── contracts/           # Phase 1 new-tool contract overview
│   └── mcp-tools.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

The repository is an existing Rust workspace; this program adds modules and
tools to existing crates and adds at most one new crate (the privileged helper,
plan-only this run). Key touch points per slice:

```text
crates/
├── daemon/src/
│   ├── shell.rs              # P1: shell lane (landed); reuse for sessions
│   ├── shell_session.rs      # P1 NEW: ShellSessionRuntime over PTY
│   ├── pty_command.rs        # P1 reuse; P3 Windows path; P3 grace ladder
│   ├── command.rs            # TC-B1 ANSI strip seam; P3 process-wrap cancel
│   ├── policy.rs             # new caps + PolicyAction variants (session/privileged/remote)
│   ├── config.rs             # [shell_session], [privileged_helper], targets config
│   ├── file_watch.rs         # P3 notify backend wiring
│   ├── ipc/handlers/         # new request handlers (session, suggest, privileged, target)
│   └── ipc/server.rs         # dispatch + self_check; TC-B3 receipt store
├── probes/src/
│   ├── process.rs            # TC-B1 strip-before-sift; P3 process-group cancel
│   └── file.rs               # P3 notify/inotify backend (keep poll fallback)
├── sifters/src/              # P2 universal extractors; TC-E1 compact projection; TC-E4 capture canonicalization
├── store/
│   ├── rules/*.json          # P2 new packs (docker, kubectl, git, ...)
│   └── src/import.rs         # P2 pack registration
├── ipc/src/protocol.rs       # new IpcRequest/Response variants; compact/wait_until flags; receipt types
├── mcp/src/tools.rs          # new tool surfaces (session/suggest/target); compact+wait_until params; tool-count anchors
├── mcp/src/main.rs           # tool-count header anchor
└── privileged/               # P4 NEW crate (plan-only this run)

tests/fixtures/contracts/mcp-tools/   # one contract fixture per new tool
scripts/smoke/                         # P6 verify-omni-{linux,wsl,windows,macos}
docs/                                  # SHELL_RUNTIME/SESSION/OMNI_PLAYBOOK; README/SPEC/ROADMAP realign
```

**Structure Decision**: Extend existing crates in place; introduce
`shell_session.rs` (P1) and the `privileged` crate (P4, plan-only). No
architectural reshaping -- every addition is a policy-gated seam consistent with
the fixed `LLM -> stdio MCP -> adapter -> local IPC -> daemon` topology.

## Phasing and review-branch policy

Slices implement in priority order. After each slice:
1. Run the verification gate (`cargo fmt --all --check && cargo clippy
   --workspace --all-targets -- -D warnings && cargo nextest run --workspace`)
   plus the slice's targeted profile (security for P1 policy, e2e for tool
   surfaces).
2. Commit to a per-slice review branch `feature/omni-<slice>` and PAUSE before
   any merge or push (operator approval required).
3. Each slice updates tool-count anchors + discovery fixture in the same change.

Per CLAUDE.md phased rule, no single change set exceeds the bounded file set;
multi-file tool additions (which legitimately touch the atomic count-anchor set)
are the documented exception and are done as one cohesive commit.

## Complexity Tracking

> No constitution violations requiring justification. Table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none) | | |
