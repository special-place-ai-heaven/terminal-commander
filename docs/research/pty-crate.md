# PTY Crate Choice

Topic: C1
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: medium-high

## Recommendation

For MVP (Linux native + WSL2): `pty-process` 0.5.3 with the `async`
feature for tokio integration.

For the future cross-platform path (macOS + Windows ConPTY):
`portable-pty` 0.9.0 wrapped in `tokio::task::spawn_blocking` or a
dedicated reader thread per PTY.

Do not lock the trait surface to one crate. The probe abstraction
in `terminal-commander-probes` should expose `AsyncRead + AsyncWrite`
PTY handles internally and pick the implementation at compile time
behind a feature flag.

## Comparison matrix

| Crate | Latest | License | Async | Linux | macOS | Windows | Edition | Notes |
|---|---|---|---|---|---|---|---|---|
| pty-process | 0.5.3 (2025-07-12) | MIT | tokio via `async` feature | yes | yes | NO | 2024 | rustix-based |
| portable-pty | 0.9.0 (2025-02-11) | MIT | blocking; dev-deps on smol | yes | yes | yes (ConPTY) | 2018 | wezterm |
| rexpect | 0.7.1 (2026-05-14) | MIT OR Apache-2.0 | NO (blocking) | yes | yes | NO | unknown | high-level expect |
| tokio-pty-process | not separately verified | unknown | tokio | unknown | unknown | unknown | unknown | superseded |

Sources:

- pty-process: https://github.com/doy/pty-process and
  https://raw.githubusercontent.com/doy/pty-process/main/Cargo.toml
  (quoted: `tokio` is optional under the `async` feature; uses
  rustix `pty`, `process`, `fs`, `termios` modules).
- portable-pty: https://lib.rs/crates/portable-pty and
  https://raw.githubusercontent.com/wez/wezterm/main/pty/Cargo.toml
  (quoted: "There is no tokio dependency.").
- rexpect: https://lib.rs/crates/rexpect.

## Per-crate detail

### pty-process (RECOMMENDED for MVP)

- Edition: 2024.
- Async: tokio under feature `async`; uses `tokio = "1.46.1"` with
  `["fs", "process", "net"]`.
- Surface: a wrapper around `tokio::process::Command` that gives you
  a master PTY handle (`AsyncRead + AsyncWrite`) and child process
  control.
- POSIX-only (rustix `pty` is POSIX).
- License: MIT.
- WSL caveat: WSL2 supports openpty/forkpty cleanly, so pty-process
  works in WSL2 the same as on bare Linux. WSL1 is not a target per
  README.

Trade-off: clean ergonomics, modern (edition 2024), aligned with
tokio. Cost: no Windows ConPTY. Acceptable per the platform decision
(Linux native + WSL2 primary; macOS/Windows-native deferred).

### portable-pty (FUTURE)

- Edition: 2018.
- Async: none directly; the crate is blocking and uses smol as a
  dev-dependency only. Adapt to tokio with `spawn_blocking` or a
  dedicated reader thread feeding a tokio channel.
- Cross-platform: Linux, macOS, Windows (ConPTY).
- License: MIT.
- WSL caveat: WSL2 uses Linux paths inside the crate, works fine.

Trade-off: best portability; cost is integration complexity (manual
blocking-to-async bridge) and slightly higher per-PTY thread overhead
(one OS thread per PTY reader).

### rexpect (NOT RECOMMENDED as primary)

- Blocking, Unix-only, expect-style API.
- Useful as test-only harness or scripted control of an external
  PTY-driven program. Not suitable as the daemon's main PTY engine
  because it abstracts away raw stream access.

### tokio-pty-process (REJECT)

- Older, low-maintenance crate. Superseded in practice by
  `pty-process` 0.5.x with the `async` feature.
- Did not produce a clean confirmation page in this research pass.
  If the architect believes this is the right choice, re-verify
  before locking. Default action: reject.

## Probe abstraction guidance

Inside `terminal-commander-probes`, define a trait such as:

```rust
trait TerminalProbe {
    type Reader: AsyncRead + Send + Unpin;
    type Writer: AsyncWrite + Send + Unpin;

    fn split(self) -> (Self::Reader, Self::Writer);
    async fn wait(self) -> std::io::Result<ExitStatus>;
    fn resize(&self, rows: u16, cols: u16) -> std::io::Result<()>;
    fn send_signal(&self, sig: Signal) -> std::io::Result<()>;
}
```

Provide one implementation backed by `pty-process` (compile-time
feature `pty-pty-process`) and reserve a second implementation backed
by `portable-pty` for the cross-platform path (compile-time feature
`pty-portable`). Default feature: `pty-pty-process`. This pattern
keeps the rest of the codebase runtime-uniform and lets the platform
decision change without rewriting probes.

## WSL2 specific notes

- WSL2 has a real Linux kernel; PTYs work via the standard pty/devpts
  filesystem. Both `pty-process` and `portable-pty` work in WSL2.
- WSL2 file watchers via inotify work; that is covered by `notify`
  (see `async-runtime.md`).
- WSL `XDG_RUNTIME_DIR` is only set when systemd is enabled
  (see `daemon-lifecycle.md`); pick the socket path with a fallback
  to `$HOME/.local/state/terminal-commander/`.
- WSLg (GUI), interop with Windows binaries, and slow `/mnt/c` IO are
  not directly PTY concerns; mention only if a future feature touches
  cross-filesystem watchers.

## Confidence

Medium-high. The MVP choice (`pty-process`) is supported by clear
evidence and matches the runtime decision. The future-portability
choice (`portable-pty`) is well established (powers wezterm) but
requires explicit blocking-to-async glue, which is a known cost.

## HALT-worthy findings

None. PTY support is available in the Rust ecosystem and integrates
with tokio for the MVP target platforms.

## SOURCE_MAP reclassification

- `pty-process` 0.5.3 has tokio support behind `async` feature:
  evidence-backed via
  https://raw.githubusercontent.com/doy/pty-process/main/Cargo.toml.
- `portable-pty` 0.9.0 is blocking-only (no tokio): evidence-backed
  via wezterm pty/Cargo.toml ("There is no tokio dependency.").
