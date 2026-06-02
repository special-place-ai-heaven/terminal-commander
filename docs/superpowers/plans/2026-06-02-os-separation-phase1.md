# OS-Separation Phase 1 (Local Gates + Make-Windows-Required) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give developers a one-command way to run the EXACT CI `pre-build-gates` for both OSes before pushing (linux via WSL, windows natively), with CI invoking the same scripts so they cannot drift — and make the windows gate an actually-required check.

**Architecture:** Two gate scripts (`scripts/linux-gate.sh`, `scripts/windows-gate.ps1`) hold the gate command lists; a `scripts/linux-gate.ps1` wrapper runs the linux gate through WSL on a Windows box. `npm-binary-build.yml`'s two `pre-build-gates*` jobs are refactored to INVOKE the scripts (keeping all infra steps + job `name:` fields). The windows gate is wired `needs:` + added to the branch-protection ruleset so it gates merges. Docs (`CONTRIBUTING.md` authoritative, `AGENTS.md` pointer) reference the scripts; existing doc drift is corrected. `.gitattributes` forces LF so the shell scripts run under WSL.

**Tech Stack:** bash, PowerShell 7 (pwsh), WSL2 (Ubuntu, cargo/rustc 1.95.0, cargo-nextest), GitHub Actions, `gh` CLI / ruleset API.

**Source spec:** `docs/superpowers/specs/2026-06-02-os-separation-design.md` (APPROVED). Read its "Ground truth" section before starting — it enumerates the exact CI steps this plan reproduces.

**Branch:** `feature/os-separation`. Commit after each task.

**SCOPE:** Phase 1 only. Phase 1.5 (narrow pre-push hook) and Phase 2 (per-crate `os/` modules) are OUT OF SCOPE.

**Cross-cutting verification rule:** the bug class this exists to kill is "looks green but a real check never ran." So every gate must FAIL LOUD on a missing tool and PROVE its asserting steps EXECUTED (non-zero test count), never skip-to-pass.

---

## Task 1: Enforce LF line endings (prereq — shell scripts must be LF under WSL)

**Files:**
- Create: `.gitattributes`

- [ ] **Step 1: Create `.gitattributes`**

```gitattributes
# Default: normalize text to LF in the repo; let git decide eol on checkout.
* text=auto
# Shell scripts MUST be LF or they fail under bash/WSL (\r treated as part of args).
*.sh text eol=lf
# PowerShell scripts are fine as LF too; keep them consistent.
*.ps1 text eol=lf
```

- [ ] **Step 2: Verify the repo has no existing CRLF shell script that will now flip**

Run: `git ls-files --eol -- '*.sh' | grep -i crlf` (PowerShell: `git ls-files --eol -- '*.sh' | Select-String crlf`)
Expected: it may list `scripts/dev/verify-baseline.sh` (currently CRLF). That is fine — it normalizes to LF on the next touch; do NOT renormalize the whole repo in this task.

- [ ] **Step 3: Commit**

```bash
git add .gitattributes
git commit -m "chore: enforce LF for .sh/.ps1 (gate scripts must be LF under WSL)"
```

---

## Task 2: `scripts/linux-gate.sh` — the linux pre-build-gates, runnable locally + invoked by CI

**Files:**
- Create: `scripts/linux-gate.sh`

This reproduces the `pre-build-gates` GATE COMMANDS (NOT the GitHub-Actions infra
steps — those stay in the workflow). It fails loud on any missing tool so it can
never skip-to-green. The MCP guards reproduce the workflow's BEHAVIOR (guard 1 =
capture + code-vs-doc post-filter with `|| true`; guard 2 = bare `if grep`).

- [ ] **Step 1: Write the script (complete, LF)**

