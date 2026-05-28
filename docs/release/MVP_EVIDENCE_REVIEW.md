# MVP evidence review

Reconciled: 2026-05-28 · Issue: [ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69) · Parent audit: [ROB-1](mention://issue/56940df9-7e91-44ca-9670-9e511328ebcd)

## Canonical sources

| Source | Role |
|--------|------|
| `.agent/goals/terminal-commander-runtime/TC35–TC48` | Active implementation tracker |
| `EVIDENCE_REPORT_RUNTIME.md` | Shipped runtime evidence on `main` |
| Root `RISK_REGISTER.md` | Open Beta risks (R-01–R-06) |
| `BACKLOG.md` | Beta GA follow-ups (P1.x) |
| `goals/TC01–TC32` | **Historical** MVP chain (frozen after this reconciliation) |

## Release gates (do not conflate)

| Gate | Closes when | Evidence | Explicitly out of scope |
|------|-------------|----------|-------------------------|
| **[ROB-1](mention://issue/56940df9-7e91-44ca-9670-9e511328ebcd) correctness audit** | P0 children [ROB-3](mention://issue/3778428f-7651-4802-ab86-ae52ed5e581a) + [ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69) shipped **or** each has a written plan with owner + ETA; Validator sign-off | This document; frozen `goals/` chain; runtime TC35–48 canonical | Provider live smokes; npm first publish; product code changes |
| **Beta GA** | Operator completes `RELEASE_CHECKLIST.md` including BACKLOG **P1.2–P1.4** provider harness smokes on host | `EVIDENCE_REPORT_RUNTIME.md`; root `RISK_REGISTER.md` R-01 | Goal frontmatter reconciliation |

[ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69) closes when S1–S4 land (docs-only). It does **not** alone close [ROB-1](mention://issue/56940df9-7e91-44ca-9670-9e511328ebcd) while [ROB-3](mention://issue/3778428f-7651-4802-ab86-ae52ed5e581a) is open.

### TC32 vs TC48

MVP-chain **TC32** asked for MVP-chain evidence consolidation; this file is the [ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69) deliverable (`Live` below). Runtime **TC48** (`.agent/goals/terminal-commander-runtime/TC48-beta-gate-evidence-review-and-backlog-rerank.md`) owns beta-gate evidence review and backlog authority on `main`. For beta posture, treat **TC48 + `EVIDENCE_REPORT_RUNTIME.md`** as canonical; link `RELEASE_CHECKLIST.md` and `BACKLOG.md` for remaining Beta blockers.

## Policy callouts

1. **TC11:** `Partial` until [ROB-3](mention://issue/3778428f-7651-4802-ab86-ae52ed5e581a) ships — `Dedupe` exists in `crates/sifters/src/noise.rs` (unit tests) but is not wired through the daemon probe runtime; BACKLOG P1.1 (`frames_suppressed`) related.
2. **TC30:** `Partial` — canonical evidence is **stdio MCP + real daemon** tests (`crates/mcp/tests/mcp_live_*.rs`, `file_tools_live_e2e.rs`, `pty_tools_live_e2e.rs`, `registry_live_*.rs`, `runtime_state_live_e2e.rs`, `scripts/smoke/verify-runtime-smoke.sh`). In-process `crates/mcp/tests/e2e.rs` is **fast-feedback only**, not TC30 proof. Missing: `scripts/demo/run-all.{sh,ps1}` (P1 backlog).
3. **TC27:** `Partial` — provider config examples ship; Codex/Claude/Cursor **live** smokes are Not Run → BACKLOG P1.2–P1.4 (Beta GA only).
4. **WSL pair (Out-of-Scope):** `packages/terminal-commander/lib/cli/pair_accept.js:75-82` — `runPairAccept` returns `PAIR_DEFERRED` when no `pair.json` exists (WSL-side daemon session token exchange not in MVP scope). Re-classify as `Deferred` if/when a security-paired Windows↔WSL feature enters scope. See BACKLOG WWS-B6.

## MVP goal matrix

| Goal | Status | Evidence (anchor) | Notes |
|------|--------|-------------------|-------|
| <a id="tc01"></a>TC01 | Deferred | `goals/SOURCE_MAP.md` | Planning artifact; research baseline in repo docs. |
| <a id="tc02"></a>TC02 | Live | `POLICY.md` · `docs/security/PRIVILEGE_MODEL.md` | Doctrine + enforcement path via TC35/TC38. |
| <a id="tc03"></a>TC03 | Live | `TESTING.md` · `.agent/goals/terminal-commander-runtime/TC42d-explicit-scope-and-nextest-gate.md` | Fixtures + nextest release gate. |
| <a id="tc04"></a>TC04 | Live | `Cargo.toml` · `EVIDENCE_REPORT_RUNTIME.md` | Workspace on `main`; chain summary gates. |
| <a id="tc05"></a>TC05 | Live | `docs/contracts/README.md` | Golden fixtures + IPC envelopes. |
| <a id="tc06"></a>TC06 | Live | `crates/core/src/lib.rs` · `EVIDENCE_REPORT_RUNTIME.md` §TC38 | Event model in production path. |
| <a id="tc07"></a>TC07 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC39 · §TC47 | Bucket APIs + load/clamp tests. |
| <a id="tc08"></a>TC08 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC39 | `event_context` bounded windows. |
| <a id="tc09"></a>TC09 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC42 | Registry validation + scoped binding. |
| <a id="tc10"></a>TC10 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC38 | Sifter on combed command path. |
| <a id="tc11"></a>TC11 | Partial | `crates/sifters/src/noise.rs:Dedupe` | Wire-through: [ROB-3](mention://issue/3778428f-7651-4802-ab86-ae52ed5e581a); observability: BACKLOG P1.1. |
| <a id="tc12"></a>TC12 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC35 · §TC39 | SQLite persistence + cursors. |
| <a id="tc13"></a>TC13 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC42 | Hot activation path. |
| <a id="tc14"></a>TC14 | Live | `docs/rules/README.md` · `EVIDENCE_REPORT_RUNTIME.md` §TC42 | Seed packs + import. |
| <a id="tc15"></a>TC15 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC38 | `command_start_combed` pipeline. |
| <a id="tc16"></a>TC16 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC38 | `command_status`, lifecycle events. |
| <a id="tc17"></a>TC17 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC39 · §TC47 | `bucket_wait` + heartbeat stress. |
| <a id="tc18"></a>TC18 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC43 | file_read/search/watch tools. |
| <a id="tc19"></a>TC19 | Superseded-by-TC44 | `.agent/goals/terminal-commander-runtime/TC44-posix-pty-spawn-and-stdin-control.md` | PTY + secret boundary on runtime chain. |
| <a id="tc20"></a>TC20 | Partial | `crates/probes/src/directory.rs` | Library only; no daemon IPC/MCP surface (TC45). |
| <a id="tc21"></a>TC21 | Superseded-by-TC37 | `.agent/goals/terminal-commander-runtime/TC37-daemon-uds-ipc-and-peer-identity.md` | UDS IPC replaces in-process router. |
| <a id="tc22"></a>TC22 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC35 · §TC38 | Persistent audit + command policy. |
| <a id="tc23"></a>TC23 | Superseded-by-TC40 | `.agent/goals/terminal-commander-runtime/TC40-rmcp-stdio-adapter-and-tool-discovery.md` | rmcp stdio MCP + UDS forwarding. |
| <a id="tc24"></a>TC24 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC41 · §TC43 | Full tool surface via daemon. |
| <a id="tc25"></a>TC25 | Live | `crates/cli/src/main.rs:run_doctor` | doctor/status subcommands. |
| <a id="tc26"></a>TC26 | Live | `docs/install/README.md` | Install/WSL docs; pair handshake Out-of-Scope (see policy). |
| <a id="tc27"></a>TC27 | Partial | `docs/integrations/codex-cli.md` · `BACKLOG.md` P1.2 | Provider **live** smokes Not Run → Beta GA. |
| <a id="tc28"></a>TC28 | Superseded-by-TC47 | `.agent/goals/terminal-commander-runtime/TC47-load-noise-and-backpressure-gate.md` | Eight stress tests = load gate. |
| <a id="tc29"></a>TC29 | Live | `EVIDENCE_REPORT_RUNTIME.md` §TC47 · §TC46 | Hardening + runtime regressions. |
| <a id="tc30"></a>TC30 | Partial | `crates/mcp/tests/mcp_live_command_e2e.rs` · `scripts/smoke/verify-runtime-smoke.sh` | `e2e.rs` = fast feedback only; `run-all` = P1. |
| <a id="tc31"></a>TC31 | Superseded-by-TC48 | `RELEASE_CHECKLIST.md` | Beta packaging meta → TC48/NPM chain. |
| <a id="tc32"></a>TC32 | Live | `docs/release/MVP_EVIDENCE_REVIEW.md` | This reconciliation deliverable. |
| WWS06 pair | Out-of-Scope | `packages/terminal-commander/lib/cli/pair_accept.js:75-82` | WSL-side daemon session token exchange not in MVP scope. Refuse-path returns `PAIR_DEFERRED` with explicit operator output. Re-classify as `Deferred` if/when a security-paired Windows↔WSL feature enters scope. |

MVP verified commits: `.agent/goals/terminal-commander-mvp/EVIDENCE_REPORT.md` §1 per row where applicable.

## Reconciled status vocabulary

| Value | Meaning |
|-------|---------|
| `Live` | MVP acceptance met on `main` |
| `Partial` | Built; named gap documented |
| `Superseded-by-TCNN` | Tracking moved to runtime chain TC35–48 |
| `Deferred` | Planning-only; no product feature expected |
| `Out-of-Scope` | Intentionally not implemented; safe refuse |

**Do not use `Pending` as reconciled status** on TC01–TC32 after [ROB-4](mention://issue/1d99ebb1-c568-48dd-85e5-a0f70e0dfe69).

### Status enum coverage (audit trail)

Frontmatter on `goals/TC01–TC32` uses four values only: `Live`, `Partial`, `Deferred`, `Superseded-by-TCNN` (concrete targets TC37/TC40/TC44/TC47/TC48). **`Out-of-Scope` appears only on the WWS06 matrix row** (not a numbered TC goal file) because WSL `pair_accept` is a cross-cutting install concern, not an MVP-chain deliverable. If a future audit adds more out-of-scope rows, extend the matrix here — do not add `Out-of-Scope` to TC frontmatter unless a TC file itself is reclassified.
