# Spec: Clear Separation of OS-Related Parts

- Date: 2026-06-02
- Status: APPROVED by adversarial agent panel (re-review: all 4 lenses approve-with-changes; prior criticals confirmed fixed; the flagged spec-text corrections have been applied). Ready for implementation planning (Phase 1).
- Scope: Terminal Commander Rust workspace
- Author: special-place-administrator (via Claude)

## Problem

OS-specific code is scattered inline across the workspace as `#[cfg(...)]`
blocks: **179 `cfg(unix | windows | target_os)` occurrences across 39 files**.
Major hotspots (this table covers ~100 of the 179; the remaining ~79 are
diffuse/low-density and are enumerated in the Phase-2 sub-spec, not here):

| Area | File(s) | cfg count |
|---|---|---|
| IPC transport (UDS vs Windows named pipe) | `daemon/src/ipc/server.rs` (20), `ipc/pipe_server.rs` (14), `ipc/mod.rs` (5) | 39 |
| PTY (unix-only) | `daemon/src/ipc/handlers/pty.rs` (12), `probes/src/pty.rs` (2) | 14 |
| Supervisor OS glue | `supervisor/src/paths.rs` (13), `replace.rs` (11), `pidfile.rs` (9) | 33 |
| Process spawn | `probes/src/process.rs` (9) | 9 |
| WSL bridge | `daemon/src/environment/wsl.rs` (4) | 4 |
| Cross-cutting seam (exists, near-empty) | `core/src/platform.rs` (1) | 1 |

### Root pain