```bash
#!/usr/bin/env bash
# scripts/linux-gate.sh — the linux pre-build-gates gate, identical to CI.
# CI (npm-binary-build.yml pre-build-gates) INVOKES this script, so it IS the
# gate; running it locally (natively on linux/mac, or via WSL on Windows) is a
# faithful pre-push check. Fails loud on a missing tool — never skips to green.
set -euo pipefail
export CARGO_TERM_COLOR=always
export CARGO_TARGET_DIR="${TC_LINUX_TARGET:-$HOME/tc-linux-target}"

require() { command -v "$1" >/dev/null 2>&1 || { echo "tc-gate: missing '$1' — $2" >&2; exit 127; }; }
require cargo         "install rustup + the pinned toolchain"
require node          "verify-optional-dependencies.js needs node"
require cargo-nextest "cargo install cargo-nextest"
# python3: the TC47 load test SELF-SKIPS (and a skipped #[test] reports PASS)
# when python3 is absent. The load test probes /usr/bin|/usr/local/bin|/bin for
# python3, so a bare PATH 'python3' is the right precheck. This REDUCES the
# false-skip; Step-2 asserts the load tests actually executed (the real guard).
require python3       "the TC47 load gate self-skips to a false pass without python3"

# Toolchain fidelity: parse [toolchain].channel from rust-toolchain.toml and
# require the active rustc to match it (so a distro 'cargo' on PATH cannot run a
# different toolchain whose clippy diverges from CI).
exp="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\(.*\)"/\1/p' rust-toolchain.toml)"
[ -n "$exp" ] || { echo "tc-gate: could not read channel from rust-toolchain.toml" >&2; exit 1; }
rustc --version | grep -q "$exp" || { echo "tc-gate: rustc != pinned $exp (is rustup the active cargo?)" >&2; exit 1; }

echo "== verify-optional-dependencies =="; node scripts/release/verify-optional-dependencies.js
echo "== fmt ==";    cargo fmt --all --check
echo "== clippy =="; cargo clippy --workspace --all-targets -- -D warnings
echo "== nextest =="; cargo nextest run --workspace
echo "== TC47 load gate =="
# Assert the load tests EXECUTED (no python3 self-skip slipping through). nextest
# prints a summary line "Summary [..] N tests run: N passed"; require N>0 and no skip line.
# Use `cargo test` (matches the CI load-gate step exactly); parse "running N tests".
# The self-skip is an early-return that STILL counts as a passed test, so the
# eprintln "skipping: python3" line on stderr is the authoritative skip detector.
load_out="$(cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture 2>&1)"; echo "$load_out"
echo "$load_out" | grep -qiE "skipping: python3" && { echo "tc-gate: load gate SELF-SKIPPED (python3) — false pass refused" >&2; exit 1; }
echo "$load_out" | grep -qE "running [1-9][0-9]* test" || { echo "tc-gate: load gate ran 0 tests — refusing false pass" >&2; exit 1; }

echo "== MCP guard 1 (no spawn/socket in crates/mcp/src) =="
out="$(grep -RE "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp/src || true)"
echo "$out"
if echo "$out" | grep -E "^crates/mcp/src/[^:]+:[[:space:]]*(let|use|fn|pub|impl|let mut)" >/dev/null; then
  echo "tc-gate: MCP guard 1 — non-doc match in production source" >&2; exit 1
fi
echo "== MCP guard 2 (no direct fs in crates/mcp/src) =="
if grep -RE "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src; then
  echo "tc-gate: MCP guard 2 — direct-fs path" >&2; exit 1
fi
echo "tc-gate: linux gate PASSED"
```

- [ ] **Step 2: Make it executable + confirm LF**

Run (bash): `chmod +x scripts/linux-gate.sh && file scripts/linux-gate.sh`
Expected: no "CRLF" in the description.

- [ ] **Step 3: Run the gate in WSL against the working tree (the real test)**

Run (PowerShell): `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && bash ./scripts/linux-gate.sh"`
Expected: ends with `tc-gate: linux gate PASSED`; the "TC47 load gate" section shows `N tests run` with N>0 (NOT "skipping: python3"). If WSL lacks python3/nextest, the precheck exits 127 with the named tool — install it (provisioning) and re-run.

- [ ] **Step 4: Negative check — prove it fails loud on a missing tool**

Run (bash): `PATH=/usr/bin TC_LINUX_TARGET=$HOME/tc-linux-target bash -c 'command -v python3 >/dev/null || echo NO-PY'` then temporarily confirm the `require python3` branch exits 127 by running the script with a shimmed empty PATH for python3 (or read-review the require() line). Expected: exit 127, message `tc-gate: missing 'python3'`.

- [ ] **Step 5: Commit**

```bash
git add scripts/linux-gate.sh
git commit -m "feat(ci): scripts/linux-gate.sh — local+CI linux pre-build-gates, fail-loud"
```

---

## Task 3: `scripts/windows-gate.ps1` — the windows pre-build-gates, runnable locally + invoked by CI

**Files:**
- Create: `scripts/windows-gate.ps1`

Runs the two windows regression tests natively, with symmetric prechecks and an
executed-count assertion (both targets are file-level `#![cfg(windows)]`, so on a
non-Windows host or with a renamed test the filter matches 0 tests = false green —
this asserts a non-zero run count).

