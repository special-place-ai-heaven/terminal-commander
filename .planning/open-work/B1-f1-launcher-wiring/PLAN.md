# B1 — F1 launcher wiring (TC_SESSION mint + export)

**Ledger:** CRITICAL — only load-bearing open item  
**Package version:** 0.1.25 (`packages/terminal-commander/package.json:3`)  
**Spec:** `docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md`  
**Rust F1 plan (completed):** `docs/superpowers/plans/2026-05-27-per-harness-session-endpoint.md`

---

## Current state

### Shipped (dormant capability)

| Layer | What exists | Citation |
|-------|-------------|----------|
| Session resolution | `TC_SOCKET` > `TC_SESSION` > per-user default; malformed token soft-fails | `crates/supervisor/src/session.rs:45-61` |
| State dir isolation | `Session` token → `base.join(token)` | `crates/supervisor/src/paths.rs:57-63` |
| Windows pipe | Daemon `pipe_name()` routes through session resolver | `crates/daemon/src/config.rs:261-263` (doc); implementation in same module |
| Tests | Session + path invariants | `session.rs:81-124`, `paths.rs:278-307` |
| Audit | F1 marked SHIPPED in flakiness audit | `docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md:51` |

### Not wired (nothing sets `TC_SESSION` in production paths)

| Layer | Gap | Citation |
|-------|-----|----------|
| Harness MCP configs | Stanza emits only `TC_WSL_DISTRO` on Windows; **no `TC_SESSION`** | `packages/terminal-commander/lib/cursor/config.js:156-181`, `lib/harness/write_all.js:30-40` |
| Packages tree | **Zero** `TC_SESSION` references | SymForge `search_text` `path_prefix: packages/` |
| Linux autostart | Hardcoded legacy socket `…/terminal-commanderd.sock`; no session subdir | `packages/terminal-commander/lib/daemon/autostart.js:45-56` |
| systemd unit | No `Environment=TC_SESSION=…` | `autostart.js:65-81` |
| MCP cold start | Resolves `state_dir` from process env; `ensure_daemon` sets **`TC_SOCKET`** to resolved endpoint (inherits rest of env) | `crates/mcp/src/main.rs:105-135`, `ensure.rs:212-228` |

### Spec vs ledger scope tension

F1 design **non-goals** explicitly defer session lifecycle (minting, per-session pidfiles as a product feature, reaping idle daemons) to a future “session supervisor” milestone; launcher only sets `TC_SESSION` (`docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md:179-184`).

Open-work ledger still lists mint + pidfiles + reaping as **B1 goals**. This plan splits:

- **Phases 1–3 (required):** wire launcher/harness/autostart so `TC_SESSION` is set consistently (minimal mint).
- **Phase 4 (optional milestone):** supervisor lifecycle — only after explicit scope promotion (see adversarial review).

Pidfiles **already** live under resolved state dir when `TC_SESSION` is set (`spec:172-177`; `replace.rs` uses `opts.state_dir`).

---

## Target behavior / acceptance criteria

### AC1 — Harness isolation

Given two harness configs written on the same machine (e.g. Cursor global + Codex), each MCP server stanza includes a **distinct**, stable `env.TC_SESSION` token matching `[A-Za-z0-9._-]`, 1–64 chars, with at least one alphanumeric (`session.rs:36-40`).

### AC2 — Client/daemon agreement

With only `TC_SESSION` set (no manual `TC_SOCKET`), MCP adapter and daemon resolve the **same** endpoint and state dir (cross-side invariant from F1 plan Task 5).

### AC3 — Autostart alignment

`autostart.sh` / systemd `ExecStart` must not start a daemon on the **legacy** default socket when `TC_SESSION` is in the operator profile; socket check and spawn must use the same resolution rules as Rust (or export `TC_SESSION` before `terminal-commanderd start`).

### AC4 — Backward compatibility

With **no** `TC_SESSION` in env, endpoints and state dirs remain byte-identical to pre-F1 (`paths.rs:300-307`, F1 spec § backward compatibility).

### AC5 — Integration proof

