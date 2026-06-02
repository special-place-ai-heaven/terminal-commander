#!/usr/bin/env pwsh
# scripts/windows-gate.ps1 — the windows pre-build-gates, identical to CI.
# CI (npm-binary-build.yml pre-build-gates-windows) INVOKES this. Run natively on
# Windows. Fails loud on a partial env and refuses a 0-tests-run false pass.
$ErrorActionPreference = 'Stop'
$env:CARGO_TERM_COLOR = 'always'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { Write-Error 'tc-gate: cargo not found'; exit 127 }
$exp = (Select-String -Path 'rust-toolchain.toml' -Pattern '^\s*channel\s*=\s*"(.*)"').Matches.Groups[1].Value
if (-not $exp) { Write-Error 'tc-gate: cannot read channel from rust-toolchain.toml'; exit 1 }
if (-not ((rustc --version) -match [regex]::Escape($exp))) { Write-Error "tc-gate: rustc != pinned $exp"; exit 1 }
if (-not ((rustup target list --installed) -match 'x86_64-pc-windows-msvc')) { Write-Error 'tc-gate: msvc target missing'; exit 1 }

function Invoke-Gate([string]$pkg, [string]$filter, [string[]]$extra) {
  Write-Host "== $pkg $filter $extra =="
  $out = & cargo test -p $pkg $filter -- --nocapture @extra 2>&1 | Tee-Object -Variable _ | Out-String
  Write-Host $out
  if ($LASTEXITCODE -ne 0) { Write-Error "tc-gate: $pkg $filter FAILED"; exit 1 }
  if ($out -notmatch '(\d+) passed' -or [int]($out | Select-String '(\d+) passed').Matches.Groups[1].Value -lt 1) {
    Write-Error "tc-gate: $pkg $filter ran 0 tests — refusing false pass"; exit 1
  }
}
# crates/probes/tests/windows_no_console_spawn.rs marks all 3 regression tests
# #[ignore] (their AttachConsole probe depends on the runner's console session),
# with the documented run mode "CI-only via cargo test -- --ignored". Without
# --include-ignored the filter matches 0 NON-ignored tests, which would trip the
# >=1-passed assertion below as a false-green. --include-ignored runs both the
# normal and ignored tests, so this genuinely EXECUTES the console regression.
Invoke-Gate 'terminal-commander-probes' 'windows_no_console' @('--include-ignored')
# crates/daemon/tests/windows_spawn_site_coverage.rs tests are NOT ignored.
Invoke-Gate 'terminal-commanderd' 'windows_spawn_site_coverage' @()
Write-Host 'tc-gate: windows gate PASSED'
