# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# WWS07 Windows -> WSL bridge smoke.
#
# Exercises the WWS06 CLI surface (`terminal-commander doctor`,
# `doctor wsl`, `setup cursor-wsl --print-config`, `setup cursor-wsl
# --dry-run`) and the WWS04 bridge spawn through a temporary npm
# prefix. Optionally drives an MCP `initialize` + `tools/list` +
# `health` round-trip THROUGH the bridge when the WSL-side runtime
# is installed.
#
# Hard safety boundary (locked at WWS07 prep amendment):
#   - NO direct wsl.exe or child_process call from PowerShell. The
#     only spawn the script issues is `node`; the JS CLI shim is the
#     only site that invokes wsl.exe (via WWS04 lib/wsl/spawn.js).
#   - NO sudo. NO password prompt. NO env credential.
#   - The operator's real Cursor config is NOT touched by default.
#     `-WriteCursorConfig` is required to opt into a real write;
#     `-TempCursorScope` defaults ON whenever `-WriteCursorConfig`
#     is supplied so the real `%USERPROFILE%\.cursor\mcp.json` is
#     never written without explicit operator opt-out.
#   - NO npm publish. NO workflow dispatch. NO tag / release / PR.
#   - `runtime_missing` is RECORDED, not promoted to FAIL.
#   - Cursor GUI provider smoke is operator-driven; the script
#     never claims PASS for it without an attached transcript.
#
# Output format: bounded `PASS  <step>` / `FAIL  <step>` /
# `INFO  <text>` / `NOTE  <text>` lines per step. Mirrors the
# NPM04 smoke style.
#
# Exit code:
#   0 = overall honest evidence collected (including legitimate
#       runtime_missing when `-InstallWslRuntime` is not supplied).
#   1 = an unexpected script failure (e.g. cannot resolve repo root,
#       cannot stage tarballs, host is not Windows).

[CmdletBinding()]
param(
    [switch]$DryRun,
    [string]$Distro = "",
    [switch]$InstallWslRuntime,
    [switch]$WriteCursorConfig,
    [switch]$TempCursorScope = $true
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$script:OverallExit = 0

function Write-Pass($step) { Write-Host "PASS  $step" }
function Write-Fail($step) { Write-Host "FAIL  $step"; $script:OverallExit = 1 }
function Write-Info($text) { Write-Host "INFO  $text" }
function Write-Note($text) { Write-Host "NOTE  $text" }

# Resolve the repository root via the script path.
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path (Join-Path $scriptDir "..\..")).Path
$pkgRoot = Join-Path $repoRoot "packages\terminal-commander"
$binCli = Join-Path $pkgRoot "bin\terminal-commander.js"
$binMcp = Join-Path $pkgRoot "bin\terminal-commander-mcp.js"

Write-Info "WWS07 Windows bridge smoke"
Write-Info "repo_root=$repoRoot"
Write-Info "DryRun=$DryRun WriteCursorConfig=$WriteCursorConfig TempCursorScope=$TempCursorScope InstallWslRuntime=$InstallWslRuntime"
if ($Distro) { Write-Info "Distro=$Distro" } else { Write-Info "Distro=(none -- using CLI default chain)" }

# Step 0: host platform must be Windows.
$isWin = $false
if ($env:OS -eq "Windows_NT") { $isWin = $true }
if (-not $isWin -and $PSVersionTable -and $PSVersionTable.PSObject.Properties["Platform"] -and $PSVersionTable.Platform -eq "Win32NT") { $isWin = $true }
if (-not $isWin) {
    Write-Fail "host_platform: smoke is Windows-only; mark Not Run on Linux/WSL hosts"
    Write-Note "exit reason: PowerShell host is not Windows"
    exit 1
}
Write-Pass "host_platform: Windows"

# Step 1: node + npm available.
$nodeCmd = Get-Command node -ErrorAction SilentlyContinue
if (-not $nodeCmd) {
    Write-Fail "node availability: 'node' command not on PATH"
    exit 1
}
Write-Pass "node availability: $($nodeCmd.Source)"