OS-specific code is **invisible to whatever OS the dev box is not**. The primary
dev machine is Windows; `cargo test`/`clippy` there do not compile `#[cfg(unix)]`
code; on linux/mac the `#[cfg(windows)]` paths never compile. Each direction has
its own CI gate the other-OS dev cannot run locally. On the trust-hardening PR
(#72) this cost two CI cycles: a clippy error in `#[cfg(unix)]` env tests, then a
`#[cfg(unix)]` integration test broken by a hardened contract — both caught by CI,
late, after a false "green locally."

### Ground truth (read from the actual workflow — this is the authority)

`.github/workflows/npm-binary-build.yml`, job `pre-build-gates`
(name: `pre-build-gates (linux-x64)`, ubuntu-24.04, Rust 1.95.0). Steps in order:

1. `node scripts/release/verify-optional-dependencies.js` (gate step, runs BEFORE
   the toolchain/cache/node infra steps in the current job)
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets -- -D warnings` (no `--all-features`)
4. `cargo nextest run --workspace` (preceded by infra step `taiki-e/install-action@nextest`)
5. TC47 load gate: `cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture`
6. MCP grep guard 1 — a `grep -RE "Command::new|Command::spawn|TcpListener|UdpSocket" ... || true`
   captured into a var, then a SECOND regex post-filter that fails ONLY on
   code-shaped matches (`^crates/mcp/src/...:(let|use|fn|pub|impl|let mut)`).
   It deliberately allows doc/comment matches. Uses `|| true` + `if`-condition.
7. MCP grep guard 2 — a bare `if grep -RE "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src; then exit 1`.

Job `pre-build-gates-windows` (name: `pre-build-gates (windows-x64)`, windows-2022,
Rust 1.95.0) runs, in one `run:` block: `cargo test -p terminal-commander-probes
windows_no_console -- --nocapture` + `cargo test -p terminal-commanderd
windows_spawn_site_coverage -- --nocapture`.

**Required-check set is enforced by the GitHub branch-protection ruleset
`main-protection` (id 17000085), NOT by any committed file.** Verified via
`gh api .../rulesets/17000085`, the required contexts are:
`pre-build-gates (linux-x64)`, `build-linux-x64 / build-linux-x64`,
`build-linux-arm64 / build-linux-arm64`, `build-mac-x64 / build-mac-x64`,
`build-mac-arm64 / build-mac-arm64`, `build-windows-x64 / build-windows-x64`,
`npm-pack (after all builds)`.

**Critical fact: `pre-build-gates (windows-x64)` is NOT in the required set, and
no `build-*` job declares `needs: pre-build-gates-windows` — the windows gate
currently gates nothing and is advisory.** A windows-only regression can merge
red. The required CONTEXT is matched by the job `name:` field (e.g.
`pre-build-gates (linux-x64)`), not the job key — renaming a job silently detaches
its required check.

Toolchain has TWO independent pins: `rust-toolchain.toml` (`channel = "1.95.0"`)
and the workflow's explicit `RUST_TOOLCHAIN: "1.95.0"` env consumed by
`dtolnay/rust-toolchain@master` (CI does NOT read the toml). They agree today but
can drift.

## Goals

1. **Verifiability (primary):** a developer can run the exact `pre-build-gates`
   gate for BOTH target OSes before pushing (linux via WSL on Windows, windows
   natively), catching OS-specific defects on the dev box. This substantially
   reduces — does not, by convention alone, eliminate — recurrence of the PR-#72
   class.
2. **No drift by construction (for deterministic gates):** the gate scripts are
   the single source of truth that CI invokes, so "green from the script" implies
   "green in the corresponding CI gate" for the DETERMINISTIC steps
   (verify-deps, fmt, clippy, nextest, build, grep guards). The TC47 load gate is
   a timing/concurrency stress suite whose pass/fail verdict is environment-
   sensitive (WSL2/drvfs vs native ubuntu-24.04): a local pass is a best-effort
   signal and CI remains authoritative for that one step.
3. **Make the windows gate actually gate:** as part of Phase 1, promote
   `pre-build-gates (windows-x64)` to a required check (ruleset + `needs:` wiring)
   so the both-gates guarantee is real, not advisory.
4. **Code locality (secondary):** OS-specific code behind clear seams instead of
   inline `cfg` sprawl (Phase 2).

## Non-goals

- No full `Platform`-trait dependency-injection abstraction.
- Not folded into the env-overlay fix (0.1.40) or any unrelated release.
- No behavior changes in Phase 1 (scripts + workflow + docs + ruleset only).
- No always-on full-suite git hook (a NARROW conditional hook is Phase 1.5).

## Decision

Safety-net first, then modularize. Review-driven decisions: CI invokes the gate
scripts; both OS gates scripted AND the windows gate made required; convention now
+ narrow conditional pre-push hook as Phase 1.5; Phase-2 transport seam LAST.

---

## Phase 1 — Gate scripts that CI invokes + make-windows-required + convention

### Component 1: `scripts/linux-gate.sh`

`bash` (shebang `#!/usr/bin/env bash`, `set -euo pipefail`). Runs the FULL
`pre-build-gates` step list in CI order. Skeleton — the MCP guards reproduce the
workflow's guard LOGIC/BEHAVIOR, not its echo text: guard 1 = a `grep ... || true`
captured into a var + a SECOND code-vs-doc post-filter that fails only on
code-shaped matches; guard 2 = a bare `if grep ...; then exit 1`. The
`|| true` / `if`-condition structure is load-bearing under `set -euo pipefail`
(a no-match `grep` exits 1, so dropping `|| true` / the `if` false-REDs), and
guard 1's two-stage capture+post-filter MUST NOT be collapsed to a plain grep or
it false-GREENs by failing to distinguish code from doc matches. Because CI now
INVOKES this script, the script's own messages REPLACE the old inline workflow
strings — match the behavior, not the literal echo text:

```bash
#!/usr/bin/env bash
set -euo pipefail
export CARGO_TERM_COLOR=always
export CARGO_TARGET_DIR="${TC_LINUX_TARGET:-$HOME/tc-linux-target}"

# Prechecks — a partial environment must FAIL LOUD, never silently skip a gate.
require() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1 ($2)"; exit 127; }; }
require cargo "install rustup + the pinned toolchain"
require node  "verify-optional-dependencies.js needs node"
require cargo-nextest "cargo install cargo-nextest"
require python3 "the TC47 load gate (load_noise_backpressure) SELF-SKIPS to a false-pass without python3 — this precheck reduces that, but AC2 (assert executed count) is the real guarantee; align this with the test's own /usr/bin|/usr/local/bin|/bin/python3 probe"
# Toolchain fidelity: parse [toolchain].channel from rust-toolchain.toml and
# require the active rustc to match it AND be rustup-managed (so a distro cargo
# on PATH cannot run a different toolchain whose clippy diverges from CI).
exp="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
rustc --version | grep -q "$exp" || { echo "rustc != pinned $exp (is rustup the active cargo?)"; exit 1; }

node scripts/release/verify-optional-dependencies.js   # no npm deps -> safe after node setup
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
# Load gate: the python3 precheck above REDUCES a false-skip, but it is NOT the
# guarantee — the load test does its OWN python3 detection and each #[test]
# early-returns with eprintln "skipping: python3 not on PATH" (a returning test
# = PASS). The real guarantee is AC2: assert a non-zero executed count / fail on
# "skipping: python3" in the captured output. Keep this precheck ALIGNED with the
# test's own detection (it probes /usr/bin/python3, /usr/local/bin/python3,
# /bin/python3 — not just `command -v python3`).
cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture
# MCP guard 1 (capture + post-filter) and guard 2 (bare if-grep) reproduce the
# workflow guard BEHAVIOR, not its echo text; the messages below are the script's
# own (they replace the old inline workflow strings). Do NOT simplify the
# `|| true` / `if` structure or collapse guard 1's two stages.
out=$(grep -RE "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp/src || true)
echo "$out"
if echo "$out" | grep -E "^crates/mcp/src/[^:]+:[ \t]*(let|use|fn|pub|impl|let mut)" >/dev/null; then
  echo "::error::MCP guard 1: non-doc match in production source"; exit 1
fi
if grep -RE "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src; then
  echo "::error::MCP guard 2: direct-fs path"; exit 1
fi
```

- **Dedicated linux-fs target dir** (`$HOME/tc-linux-target`, override `TC_LINUX_TARGET`)
  so a WSL/linux build never clobbers the Windows `target/`.
- **CI invokes this script.** The `pre-build-gates` job keeps EXACTLY these infra
  steps (do not drop any): `actions/checkout@v4`, `dtolnay/rust-toolchain@master`
  (toolchain `${RUST_TOOLCHAIN}`), `Swatinem/rust-cache@v2`, `actions/setup-node@v4`,
  AND `taiki-e/install-action@nextest`. It then replaces the inner gate `run:`
  steps with `run: bash scripts/linux-gate.sh`. The job **name:**
  `pre-build-gates (linux-x64)` and key MUST be preserved verbatim (it is the
  required-check context). Note `verify-optional-dependencies.js` moves from
  pre-toolchain step #1 to inside the script (after node setup) — safe, it
  `require()`s only local files, no `node_modules`.

### Component 2: `scripts/windows-gate.ps1`

Runs the `pre-build-gates-windows` commands natively on the Windows dev box (no
WSL). Symmetric prechecks to Component 1: require the `x86_64-pc-windows-msvc`
target and assert the active rustc == the pinned channel (a partial windows env
must also fail loud, not pass).

```powershell
cargo test -p terminal-commander-probes windows_no_console -- --nocapture
cargo test -p terminal-commanderd windows_spawn_site_coverage -- --nocapture
```

CI's `pre-build-gates-windows` job is refactored to invoke this script (one `run:`
block replaces the current bundled block). Preserve the job **name:**
`pre-build-gates (windows-x64)` verbatim. A linux/mac contributor cannot run
`cfg(windows)` tests locally at all; for them the windows gate is a CI backstop
(documented honestly — and made meaningful by Component 5).

