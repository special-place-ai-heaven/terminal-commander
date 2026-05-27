# B2 adversarial review — M1–M8 bundle

**Date:** 2026-05-28

---

## Is this bundle worth a dedicated milestone?

**No.** None block B1. Treat as **quality debt** with clear ROI on M8, M3, M2 (flake), then M6/M5 (correctness polish).

---

## Per-item challenge

| ID | Is it real? | Worth fixing now? | Breaks B1? |
|----|-------------|-------------------|------------|
| M1 | Yes, latent | Low urgency until second test added | No |
| M2 | Yes, CI flake | **Yes** if CI red | No |
| M3 | Yes | **Yes** if bucket tests flake | No |
| M4 | **Mostly fixed** | Update audit/ledger; add test only | No |
| M5 | Yes, edge case | Low; fix when touching update locks | No |
| M6 | Yes, ugly errors | Low effort, good DX | No |
| M7 | Yes, inconsistent | Policy call — not a security fix if same-user | No |
| M8 | Yes, dev-env flake | **High** value per line changed | No |

---

## Missing integration coverage

- B2 does not add MCP/daemon features — unit/integration tests listed per item suffice.
- **Do not** block B1 on M2/M3 completion.

---

## Ledger accuracy

| Claim | Verdict |
|-------|---------|
| M4 open | **Stale** — `replace.rs:230-242` addresses kill-time re-verify |
| M2 ipc_bucket 400/800ms | **Partially wrong** — 40/50ms in that file; 400/800 elsewhere |
| M5 WAIT_FAILED in codebase | **Confirmed** — no symbol name in source; logic at `update_locks.rs:205-206` |

---

## Verdict: **APPROVE** (bundle plan with edits)

### Edits applied to plan

1. Mark **M4** as close/test-only.  
2. Correct **M2** line references.  
3. Prioritize **M8, M3, M2** over M1/M5/M7.

### Reject entire bundle?

**No.**
