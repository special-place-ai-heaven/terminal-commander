# Spec: Clear Separation of OS-Related Parts

- Date: 2026-06-02
- Status: revised after agent doc-review (4 lenses, 26 findings); pending final spec review
- Scope: Terminal Commander Rust workspace
- Author: special-place-administrator (via Claude)

## Problem

OS-specific code is scattered inline across the workspace as `#[cfg(...)]`
blocks: **179 `cfg(unix | windows | target_os)` occurrences across 39 files**.
Hotspots:

| Area | File(s) | cfg count |
|---|---|---|
| IPC transport (UDS vs Windows named pipe) | `daemon/src/ipc/server.rs` (20), `ipc/pipe_server.rs` (14), `ipc/mod.rs` (5) | 39 |
| PTY (unix-only) | `daemon/src/ipc/handlers/pty.rs` (12), `probes/src/pty.rs` (2) | 14 |
| Supervisor OS glue | `supervisor/src/paths.rs` (13), `replace.rs` (11), `pidfile.rs` (9) | 33 |
| Process spawn | `probes/src/process.rs` (9: `windows_silent`, env, spawn) | 9 |
| WSL bridge | `daemon/src/environment/wsl.rs` (4) | 4 |
| Cross-cutting seam (exists, near-empty) | `core/src/platform.rs` (1) | 1 |

### Root pain (what drives this work)

OS-specific code is **invisible to whatever OS the dev box is not**. The primary
dev machine is Windows; `cargo test` / `cargo clippy` there do **not** compile
`#[cfg(unix)]` code. Symmetrically, on a linux/mac box the `#[cfg(windows)]`
paths never compile. Each direction has a dedicated, **required** CI gate the
other-OS dev cannot run locally:

- **linux gate** (`pre-build-gates`, ubuntu-24.04): the unix-compiled lint/test
  surface.
- **windows gate** (`pre-build-gates-windows`, windows-2022): the
  `windows_no_console` + `windows_spawn_site_coverage` regressions (ROB-6
  AttachConsole spawn).

On the trust-hardening PR (#72) this cost **two CI cycles**: a
`clippy::literal_string_with_formatting_args` error in `#[cfg(unix)]` env tests
(never linted on Windows), then a `#[cfg(unix)]` integration test broken by a
hardened contract (never run on Windows). Both were caught by CI — late, after
push, after a false "green locally" claim.

### The actual CI gate (ground truth)

Read from `.github/workflows/npm-binary-build.yml`. This is the authority the
local gate must match — NOT a remembered subset.

`pre-build-gates` (ubuntu-24.04, Rust 1.95.0) runs, in order:

1. `node scripts/release/verify-optional-dependencies.js`
2. `cargo fmt --all --check`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo nextest run --workspace`
5. TC47 load gate: `cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture`
6. MCP grep guard 1: no `Command::new|Command::spawn|TcpListener|UdpSocket` as
   non-doc lines in `crates/mcp/src`
7. MCP grep guard 2: no `tokio::fs|std::fs|File::open|read_to_string|read_to_end`
   in `crates/mcp/src`

`pre-build-gates-windows` (windows-2022, Rust 1.95.0) runs:

- `cargo test -p terminal-commander-probes windows_no_console -- --nocapture`
- `cargo test -p terminal-commanderd windows_spawn_site_coverage -- --nocapture`

Required check set (per `release-pr-sync.yml`): `pre-build-gates` +
`pre-build-gates-windows` are gates; then 5 `build-*` + `npm-pack`.

A 3-step (fmt/clippy/nextest) local script would pass while CI fails on the load
gate or MCP guards — the exact wasted-cycle failure this spec exists to kill. The
earlier draft of this spec made that error (it was written from the CI failure
log, not the workflow file). This revision is grounded in the workflow.

## Goals

1. **Verifiability (primary):** a developer can run the **exact** `pre-build-gates`
   gate for BOTH target OSes before pushing — the unix gate via WSL on a Windows
   box, the windows gate natively — so OS-specific defects are caught on the dev
   box. This substantially reduces (does not, by convention alone, eliminate)
   recurrence of the PR-#72 class of failure.
2. **No drift, by construction:** the gate scripts are the **single source of
   truth** that CI itself invokes, so "green from the script" implies "green in
   the corresponding CI gate" because they run the identical commands.
3. **Code locality (secondary):** OS-specific code lives behind clear seams /
   dedicated modules instead of inline `cfg` sprawl (Phase 2).

## Non-goals

- No full `Platform`-trait dependency-injection abstraction (over-engineering for
  a 2-OS + WSL target).
- Not folded into the 0.1.39 trust-hardening release (already shipped; this is its
  own branch/PR).
- No behavior changes in Phase 1. Phase 2 relocation is behavior-preserving
  per-seam (with the transport seam explicitly flagged — see Phase 2).
- No always-on full-suite git hook. (A NARROW conditional hook is a Phase-1.5
  fast-follow — see below.)

## Decision

Two phases, **safety-net first, then modularize**, with three decisions taken
during review:

- **CI invokes the gate scripts** (single source of truth; drift impossible).
- **Both gates** are scripted and runnable locally (close both OS blind spots).
- **Convention now; narrow pre-push hook as Phase 1.5** fast-follow.

---

## Phase 1 — Local gates that CI invokes + convention

### Component 1: `scripts/linux-gate.sh`

A `bash` script (shebang `#!/usr/bin/env bash`, `set -euo pipefail` — pipefail
matters because the MCP guards pipe `grep` output) that runs the **full**
`pre-build-gates` step list, in CI order:

