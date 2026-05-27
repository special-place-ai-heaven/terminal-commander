# D adversarial review

**Date:** 2026-05-28

## Is this real?

- **D1:** Yes — multiple stale deferral comments exist.  
- **D2:** Yes — `NotImplemented` is dead code in production catalogue.

## Worth doing?

**Low priority.** Value is **agent/navigability**, not runtime. Still cheap if batched.

## Risk

- Wrongly removing a comment that still reflects a real gap → **mitigate** with per-file goal check, not blanket delete.
- Removing `NotImplemented` breaks external MCP clients that expect enum exhaustiveness — **grep fixtures** first (`tests/fixtures/contracts/`).

## Breaks B1?

**No.**

## Verdict: **APPROVE**

Do after B1 Phase 1 or in parallel; do not expand into doc rewrites beyond comment accuracy.