### Component 3: `scripts/linux-gate.ps1` (Windows -> WSL wrapper)

- Resolve the repo root with `wsl.exe wslpath -a` (handles spaces / `/mnt`). If
  `wslpath` fails or returns empty (e.g. a UNC path, a network/virtual drive, or a
  drive not mounted under `/mnt` in WSL), the wrapper MUST error loud with specific
  remediation and exit non-zero — never fall through to an unresolved path.
  Otherwise
  invoke `wsl.exe -e bash -lc "cd '<wslpath>' && bash ./scripts/linux-gate.sh"`,
  forward the exit code.
- **Run against the ACTUAL working tree being pushed** (the `/mnt/e` checkout) by
  default — the linux-fs `CARGO_TARGET_DIR` already mitigates the compile cost, so
  the drvfs penalty is acceptable for a pre-push check. Do NOT default to a
  separate WSL-fs clone: a stale `~/src` clone validates code you are not pushing
  (a false-green by tree divergence). If a WSL-fs clone is used for fast routine
  iteration, the wrapper/doc must require syncing it to the exact commit +
  uncommitted changes before trusting it as a pre-push gate.
- WSL absent -> warn + exit non-zero (skipped != passed). Missing
  rustup/cargo/cargo-nextest/node/python3 INSIDE WSL -> specific remediation. A
  one-time WSL provisioning note (rustup + 1.95.0 toolchain, cargo-nextest, node,
  **python3**) ships in CONTRIBUTING.

### Component 4: Convention (and CORRECT doc reconciliation)

