# B1 adversarial review — F1 launcher wiring

**Reviewer stance:** Skeptical eng manager  
**Plan under review:** [PLAN.md](./PLAN.md)  
**Date:** 2026-05-28

---

## 1. Is this real?

| Question | Finding |
|----------|---------|
| Is F1 Rust work done? | **Yes.** `resolve_session`, path subdirs, daemon pipe routing exist and are tested. |
| Is “dormant” accurate? | **Yes.** No production JS sets `TC_SESSION`; autostart uses legacy socket path only (`autostart.js:45-56`). |
| Does multi-agent break today? | **Yes, by default.** All harnesses share per-user default endpoint unless operator manually exports `TC_SESSION`. |

**Verdict on reality:** Problem is real; ledger correctly identifies wiring as the gap, not Rust resolution.

---

## 2. Scope creep

| Item | Assessment |
|------|------------|
| Mint + export | **In scope** — required to make F1 usable; aligns with spec “launcher sets `TC_SESSION`” (`spec:53-54`). |
| Per-session pidfiles | **Already in Rust** when `TC_SESSION` set — not new work; ledger overstates as greenfield. |
| Reaping idle daemons | **Scope creep vs F1 spec** — explicitly deferred to “session supervisor” (`spec:181-183`). Must be Phase 4 + spec amendment or dropped from B1. |
| Central router | Correctly excluded. |
| Auto-derive from tty | Correctly excluded. |

**Required plan edit:** Split Phase 4 from B1 acceptance; mark reap as **non-goal for B1 closure** unless product overrides spec.

---

## 3. Security

| Threat | Challenge |
|--------|-----------|
| Predictable `TC_SESSION` | Attacker squats pipe/socket if token guessable. Mint must include entropy or machine secret; document threat model for shared machines. |
| Token in MCP JSON on disk | `~/.cursor/mcp.json` world-readable — session id is not a secret but implies isolation boundary; acceptable if token is unguessable. |
| Env inheritance (M7) | `terminal-commander-mcp.js` passes full `process.env` (`:41-44`). Harness `TC_SESSION` in stanza is correct; admin shim still inherits secrets — track under B2 M7, not B1 blocker. |
| Malformed token | Rust soft-fails to shared default (`session.rs:56-59`) — silent loss of isolation; doctor should warn. |

---

## 4. Race conditions

| Scenario | Gap |
|----------|-----|
| Autostart vs MCP spawn | Two starters, one token: autostart without `TC_SESSION` + MCP with token → **split brain**. Phase 2 is mandatory before autostart is “safe” for B1. |
| ensure sets `TC_SOCKET` | Forces full override tier on daemon child (`ensure.rs:222`). Safe if parent already resolved with same env; unsafe if parent env changes between resolve and spawn. Low probability. |
| Replace/kill cross-session | `replace_if_stale` scopes to `state_dir` (`replace.rs:207-214`) — good. Wrong `TC_SESSION` in harness only hurts that harness. |

---

## 5. SymForge vs manual `TC_SESSION`

Operators can `export TC_SESSION=agent-1` manually — **works today** for Rust stack. B1 is not about SymForge; it is about **default install path** (`setup harness`, autostart). Plan should state: manual export remains supported escape hatch (spec § `TC_SOCKET` still wins).

---

## 6. Failure modes

| Failure | User impact |
|---------|-------------|
| Harness write skipped (`STUB_UNVERIFIED`) | Provider still on shared daemon — document. |
| Invalid persisted token | Falls back to shared default with stderr warning — doctor should surface. |
| WSL autostart without session file | Legacy daemon on shared socket — Phase 2 fix. |
| Migration: existing users | No `TC_SESSION` until re-run setup — expected; not a regression. |

---

## 7. Missing integration tests?

| Gap | Recommendation |
|-----|----------------|
| No E2E harness→MCP→daemon with `TC_SESSION` | **Blocker for B1 done** — add in Phase 1 or 3. |
| No test that autostart respects session | Phase 2 shell test or JS mock of rendered script. |
| Windows pipe + session | Run on Windows CI agent, not WSL-only. |

---

## 8. Does plan break B1 if B2 runs first?

B2 M8/M3 do not block B1. B2 M7 (full env inherit) is adjacent — optional to filter secrets before B1 ships multi-agent.

---

## Verdict: **REVISE**

### Required plan edits

1. **Remove Phase 4 from B1 definition of done** — move to separate milestone referencing F1 spec “session supervisor” (`spec:181-183`), unless PM signs spec change.
2. **Add doctor warning** for missing `TC_SESSION` when multiple harnesses detected.
3. **Add AC: autostart parity** — explicit gate for Phase 2 completion.
4. **Document** that pidfile-per-session is automatic via `resolve_state_dir_with`, not new JS work.
5. **Mint threat model** paragraph (entropy + file permissions).

### Approve without revision?

**No** — scope boundary on reaping/lifecycle must be explicit to avoid implementing a second spec inside B1.

### Reject?

**No** — wiring is necessary and correctly prioritized as load-bearing.
