# B3 — Low nits (verified locations)

**Source:** `docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md` LOW section  
**Posture:** Fix in spare cycles; one “papercuts” PR acceptable.  
**Re-verified 2026-05-28:** all 9 locations + corrected paths confirmed against `main`; no content changes.

---

## Verification table

| Ledger ref | Actual path / lines | Present? | Notes |
|------------|---------------------|----------|-------|
| `daemon/server.rs:1444` | `crates/daemon/src/ipc/server.rs:1444-1456` | **Yes** | `serde_json::to_string(sid).unwrap_or_else(\|_\| "null".to_owned())` in audit metadata |
| `command.rs:602` | `crates/daemon/src/command.rs:601-642` | **Yes** | Receipt/metrics on exit path; race with stop noted in audit |
| `pty_command.rs:397` | `crates/daemon/src/pty_command.rs:396-410` | **Yes** | `stop` removes live binding; metrics read before cancel |
| `store/registry.rs:486` | `crates/store/src/registry.rs:486-493` | **Yes** | Unknown status → `Draft` silently |
| `store/lib.rs:427` | `crates/store/src/lib.rs:427` | **Yes** | `let _ = self.evict_expired(...).ok()` |
| `core/context.rs:340` | `crates/core/src/context.rs:340-341` | **Yes** | Tautological `truncated_before` first operand |
| `core/tests/load.rs:56` | `crates/core/tests/load.rs:56-65` | **Yes** | `elapsed < 5.0s` wall clock |
| `sifters/noise.rs:92` | `crates/sifters/src/noise.rs:92-94` | **Yes** | `OffsetDateTime::now_utc()` in `apply` vs injected `ts` in drafts |
| `bin/*.js` dead `bridge_required` | `terminal-commander.js:264`, `terminal-commanderd.js:34` | **Yes** | `resolveBinary` never returns `bridge_required` (`resolve-binary.js:113-117`) |
| `lib/daemon/autostart.js:243` | `packages/terminal-commander/lib/daemon/autostart.js:243` | **Yes** | `runAutostartOnce(homeDir)` return ignored |

**Ledger path correction:** `lib/daemon/` → `packages/terminal-commander/lib/daemon/`.

---

## Proposed fixes (sketch)

| Item | Fix | Effort |
|------|-----|--------|
| ipc/server audit null fallback | Optional: `tracing::debug!` on serialize fail; keep fallback | **S** |
| command/pty metrics race | Comment documenting race OR read metrics from JobManager after join | **M** |
| parse_status | Return `EventStoreError::InvalidPayload` for unknown status (match `parse_scope`) | **S** |
| evict_expired swallow | `tracing::warn!` once on `Err` | **S** |
| context truncated_before | Remove dead operand; simplify condition | **S** |
| load.rs timing | Drop wall-clock assert; keep count assertions | **S** |
| noise Dedupe clock | Pass `now` into `apply` from caller / test clock | **S** |
| bridge_required branches | Delete branches + update tests | **S** |
| autostart runOnce | Log `exit_code` when `!ok` | **S** |

---

## Non-goals

- No behavior change to IPC contracts.
- No refactor of command/pty lifecycle (B3 only documents or one-line logs unless product asks).
