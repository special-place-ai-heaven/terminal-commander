# Spec: Clear Separation of OS-Related Parts

- Date: 2026-06-02
- Status: approved (design), pending spec review
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

OS-specific code — especially `#[cfg(unix)]` tests and the Windows named-pipe
transport — is **invisible to a single-OS dev box**. The primary dev machine is
Windows; `cargo test` / `cargo clippy` there do **not** compile `#[cfg(unix)]`
code, so unix-only defects only surface in CI-linux. On the trust-hardening PR
(#72) this cost **two CI cycles**:

1. A `clippy::literal_string_with_formatting_args` error in `#[cfg(unix)]` env
   tests (never linted on Windows).
2. A `#[cfg(unix)]` integration test (`daemon_unavailable_envelope`) broken by a
   hardened contract (`scope` now required) — never run on Windows.

CI already runs the correct linux gate (`pre-build-gates`: `cargo fmt --check` +
`cargo clippy --workspace --all-targets -- -D warnings` + `cargo nextest run
--workspace`). The gap is **not** CI coverage — CI caught both. The gap is that
the check runs **late** (after push), so cycles were wasted and "green locally"
was a false claim.

## Goals

1. **Verifiability (primary):** a reliable, low-friction way to run the exact
   linux gate **locally before pushing**, so OS-specific defects are caught on
   the dev box and the blind spot cannot recur.
2. **Code locality (secondary):** OS-specific code lives behind clear seams /
   dedicated modules instead of inline `cfg` sprawl, so each OS's behavior is in
   one readable place and changes are isolated.

## Non-goals

- No full `Platform`-trait dependency-injection abstraction. Over-engineering for
  a 2-OS (+WSL) target; the indirection cost exceeds the benefit.
- Not folded into the in-flight 0.1.39 trust-hardening PR. This is its own
  branch / PR (scope discipline).
- No behavior changes. Phase 2 relocation is strictly behavior-preserving.
- No mandatory git hook that runs the full suite on every push (too slow; would
  punish docs-only pushes and break on machines without WSL).

## Decision

Two phases, sequenced **safety-net first, then modularize**:

- **Phase 1 (this spec, implement now):** local linux gate + convention.
- **Phase 2 (outlined here, separate spec when started):** per-crate `os/`
  module consolidation.

Phase 1 is the load-bearing fix — it kills recurrence. Phase 2 is cleanliness and
depends on Phase 1 being in place (every Phase-2 relocation is verified by the
Phase-1 gate before push).

---

## Phase 1 — Local linux gate + convention

### Component 1: `scripts/linux-gate.sh`

A POSIX `sh` script that mirrors CI's `pre-build-gates` **exactly**, so that
"green from this script" implies "green in CI":

```sh
set -eu
export CARGO_TERM_COLOR=always
export CARGO_TARGET_DIR="${TC_LINUX_TARGET:-$HOME/tc-linux-target}"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

- **Dedicated target dir** (`$HOME/tc-linux-target`, overridable via
  `TC_LINUX_TARGET`) so a WSL/linux build never clobbers the Windows `target/`
  (mixed-OS artifacts in one `target/` cause churn/corruption). Overridable so
  callers can point it at fast storage.
- Runs **natively** on linux/mac (just cargo); on Windows it is invoked through
  WSL by the `.ps1` wrapper.
- Exact-mirror principle: if CI's `pre-build-gates` step list changes, this script
  changes with it. The script is the single source of "what the linux gate is."
- Requires `cargo-nextest`; if absent, the script prints the one-line install
  hint (`cargo install cargo-nextest`) and exits non-zero (does not silently fall
  back to `cargo test`, which would diverge from CI).

### Component 2: `scripts/linux-gate.ps1`

A thin Windows wrapper for dev-box ergonomics:

- Detects WSL (`wsl.exe -l -q`).
- If present: runs `wsl.exe -e bash -lc "cd <repo> && scripts/linux-gate.sh"`,
  forwarding the exit code.
- If **absent**: prints a clear warning — *"WSL not found; this change's
  OS-specific (cfg(unix)) paths were NOT verified locally and will only be checked
  in CI"* — and exits **non-zero**. Honesty over a silent pass: the caller learns
  the gate did not actually run, rather than mistaking "skipped" for "passed."

### Component 3: Convention

Add an **"OS-specific code"** section to both `AGENTS.md` and `CONTRIBUTING.md`:

- The rule: *When a change touches `cfg(unix)` / `cfg(windows)` / `target_os`
  code, or any test, run the linux gate before pushing* —
  `pwsh scripts/linux-gate.ps1` on Windows, `scripts/linux-gate.sh` on linux/mac.
- The why: *A Windows-only `cargo test` / `cargo clippy` does NOT compile
  `#[cfg(unix)]` code. Claiming "tests pass" from a single OS is false for the
  other OS's paths.*
- A pointer to where OS seams live (and, post-Phase-2, will live), so contributors
  know where to put new OS-specific code instead of adding inline `cfg`.

AGENTS.md is the agent-facing copy (this project is agent-driven); CONTRIBUTING.md
is the human copy. Same rule, both audiences.

### Phase 1 acceptance criteria

- `scripts/linux-gate.sh` exists, is executable, runs the three steps with the
  dedicated target dir, and exits non-zero on any step failure.
- Running `scripts/linux-gate.sh` in WSL on a known-green commit reproduces CI's
  `pre-build-gates` result (clippy clean, `nextest run --workspace` all pass, fmt
  clean) — evidence: paste the final `nextest` summary line.
- `scripts/linux-gate.ps1` runs the sh script through WSL and forwards the exit
  code; with WSL unavailable it warns and exits non-zero (not 0).
- `AGENTS.md` and `CONTRIBUTING.md` each contain the OS-specific-code convention
  with the exact gate command.
- No production code changed in Phase 1 (scripts + docs only).

---

## Phase 2 — Per-crate `os/` modules (outline only)

Recorded here so Phase 1 does not paint us into a corner. Phase 2 gets its **own
spec** when started. Consolidate inline `cfg` sprawl into dedicated per-subsystem
seams, tackled one subsystem at a time, **phased to <=5 files each**, every phase
gated by the Phase-1 linux gate before push. Strictly behavior-preserving.

Target seams, in recommended order (biggest locality win first):

1. **IPC transport** — the largest fork. A `Transport` seam splitting the Unix
   domain socket and Windows named-pipe implementations into
   `ipc/transport/{unix,windows}.rs` behind a shared trait/enum, collapsing the
   inline `cfg` in `ipc/server.rs` (20), `pipe_server.rs` (14), `ipc/mod.rs` (5).
2. **PTY** — unix-only. Consolidate `handlers/pty.rs` (12) + `probes/pty.rs`
   behind a `#[cfg(unix)]` module with a single `Unsupported`/no-op Windows stub
   seam (honest `UnsupportedPlatform` already exists from the trust-hardening
   discovery work).
3. **Supervisor OS glue** — `paths.rs` (13), `pidfile.rs` (9), `replace.rs` (11)
   into `supervisor/os/{unix,windows}.rs`.
4. **Process spawn** — `probes/src/process.rs` (9: `windows_silent`, env handling,
   spawn) into a small `process/os.rs` seam.
5. **`core/platform.rs`** — grow the existing near-empty seam into the home for
   cross-cutting platform constants/helpers shared across crates.

Phase 2 success shape (for its future spec): the inline `cfg` count drops
sharply, each OS implementation is readable in one file per subsystem, and the
public behavior is identical (verified by the unchanged test suite through the
Phase-1 gate).

## Sequencing

1. Phase 1 implemented on `feature/os-separation`, verified by running the gate
   itself in WSL, opened as its own PR, merged independently of 0.1.39.
2. Phase 2 begins only after Phase 1 is merged (so each relocation can be gated
   locally). Phase 2 is itself decomposed into per-subsystem sub-phases, each its
   own small PR.

## Risks and mitigations

- **WSL not present on a contributor machine.** Mitigation: the `.ps1` wrapper
  fails loud (non-zero + explicit "not verified locally"), and CI remains the
  backstop. The gate is a convention, not a hard hook, so it never *blocks* a
  contributor who lacks WSL — it just tells the truth about what was checked.
- **Script drifts from CI.** Mitigation: the script is declared the single source
  of the gate's step list; any CI `pre-build-gates` change updates the script in
  the same PR. (A future hardening could have CI invoke the script directly so
  they cannot diverge — noted, not in scope for Phase 1.)
- **Cold WSL build is slow** (~10-20 min first run; ~1-2 min warm). Accepted: the
  gate is run on OS-touching changes before push, not on every keystroke; the
  dedicated warm target dir keeps repeat runs fast.

## Evidence / verification for this spec's implementation

- Phase 1: run `scripts/linux-gate.sh` in WSL on HEAD; paste clippy result +
  `nextest run --workspace` summary (must be 0 failed) + fmt `--check` exit 0.
- Confirm `linux-gate.ps1` WSL-present and WSL-absent behavior (exit codes).
- Confirm the convention text is present in both `AGENTS.md` and `CONTRIBUTING.md`.
