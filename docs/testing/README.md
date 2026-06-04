# Trust-test goals (A/B)

Cross-LLM trust validation for Terminal Commander: hand each goal file to the
matching external agent and have it drive TC, then report whether it **trusts and
routes through TC** versus **falls back to raw shell**.

- `codex-tc-trust-test-goal.md` — paste into **Codex**.
- `cursor-tc-trust-test-goal.md` — paste into **Cursor**.

## How to run

These are **runtime** tests, not source tests — they exercise the **installed**
TC (its MCP tools + CLI against the running daemon), independent of git branch.

1. Update to the version you want to test: `terminal-commander update`
   (the daemon autostarts on first tool use).
2. Give the matching goal file to Codex / Cursor.
3. Collect the agent's report (first-try success rate, any fall-back-to-raw-shell,
   schema/error ergonomics gaps).

The value is the *other* model's perspective — run them in Codex/Cursor, not in
the same agent that built TC.
