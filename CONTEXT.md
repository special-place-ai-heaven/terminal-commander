# Terminal Commander — Runtime Observability

Terms for how the daemon exposes live work to callers through IPC/MCP snapshots.

## Language

**Probe**:
A single live or lingering runtime binding (command job, PTY job, or file watch) identified by `probe_id`, `job_id`, and `bucket_id`.
_Avoid_: task, session (unless explicitly a shell session)

**Unified runtime view**:
The aggregate snapshot built by `collect_probes` and returned via `runtime_state`, `probe_list`, and `probe_status`.
_Avoid_: PTY list, operator list

**Dedicated PTY list**:
The operator-facing snapshot from `pty_command_list` showing only non-terminal PTY jobs.
_Avoid_: unified view, probe list

**Unified-view contract**:
After starting a live PTY, the unified runtime view must include that PTY as a `ProbeKind::Pty` row and increment `pty_jobs`; lingering terminal PTYs may remain visible after exit.
_Avoid_: view parity with `pty_command_list`

**View parity**:
Requiring two listing APIs to return the same job set and filtering rules.
_Avoid_: unified-view contract (these are different goals)

## Relationships

- The **unified runtime view** aggregates command, PTY, and file-watch **probes** in one list
- The **dedicated PTY list** reads the same PTY registry but applies a live-only filter
- **`probe_list`** is a flat projection of the same **probes** as **`runtime_state`** (without bucket/rule aggregates)

## Example dialogue

> **Dev:** "T1 should make Windows match `pty_command_list`, right?"
> **Domain expert:** "No — match the **unified-view contract**. Same PTY accessor, not **view parity**. A exited PTY can appear in `runtime_state` but not in `pty_command_list`; that's intentional."

## Flagged ambiguities

- Goal doc wording "exactly the way `pty_command_list`" — **resolved**: means same Windows-capable accessor (`state.pty.list()`), not the same filtered job set. Fix is cfg widen only (`#[cfg(unix)]` → `#[cfg(any(unix, windows))]`), no body change.

## Regression test (Q2 — resolved)

**Windows unified-view regression test**:
A `#[cfg(windows)]` integration test beside the other runtime-view contract tests that asserts the **unified-view contract** after `PtyCommandStart` on an isolated daemon — not view parity with `pty_command_list`.
_Avoid_: duplicating the Unix aggregate test, placing it in platform-availability tests

**Test assertions**:
Prefer `pty_jobs >= 1` plus a matching `ProbeKind::Pty` row when job set is not fully controlled; use `pty_jobs == 1` when the daemon state is fresh and isolated.
_Avoid_: exact-count assertions against a shared or leaky daemon

**Test teardown**:
Any spawned PTY child must be stopped/killed before test exit so CI does not accumulate stray processes.

## Regression guard layers (Q3 — C-minus, resolved)

**Cfg sentinel**:
A source-level test scoped to `collect_probes` that asserts the PTY enumeration block is reachable on Windows (cfg admits `windows`, not `#[cfg(unix)]`-only). Tolerant of formatting; self-documented as guarding T1 because headless CI cannot rely on live ConPTY.
_Avoid_: exact byte-string match, line-number anchors, deleting as "nonsense grep"

**Live unified-view test**:
IPC path `PtyCommandStart` → `RuntimeState` on fresh isolated daemon; assert `pty_jobs == 1` and matching `ProbeKind::Pty` `probe_id`. Child: `ping -n 60 127.0.0.1` (native; `cmd` is ShellInterpreterDenied). Skip loudly on ConPTY DLL-init / `UnsupportedPlatform` (mirror `conpty_e2e`).
_Avoid_: view parity with `pty_command_list`, Node dependency, silent skip

**Merge gate wiring**:
Required CI on Windows runs `scripts/windows-gate.ps1` only — not `cargo test --workspace`. The cfg sentinel must be invoked from `windows-gate.ps1` so gap #1 is enforced before merge; the live test may remain workspace-only with environmental skip.

## T1 PR scope (Q4 — resolved)

All three touch points are one change plus enforcement, in-scope for T1:
- `runtime.rs` — cfg widen (production fix)
- `runtime_state_windows.rs` — sentinel + live unified-view test
- `windows-gate.ps1` — register **sentinel test binary only** (not full workspace test on Windows)

PR description must record the scope expansion beyond goal §4's file list and justify it as non-regression enforcement, not production sprawl.

## Acceptance contract (Q5 — resolved)

Platform-asymmetric daemon surface fixes use a split bar:

1. **Static sentinel** — merge-gated via `windows-gate.ps1`; catches cfg re-narrowing headlessly.
2. **Live dogfood** — required author step with evidence (`T1_BUG_PRESENT NO`, `pty_jobs == 1`, matching `probe_id`); catches behavioral breaks CI cannot run.
3. **`UNVERIFIED` live step** — permitted only with an explicit stated reason in Known gaps; never silent; reviewers bounce PRs with unjustified UNVERIFIED.

For this T1 PR on a Windows host with a working harness, live PASS is mandatory.
