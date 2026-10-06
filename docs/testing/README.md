# Trust-test goals (A/B)

Cross-LLM trust validation for Terminal Commander: hand each goal file to the
matching external agent and have it drive TC, then report whether it **trusts and
routes through TC** versus **falls back to raw shell**.

- `codex-tc-trust-test-goal.md` — paste into **Codex**.
- `cursor-tc-trust-test-goal.md` — paste into **Cursor**.

Note (2026-10-05): the goal files are dated baseline kits written against TC
0.1.38 with a 17-tool list; the live surface is now 60 tools (six compact
facades), so use them as a method, not as a current tool inventory.

## How to run

These are **runtime** tests, not source tests — they exercise the **installed**
TC (its MCP tools + CLI against the running daemon), independent of git branch.

1. Update to the version you want to test: `npm update -g terminal-commander`,
   then `terminal-commander restart` to swap the running daemon (the daemon
   also autostarts on first tool use).
2. Give the matching goal file to Codex / Cursor.
3. Collect the agent's report (first-try success rate, any fall-back-to-raw-shell,
   schema/error ergonomics gaps).

The value is the *other* model's perspective — run them in Codex/Cursor, not in
the same agent that built TC.