Add the authoritative "OS-specific code" convention to `CONTRIBUTING.md`: when a
change touches `cfg(unix)`/`cfg(windows)`/`target_os` code or any test, run the
gates before pushing — `pwsh scripts/linux-gate.ps1` + `pwsh scripts/windows-gate.ps1`
on Windows, `bash scripts/linux-gate.sh` on linux/mac. Pointer: current OS seams
live in `crates/core/src/platform.rs` + the hotspots above; Phase 2 consolidates
them. `AGENTS.md` is a small "Cursor Cloud specific instructions" file that defers
routine commands to README/CONTRIBUTING/TESTING — do NOT duplicate the convention
there; add a ONE-LINE pointer in `AGENTS.md` to the CONTRIBUTING OS-specific-code
section (and update both in the same commit when the pointer changes).

Reconcile EXISTING drift (grounded — CONTRIBUTING currently MISLABELS this, and
the labels must be CORRECTED, not preserved):
- CONTRIBUTING **mislabels** the seven-step `cargo deny`/`cargo hack`/`cargo machete`
  pipeline as "The canonical CI sequence" (CONTRIBUTING.md:112) and marks
  `cargo-machete`/`cargo-hack` as "required (CI)" (CONTRIBUTING.md:83-84). Both
  labels are FALSE: verified `grep -rn` over `.github/workflows/` finds ZERO
  occurrences of `cargo-deny`/`cargo-hack`/`cargo-machete`/`rust-version` — no
  workflow runs any of them. The reconciliation MUST CORRECT those labels to
  "RECOMMENDED / not yet wired into CI (tracked in
  `docs/research/tooling-baseline.md`)". Do NOT say CONTRIBUTING "already
  attributes them correctly" — it does not.
- The authoritative PR gate is `scripts/linux-gate.sh` (the real `pre-build-gates`),
  NOT the seven-step block. CONTRIBUTING's local-commands / pre-commit guidance
  (CONTRIBUTING.md:104, `cargo nextest run --workspace --profile default
  --no-fail-fast`, no clippy `--all-features`) must be REPOINTED at
  `scripts/linux-gate.sh` so there is one authoritative list. Note the seven-step
  block's clippy uses `--all-features` (CONTRIBUTING.md:120) which the REAL gate
  (`npm-binary-build.yml` pre-build-gates, line 79) does NOT — that seven-step
  block is the aspirational tooling baseline, not the PR gate.
- CONTRIBUTING.md:64 states "Rust 1.92.0 (MSRV floor set by rmcp 1.7.0)" while the
  active pin is 1.95.0 (`rust-toolchain.toml`). Annotate 1.92.0 as the MSRV floor
  that is DOCUMENTED but NOT currently CI-enforced (no workflow runs
  `cargo-hack`/`--rust-version`), and 1.95.0 as the active developer/CI pin
  (`rust-toolchain.toml` + the workflow `RUST_TOOLCHAIN` env).

Note the existing sibling `scripts/dev/verify-baseline.sh` (a separate
fixture/doctrine/secret-scan check, currently carrying a stale
`EXPECTED_BRANCH=feature/terminal-commander-mvp` — evidence orphaned scripts
drift). Co-locate the new gate scripts at `scripts/` top level and state in one
line that `linux-gate.sh` is the CI-invoked correctness gate while
`scripts/dev/verify-baseline.sh` remains the separate fixture check.

### Component 5: Make the windows gate required + re-trigger on script edits

Two workflow/config changes that make the design's guarantees real:
- **Promote `pre-build-gates (windows-x64)` to a required check.** Add it to the
  `main-protection` ruleset (17000085) required contexts AND change
  `build-windows-x64`'s current `needs: pre-build-gates` to
  `needs: [pre-build-gates, pre-build-gates-windows]` (ADD the windows gate; do NOT
  drop the existing linux dep) so a red windows gate blocks the merge. Today it
  gates nothing — without this, the both-gates rationale is half-broken. (Ruleset
  edit is out-of-repo GitHub config; call it out as a named manual step with the
  `gh api` command.) Also update the comment in `release-pr-sync.yml:34-35` that
  enumerates the required set as "pre-build-gates + 5 build-* + npm-pack" — it omits
  the windows gate and must be updated once the windows gate is required.
- **Add the gate scripts to the workflow `paths:` filter.** `npm-binary-build.yml`
  only triggers on `Cargo.toml`, `Cargo.lock`, `crates/**`, `packages/**`,
  `scripts/release/**`, `scripts/smoke/**`, and the two workflow files — so a PR
  editing ONLY `scripts/linux-gate.sh` would NOT re-run the gate that now depends
  on it. Add `scripts/linux-gate.sh`, `scripts/windows-gate.ps1`,
  `scripts/linux-gate.ps1` (or `scripts/*.sh` + `scripts/*.ps1`) to BOTH `paths:`
  blocks. Otherwise the single source of truth is editable without CI re-validating it.
