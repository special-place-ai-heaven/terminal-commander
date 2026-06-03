# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Terminal Commander subscription keep-alive Stop hook (REFERENCE, default OFF).
# Mirrors the .sh contract on Windows PowerShell:
#
# Reads the Stop-hook JSON from stdin. If TC_KEEPALIVE=1 AND TC_KEEPALIVE_SUB is
# set to an open sub_id, it pulls pending events ONCE; if any are pending, it
# BLOCKS the stop and injects them. Bounded HARD to TC_KEEPALIVE_MAX (default 3)
# CONSECUTIVE keep-alives per session_id (its OWN counter in a temp file --
# stop_hook_active is a boolean, not a count). On budget exhaustion it
# force-allows stop with a loud message. No-ops when unset / no events / headless.
#
# ANTI-GOAL: high-rate event streams. Use a `Monitor` over
# `terminal-commander subscription-stream <sub_id>` for those. This hook is for
# low-rate completion/activation watches only.
$ErrorActionPreference = 'Stop'

# Default OFF: do nothing unless explicitly enabled with an open sub_id.
if ($env:TC_KEEPALIVE -ne '1' -or [string]::IsNullOrEmpty($env:TC_KEEPALIVE_SUB)) { exit 0 }

$raw = [Console]::In.ReadToEnd()
$session = 'unknown'
$stopActive = $false
try {
  $parsed = $raw | ConvertFrom-Json
  if ($parsed.session_id) { $session = [string]$parsed.session_id }
  if ($parsed.stop_hook_active) { $stopActive = [bool]$parsed.stop_hook_active }
} catch {}

# Respect stop_hook_active: when ALREADY continuing from a prior Stop block, do
# not stack another block -- let this turn complete.
if ($stopActive) { exit 0 }

if ([string]::IsNullOrEmpty($session)) { $session = 'unknown' }
$max = if ($env:TC_KEEPALIVE_MAX) { [int]$env:TC_KEEPALIVE_MAX } else { 3 }

# Per-session counter file (keyed by session_id) under the temp dir.
$counterFile = Join-Path $env:TEMP "tc-keepalive-$session.count"
$count = 0
if (Test-Path $counterFile) {
  $parsedCount = 0
  if ([int]::TryParse((Get-Content $counterFile -Raw).Trim(), [ref]$parsedCount)) { $count = $parsedCount }
}

# One-shot pull of pending events (bounded by the daemon's pull cap). The CLI
# bridge exits non-zero on an unknown/closed sub_id; treat that as "no events".
$events = ''
try {
  $events = (& terminal-commander subscription-stream $env:TC_KEEPALIVE_SUB --max 20 2>$null) -join "`n"
} catch { $events = '' }

if ([string]::IsNullOrWhiteSpace($events)) {
  # Nothing pending (or headless / sub closed): reset the budget, allow stop.
  Remove-Item $counterFile -ErrorAction SilentlyContinue
  exit 0
}

if ($count -ge $max) {
  # Budget exhausted: force-allow stop with a loud message; reset the budget.
  Remove-Item $counterFile -ErrorAction SilentlyContinue
  @{ continue = $false; stopReason = 'terminal-commander keep-alive budget exhausted; events pending -- resume via subscription_pull' } | ConvertTo-Json -Compress
  exit 0
}

# Under budget: block the stop and inject the events as additionalContext.
($count + 1) | Set-Content $counterFile
$obj = @{
  decision = 'block'
  reason   = 'terminal-commander: new subscription events'
  hookSpecificOutput = @{ hookEventName = 'Stop'; additionalContext = $events }
}
$obj | ConvertTo-Json -Compress -Depth 6
exit 0