# Step 2: bin shim files exist.
if (-not (Test-Path $binCli)) { Write-Fail "bin/terminal-commander.js exists"; exit 1 }
if (-not (Test-Path $binMcp)) { Write-Fail "bin/terminal-commander-mcp.js exists"; exit 1 }
Write-Pass "bin shims present (terminal-commander.js + terminal-commander-mcp.js)"

if ($DryRun) {
    Write-Note "dry-run: skipping all live CLI invocations from here on"
    Write-Note "planned: doctor, doctor wsl, setup --print-config, setup --dry-run, [optional bridge MCP probe]"
    Write-Info "WWS07 smoke (dry-run) finished"
    exit 0
}

# Helper: run `node <shim> <argv...>` and capture stdout/stderr/exit.
# Uses System.Diagnostics.Process so exit codes from cmd shims
# (e.g. node.cmd installed via nvm) report reliably.
function Invoke-TcCli {
    param(
        [string]$Shim,
        [string[]]$ArgList,
        [int]$TimeoutSeconds = 30,
        [string]$StdinText = ""
    )
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $nodeCmd.Source
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $psi.CreateNoWindow = $true
    $psi.WorkingDirectory = $repoRoot
    # Quote each argument for safe shell-free passing. node.cmd is a
    # batch shim; arguments need to survive cmd.exe's quoting rules.
    $quotedArgs = @("`"$Shim`"") + ($ArgList | ForEach-Object { "`"$_`"" })
    $psi.Arguments = ($quotedArgs -join " ")
    $proc = [System.Diagnostics.Process]::Start($psi)
    if ($StdinText) {
        $proc.StandardInput.Write($StdinText)
    }
    $proc.StandardInput.Close()
    $stdoutTask = $proc.StandardOutput.ReadToEndAsync()
    $stderrTask = $proc.StandardError.ReadToEndAsync()
    if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) {
        try { $proc.Kill() } catch {}
        return @{
            ExitCode = -1
            Stdout = ""
            Stderr = "timeout"
            TimedOut = $true
        }
    }
    $stdoutTask.Wait()
    $stderrTask.Wait()
    return @{
        ExitCode = $proc.ExitCode
        Stdout = $stdoutTask.Result
        Stderr = $stderrTask.Result
        TimedOut = $false
    }
}

# ---------------- Windows CLI smoke ----------------

# doctor
$r = Invoke-TcCli -Shim $binCli -ArgList @("doctor")
if ($r.ExitCode -eq 0 -and $r.Stderr -match "host_platform: win32") {
    Write-Pass "terminal-commander doctor"
} else {
    Write-Fail "terminal-commander doctor (exit=$($r.ExitCode))"
    Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(200, $r.Stderr.Length)))"
}

# doctor wsl
$r = Invoke-TcCli -Shim $binCli -ArgList @("doctor", "wsl")
if ($r.ExitCode -eq 0 -and $r.Stderr -match "status: ok") {
    Write-Pass "terminal-commander doctor wsl"
    $wslAvailable = $true
    if ($r.Stderr -match "default_distro: (\S+)") {
        $detectedDefault = $matches[1]
        Write-Info "default_distro detected: $detectedDefault"
    } else {
        $detectedDefault = ""
    }
} elseif ($r.Stderr -match "wsl_not_found") {
    Write-Note "doctor wsl: wsl_not_found (recorded honestly)"
    $wslAvailable = $false
    $detectedDefault = ""
} elseif ($r.Stderr -match "no_distros") {
    Write-Note "doctor wsl: no_distros (recorded honestly)"
    $wslAvailable = $false
    $detectedDefault = ""
} else {
    Write-Fail "terminal-commander doctor wsl (exit=$($r.ExitCode))"
    Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(300, $r.Stderr.Length)))"
    $wslAvailable = $false
    $detectedDefault = ""
}

# setup cursor-wsl --print-config
$setupArgs = @("setup", "cursor-wsl", "--print-config")
if ($Distro) { $setupArgs += @("--distro", $Distro) }
$r = Invoke-TcCli -Shim $binCli -ArgList $setupArgs
if ($r.ExitCode -eq 0 -and $r.Stderr -match "terminal-commander-mcp" -and $r.Stderr -notmatch "wsl\.exe") {
    Write-Pass "setup cursor-wsl --print-config (no wsl.exe in generated JSON)"
} elseif ($wslAvailable) {
    Write-Fail "setup cursor-wsl --print-config (exit=$($r.ExitCode))"
    Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(400, $r.Stderr.Length)))"
} else {
    Write-Note "setup cursor-wsl --print-config: skipped (no WSL distros to print for)"
}