Automated test(s): spawn MCP or CLI with harness-like `TC_SESSION`, assert daemon binds non-default path/pipe and `system_discover` succeeds.

### AC6 — Windows+WSL forwarding (HARD AC, verified 2026-05-28)

`TC_SESSION` set in the **Windows** MCP stanza must reach the **WSL-side** MCP
process. The bridge spawns `wsl.exe -d <distro> -- bash -lc 'exec
terminal-commander-mcp'` with `env: buildFilteredEnv(process.env)`
(`lib/wsl/spawn.js:296,153-157`), but `wsl.exe` does NOT translate a Windows env
block into the Linux process — only vars listed in **`WSLENV`** cross the boundary,
and there is currently **zero `WSLENV` usage** in the repo. So without a fix, a
Windows-stanza `TC_SESSION` is silently dropped and the flagship Cursor-on-Windows
path stays shared-daemon. Fix in Phase 2a (add `TC_SESSION` to `WSLENV`, or inject
into the sanitized `bash -lc` constant). `buildFilteredEnv` is a deny-list and
already passes `TC_SESSION` through — the missing piece is the WSL boundary, not
the filter.

### Doctor warning (AC)

`doctor_harness.js` warns "shared daemon mode" when multiple harness configs are
detected without `TC_SESSION` (malformed/absent token soft-fails to the shared
default in Rust — silent isolation loss).

### Non-goals (reaffirmed from F1 spec)

- No central router / aggregator / proxy (`spec:185-186`).
- No auto-derive `TC_SESSION` from tty/console session (`spec:191-192`).
- Do not change `TC_SOCKET` escape-hatch semantics (`spec:187`).
- Phase 4 only if product explicitly adopts “session supervisor” scope.

---

## Phased plan

### Phase 1 — Session id module + harness export (S)

**Files (create/modify):**

| File | Change |
|------|--------|
| `packages/terminal-commander/lib/session/mint.js` (new) | Deterministic or random mint; persist per harness id under `~/.config/terminal-commander/sessions/` |
| `packages/terminal-commander/lib/harness/write_all.js` | Add `TC_SESSION` to stanza `env` (with `TC_WSL_DISTRO` when present) |
| `packages/terminal-commander/lib/cursor/config.js` | Extend `buildTerminalCommanderServerConfig` to accept `sessionToken` → `env.TC_SESSION` |
| `packages/terminal-commander/lib/harness/io/toml_mcp.js` | Codex TOML env block if supported |
| `packages/terminal-commander/test/*` | Mint stability, stanza shape, sanitization rejects |

**Mint strategy (proposal):** `tc-` + first 12 chars of SHA-256(harness_id + stable machine id + optional workspace path). Survives re-run of `setup harness`; distinct per provider id.

**Touch:** `setup_harness.js`, `doctor_harness.js` (surface session id in doctor output).

### Phase 2 — Autostart + bootstrap (M)

**Files:**

| File | Change |
|------|--------|
| `packages/terminal-commander/lib/daemon/autostart.js` | Source `TC_SESSION` from `~/.config/terminal-commander/session` or profile snippet; fix socket existence check to use session-aware path or delegate to `terminal-commander doctor daemon` |
| `packages/terminal-commander/lib/bootstrap/constants.js` | Bridge hook exports session if needed |
| `packages/terminal-commander/lib/bootstrap/orchestrator.js` | Pass session into WSL autostart install |

**AC3 implementation sketch:** Profile snippet sets `export TC_SESSION=…`; autostart script uses same env when calling `terminal-commanderd`.

**Line 243 nit (B3):** `runAutostartOnce` return value — log non-zero exit (`autostart.js:163-167`, call site `243`).

### Phase 3 — Spawn contract: docs + E2E proof (S) — NO Rust allowlist change

**CORRECTION (verified 2026-05-28):** Do **NOT** add `TC_SESSION` to
`FORWARDED_ENV_ALLOWLIST`. The architecture is correct-by-design: the MCP/parent
process resolves `TC_SESSION` itself, computes the endpoint, and sets `TC_SOCKET`
explicitly on the daemon child (`ensure.rs:212-222`). The daemon binds that
`TC_SOCKET` and must **never re-resolve** `TC_SESSION` — re-resolution would muddy
the single-source-computes-endpoint invariant. Widening the allowlist is wrong.

