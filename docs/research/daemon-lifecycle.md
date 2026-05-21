# Daemon Lifecycle (User-Mode, Systemd-Optional)

Topic: C2
Author: research agent R1-alpha
Date: 2026-05-21
Confidence: medium-high

## Recommendation

For MVP, run `terminal-commanderd` as a user-mode foreground process
managed by a wrapper of the user's choice:

1. Primary mode: foreground supervised process. `terminal-commanderd`
   runs in the foreground, writes a PID file, and exits cleanly on
   SIGTERM / SIGINT.
2. Optional: a systemd user unit shipped alongside the daemon as a
   convenience, used only when systemd is available.
3. Optional: `--daemonize` flag using the `fork` crate when the user
   explicitly wants a detached double-fork daemon.

Do NOT assume systemd is available. WSL2 systemd support exists but
is opt-in and not present on all distros / WSL versions.

## Concrete pieces

### Foreground supervisor (primary)

`terminal-commanderd` runs attached to the controlling terminal,
logs to stderr or to a configured file path, and shuts down cleanly
on SIGTERM. Users wrap it as needed:

- under `tmux new -s tcd 'terminal-commanderd ...'`
- under a process supervisor of their choice (`s6`, `runit`,
  `pm2`, etc.)
- under a systemd user unit (see below)

This mode has the lowest friction and works identically on bare
Linux, in WSL2 with or without systemd, and on macOS.

### PID file convention

Path:

- Linux: `${XDG_RUNTIME_DIR}/terminal-commander/daemon.pid` if
  `XDG_RUNTIME_DIR` is set; otherwise
  `${HOME}/.local/state/terminal-commander/daemon.pid`.
- macOS: `${HOME}/Library/Application Support/terminal-commander/daemon.pid`.

Contents: ASCII decimal PID + trailing newline.

Behavior on start:

1. If PID file exists and contains a live PID running the same binary
   (cross-check `/proc/<pid>/comm` on Linux, or `kill(pid, 0)` for
   liveness on other Unix), exit with a clear "already running"
   message.
2. Otherwise, write the PID file atomically (`O_CREAT | O_EXCL`
   pattern via a tempfile + rename).

Behavior on shutdown: remove the PID file in the SIGTERM/SIGINT
handler.

### `daemonize` and `fork` crates

- `daemonize` 0.5.0 (2023-02-25) is marked unmaintained on lib.rs.
  Avoid.
  Source: https://lib.rs/crates/daemonize.
- `fork` 0.7.0 (2026-03-09) is maintained and actively published; it
  supports Linux, macOS, FreeBSD, NetBSD, OpenBSD. License BSD-3-Clause.
  Source: https://lib.rs/crates/fork.

For an optional `--daemonize` flag use `fork::daemon` (double-fork
pattern). License: BSD-3-Clause; the project's overall license will
need to allow this or a re-implementation can be done in <100 lines
using `nix`.

### Optional systemd user unit

Ship a unit file as a config example, not as a hard dependency:

```text
# ~/.config/systemd/user/terminal-commanderd.service
[Unit]
Description=Terminal Commander Daemon
After=default.target

[Service]
Type=notify
ExecStart=%h/.local/bin/terminal-commanderd --notify-systemd
Restart=on-failure
WatchdogSec=30s

[Install]
WantedBy=default.target
```

When the user starts the daemon under systemd, the binary should
detect `NOTIFY_SOCKET` and emit READY/WATCHDOG using `sd-notify`:

- Crate: `sd-notify` 0.5.0 (2026-03-09). Dual MIT / Apache-2.0.
  Source: https://lib.rs/crates/sd-notify.
- Wrap the call so that if `NOTIFY_SOCKET` is unset, the calls are
  no-ops. That way the same binary runs cleanly without systemd.

### WSL2 systemd note (IMPORTANT)

systemd is supported in WSL2 but opt-in:

- WSL 0.67.6 or newer is required.
- The user must edit `/etc/wsl.conf` and add
  `[boot]\nsystemd=true`, then restart WSL with `wsl --shutdown`.
- It is the default on freshly installed current-Ubuntu-on-WSL,
  but NOT on older distros or on Debian without explicit opt-in.

Source: https://learn.microsoft.com/en-us/windows/wsl/systemd

This means `terminal-commanderd` MUST work when systemd is absent.
A systemd user unit is a convenience, not the entry point.

ARCH MUST NOT assume systemd in WSL. (Reaffirmed.)

### macOS launchd (deferred)

macOS is not an MVP target per the platform decision, but for
parity: ship an example `~/Library/LaunchAgents/io.terminal-commander.daemon.plist`
when macOS support lands. Out of MVP scope.

### Windows SCM (deferred)

Windows-native is out of MVP scope. The eventual approach uses the
Service Control Manager via the `windows-service` crate.

## Lifecycle states and transitions

```text
[exec]
   |
   v
[bind socket + write PID file + open store] -- failure --> [exit]
   |
   v
[ready]  -- SIGTERM/SIGINT --> [drain probes] --> [release PID] --> [exit]
                                                            ^
                                                            |
                                                    --watchdog timeout--
```

If running under systemd `Type=notify`, send `READY=1` after the
"bind socket + ... + open store" step. Send `WATCHDOG=1` on the
watchdog interval. Send `STOPPING=1` when entering drain. All
no-ops when `NOTIFY_SOCKET` is unset.

## Drain behavior on shutdown

On SIGTERM:

1. Stop accepting new MCP/control connections.
2. Stop spawning new child processes for probes.
3. For each active probe, send SIGTERM to the child process group
   (see `process-cleanup.md`), wait up to N seconds (config), then
   SIGKILL.
4. Flush in-memory event buckets to the store.
5. Close the store cleanly.
6. Remove the PID file and exit 0.

This guarantees no zombie children and a recoverable store after
restart.

## Confidence

Medium-high. The pattern is conventional. The main risk is shipping
a systemd-only assumption by accident; this file states explicitly
that systemd is optional.

## HALT-worthy findings

None.

## SOURCE_MAP reclassification

- WSL systemd is opt-in: evidence-backed via
  https://learn.microsoft.com/en-us/windows/wsl/systemd (requires
  WSL 0.67.6 + `/etc/wsl.conf`).
- `daemonize` 0.5.0 is unmaintained: evidence-backed via
  https://lib.rs/crates/daemonize.
- `fork` 0.7.0 is the maintained Unix daemonization crate:
  evidence-backed via https://lib.rs/crates/fork.
- `sd-notify` 0.5.0 is current and dual-licensed: evidence-backed
  via https://lib.rs/crates/sd-notify.
