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
# Requires a built daemon: cargo build -p terminal-commanderd.
#
# Safety: launches only the local terminal-commanderd.exe; isolated TC_DATA
# temp dirs; kills both daemons and removes temp dirs on exit. No network,
# no sudo, does not touch any real config.

param(
  [string]$DaemonPath = "$PSScriptRoot\..\..\target\debug\terminal-commanderd.exe",
  [string]$TokenA = "tc-sess-aaaa1111",
  [string]$TokenB = "tc-sess-bbbb2222"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $DaemonPath)) {
  Write-Error "daemon not found at $DaemonPath; run: cargo build -p terminal-commanderd"
  exit 2
}

$dataA = Join-Path $env:TEMP ("tc-sess-A-" + [System.Guid]::NewGuid().ToString("N").Substring(0, 8))
$dataB = Join-Path $env:TEMP ("tc-sess-B-" + [System.Guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force -Path $dataA, $dataB | Out-Null

function Start-SessionDaemon([string]$tok, [string]$data) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $DaemonPath
  $psi.Arguments = "start --mode ipc-server"
  $psi.UseShellExecute = $false
  $psi.RedirectStandardError = $true
  $psi.RedirectStandardOutput = $true
  $psi.EnvironmentVariables["TC_SESSION"] = $tok
  $psi.EnvironmentVariables.Remove("TC_SOCKET") | Out-Null
  $psi.EnvironmentVariables["TC_DATA"] = $data
  return [System.Diagnostics.Process]::Start($psi)
}

$pA = $null
$pB = $null
try {
  $pA = Start-SessionDaemon $TokenA $dataA
  $pB = Start-SessionDaemon $TokenB $dataB
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

  if ($hasA -and $hasB -and ($nameA -ne $nameB) -and $hasBAfter) {
    Write-Output "E2E-RESULT: PASS"
    $exit = 0
  } else {
    Write-Output "E2E-RESULT: FAIL"
    $exit = 1
  }
} finally {
  foreach ($p in @($pA, $pB)) {
    if ($p -and -not $p.HasExited) {
      try { $p.Kill(); $p.WaitForExit(3000) | Out-Null } catch { }
    }
  }
  Remove-Item -Recurse -Force $dataA, $dataB -ErrorAction SilentlyContinue
}

exit $exit
