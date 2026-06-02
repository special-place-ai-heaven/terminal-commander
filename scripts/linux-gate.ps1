#!/usr/bin/env pwsh
# scripts/linux-gate.ps1 — run scripts/linux-gate.sh inside WSL against THIS
# working tree (the code you are about to push), not a separate clone.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot   # repo root (scripts/ is one level down)

$wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
if (-not $wsl) {
  Write-Warning 'WSL not found — cfg(unix) paths were NOT verified locally, only CI will check them.'
  exit 2   # non-zero: skipped != passed
}
$wslPath = (& wsl.exe wslpath -a "$repo") 2>$null
if (-not $wslPath) { Write-Error "tc-gate: wslpath failed for '$repo' (UNC/unmounted drive?) — cannot run the linux gate"; exit 1 }
$wslPath = $wslPath.Trim()

# Pre-flight the toolchain inside WSL with a specific remediation, not a generic cargo error.
& wsl.exe -e bash -lc "command -v cargo >/dev/null && command -v cargo-nextest >/dev/null && command -v node >/dev/null && command -v python3 >/dev/null"
if ($LASTEXITCODE -ne 0) { Write-Error 'tc-gate: WSL missing rustup/cargo-nextest/node/python3 — provision WSL (see CONTRIBUTING) then retry'; exit 1 }

& wsl.exe -e bash -lc "cd '$wslPath' && bash ./scripts/linux-gate.sh"
exit $LASTEXITCODE
