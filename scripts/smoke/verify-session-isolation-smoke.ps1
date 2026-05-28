# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# F1 per-harness session isolation smoke (Windows).
#
# Proves the product promise "one daemon per harness": two distinct
# TC_SESSION tokens launch two independent daemons bound to two distinct
# named pipes, simultaneously, with no collision. Mirrors the resolver path
# used in production (daemon DaemonConfig::pipe_name -> supervisor
# resolve_socket_path: TC_SOCKET > TC_SESSION > username default).
#
# Phase H extension: also exercises `terminal-commander session list` and
# `terminal-commander session reap --all` end-to-end against a SHARED base
# TC_DATA dir (real-world shape: one user, one base, many sessions as
# subdirs keyed by TC_SESSION token).
#
# Requires built binaries:
#   cargo build -p terminal-commanderd
#   cargo build -p terminal-commander
#
# Safety: launches only the local terminal-commanderd.exe; isolated TC_DATA
# temp dir; kills both daemons and removes the temp dir on exit. No network,
# no sudo, does not touch any real config.

param(
  [string]$DaemonPath = "$PSScriptRoot\..\..\target\debug\terminal-commanderd.exe",
  [string]$CliPath = "$PSScriptRoot\..\..\target\debug\terminal-commander.exe",
  [string]$TokenA = "tc-sess-aaaa1111",
  [string]$TokenB = "tc-sess-bbbb2222"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $DaemonPath)) {
  Write-Error "daemon not found at $DaemonPath; run: cargo build -p terminal-commanderd"
  exit 2
}
if (-not (Test-Path $CliPath)) {
  Write-Error "cli not found at $CliPath; run: cargo build -p terminal-commander"
  exit 2
}

# Single shared base directory: both daemons read TC_DATA=$dataBase and the
# supervisor places each session's state under $dataBase\<TC_SESSION>. This is
# the real product shape (one user, one base, many sessions) and lets a
# single `session list` enumerate BOTH seeded sessions in one shot.
$dataBase = Join-Path $env:TEMP ("tc-sess-base-" + [System.Guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force -Path $dataBase | Out-Null

function Start-SessionDaemon([string]$tok, [string]$dataBase) {
  # Mirror the production launch shape (supervisor::ensure spawns with
  # `--data-dir <state_dir> start --mode ipc-server`). state_dir for a
  # seeded session is the per-token subdir of $dataBase. Passing it on
  # the command line is what lets `session reap`'s identity gate
  # (`pid_belongs_to_daemon`) match this process's CommandLine to the
  # seeded state_dir — without it, reap correctly refuses to kill.
  $stateDir = Join-Path $dataBase $tok
  New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $DaemonPath
  $psi.Arguments = "--data-dir `"$stateDir`" start --mode ipc-server"
  $psi.UseShellExecute = $false
  $psi.RedirectStandardError = $true
  $psi.RedirectStandardOutput = $true
  $psi.EnvironmentVariables["TC_SESSION"] = $tok
  $psi.EnvironmentVariables.Remove("TC_SOCKET") | Out-Null
  $psi.EnvironmentVariables["TC_DATA"] = $dataBase
  return [System.Diagnostics.Process]::Start($psi)
}

$pA = $null
$pB = $null
$exit = 1
try {
  $pA = Start-SessionDaemon $TokenA $dataBase
  $pB = Start-SessionDaemon $TokenB $dataBase
  Start-Sleep -Seconds 2

  $nameA = "terminal-commander-$TokenA"
  $nameB = "terminal-commander-$TokenB"
  $pipes = [System.IO.Directory]::GetFiles("\\.\pipe\")
  $hasA = ($pipes | Where-Object { $_ -like "*$nameA" }).Count -gt 0
  $hasB = ($pipes | Where-Object { $_ -like "*$nameB" }).Count -gt 0

  Write-Output "pipe A ($nameA) present: $hasA"
  Write-Output "pipe B ($nameB) present: $hasB"
  Write-Output "distinct endpoints: $($nameA -ne $nameB)"

  # Independence: killing A must not take down B.
  if (-not $pA.HasExited) { $pA.Kill(); $pA.WaitForExit(3000) | Out-Null }
  Start-Sleep -Seconds 1
  $pipes2 = [System.IO.Directory]::GetFiles("\\.\pipe\")
  $hasBAfter = ($pipes2 | Where-Object { $_ -like "*$nameB" }).Count -gt 0
  Write-Output "after killing A, B pipe still present: $hasBAfter"

  # ---- session list / reap E2E (Phase H extension) ----
  # A was just killed above to prove independence. Respawn it so list+reap
  # can exercise BOTH session tokens in the live state, then reap --all and
  # assert both pipes go away.
  $pA = Start-SessionDaemon $TokenA $dataBase
  Start-Sleep -Seconds 2

  # `terminal-commander session list` enumerates default + seeded sessions
  # under TC_DATA. Both A and B are seeded subdirs of $dataBase.
  $env:TC_DATA = $dataBase
  $env:TC_SESSION = ""
  $env:TC_SOCKET = ""
  $listOut = & $CliPath session list 2>&1 | Out-String
  Write-Output "session list output:"
  Write-Output $listOut
  $listShowsA = $listOut -match [regex]::Escape($TokenA)
  $listShowsB = $listOut -match [regex]::Escape($TokenB)
  Write-Output "list shows tokenA: $listShowsA"
  Write-Output "list shows tokenB: $listShowsB"

  # `session reap --all` should signal both daemons to stop and clean state.
  $reapOut = & $CliPath session reap --all 2>&1 | Out-String
  $reapExit = $LASTEXITCODE
  Write-Output "session reap --all exit: $reapExit"
  Write-Output "session reap --all output:"
  Write-Output $reapOut
  Start-Sleep -Seconds 2

  $pipes3 = [System.IO.Directory]::GetFiles("\\.\pipe\")
  $tokenAPipeAfterReap = ($pipes3 | Where-Object { $_ -like "*$nameA" }).Count -gt 0
  $tokenBPipeAfterReap = ($pipes3 | Where-Object { $_ -like "*$nameB" }).Count -gt 0
  Write-Output "after reap --all, tokenA pipe present: $tokenAPipeAfterReap"
  Write-Output "after reap --all, tokenB pipe present: $tokenBPipeAfterReap"

  if ($hasA -and $hasB -and ($nameA -ne $nameB) -and $hasBAfter `
      -and $listShowsA -and $listShowsB `
      -and (-not $tokenAPipeAfterReap) -and (-not $tokenBPipeAfterReap)) {
    Write-Output "E2E-RESULT: PASS"
    $exit = 0
  } else {
    Write-Output "E2E-RESULT: FAIL (hasA=$hasA hasB=$hasB distinct=$($nameA -ne $nameB) hasBAfter=$hasBAfter listShowsA=$listShowsA listShowsB=$listShowsB tokenAPipeAfterReap=$tokenAPipeAfterReap tokenBPipeAfterReap=$tokenBPipeAfterReap)"
    $exit = 1
  }
} finally {
  foreach ($p in @($pA, $pB)) {
    if ($p -and -not $p.HasExited) {
      try { $p.Kill(); $p.WaitForExit(3000) | Out-Null } catch { }
    }
  }
  Remove-Item -Recurse -Force $dataBase -ErrorAction SilentlyContinue
}

exit $exit