- **Enforce LF on the shell scripts via `.gitattributes`.** The repo has NO
  `.gitattributes` today and the Windows checkout working tree is CRLF
  (`core.autocrlf=true`; e.g. `scripts/dev/verify-baseline.sh` carries `^M`
  terminators). A CRLF `linux-gate.sh` fails under WSL (`bash` chokes on the `\r`),
  and `cargo fmt --check` can flag CRLF in tracked text. Phase 1 MUST add a
  `.gitattributes` that forces LF for `scripts/*.sh` (and a sane default such as
  `* text=auto eol=lf` for the repo generally), and the new gate scripts MUST be
  committed with LF endings.

### Phase 1.5 (fast-follow)

A narrow pre-push hook that runs the gate ONLY when the pushed commit range
touches `cfg`/test files AND WSL is present — skipping docs-only pushes and
WSL-less machines with a warning. NOTE: a pre-push hook does NOT see a staged diff;
git feeds it the push ref-range on stdin (lines of
`<local-ref> <local-sha> <remote-ref> <remote-sha>`), so the hook computes the
changed files across that commit range (e.g. `git diff --name-only
<remote-sha>..<local-sha>`, handling the all-zeros remote-sha for a new branch) and
greps those paths for `cfg`/test files. Deferred so the scripts stabilize before
auto-enforcement.

### Phase 1 acceptance criteria

- AC1: `scripts/linux-gate.sh` exists (bash, `set -euo pipefail`), runs the full
  step list in CI order with the dedicated target dir, prechecks (incl. `python3`
  and the rust-toolchain-channel assertion), and the EXACT two MCP guards; exits
  non-zero on any failure.
