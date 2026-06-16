# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# US6 / T055: native Windows omni acceptance smoke.
#
# Builds a fresh daemon + MCP adapter (native Windows cargo) and runs the
# RUNNABLE subset of the O-01..O-14 omni acceptance sequence against them,
# EXITING NON-ZERO on any RAN-gate failure. The O-sequence and per-gate
# checks live in scripts/smoke/omni-o-runner.py (shared with the POSIX
# smokes); this script is the Windows build/daemon/teardown harness.
#
# Windows availability (honest per the omni_status matrix):
#   - PTY: AVAILABLE via ConPTY (portable-pty). O-07 (native Windows PTY
#     python REPL) RUNS here; the posix-PTY gate O-03 is skipped.
#   - Sessions: NOT available (the shell-session runtime is unix-only for
#     now). O-02 is SKIPPED WITH A LOUD NOTICE.
#   - Privileged (O-06), macOS (O-08), SSH/container (O-09/O-10),
#     IPC-fault (O-13), provider-trust (O-14): SKIPPED WITH A LOUD NOTICE.
# A skipped gate is NEVER counted as a pass; the runner prints which gates
# ran vs were skipped.
#
# Transport: Windows IPC is a named pipe. A unique TC_SOCKET pipe name is
# handed to BOTH the daemon and the adapter (both resolve TC_SOCKET first).
#
# Usage:  pwsh -File scripts/smoke/verify-omni-windows.ps1
# Exit:   0 = all RAN gates passed; 1 = a RAN gate failed; 2 = env problem.

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Err($text) { Write-Host "verify-omni-windows: $text" }

# Resolve repo root from the script path.
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..\..")).Path

# Host must be Windows.
$isWin = ($env:OS -eq "Windows_NT")
if (-not $isWin -and $PSVersionTable -and $PSVersionTable.PSObject.Properties["Platform"] -and $PSVersionTable.Platform -eq "Win32NT") {
    $isWin = $true
}
if (-not $isWin) {
    Write-Err "this smoke is Windows-only; run the POSIX siblings on Linux/WSL/macOS"
    exit 2
}

# Dependencies.
foreach ($dep in @("cargo", "python")) {
    if (-not (Get-Command $dep -ErrorAction SilentlyContinue)) {
        Write-Err "missing dependency: $dep"
        exit 2
    }
}

Push-Location $repoRoot
$daemonProc = $null
$tmpDir = $null
try {
    Write-Err "repo_root=$repoRoot"
    Write-Err "building debug binaries (native windows cargo)"
    & cargo build -p terminal-commanderd -p terminal-commander-mcp --bins | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Err "cargo build failed"; exit 2 }

    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
    $daemonBin = Join-Path $repoRoot "$targetDir\debug\terminal-commanderd.exe"
    $mcpBin = Join-Path $repoRoot "$targetDir\debug\terminal-commander-mcp.exe"
    if (-not (Test-Path $daemonBin)) { Write-Err "daemon binary not found at $daemonBin"; exit 2 }
    if (-not (Test-Path $mcpBin)) { Write-Err "mcp binary not found at $mcpBin"; exit 2 }

    # Private temp data dir + smoke-only full-capability config. allow_shell
    # ON so O-01 runs; allow_session is moot on Windows (runtime unix-only).
    # allow_privileged stays FALSE -- the helper is plan-only.
    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("omni-smoke-" + [System.Guid]::NewGuid().ToString("N").Substring(0, 12))
    $dataDir = Join-Path $tmpDir "data"
    New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
    $config = Join-Path $tmpDir "terminal-commanderd.toml"
    $dataDirToml = $dataDir.Replace('\', '\\')
    @"
[daemon]
data_dir = "$dataDirToml"

[policy]
profile = "developer_local"
profile_version = "1"

[policy.caps]
allow_shell = true
allow_session = true
allow_privileged = false
allow_remote = false

[sifters]
universal_extractors = true
"@ | Set-Content -Path $config -Encoding UTF8

    # Unique named-pipe endpoint shared by daemon + adapter via TC_SOCKET.
    $pipe = "\\.\pipe\tc-omni-smoke-" + [System.Guid]::NewGuid().ToString("N").Substring(0, 12)
    $env:TC_SOCKET = $pipe
    $daemonLog = Join-Path $tmpDir "daemon.log"

    Write-Err "starting daemon: data_dir=$dataDir pipe=$pipe"
    $daemonProc = Start-Process -FilePath $daemonBin `
        -ArgumentList @("--config", $config, "--data-dir", $dataDir, "start", "--mode", "ipc-server") `
        -PassThru -NoNewWindow -RedirectStandardOutput $daemonLog -RedirectStandardError "$daemonLog.err"

    # Readiness: poll the adapter health tool until the daemon answers (no
    # filesystem socket to stat on Windows). Bounded ~8s.
    $ready = $false
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 200
        if ($daemonProc.HasExited) {
            Write-Err "daemon exited early (code $($daemonProc.ExitCode)); log:"
            if (Test-Path $daemonLog) { Get-Content $daemonLog | ForEach-Object { Write-Err $_ } }
            if (Test-Path "$daemonLog.err") { Get-Content "$daemonLog.err" | ForEach-Object { Write-Err $_ } }
            exit 2
        }
        # Probe via the runner's own MCP client is heavy; instead do a cheap
        # named-pipe existence check by attempting to open it.
        if (Test-Path $pipe) { $ready = $true; break }
    }
    if (-not $ready) {
        Write-Err "daemon pipe did not appear within ~8s ($pipe)"
        if (Test-Path $daemonLog) { Get-Content $daemonLog | ForEach-Object { Write-Err $_ } }
        exit 2
    }
    Write-Err "daemon up (pid=$($daemonProc.Id))"

    Write-Err "running O-01..O-14 sequence (profile=windows)"
    $env:OMNI_REPO_ROOT = $repoRoot
    & python (Join-Path $scriptDir "omni-o-runner.py") $mcpBin $pipe "windows"
    $runnerRc = $LASTEXITCODE
    Write-Err "O-runner exit code: $runnerRc"
    exit $runnerRc
}
finally {
    if ($daemonProc -and -not $daemonProc.HasExited) {
        try { Stop-Process -Id $daemonProc.Id -Force -ErrorAction SilentlyContinue } catch { }
    }
    if ($tmpDir -and (Test-Path $tmpDir)) {
        try { Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue } catch { }
    }
    Pop-Location
}