```bash
#!/usr/bin/env bash
set -euo pipefail
export CARGO_TERM_COLOR=always
export CARGO_TARGET_DIR="${TC_LINUX_TARGET:-$HOME/tc-linux-target}"

# Toolchain fidelity: CI pins Rust 1.95.0 via rust-toolchain.toml. Fail loud if
# the active toolchain is not rustup-managed at the pinned channel, else clippy
# results can diverge from CI and look like they "ran".
require() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1 ($2)"; exit 127; }; }
require cargo "install rustup + the pinned toolchain"
require node "install node (verify-optional-dependencies.js needs it)"
require cargo-nextest "cargo install cargo-nextest"
# (assert rustc version matches rust-toolchain.toml channel; fail on mismatch)

node scripts/release/verify-optional-dependencies.js
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture
# MCP grep guard 1 + guard 2 (verbatim from the workflow)
```

- **Single source of truth:** the workflow's `pre-build-gates` job is refactored
  to `run: bash scripts/linux-gate.sh` (CI keeps the infra steps — checkout,
  `dtolnay/rust-toolchain`, `rust-cache`, `setup-node` — then calls the script).
  Local and CI run the identical gate commands; divergence is impossible.
- **Dedicated target dir** (`$HOME/tc-linux-target`, overridable via
  `TC_LINUX_TARGET`) so a WSL/linux build never clobbers the Windows `target/`.
- **Prechecks** fail loud with the exact remediation when `cargo`/`node`/
  `cargo-nextest` are absent or the toolchain channel mismatches — so a partial
  environment cannot produce a misleading green.
- Runs natively on linux/mac; on Windows it is invoked through WSL by the `.ps1`.

### Component 2: `scripts/windows-gate.ps1`

Runs the `pre-build-gates-windows` commands natively on the Windows dev box (no
WSL needed for this half):

```powershell
cargo test -p terminal-commander-probes windows_no_console -- --nocapture
cargo test -p terminal-commanderd windows_spawn_site_coverage -- --nocapture
```

CI's `pre-build-gates-windows` job is refactored to invoke this script (same
single-source-of-truth principle). It runs natively on the Windows-primary dev
box. A contributor on linux/mac cannot run `cfg(windows)` tests locally at all;
for them the windows gate stays a CI backstop (documented honestly).

### Component 3: `scripts/linux-gate.ps1` (Windows -> WSL wrapper)

- Resolves the repo root and converts it with `wsl.exe wslpath -a` (handles spaces
  and `/mnt/…` translation) rather than string-interpolating a Windows path.
- Invokes `wsl.exe -e bash -lc "cd '<wslpath>' && bash ./scripts/linux-gate.sh"`,
  forwarding the exit code.