- [ ] **Step 1: Write the script (complete, LF)**

```powershell
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

function Invoke-Gate([string]$pkg, [string]$filter) {
  Write-Host "== $pkg $filter =="
  $out = & cargo test -p $pkg $filter -- --nocapture 2>&1 | Tee-Object -Variable _ | Out-String
  Write-Host $out
  if ($LASTEXITCODE -ne 0) { Write-Error "tc-gate: $pkg $filter FAILED"; exit 1 }
  if ($out -notmatch '(\d+) passed' -or [int]($out | Select-String '(\d+) passed').Matches.Groups[1].Value -lt 1) {
    Write-Error "tc-gate: $pkg $filter ran 0 tests — refusing false pass"; exit 1
  }
}
Invoke-Gate 'terminal-commander-probes' 'windows_no_console'
Invoke-Gate 'terminal-commanderd' 'windows_spawn_site_coverage'
Write-Host 'tc-gate: windows gate PASSED'
```

- [ ] **Step 2: Run it natively (the real test)**

Run (PowerShell): `pwsh -File scripts/windows-gate.ps1`
Expected: ends `tc-gate: windows gate PASSED`; each section shows `>=1 passed`.
NOTE: `windows_no_console_spawn` tests may be `#[ignore]`d in CI (run via `-- --ignored`); confirm the filter actually executes a non-ignored test, else add `-- --include-ignored` to match how CI runs them. Read `crates/probes/tests/windows_no_console_spawn.rs` to confirm the run mode before trusting the count.

- [ ] **Step 3: Commit**

```bash
git add scripts/windows-gate.ps1
git commit -m "feat(ci): scripts/windows-gate.ps1 — local+CI windows pre-build-gates, fail-loud"
```

---

## Task 4: `scripts/linux-gate.ps1` — WSL wrapper for Windows devs

**Files:**
- Create: `scripts/linux-gate.ps1`

- [ ] **Step 1: Write the wrapper (complete, LF)**

```powershell
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
```

- [ ] **Step 2: Run it (the real test)**

Run (PowerShell): `pwsh -File scripts/linux-gate.ps1`
Expected: forwards to the WSL gate; ends with `tc-gate: linux gate PASSED` and exit 0. (If WSL absent on a machine: exits 2 with the warning — verify that branch by reading the code.)

- [ ] **Step 3: Commit**

```bash
git add scripts/linux-gate.ps1
git commit -m "feat(ci): scripts/linux-gate.ps1 — WSL wrapper running the linux gate on the working tree"
```

---

## Task 5: Refactor `npm-binary-build.yml` to INVOKE the scripts (no drift) + paths filter

**Files:**
- Modify: `.github/workflows/npm-binary-build.yml`

Read the current file first (`pre-build-gates` ~L55-101, `pre-build-gates-windows`
~L103-122, `paths:` ~L20-40, `build-windows-x64` ~L124-143). The refactor REPLACES
the inner gate `run:` steps with a single script invocation while KEEPING every
infra step and the job `name:` fields verbatim (the `name:` is the required-check
context — renaming silently detaches it).

- [ ] **Step 1: `pre-build-gates` — replace the gate `run:` steps with the script**

Keep these infra steps unchanged: `actions/checkout@v4`, `dtolnay/rust-toolchain@master`
(toolchain `${{ env.RUST_TOOLCHAIN }}`, components clippy,rustfmt), `Swatinem/rust-cache@v2`,
`actions/setup-node@v4`, AND `taiki-e/install-action@nextest`. Remove the individual
`cargo fmt`/`clippy`/`nextest`/load-gate/guard-1/guard-2 `run:` steps and the
`verify-optional-dependencies.js` step, replacing ALL of them with one step:

```yaml
      - name: pre-build gates (scripts/linux-gate.sh)
        run: bash scripts/linux-gate.sh
```

Do NOT change `name: pre-build-gates (linux-x64)` or `runs-on: ubuntu-24.04`.
(Note: `verify-optional-dependencies.js` now runs INSIDE the script, after
`setup-node`. Safe — it `require()`s only local files, no `node_modules`.)

- [ ] **Step 2: `pre-build-gates-windows` — replace the bundled `run:` with the script**

Keep `actions/checkout@v4`, `dtolnay/rust-toolchain@master` (msvc target),
`Swatinem/rust-cache@v2`. Replace the `ROB-6 Windows spawn regression` run block with:

```yaml
      - name: pre-build gates (scripts/windows-gate.ps1)
        shell: pwsh
        run: ./scripts/windows-gate.ps1
```

