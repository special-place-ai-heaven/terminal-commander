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

# NOTE on the test selector. The CI command (npm-binary-build.yml) is
#   cargo test -p <pkg> <name> -- --nocapture
# where <name> is the TEST FILE/BINARY name (windows_no_console,
# windows_spawn_site_coverage). But cargo's POSITIONAL argument is a test
# *function-name* substring filter — and NONE of the actual #[test] fn names
# contain those file-name strings, so the CI command matches 0 tests and passes
# VACUOUSLY. That is the exact "looks green but never ran" false-pass this gate
# exists to kill. To genuinely EXECUTE the regressions we select by binary with
# `--test <binary>` (which runs every test in that integration target).
function Invoke-Gate([string]$pkg, [string]$bin, [string[]]$extra) {
  Write-Host "== $pkg --test $bin $extra =="
  $out = & cargo test -p $pkg --test $bin -- --nocapture @extra 2>&1 | Tee-Object -Variable _ | Out-String
  Write-Host $out
  if ($LASTEXITCODE -ne 0) { Write-Error "tc-gate: $pkg --test $bin FAILED"; exit 1 }
  if ($out -notmatch '(\d+) passed' -or [int]($out | Select-String '(\d+) passed').Matches.Groups[1].Value -lt 1) {
    Write-Error "tc-gate: $pkg --test $bin ran 0 tests — refusing false pass"; exit 1
  }
}
# crates/probes/tests/windows_no_console_spawn.rs runs by default (the ConPTY fix
# replaced the session-fragile AttachConsole-HWND probe with a poll-for-window
# detector and removed the #[ignore]s). --include-ignored is kept defensively so a
# future re-ignored windows regression still EXECUTES here rather than skipping to
# a false green; the >=1-passed assertion below refuses a 0-tests-run pass.
Invoke-Gate 'terminal-commander-probes' 'windows_no_console_spawn' @('--include-ignored')
# crates/daemon/tests/windows_spawn_site_coverage.rs tests are NOT ignored.
Invoke-Gate 'terminal-commanderd' 'windows_spawn_site_coverage' @()

# F-010 / O-07: live ConPTY child-output + secret-gate e2e. Runs on GitHub Actions
# (required pre-build-gates-windows) and when a developer opts in with
# TC_CONPTY_E2E=1. Refuse a vacuous pass if the opt-in tests self-skip (headless
# DLL-init) — same false-green guard as the linux load gate's python3 detector.
$runConptyE2e = ($env:GITHUB_ACTIONS -eq 'true') -or ($env:TC_CONPTY_E2E -eq '1')
if ($runConptyE2e) {
  if ($env:GITHUB_ACTIONS -eq 'true') { $env:TC_CONPTY_E2E = '1' }
  Write-Host '== ConPTY live e2e (TC_CONPTY_E2E=1) =='
  $conptyOut = & cargo test -p terminal-commander-probes conpty_ -- --nocapture 2>&1 | Tee-Object -Variable _ | Out-String
  Write-Host $conptyOut
  if ($LASTEXITCODE -ne 0) { Write-Error 'tc-gate: ConPTY e2e FAILED'; exit 1 }
  if ($conptyOut -match 'SKIP conpty_repl_produces_bounded_combed_output' -or
      $conptyOut -match 'SKIP conpty_secret_prompt_sets_active_flag_and_denies_write') {
    Write-Error 'tc-gate: ConPTY child-output e2e self-SKIPPED — refusing false pass (ConPTY children did not initialize on this host)'
    exit 1
  }
  if ($conptyOut -notmatch '(\d+) passed' -or [int]($conptyOut | Select-String '(\d+) passed').Matches.Groups[1].Value -lt 3) {
    Write-Error 'tc-gate: ConPTY e2e ran fewer than 3 tests — refusing false pass'
    exit 1
  }
} else {
  Write-Host '== ConPTY live e2e: skipped locally (CI runs with GITHUB_ACTIONS; set TC_CONPTY_E2E=1 to opt in) =='
}

Write-Host 'tc-gate: windows gate PASSED'