- WSL absent -> prints *"WSL not found; cfg(unix) paths were NOT verified locally,
  only in CI"* and exits **non-zero** (honest: skipped != passed).
- Missing `rustup`/`cargo`/`cargo-nextest`/`node` **inside** WSL -> emits the
  specific remediation (not a generic cargo error). A one-time WSL provisioning
  note (rustup + 1.95.0 toolchain honoring `rust-toolchain.toml`, `cargo-nextest`,
  node) ships in CONTRIBUTING so first-run cost is not hidden.
- drvfs note: building from `/mnt/e` (Windows fs) is materially slower than the
  linux fs. The doc recommends cloning into the WSL filesystem (`~/src/...`) for
  routine gate runs, or accepting the penalty; `CARGO_TARGET_DIR` is already on
  the linux fs to avoid the worst of it.

### Component 4: Convention (and doc reconciliation)

Add an **"OS-specific code"** section to both `AGENTS.md` and `CONTRIBUTING.md`
with **identical text** (sync rule: update both in the same commit; AGENTS.md is
the agent copy, CONTRIBUTING.md the human copy):

- Rule: *When a change touches `cfg(unix)` / `cfg(windows)` / `target_os` code, or
  any test, run the gate before pushing* — `pwsh scripts/linux-gate.ps1` (unix
  half via WSL) and `pwsh scripts/windows-gate.ps1` (windows half) on the Windows
  dev box; `bash scripts/linux-gate.sh` on linux/mac.
- Why: *a single-OS `cargo test`/`cargo clippy` does not compile the other OS's
  `cfg` paths; "tests pass" from one OS is false for the other.*
- Pointer: *current OS seams live in `crates/core/src/platform.rs` and the
  inline `cfg` hotspots above; Phase 2 will consolidate them into per-crate `os/`
  modules.*

**Reconcile existing drift (same PR):** CONTRIBUTING.md already documents gate
commands that do NOT match the real PR gate — a "7-step CI sequence"
(`cargo deny`/`cargo hack`/`cargo machete`) that is actually the **release** gate
(`release-please.yml`), and a clippy line with `--all-features` the PR gate does
not use. Fix/annotate these so there is exactly one authoritative step list (the
scripts), and docs reference the scripts by command rather than re-listing steps.

### Phase 1.5 (fast-follow, after the scripts prove out)

A **narrow** pre-push hook that runs the gate ONLY when (1) the staged diff
touches `cfg`/test files AND (2) WSL is present — skipping docs-only pushes and
WSL-less machines with a warning. This sidesteps every objection to an always-on
hook while enforcing the convention for the exact change class that burned PR #72.
Deferred to its own small change so the scripts stabilize before auto-enforcement.

### Phase 1 acceptance criteria

- AC1: `scripts/linux-gate.sh` exists (bash, `set -euo pipefail`), runs the full
  `pre-build-gates` step list in CI order with the dedicated target dir and
  prechecks, and exits non-zero on any step failure.
- AC2: On a known-green commit, `scripts/linux-gate.sh` in WSL exits **0** with
  clippy clean, `nextest run --workspace` all-pass, fmt `--check` clean, the TC47
  load gate passing, and both MCP guards passing. Evidence: paste the nextest
  summary line + clippy/fmt exit 0 + load-gate result.
- AC3: `scripts/windows-gate.ps1` runs both windows regression tests and exits
  non-zero on failure (verified on the Windows dev box).
- AC4: `scripts/linux-gate.ps1` runs the sh script through WSL via `wslpath`,
  forwards the exit code, and with WSL absent warns + exits non-zero.
- AC5: CI's `pre-build-gates` and `pre-build-gates-windows` jobs invoke the
  scripts (a green CI run confirms the refactor preserved the gate exactly).
- AC6: `AGENTS.md` and `CONTRIBUTING.md` carry the identical OS-specific-code
  convention; CONTRIBUTING's stale "7-step CI sequence" + `--all-features` clippy
  drift are fixed/annotated; docs reference the scripts.
- AC7: No production `crates/**/src` code changed in Phase 1 (scripts + workflow +
  docs only).

---

