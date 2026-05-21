# Process Cleanup: Children, Signals, and Reaping

Topic: C3
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: medium-high

## Recommendation

For each child process spawned by a probe:

1. Place the child in its own process group via `setpgid` immediately
   after fork (in practice: use `process-wrap` with `ProcessGroup`,
   which wraps this idiom safely).
2. On shutdown or `command_send_signal`, send the signal to the
   process group via `killpg(pgid, signum)`, not to a single PID.
3. Use a SIGTERM-then-SIGKILL ladder with a configurable timeout
   (default 5 seconds).
4. Reap children with `tokio::process::Child::wait`. Do NOT install
   a custom global SIGCHLD handler in addition; tokio's process
   driver already handles SIGCHLD when children are spawned via
   `tokio::process::Command`.
5. Treat the daemon process itself as the parent of all probe
   children; do not orphan probes.

## Why a process group

The child of a probe may fork further (e.g. `cargo test` spawns
rustc + test binaries; `npm install` spawns node + scripts). Killing
just the immediate child leaks descendants. Process groups are the
POSIX-standard way to ensure all descendants die together. See
`watchexec` documentation: "Uses process groups to keep hold of
forking programs."

Source: https://github.com/watchexec/watchexec (architecture
notes), and the dedicated `command-group` / `process-wrap` crates
that exist precisely for this pattern.

## Crate choice for process groups

### Primary: `process-wrap` 9.1.0

- License: Apache-2.0 OR MIT.
- MSRV: 1.87.0 (lower than rmcp MSRV floor, non-binding).
- Tokio support: yes, via `tokio1` feature.
- Provides:
  - `ProcessGroup::leader()` for POSIX systems (makes the child the
    leader of its own process group).
  - `ProcessSession` (new session + new process group).
  - `JobObject` for Windows (Job Objects, parity with process group).
  - `KillOnDrop` wrapper for automatic termination on Drop.

Source: https://github.com/watchexec/process-wrap

Recommendation: use `process-wrap` with the `tokio1` feature to wrap
the `tokio::process::Command` returned by `pty-process`. This gives:

- a tokio-async `Child` handle,
- process-group semantics on POSIX,
- a single abstraction that will work for Windows ConPTY later via
  JobObject.

### Predecessor: `command-group` 5.0.1

- Deprecated; the README states: "The successor of command-group is
  [process-wrap](https://github.com/watchexec/process-wrap). No
  further work will be done on command-group."
- Do not use for new code.

Source: https://github.com/watchexec/command-group

### Lower-level alternative: `nix`

`nix` 0.31.3 (MSRV 1.69, MIT) provides:

- `nix::unistd::setpgid`
- `nix::unistd::killpg` (via `nix::sys::signal::killpg`)
- `nix::sys::signal::kill`
- `nix::sys::signal::Signal::*`

Use `nix` directly only if `process-wrap` becomes unavailable or has
a bug. Cost: more boilerplate, easy to misuse on Windows.

Source: https://lib.rs/crates/nix

## SIGTERM-then-SIGKILL pattern

Algorithm per child:

```text
killpg(pgid, SIGTERM)
wait up to N seconds for child to exit (tokio::time::timeout)
if still alive:
    killpg(pgid, SIGKILL)
    wait again (no timeout; SIGKILL cannot be blocked)
reap exit status
```

Default `N` = 5 seconds. Expose as `command_send_signal` parameter
and as daemon-wide config (`shutdown_grace_ms`).

For `command_send_signal` from the MCP surface, the same `killpg`
path applies: an LLM-initiated signal hits the whole probe group,
not just the immediate child. This matches user expectation: sending
SIGINT to a `cargo test` probe should interrupt the whole test run,
not just the cargo wrapper.

## SIGCHLD and zombie prevention

When using `tokio::process::Command`:

- The tokio runtime installs a SIGCHLD-aware driver on Unix and
  reaps children automatically as soon as `Child::wait` is polled.
- Always poll `wait()` for every spawned child. Letting a `Child`
  drop without `wait` can leak the entry (still reaped, but logged).
- Do NOT install a separate global SIGCHLD handler in the daemon;
  it conflicts with tokio's. If a non-tokio child is ever needed,
  prefer `nix::sys::wait::waitpid(Pid::from_raw(-1), Some(WNOHANG))`
  in a dedicated reaping task triggered by `tokio::signal::unix`.

Sources:

- tokio process docs (well-known behavior, runtime owns SIGCHLD when
  children are spawned through it).
- `nix::sys::wait` API on https://lib.rs/crates/nix.

## tokio::signal vs nix vs std

| Need | Use |
|---|---|
| Daemon SIGTERM/SIGINT handler | `tokio::signal::unix::signal(SignalKind::terminate())` and `interrupt()` |
| Send SIGTERM to a child PG | `process-wrap` API or `nix::sys::signal::killpg` |
| Wait for child exit | `tokio::process::Child::wait` |
| Manual SIGCHLD reaping (rare) | `nix::sys::wait::waitpid` in a `tokio::signal::unix` listener |
| Windows ctrl-c | `tokio::signal::ctrl_c()` (post-MVP) |

The std library's signal facilities are not sufficient for this use
case; use `tokio::signal` for daemon-level handling.

## Prior art

- `watchexec` uses process groups via the `watchexec-supervisor` +
  `command-group` (now `process-wrap`) crates. The library is built
  for exactly this scenario.
  Source: https://github.com/watchexec/watchexec
- `cargo-watch` delegates to watchexec for child management:
  "Cargo Watch uses the Watchexec library interface and calls it
  with its own custom options."
  Source: https://github.com/watchexec/cargo-watch

Conclusion: the recommended pattern (`process-wrap` +
process-group + SIGTERM-then-SIGKILL ladder) is exactly what the
canonical Rust file-watcher / command-runner ecosystem already uses
in production.

## Edge cases to design for

1. PTY-attached child: when a child runs under a PTY, closing the
   master fd typically sends SIGHUP to the foreground group. Still
   issue SIGTERM explicitly so behavior is uniform across probes
   that do not have a PTY.
2. Detached daemons spawned by a child (e.g. `cargo run -- &`):
   process groups catch them only if they remain in the same group.
   Use `ProcessSession` (new session + new group) when sandboxing is
   needed; `ProcessGroup::leader()` is enough for the common case.
3. Foreign daemons (e.g. an `npm` script that starts a watcher and
   forks into the background): document as a known limitation. The
   `command_exit_info` event should include "leaked descendants
   suspected" when the probe exits but the process group is not
   empty after grace period.
4. WSL2: same Linux semantics for setpgid/killpg/SIGTERM/SIGKILL,
   verified via standard Linux kernel.

## Confidence

Medium-high. The pattern is well established in the Rust ecosystem
(watchexec / cargo-watch use it in production). The remaining
uncertainty is edge cases around detached descendants; those are
documented as known limitations rather than blockers.

## HALT-worthy findings

None.

## SOURCE_MAP reclassification

- Process-group cleanup pattern is the watchexec/cargo-watch baseline:
  evidence-backed via
  https://github.com/watchexec/watchexec (process groups quote) and
  https://github.com/watchexec/cargo-watch.
- `process-wrap` 9.1.0 supersedes `command-group`: evidence-backed
  via project README quote.
- tokio's process driver reaps children automatically: well-known
  tokio behavior; inferred-evidence-backed, not externally verified
  in this pass.
