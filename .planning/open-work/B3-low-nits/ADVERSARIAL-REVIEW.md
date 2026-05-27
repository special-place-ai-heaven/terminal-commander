# B3 adversarial review

**Date:** 2026-05-28

## Is this real?

All nine locations **verified** in current `main` (see PLAN table). Ledger line numbers are **mostly accurate**; path prefixes were wrong for JS/Rust server.

## Is fix worth it?

| Item | Worth it? |
|------|-----------|
| parse_status → Draft | **Yes** — silent data corruption risk |
| evict swallow | **Yes** — hides DB corruption |
| bridge_required dead code | **Yes** — reduces confusion |
| autostart silent fail | **Yes** — operability |
| context tautology | **Yes** — clarity (1 line) |
| audit null fallback | **Optional** — benign per audit |
| command/pty metrics race | **Only if** users report wrong metrics; else comment-only |
| load 5s bound | **Low** — generous bound |
| noise clock | **Low** until test flakes |

## Does B3 break B1?

**No.**

## Verdict: **APPROVE**

Ship as single papercuts PR; prioritize `parse_status`, `evict_expired` log, `bridge_required` removal, autostart log.
