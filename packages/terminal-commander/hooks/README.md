# Terminal Commander Stop-hook keep-alive (reference, default OFF)

A reference Claude Code `Stop` hook that keeps a session alive while a Terminal
Commander subscription still has pending events, then injects those events so
the model reacts in-session instead of stopping.

This is the ONE Real-Time-Active pattern that can wedge a session, so it is:

- DEFAULT OFF (guarded by an env opt-in),
- bounded HARD to a small consecutive budget (default 3), and
- a no-op in headless runs and when nothing is pending.

ANTI-GOAL: high-rate event streams. For those use a `Monitor` over
`terminal-commander subscription-stream <sub_id>` (one model turn per matched
event line). This hook is for LOW-RATE completion/activation watches only
("block stop while my long build is still producing high-sev events").

## Files

| File | Role |
| --- | --- |
| `tc-subscription-keepalive.sh` | POSIX/bash reference hook. |
| `tc-subscription-keepalive.ps1` | Windows PowerShell twin (same contract). |
| `settings.snippet.json` | Copy-paste `Stop` hook registration for `settings.json`. |

## Enable

1. Open a subscription and note its `sub_id`:
   `subscription_open { sources: { kind: "all" }, severity_min: "high" }`.
2. Copy the matching script somewhere stable, e.g.
   `~/.terminal-commander/hooks/tc-subscription-keepalive.sh` (or `.ps1`), and
   make the `.sh` executable (`chmod +x`).
3. Merge `settings.snippet.json` into your Claude Code `settings.json` (adjust
   the `command` path; use the `.ps1` on Windows).
4. Export the opt-in env for the session:
   - `TC_KEEPALIVE=1` (master switch; unset = no-op),
   - `TC_KEEPALIVE_SUB=<sub_id>` (the open subscription to watch),
   - `TC_KEEPALIVE_MAX=3` (optional; consecutive keep-alive budget, default 3).

## Contract

- On a `Stop` event the hook reads the hook JSON from stdin
  (`{ session_id, stop_hook_active, ... }`).
- If `stop_hook_active` is `true` (Claude Code is ALREADY continuing from a
  prior block), it exits 0 (allow) to avoid stacking blocks.
- It pulls pending events ONCE via
  `terminal-commander subscription-stream <sub_id> --max 20`.
- No events (or headless / closed sub): it resets the budget and exits 0
  (allow the stop).
- Events pending and budget remaining: it prints
  `{"decision":"block","reason":...,"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"<events>"}}`
  and increments a PER-SESSION counter.
- Budget exhausted (`count >= max`): it prints
  `{"continue":false,"stopReason":"...budget exhausted...resume via subscription_pull"}`
  and resets the counter.

### Why a temp-file counter

`stop_hook_active` is a BOOLEAN ("a Stop hook is already continuing this
session"), NOT a count. So the hook keeps its OWN consecutive-keep-alive
counter in a temp file keyed by `session_id`
(`$TMPDIR/tc-keepalive-<session_id>.count` on POSIX, `%TEMP%` on Windows). The
counter resets whenever there are no pending events or the budget is hit, so a
later burst of events gets a fresh budget.

## Manual verification recipe

Hooks only fire in a live interactive session, so verify as a user:

1. Open a sub (`subscription_open { sources: { kind: "all" }, severity_min: "high" }`),
   note the `sub_id`.
2. Export `TC_KEEPALIVE=1`, `TC_KEEPALIVE_SUB=<sub_id>`; wire the `Stop` hook
   from `settings.snippet.json`.
3. Start a command that emits a high-sev event (a `command_start_combed` whose
   rule fires), then ask the model to stop.
4. Observe: the stop is BLOCKED and the event text is injected, up to 3 times,
   then a loud force-stop (`continue:false`).
5. Unset `TC_KEEPALIVE` (or close the sub): the model stops normally (no-op).

### Offline smoke (no live session)

You can exercise the decision branches directly by piping a synthetic Stop-hook
payload to the script. With `TC_KEEPALIVE` UNSET the hook must produce no output
and exit 0 regardless of input:

```bash
printf '{"session_id":"smoke","stop_hook_active":false}' \
  | ./tc-subscription-keepalive.sh ; echo "exit=$?"
# expected: no stdout, exit=0 (default OFF)
```

```powershell
'{"session_id":"smoke","stop_hook_active":false}' `
  | pwsh -NoProfile -File ./tc-subscription-keepalive.ps1 ; "exit=$LASTEXITCODE"
# expected: no stdout, exit=0 (default OFF)
```

With `TC_KEEPALIVE=1` and `stop_hook_active:true`, the hook must also exit 0
with no block (it never stacks on an already-continuing turn).
