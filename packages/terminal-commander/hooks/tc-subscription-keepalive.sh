#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Terminal Commander subscription keep-alive Stop hook (REFERENCE, default OFF).
#
# Reads the Stop-hook JSON from stdin. If TC_KEEPALIVE=1 AND TC_KEEPALIVE_SUB is
# set to an open sub_id, it pulls pending events ONCE; if any are pending, it
# BLOCKS the stop and injects them so the model reacts in-session. Bounded HARD:
# at most TC_KEEPALIVE_MAX (default 3) CONSECUTIVE keep-alives per session_id
# (its OWN counter in a temp file -- stop_hook_active is a boolean, not a count).
# On budget exhaustion it force-allows stop with a loud message. No-ops when the
# guard env is unset, when there are no events, or in a headless run.
#
# ANTI-GOAL: high-rate event streams. Use a `Monitor` over
# `terminal-commander subscription-stream <sub_id>` for those. This hook is for
# low-rate completion/activation watches only.
set -euo pipefail

# Default OFF: do nothing unless explicitly enabled with an open sub_id.
if [ "${TC_KEEPALIVE:-0}" != "1" ] || [ -z "${TC_KEEPALIVE_SUB:-}" ]; then
  exit 0
fi

input="$(cat)"

# Respect stop_hook_active: when Claude Code is ALREADY continuing from a prior
# Stop block, do not stack another block on top -- let this turn complete.
if printf '%s' "$input" | grep -q '"stop_hook_active"[[:space:]]*:[[:space:]]*true'; then
  exit 0
fi

session_id="$(printf '%s' "$input" \
  | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
session_id="${session_id:-unknown}"
# Sanitize: session_id is UNTRUSTED stdin JSON and is spliced into a temp-file
# path below. Collapse anything outside [A-Za-z0-9_-] to '_' so it cannot
# traverse out of the temp dir (e.g. "../../etc/x"). Done BEFORE building paths.
session_id="${session_id//[^A-Za-z0-9_-]/_}"
max="${TC_KEEPALIVE_MAX:-3}"

# Per-session counter file (keyed by the SANITIZED session_id) under temp dir.
counter_file="${TMPDIR:-/tmp}/tc-keepalive-${session_id}.count"
count="$(cat "$counter_file" 2>/dev/null || echo 0)"
case "$count" in
  ''|*[!0-9]*) count=0 ;;
esac

# One-shot pull of pending events (bounded by the daemon's pull cap). MUST be
# the ONE-SHOT `subscription-pull` verb: it issues a single pull and EXITS
# immediately (even on empty), so the hook never blocks/loops and cannot wedge
# the session -- unlike `subscription-stream`, which loops ~8 s/pull forever.
# The CLI exits non-zero on an unknown/closed sub_id; treat that as "no events".
events="$(terminal-commander subscription-pull "$TC_KEEPALIVE_SUB" --max 20 2>/dev/null || true)"

if [ -z "$events" ]; then
  # Nothing pending (or headless / sub closed): reset the budget, allow stop.
  rm -f "$counter_file"
  exit 0
fi

if [ "$count" -ge "$max" ]; then
  # Budget exhausted: force-allow stop with a loud message; reset the budget.
  rm -f "$counter_file"
  printf '%s' '{"continue":false,"stopReason":"terminal-commander keep-alive budget exhausted; events pending -- resume via subscription_pull"}'
  exit 0
fi

# Under budget: block the stop and inject the events as additionalContext.
echo $((count + 1)) > "$counter_file"
# JSON-escape the events blob; fall back to a bounded literal if python3 is absent.
esc="$(printf '%s' "$events" \
  | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' 2>/dev/null \
  || printf '"%s"' "events pending (install python3 to inline them)")"
printf '{"decision":"block","reason":"terminal-commander: new subscription events","hookSpecificOutput":{"hookEventName":"Stop","additionalContext":%s}}' "$esc"
exit 0