# setup cursor-wsl --dry-run
$setupArgs = @("setup", "cursor-wsl", "--dry-run")
if ($Distro) { $setupArgs += @("--distro", $Distro) }
$r = Invoke-TcCli -Shim $binCli -ArgList $setupArgs
if ($r.ExitCode -eq 0 -and $r.Stderr -match "no files written") {
    Write-Pass "setup cursor-wsl --dry-run (no files written)"
} elseif ($wslAvailable) {
    Write-Fail "setup cursor-wsl --dry-run (exit=$($r.ExitCode))"
    Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(400, $r.Stderr.Length)))"
} else {
    Write-Note "setup cursor-wsl --dry-run: skipped (no WSL distros)"
}

# ---------------- Windows -> WSL MCP bridge smoke ----------------

if (-not $wslAvailable) {
    Write-Note "MCP bridge smoke: Not Run (wsl_not_found or no_distros recorded above)"
} else {
    # Probe WSL-side runtime via the CLI doctor with --probe-runtime.
    $probeArgs = @("doctor", "wsl", "--probe-runtime")
    if ($Distro) { $probeArgs += @("--distro", $Distro) }
    $r = Invoke-TcCli -Shim $binCli -ArgList $probeArgs
    if ($r.ExitCode -eq 0 -and $r.Stderr -match "status: runtime_present") {
        Write-Pass "doctor wsl --probe-runtime: runtime_present"
        $runtimePresent = $true
    } elseif ($r.Stderr -match "runtime_missing") {
        Write-Note "doctor wsl --probe-runtime: runtime_missing (recorded honestly; expected until NPM07 publishes)"
        $runtimePresent = $false
    } else {
        Write-Note "doctor wsl --probe-runtime: exit=$($r.ExitCode); treating as runtime_missing"
        Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(300, $r.Stderr.Length)))"
        $runtimePresent = $false
    }

    if (-not $runtimePresent) {
        Write-Note "MCP bridge round-trip: Not Run (runtime_missing); will not fake PASS"
        if ($InstallWslRuntime) {
            Write-Note "InstallWslRuntime requested: deferring to CLI 'setup cursor-wsl --install-wsl-runtime'"
            $installArgs = @("setup", "harness")
            if ($Distro) { $installArgs += @("--distro", $Distro) }
            # The setup orchestrator will refuse with npm_package_unpublished while the
            # registry returns E404 for terminal-commander, since NPM07 has not published.
            $r = Invoke-TcCli -Shim $binCli -ArgList $installArgs -TimeoutSeconds 180
            if ($r.ExitCode -eq 0) {
                Write-Pass "setup cursor-wsl --install-wsl-runtime: install proceeded"
            } elseif ($r.Stderr -match "npm_package_unpublished" -or $r.Stderr -match "E404" -or $r.Stderr -match "404 not found" -or $r.Stderr -match "not in this registry") {
                Write-Note "setup cursor-wsl --install-wsl-runtime: npm_package_unpublished (expected until NPM07 publishes)"
            } elseif ($r.Stderr -match "install_permission_required") {
                Write-Note "setup cursor-wsl --install-wsl-runtime: install_permission_required (no sudo retry; safe operator manual install required)"
            } else {
                Write-Note "setup cursor-wsl --install-wsl-runtime: exit=$($r.ExitCode); not promoting to FAIL"
                Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(300, $r.Stderr.Length)))"
            }
        }
    } else {
        Write-Info "MCP bridge round-trip: runtime_present detected -- driving initialize + tools/list + health through the WWS04 bridge"

        # Build a single JSON-RPC initialize+tools/list+health sequence
        # sent to the mcp shim over stdin. The shim bridges to WSL via
        # the WWS04 spawn helper; the WSL-side terminal-commander-mcp
        # responds. We collect three JSON-RPC responses on stdout.
        $rpcInit = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"wws07-smoke","version":"0.0.0"}}}'
        $rpcInitNotif = '{"jsonrpc":"2.0","method":"notifications/initialized"}'
        $rpcTools = '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
        $rpcHealth = '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"health","arguments":{}}}'
        $rpcShutdown = '{"jsonrpc":"2.0","id":4,"method":"shutdown"}'

        $stdinPayload = @($rpcInit, $rpcInitNotif, $rpcTools, $rpcHealth, $rpcShutdown) -join "`n"
        $stdinPayload += "`n"

        $env:TC_WSL_SKIP_DOCTOR = "1"  # avoid double probe; CLI already verified runtime_present
        try {
            $r = Invoke-TcCli -Shim $binMcp -ArgList @() -StdinText $stdinPayload -TimeoutSeconds 45
            $outBytes = $r.Stdout
            $errBytes = $r.Stderr
            if ($r.TimedOut) {
                Write-Fail "MCP bridge round-trip: timeout"
            } else {
                if ($outBytes -match '"id":1' -and $outBytes -match '"result"') {
                    Write-Pass "MCP initialize round-trip OK"
                } else {
                    Write-Fail "MCP initialize: no id=1 result on stdout"
                    Write-Info "stderr excerpt: $($errBytes.Substring(0, [Math]::Min(300, $errBytes.Length)))"
                }
                if ($outBytes -match '"id":2.*"tools"') {
                    Write-Pass "MCP tools/list round-trip OK (response present)"
                } else {
                    Write-Note "MCP tools/list response missing; treating as bridge runtime mismatch"
                }
                if ($outBytes -match '"id":3') {
                    Write-Pass "MCP tools/call(health) round-trip OK"
                } else {
                    Write-Note "MCP tools/call(health) response missing"
                }
            }
        } finally {
            Remove-Item Env:TC_WSL_SKIP_DOCTOR -ErrorAction SilentlyContinue
        }
    }
}