Do NOT change `name: pre-build-gates (windows-x64)`.

- [ ] **Step 3: Add the gate scripts to BOTH `paths:` blocks**

In the `push:` and `pull_request:` `paths:` lists, add:

```yaml
      - "scripts/linux-gate.sh"
      - "scripts/windows-gate.ps1"
      - "scripts/linux-gate.ps1"
```

(Without this, a PR editing ONLY a gate script would not re-run the gate that now
depends on it.)

- [ ] **Step 4: Validate the YAML**

Run: `python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/npm-binary-build.yml')); print('yaml ok')"`
Expected: `yaml ok`.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/npm-binary-build.yml
git commit -m "ci: pre-build-gates jobs invoke scripts/{linux,windows}-gate (single source of truth) + paths filter"
```

---

## Task 6: Wire the windows gate as a blocking dependency

**Files:**
- Modify: `.github/workflows/npm-binary-build.yml` (`build-windows-x64` job)
- Modify: `.github/workflows/release-pr-sync.yml` (stale required-set comment, ~L34-35)

- [ ] **Step 1: `build-windows-x64` needs both gates**

Change its `needs: pre-build-gates` to:

```yaml
    needs: [pre-build-gates, pre-build-gates-windows]
```

(ADD the windows gate; do NOT drop the linux dep. This makes a red windows gate
block the windows build — and thus the `build-windows-x64` required check.)

- [ ] **Step 2: Fix the stale comment in release-pr-sync.yml**

Update the ~L34-35 comment that enumerates "pre-build-gates + 5 build-* + npm-pack"
to also mention `pre-build-gates (windows-x64)` now that it gates.

- [ ] **Step 3: Validate YAML + commit**

Run: `python -c "import yaml; [yaml.safe_load(open(f)) for f in ['.github/workflows/npm-binary-build.yml','.github/workflows/release-pr-sync.yml']]; print('ok')"`
```bash
git add .github/workflows/npm-binary-build.yml .github/workflows/release-pr-sync.yml
git commit -m "ci: build-windows-x64 needs the windows gate; fix release-pr-sync required-set comment"
```

---

## Task 7: Make `pre-build-gates (windows-x64)` a REQUIRED check (ruleset) — HUMAN-APPROVAL GATE

**Files:** none in-repo — this edits GitHub branch-protection ruleset `main-protection` (id 17000085), an OUTWARD config op.

- [ ] **Step 1: STOP — get explicit human approval.** This changes branch
      protection on `main`. Do NOT run the PATCH without the user saying go.

- [ ] **Step 2: Read the current ruleset**

Run: `gh api repos/special-place-ai-heaven/terminal-commander/rulesets/17000085`
Expected: the `required_status_checks` rule lists 7 contexts WITHOUT `pre-build-gates (windows-x64)`.

- [ ] **Step 3: Add the windows gate context (after approval)**

PATCH the ruleset's `required_status_checks` parameters to add the context
`pre-build-gates (windows-x64)` (preserve the existing 7). Use
`gh api -X PUT repos/.../rulesets/17000085 --input <edited.json>` (GitHub rulesets
require the full object on update — fetch, add the one context, PUT back).

- [ ] **Step 4: Verify**

Run: `gh api repos/special-place-ai-heaven/terminal-commander/rulesets/17000085 | python -c "import sys,json; d=json.load(sys.stdin); print([c['context'] for r in d['rules'] if r['type']=='required_status_checks' for c in r['parameters']['required_status_checks']])"`
Expected: the list now includes `pre-build-gates (windows-x64)`.

(No commit — this is GitHub config, not repo state.)

---

## Task 8: Convention + doc reconciliation

**Files:**
- Modify: `CONTRIBUTING.md`
- Modify: `AGENTS.md`

- [ ] **Step 1: CONTRIBUTING — add the authoritative "OS-specific code" section**

Add a section: when a change touches `cfg(unix)`/`cfg(windows)`/`target_os` code
or any test, run the gates before pushing — `pwsh scripts/linux-gate.ps1` +
`pwsh scripts/windows-gate.ps1` on Windows; `bash scripts/linux-gate.sh` on
linux/mac. State that `scripts/linux-gate.sh` IS the PR gate CI runs (single
source of truth), and `scripts/dev/verify-baseline.sh` remains a separate
fixture/doctrine check. Include the one-time WSL provisioning list: rustup +
1.95.0 toolchain, cargo-nextest, node, **python3**.

- [ ] **Step 2: CONTRIBUTING — correct the drift**

- The seven-step `cargo deny`/`cargo hack`/`cargo machete` pipeline (CONTRIBUTING
  ~L112 "The canonical CI sequence"; tools ~L83-84 "required (CI)") is NOT wired
  into any workflow — relabel it RECOMMENDED / not-yet-in-CI (tracked in
  `docs/research/tooling-baseline.md`).
- Repoint the local/pre-commit guidance (~L104) at `scripts/linux-gate.sh`.
- Toolchain (~L64): 1.92.0 is the MSRV floor (documented, NOT CI-enforced — no
  `cargo hack --rust-version` gate exists); 1.95.0 is the active dev/CI pin
  (`rust-toolchain.toml`).

- [ ] **Step 3: AGENTS.md — one-line pointer**

Add a single line under its existing content pointing to CONTRIBUTING's
"OS-specific code" section (do NOT duplicate the full convention).

- [ ] **Step 4: Verify the claims still hold + commit**

Run: `grep -rniE "cargo.hack|cargo.deny|cargo.machete|rust-version" .github/workflows/` (expect: no matches — confirms the "not in CI" relabel is correct).
```bash
git add CONTRIBUTING.md AGENTS.md
git commit -m "docs: OS-specific-code gate convention; correct CONTRIBUTING CI/toolchain drift"
```

---

## Task 9: AC8 — tie the two toolchain pins

**Files:**
- Modify: `scripts/linux-gate.sh` (add a pin-consistency check) OR add a tiny check step.

- [ ] **Step 1: Add a pin-consistency assertion**

In `scripts/linux-gate.sh`, after the rustc-channel check, assert the workflow's
`RUST_TOOLCHAIN` matches `rust-toolchain.toml`'s channel so the two pins cannot
silently drift:

```bash
wf_pin="$(grep -E 'RUST_TOOLCHAIN:' .github/workflows/npm-binary-build.yml | head -1 | sed -E 's/.*"([0-9.]+)".*/\1/')"
[ "$wf_pin" = "$exp" ] || { echo "tc-gate: workflow RUST_TOOLCHAIN ($wf_pin) != rust-toolchain.toml ($exp)" >&2; exit 1; }
```

- [ ] **Step 2: Re-run the linux gate (Task 2 Step 3) to confirm still PASSED**

- [ ] **Step 3: Commit**

```bash
git add scripts/linux-gate.sh
git commit -m "ci: linux-gate asserts workflow RUST_TOOLCHAIN == rust-toolchain.toml channel"
```

---

## Task 10: Full dual-OS verification + PR (the acceptance gate)

- [ ] **Step 1: Linux gate (WSL) on HEAD** — `pwsh -File scripts/linux-gate.ps1`. Expected: `linux gate PASSED`, load gate `N>0 tests run` (NOT skipped).
- [ ] **Step 2: Windows gate (native)** — `pwsh -File scripts/windows-gate.ps1`. Expected: `windows gate PASSED`, each section `>=1 passed`.
- [ ] **Step 3: Push branch + open PR** (own PR; conventional title). The PR's CI is the real proof the refactor preserved the gates.
- [ ] **Step 4: Confirm on the PR checks tab** that BOTH `pre-build-gates (linux-x64)` and `pre-build-gates (windows-x64)` contexts are present + green, the build matrix passes, and (after Task 7) `pre-build-gates (windows-x64)` shows as a REQUIRED check.
- [ ] **Step 5: Confirm a script-only change re-triggers** — verify the paths filter by checking that the PR (which adds the scripts) ran the gate jobs.
- [ ] **Step 6: Merge per the usual auto-merge flow (human-authorized).**

**Acceptance (maps to spec AC1-AC9):**
- AC1/AC2: linux gate exists, fails loud, load gate EXECUTED (>0). [Task 2, Task 10.1]
- AC3: windows gate asserts >0 executed. [Task 3, Task 10.2]
- AC4: WSL wrapper runs on the working tree, warns+nonzero when WSL absent. [Task 4]
- AC5: CI invokes the scripts, infra + job `name:` preserved, green run. [Task 5, Task 10.3-4]
- AC6: convention in CONTRIBUTING + AGENTS pointer; drift corrected. [Task 8]
- AC7: windows gate required (ruleset) + `needs:` both + scripts in paths. [Task 6, Task 7, Task 5.3]
- AC8: toolchain-pin tie. [Task 9]
- AC9: no `crates/**/src` changed. [verify: `git diff --stat origin/main... -- crates | grep src` is empty]
