# Implementation Plan: Dogfood Remediation Batch

**Branch**: `002-dogfood-remediation` | **Date**: 2026-07-02 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/002-dogfood-remediation/spec.md`

## Summary

Nine independently shippable remediation stories from the 2026-07-02 dogfood
rounds, all additive: facade-wide strict parameter validation (all missing
fields at once + unknown-field rejection, validated against the advertised
schemars schema), registry pack idempotency and bulk deactivate, a
policy-gated directory listing on the files facade, compact projection on
`wait`/`events` plus liveness-delta on `sub_pull`, `event_context` by
`event_id` alone, `pty_stdin` bounded wait, file append mode, npm/TS
suggestion heuristics, the WSL nested-shell gate (fail-closed under
`allow_shell=false`), and an optional Windows pipe-instance pool. No
wire-protocol breaking change anywhere: every new field is
`#[serde(default)]`-optional, every new operation is a new IPC variant, and
existing well-formed calls stay byte-identical. All design decisions with
code anchors are in [research.md](./research.md) (D1-D11).

## Technical Context

**Language/Version**: Rust, MSRV 1.92.0 (workspace `rust-version`; lowering
requires an explicit goal per constitution)

**Primary Dependencies**: tokio (async runtime, named-pipe/UDS servers),
rmcp (stdio MCP), serde/serde_json + schemars (facade schemas), rusqlite +
refinery (WAL SQLite), regex (bounded compile path)

**Storage**: SQLite via `crates/store` (migrations V0001-V0007). This
feature adds NO migration: pack membership resolves from embedded seed-pack
JSON; liveness-delta state is in-memory per subscription

**Testing**: cargo nextest (workspace), plus `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets -- -D warnings` on BOTH Windows and
Linux (WSL) — the Linux gate is run pre-push in WSL with
`CARGO_TARGET_DIR=$HOME/tc-linux-target`

**Target Platform**: Windows 11 (named pipe) + Linux/WSL (UDS); daemon +
stdio MCP adapter, local-only

**Project Type**: Two-process Rust workspace — thin MCP adapter
(`crates/mcp`) forwarding 1:1 over local IPC to the daemon
(`crates/daemon`), with `crates/ipc` (wire), `crates/core` (domain types),
`crates/store` (persistence), `crates/sifters` (pure heuristics),
`crates/probes` (I/O probes)

**Performance Goals**: SC-004 — compact `wait` + delta `sub_pull` cost >=60%
fewer response tokens than full records + full liveness on the findings-doc
repro shapes; no regression to the pull fast path or the pipe accept path

**Constraints**: additive-only wire evolution (`#[serde(default,
skip_serializing_if = ...)]` posture); bounded responses everywhere (entry
caps + truthful truncation flags); policy-before-spawn and default-deny
preserved; adapter spawns nothing

**Scale/Scope**: 6 crates touched, ~9 user stories, ~17 files of production
code + tests; each story independently implementable and testable

## Constitution Check

*GATE: evaluated against Constitution v1.0.0 before Phase 0; re-evaluated
after Phase 1 design (below).*

| # | Principle | Verdict | Evidence |
|---|-----------|---------|----------|
| I | Two-process boundary | PASS | All new capability (listing, append, bulk deactivate, settle-wait) lives daemon-side behind new/extended IPC; adapter changes are validation + projection only. The `mcp_crate_contains_no_command_spawn` grep-test (`crates/daemon/tests/security.rs:201`) stays green. |
| II | Policy-before-spawn, default-deny | PASS (strengthened) | US8 CLOSES an argv-smuggling gap (`wsl.exe -e bash`) under the existing `allow_shell` cap, fail-closed on unknown constructions. Directory listing and append reuse the existing FileRead/FileWrite policy actions — no new cap, nothing defaults open. `SHELL_INTERPRETERS_DENY` remains intact and gains carrier-aware enforcement. |
| III | Combed, bounded output | PASS | Listing is capped (`MAX_FILE_LIST_ENTRIES`) with truthful `truncated` + `total_entries`; pty_stdin wait returns combed signals through the existing settle machinery; compact/delta REDUCE tokens without hiding data (full records stay cursor-fetchable). |
| IV | Local-only privilege boundary | PASS | US9 changes only how many pipe instances are pending locally; SDDL, peer-identity recording, and no-TCP posture unchanged. |
| V | Audit every gated action | PASS | Listing audited at dispatch (`ipc_file_list_dir`); append rides the existing `file_write` domain audit; bulk deactivate audits per existing deactivate pattern; nested-shell classification recorded in audit `metadata_json` + reason on allow, `command_rejected` on deny. |
| VI | No-mock, verification gate | PASS | SC-008 mandates a red->green test per FR; gate commands run on both platforms; all tests drive real daemon round-trips via the established harnesses. |
| VII | Honest degradation, suggest-never-auto-activate | PASS | Suggest handler remains stateless (structurally cannot persist/activate); partial bulk-deactivate success is explicit per-rule; truncation is flagged, never silent; degraded-path payloads untouched. |