## Phase 2 — Per-crate `os/` modules (outline only)

Recorded so Phase 1 does not paint us into a corner. Phase 2 gets its **own
spec**. Consolidate inline `cfg` sprawl into per-subsystem seams, one subsystem at
a time, **phased to <=5 files each**, every phase gated by the Phase-1 gates
before push. Behavior-preserving **per seam** (not asserted as a blanket — see the
transport caveat).

Recommended order — **lowest cross-OS risk first**, transport LAST (reversed from
the earlier draft, per review):

1. **Process spawn** — `probes/src/process.rs` (9) into a small `process/os.rs`
   seam (`windows_silent`, env, spawn). Low risk; mostly mechanical.
2. **Supervisor OS glue** — `paths.rs` (13), `pidfile.rs` (9), `replace.rs` (11)
   into `supervisor/os/{unix,windows}.rs`.
3. **PTY** — unix-only; `handlers/pty.rs` (12) + `probes/pty.rs` behind a
   `#[cfg(unix)]` module with a single Windows `Unsupported` stub seam.
4. **`core/platform.rs`** — grow the existing seam into the home for cross-cutting
   platform constants/helpers.
5. **IPC transport (LAST)** — the largest fork (UDS vs Windows named pipe). A
   shared **seam** (design TBD in the Phase 2 spec — trait vs enum is a Phase-2
   decision, not pre-committed here) splitting the implementations into
   `ipc/transport/{unix,windows}.rs`. Done LAST because it carries the highest
   behavior-change risk (error mapping, connection lifecycle, partial-read
   semantics differ between transports) and the WEAKEST local verification: the
   linux gate exercises only the UDS branch; the named-pipe branch is compiled +
   tested only by the windows gate. Its Phase-2 sub-spec MUST require
   behavior-preservation evidence on **both** branches (linux UDS via the linux
   gate AND windows named pipe via the windows gate) before merge.

## Sequencing

1. Phase 1 on `feature/os-separation`: write scripts, refactor both CI gate jobs
   to invoke them, add the convention + reconcile docs. Verify by running the
   linux gate in WSL and the windows gate natively, plus a green CI run proving
   the CI refactor preserved the gates. Own PR, merged independently.
2. Phase 1.5 (narrow hook) after Phase 1 merges and the scripts prove out.
3. Phase 2 begins only after Phase 1, decomposed per-subsystem (each its own small
   PR), transport last.

## Risks and mitigations

- **WSL absent / unprovisioned on a dev machine.** The `.ps1` fails loud
  (non-zero + "not verified locally") and emits specific remediation for missing
  rustup/toolchain/nextest/node; CONTRIBUTING carries a one-time provisioning
  note. CI remains the backstop. The gate is a convention (+ Phase-1.5 narrow
  hook), never a hard block for a WSL-less contributor.
- **Script <-> CI drift.** Eliminated by construction: CI invokes the scripts
  (Goal 2 / AC5). There is no separately-maintained mirror to drift.
- **Toolchain mismatch in WSL** (distro `cargo` vs rustup 1.95.0). The script
  asserts the active toolchain matches `rust-toolchain.toml` and fails loud.
- **Cold/drvfs slowness** (~10-20 min first run from `/mnt/e`; faster from the
  linux fs). Mitigated by the dedicated linux-fs target dir + the
  clone-into-WSL recommendation; accepted because the gate runs on OS-touching
  changes before push, not continuously.
- **Linux/mac contributor cannot run the windows gate locally.** Accepted
  residual: documented honestly; the windows gate stays a CI backstop for them
  (the dev-box reality here is Windows-primary, where both halves run locally).

## Evidence / verification for this spec's implementation

- AC2: run `scripts/linux-gate.sh` in WSL on HEAD; paste clippy/fmt exit 0,
  nextest `--workspace` summary (0 failed), load-gate pass, guards pass.
- AC3/AC4: run `windows-gate.ps1` and `linux-gate.ps1` on the Windows dev box;
  show exit codes (incl. WSL-absent path).
- AC5: a green CI run on the Phase-1 PR confirms both refactored gate jobs.
- AC6: convention text present in both docs; CONTRIBUTING drift fixed.