**Files:**

| File | Change |
|------|--------|
| `crates/supervisor/src/ensure.rs` | **No allowlist change.** Add a comment near `FORWARDED_ENV_ALLOWLIST` (`:29`) documenting that `TC_SESSION` is intentionally NOT forwarded (parent computes endpoint → sets `TC_SOCKET`) so future readers don't "fix" it |
| `crates/supervisor/tests/` | Spawn with `TC_SESSION` set in parent env; assert child state dir contains token segment AND client/daemon resolve the same endpoint (cross-side invariant) |
| `docs/integrations/*.md` | Operator docs: manual `export TC_SESSION` for non-setup flows; `TC_SOCKET` still wins |

**Note:** `ensure_daemon` always sets `TC_SOCKET` from the computed endpoint
(`ensure.rs:212-222`). That is correct *because* `state_dir`/`endpoint` were
computed with the same `TC_SESSION` in the MCP process env (`mcp/main.rs:105-135`).
This is why no Rust change is needed — the wiring is purely getting `TC_SESSION`
into the MCP process's own env (Phase 1) and across the WSL boundary (Phase 2a).

### Phase 4 — Session supervisor (OUT OF B1 — separate future milestone)

**DECISION (2026-05-28): excluded from B1 definition-of-done.** Operator chose
spec-aligned scope; both adversarial reviews returned REVISE on this exact point.
Reaping idle daemons + token→pid registry + `session list/reap` CLI contradict F1
spec non-goals (`spec:179-184`) and require their own spec/ADR before any work.
B1 is "done" at AC1–AC6 + doctor warning, WITHOUT any Phase 4 item.

Listed below for the future milestone only — do not implement under B1:

- Idle daemon reap (TTL, last IPC time).
- Explicit session registry JSON (token → pid, endpoint, started_at).
- `terminal-commander session list|reap` CLI.

Rust pidfile already written per state dir (`crates/supervisor/src/pidfile.rs`, version-replace plan).

---

## Risks

| Risk | Mitigation |
|------|------------|
| Stale harness configs without `TC_SESSION` | `setup harness --force` migration; doctor warns “shared daemon mode” |
| Autostart races legacy socket | Phase 2 must not gate start on wrong `SOCK` path |
| `TC_SOCKET` set by ensure masks `TC_SESSION` tier | Acceptable if values are consistent; integration test |
| Weak mint → collision | Use harness_id + machine key; unit test collision resistance |
| WSL bridge env not forwarding `TC_SESSION` | Extend `filtered_env.js` / WSL bridge allowlist if bridge path used |

---

## Test strategy

| Level | What |
|-------|------|
| Unit (JS) | `is_valid_session_token` parity with Rust rules (mirror regex/doc) |
| Unit (Rust) | Existing `session.rs` + `paths.rs` tests — keep green |
| Integration | Harness write dry-run snapshot includes `env.TC_SESSION` |
| E2E | Two tokens → two distinct sockets; MCP `health` on each |
| Manual | Cursor + Codex simultaneous on Windows/WSL |

---

## Rollout

1. Ship Phase 1 behind no flag (new installs get session on `setup harness`).
2. Phase 2 before recommending systemd autostart for multi-agent users.
3. Phase 3 in same release as Phase 1–2.
4. Phase 4 only with ADR + spec amendment.

---

## File touch list (summary)

**Create:** `lib/session/mint.js`, tests, optional `~/.config/terminal-commander/session` convention doc.

**Modify:** `lib/harness/write_all.js`, `lib/cursor/config.js`, `lib/daemon/autostart.js`, `lib/bootstrap/*`, `crates/supervisor/src/ensure.rs`, integration tests, `docs/integrations/cursor.md`.

**Do not modify for B1:** `session.rs` resolution logic (unless mint rules need shared crate — prefer JS-only mint).