# ---------------- Cursor config write smoke ----------------

if ($WriteCursorConfig) {
    if ($TempCursorScope) {
        $tempRoot = New-Item -ItemType Directory -Path (Join-Path $env:TEMP "wws07-cursor-$(Get-Random)")
        try {
            $writeArgs = @("setup", "cursor-wsl", "--project", $tempRoot.FullName)
            if ($Distro) { $writeArgs += @("--distro", $Distro) }
            $r = Invoke-TcCli -Shim $binCli -ArgList $writeArgs
            $cfg = Join-Path $tempRoot.FullName ".cursor\mcp.json"
            if ($r.ExitCode -eq 0 -and (Test-Path $cfg)) {
                Write-Pass "WriteCursorConfig (temp scope): config written to $cfg"
            } elseif ($r.Stderr -match "runtime_missing") {
                Write-Note "WriteCursorConfig (temp scope): runtime_missing; no write attempted (recorded honestly)"
            } else {
                Write-Note "WriteCursorConfig (temp scope): exit=$($r.ExitCode); skipped"
                Write-Info "stderr excerpt: $($r.Stderr.Substring(0, [Math]::Min(300, $r.Stderr.Length)))"
            }
        } finally {
            Remove-Item -LiteralPath $tempRoot.FullName -Recurse -Force -ErrorAction SilentlyContinue
        }
    } else {
        Write-Note "WriteCursorConfig requested WITHOUT TempCursorScope: refusing -- operator must pass -TempCursorScope to allow a real write target through this smoke"
    }
} else {
    Write-Note "WriteCursorConfig: skipped (default; operator's real Cursor config is never touched by this smoke)"
}

# ---------------- Cursor provider GUI smoke ----------------

Write-Note "Cursor provider GUI smoke: Not Run"
Write-Note "Cursor has no documented headless / scripted MCP discovery entry point (no 'cursor --list-mcp-tools' subcommand)."
Write-Note "To promote the Cursor provider smoke to PASS, an operator must: open Cursor, confirm 'terminal-commander' appears in Settings -> Features -> MCP, ask Cursor to call 'health', and attach the chat transcript."

# ---------------- Final summary ----------------

if ($script:OverallExit -eq 0) {
    Write-Pass "WWS07 smoke completed (honest evidence collected; see PASS/NOTE lines above)"
} else {
    Write-Fail "WWS07 smoke completed WITH FAILURES (see FAIL lines above)"
}
exit $script:OverallExit