- AC2: On a known-green commit, the script exits 0 with fmt/clippy clean,
  nextest all-pass, **the load gate actually EXECUTED its tests (not "skipping:
  python3"; non-zero executed count)**, and both guards passing. Evidence: nextest
  summary + load-gate executed-count + clippy/fmt exit 0.
- AC3: `scripts/windows-gate.ps1` runs both windows tests, asserts a non-zero
  executed-test count (a zero-match filter / cfg-skipped harness must FAIL, not
  pass), and exits non-zero on failure (verified on the Windows dev box).
- AC4: `scripts/linux-gate.ps1` runs the sh script through WSL via `wslpath`
  against the actual working tree, forwards the exit code, warns + exits non-zero
  when WSL absent.
- AC5: CI's `pre-build-gates` and `pre-build-gates-windows` jobs invoke the
  scripts while keeping ALL infra steps (incl. `install-action@nextest`) and the
  job `name:` fields verbatim. A green CI run preserves the gate command set +
  pass/fail semantics (step ordering may change). Verify on the PR's required-check
  contexts tab that both `pre-build-gates (linux-x64)` and
  `pre-build-gates (windows-x64)` are still listed/green after the refactor.
- AC6: `CONTRIBUTING.md` carries the authoritative OS-specific-code convention
  referencing the scripts and `AGENTS.md` carries a ONE-LINE pointer to it (not a
  duplicated section); CONTRIBUTING's local/pre-commit block repoints at
  `scripts/linux-gate.sh`; the toolchain 1.92.0-vs-1.95.0 drift is annotated
  (1.92.0 = MSRV floor, documented but NOT CI-enforced; 1.95.0 = active pin); the
  deny/hack/machete seven-step block and the "required (CI)" tool labels are
  CORRECTED to "RECOMMENDED / not wired into CI (tooling-baseline.md)", not left as
  "the canonical CI sequence."
- AC7: `pre-build-gates (windows-x64)` is a required check (ruleset) and
  `build-windows-x64` declares `needs: [pre-build-gates, pre-build-gates-windows]`
  (both deps present, linux dep NOT dropped); the `release-pr-sync.yml:34-35`
  required-set comment is updated to include the windows gate; the gate scripts are
  in both `paths:` blocks (a gate-script-only PR triggers the gate jobs).
- AC8: One toolchain authority — add a check (or doc) that the workflow
  `RUST_TOOLCHAIN` env equals `rust-toolchain.toml` `[toolchain].channel`.
- AC9: No production `crates/**/src` code changed in Phase 1.

---

## Phase 2 — Per-crate `os/` modules (outline only; own sub-spec)

Consolidate inline `cfg` sprawl into per-subsystem seams, one at a time, phased to
<=5 files each, every phase gated by the Phase-1 gates. Behavior-preserving PER
SEAM (not a blanket claim). Recommended order — lowest cross-OS risk first,
transport LAST:

1. Process spawn — `probes/src/process.rs` into `process/os.rs`.
2. Supervisor OS glue — `paths.rs`/`pidfile.rs`/`replace.rs` into `supervisor/os/{unix,windows}.rs`.
3. PTY — unix-only behind a `#[cfg(unix)]` module + a Windows `Unsupported` stub.
4. `core/platform.rs` — grow into the cross-cutting platform seam.
5. IPC transport (LAST) — UDS vs Windows named pipe into `ipc/transport/{unix,windows}.rs`
   behind a shared seam (trait vs enum is a Phase-2 decision, NOT pre-committed).
   Highest behavior-change risk + weakest local verification (linux gate exercises
   only UDS; named pipe only via the windows gate). Its sub-spec MUST require
   behavior-preservation evidence on BOTH branches before merge. The Phase-2
   sub-spec also enumerates the ~79 cfg occurrences outside the hotspot table so
   the ordering is grounded in the full surface.

## Sequencing

1. Phase 1 on `feature/os-separation`: scripts + CI refactor (both gate jobs
   invoke scripts, names preserved, infra intact, paths filter updated) +
   make-windows-required (ruleset + needs:) + convention/doc reconciliation.
   Verify: linux gate in WSL against the working tree (load gate EXECUTED),
   windows gate natively, a green CI run showing both required contexts intact.
2. Phase 1.5 narrow hook after Phase 1 merges.
3. Phase 2 per-subsystem, transport last.

## Risks and mitigations

- **WSL absent/unprovisioned.** `.ps1` fails loud + specific remediation;
  provisioning note covers rustup/toolchain/nextest/node/**python3**; CI backstop.
- **Script <-> CI drift.** Eliminated for the command set by CI invoking the
  scripts (AC5) + the paths-filter re-trigger (AC7). The job `name:` must be
  preserved or the required-check context detaches.
- **Toolchain drift (two pins).** AC8 ties workflow `RUST_TOOLCHAIN` to
  `rust-toolchain.toml`; the script asserts the active rustc matches the channel.
- **Load gate is environment-sensitive** (timing/concurrency; WSL2/drvfs vs
  ubuntu-24.04). A WSL pass is best-effort; CI is authoritative for that step
  (Goal 2). The `python3` precheck REDUCES the false-skip but does NOT guarantee
  execution — the load test does its OWN python3 detection and self-skips with a
  PASSing returning `#[test]`. AC2 (assert non-zero executed count / fail on
  "skipping: python3" in captured output) is the authoritative guarantee; keep the
  precheck ALIGNED with the test's own python3 probe.
- **Windows gate was advisory.** Component 5 makes it required so the both-gates
  guarantee holds; until the ruleset edit lands, a red windows gate does not block.
- **drvfs slowness / tree divergence.** Default = same working tree (penalty
  mitigated by linux-fs target dir); a WSL-fs clone must be commit-synced or it is
  a false-green source.
- **CRLF line endings on the shell gate.** No `.gitattributes` exists and the
  Windows checkout is CRLF, so a committed `linux-gate.sh` would carry `\r` and
  fail under WSL `bash` (and `cargo fmt --check` can flag CRLF). Mitigation: Phase 1
  adds a `.gitattributes` forcing LF for `scripts/*.sh` (and a repo-wide
  `text=auto eol=lf` default) and commits the gate scripts with LF.

## Evidence / verification for implementation

- AC2: run `scripts/linux-gate.sh` in WSL on HEAD; paste fmt/clippy exit 0,
  nextest summary, load-gate EXECUTED-count (not skipped), guards pass.
- AC3/AC4: run `windows-gate.ps1` + `linux-gate.ps1` on Windows; show exit codes
  + executed-test counts + the WSL-absent path.
- AC5/AC7: a green CI run on the Phase-1 PR; inspect the required-check contexts
  showing both `pre-build-gates (...)` names; confirm a script-only edit
  re-triggers the jobs; confirm `build-windows-x64` declares
  `needs: [pre-build-gates, pre-build-gates-windows]` (both deps).
- AC6/AC8: convention present in both docs; CONTRIBUTING drift fixed; toolchain
  pins reconciled.