**Post-design re-check (after Phase 1)**: PASS — no design artifact
introduces a violation. The tool COUNT is unchanged (facade actions are not
tools), so the catalogue count anchors hold; the `system_discover` fixture
and the MCP contract fixtures must be updated in the same change as the
schema changes (tracked in contracts/mcp-facade.md).

**Violations requiring justification**: none.

## Project Structure

### Documentation (this feature)

```text
specs/002-dogfood-remediation/
├── spec.md              # Remediation contract (9 user stories, FR-001..070)
├── plan.md              # This file
├── research.md          # Phase 0: decisions D1-D11 with code anchors
├── data-model.md        # Phase 1: entities + wire-type deltas
├── quickstart.md        # Phase 1: validation guide (gate + per-story proof)
├── contracts/
│   ├── mcp-facade.md    # Facade action/param/error contract deltas
│   ├── ipc-wire.md      # IPC request/response deltas + serde posture
│   └── policy-wsl.md    # WSL nested-shell classification contract
├── checklists/
│   └── requirements.md  # Spec quality checklist (all pass)
└── tasks.md             # Phase 2 output (/speckit-tasks — not yet created)
```

### Source Code (repository root)

```text
crates/
├── mcp/                       # US1 (validator), US4 (compact), parts of US2/3/5
│   ├── src/facades.rs         # 5 facade action enums (+ files List arm)
│   ├── src/tools.rs           # param structs, handlers, dispatch, projection
│   └── tests/                 # e2e + contract-fixture tests
│       └── fixtures/contracts/mcp-tools/   # *.v1.json fixtures to update
├── ipc/
│   └── src/protocol.rs        # new FileListDir + RegistryDeactivateBulk,
│                              # optional fields on Pull/EventContext/PtyStdin/
│                              # FileWrite params, new caps constants
├── daemon/
│   ├── src/command.rs         # US8 classifier + argv-lane guard
│   ├── src/pty_command.rs     # US8 second call site; US5 settle-wait
│   ├── src/policy.rs          # (read-only: caps accessor; no changes expected)
│   ├── src/subscriptions/     # US4 delta: model.rs (+last_liveness), pull.rs
│   ├── src/ipc/handlers/
│   │   ├── file.rs            # US3 handle_file_list_dir; US6 append
│   │   ├── registry.rs        # US2 bulk deactivate handler
│   │   ├── bucket.rs          # US5 event_context by id
│   │   └── subscription.rs    # US4 delta wiring + seek baseline reset
│   ├── src/ipc/pipe_server.rs # US9 (optional) pending-instance pool
│   └── tests/                 # integration tests (see quickstart.md)
├── store/
│   └── src/import.rs          # US2 idempotent import loop
├── sifters/
│   └── src/suggest.rs         # US7 heuristics table (+2), stream -> Option
└── core/                      # (read-only: RuleDefinition PartialEq, ids)

docs/security/POLICY.md        # US8 FR-061 stance documentation
```

**Structure Decision**: existing workspace layout; no new crates, no new
top-level directories. Every story maps onto existing seams listed above —
the full per-story file/symbol targets are in research.md D1-D11 and the
contracts/ directory.

## Implementation phasing (input to /speckit-tasks)

Stories are independent by design; the only soft ordering constraints:

1. **US1 (validator)** derives required/allowed fields from the schemars
   schema at runtime, so it self-updates as later stories add fields — it
   can land first (P1) without ordering hazards.
2. **Wire-first within each story**: protocol.rs types -> daemon handler ->
   MCP param/handler -> contract fixtures, verifying at each step.
3. **US9 ships last** (optional; must not destabilize the accept path all
   other stories' tests ride on). Skipping with written rationale is
   compliant.
4. Suggested batches respecting the 5-file phase discipline:
   P1 = US1, US2, US3; P2 = US4, US5, US8, US6 (security before
   convenience); P3 = US7, US9.

## Complexity Tracking

No constitution violations to justify — table intentionally empty.
