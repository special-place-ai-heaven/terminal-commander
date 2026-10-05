// Daemon pidfile: records the running daemon's pid + version +
// endpoint so a newer install can find and replace a stale daemon
// without depending on any IPC method the stale daemon may lack.
//
// The pidfile is the keystone primitive for version-aware replacement
// (see docs/superpowers/specs/2026-05-27-daemon-version-replace-design.md).
// A reachable daemon with NO pidfile predates this feature and is stale
// by construction; the replace path then uses an OS query to find its
// pid (see `replace.rs`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Contents of the daemon pidfile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningDaemon {
    pub pid: u32,
    pub version: String,
    /// The endpoint path/pipe the daemon bound, cross-checked before
    /// any kill so we never kill a process bound to a different socket.
    pub endpoint: String,
}

/// Pidfile path inside the given state dir.
#[must_use]
pub fn pidfile_path(state_dir: &Path) -> PathBuf {
    state_dir.join("terminal-commanderd.pid")
}

/// Path to the cross-process bring-up lock, a sibling of the pidfile.
/// Held (advisory, non-blocking) around the probe -> spawn (and probe
/// -> kill -> spawn) critical section so two cold-starting adapters
/// single-flight daemon launch instead of racing to orphan each other's
/// daemon (H6). The lock *file* itself carries no liveness meaning; see
/// `proc_lock` for why it is never deleted.
pub fn lock_path(state_dir: &Path) -> PathBuf {
    state_dir.join("terminal-commanderd.lock")
}

/// Write the pidfile atomically (tmp + rename).
pub fn write_pidfile(state_dir: &Path, rec: &RunningDaemon) -> std::io::Result<()> {
    crate::paths::ensure_private_dir(state_dir)?;
    let path = pidfile_path(state_dir);
    let tmp = path.with_extension(format!("pid.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(rec).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)
}

/// Remove the pidfile (best-effort; ignore missing).
pub fn remove_pidfile(state_dir: &Path) {
    let _ = std::fs::remove_file(pidfile_path(state_dir));
}

/// Read the pidfile if present + parseable. A pidfile whose pid is no
/// longer alive is treated as absent (returns `None`).
#[must_use]
pub fn read_pidfile(state_dir: &Path) -> Option<RunningDaemon> {
    let bytes = std::fs::read(pidfile_path(state_dir)).ok()?;
    let rec: RunningDaemon = serde_json::from_slice(&bytes).ok()?;
    if pid_alive(rec.pid) { Some(rec) } else { None }
}

/// Read + parse the pidfile WITHOUT the liveness filter. Returns the recorded
/// `RunningDaemon` even when its pid is dead, so enumeration can classify stale
/// entries. Returns `None` only when the file is absent or unparseable.
#[must_use]
pub fn read_pidfile_raw(state_dir: &Path) -> Option<RunningDaemon> {
    let bytes = std::fs::read(pidfile_path(state_dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Cross-platform "is this pid alive" check. Uses OS facilities rather
/// than a libc dependency so the supervisor crate stays dep-light.
///
/// On Linux this reads `/proc/<pid>/stat` -- fork-free, so the
/// liveness checks on the daemon bring-up and reap paths no longer pay a
/// `fork`+`exec` per call (review finding #5).
///
/// A zombie (exited, not yet reaped) reads as dead on every Unix: it cannot
/// serve, and counting it alive blocked daemon restarts after a crash.
///
/// Cross-user semantics (Linux). `/proc/<pid>` exists for a live process
/// regardless of its owner, so unlike the
/// previous `kill -0` probe this never reports another user's live process
/// as dead via `EPERM`. The one environment where the two diverge in the
/// other direction is `hidepid=2`, under which another user's `/proc/<pid>`
/// is hidden and this reports dead -- matching `kill -0`'s `EPERM` result.
/// In every case the supervisor's own daemon is the same user, so the
/// existence answer is identical to the old one for the pids we actually
/// act on; the kill paths are independently identity-gated, so a cross-user
/// pid reported alive is never killed.
///
/// On Windows this is a native `OpenProcess` + `GetExitCodeProcess` probe
/// (`replace::windows_native::pid_alive`) -- spawn-free, so the GUI-subsystem
/// daemon never pops a console window from its liveness ticks.
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        // `/proc/<pid>/stat` exists for a running process AND for an unreaped
        // zombie. A zombie has exited and can serve nothing: a daemon that
        // crashed under an adapter that never waited on it stays one until
        // the adapter exits, and counting it alive made every restart exit
        // with "a live daemon already serves this endpoint". So read the
        // state field (the first char after the last ')', since the command
        // name may contain parentheses) and treat Z/X as dead. A read error
        // keeps the old "treat as dead on failure" default.
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.trim_start().chars().next())
                .is_some_and(|state| state != 'Z' && state != 'X')
        })
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        // macOS / *BSD have no /proc. `ps -o stat= -p <pid>` fails when the
        // pid does not exist and prints a state starting with 'Z' for a
        // zombie, which is dead for the same reason as on Linux.
        std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .is_ok_and(|out| {
                out.status.success()
                    && !String::from_utf8_lossy(&out.stdout)
                        .trim_start()
                        .starts_with('Z')
            })
    }
    #[cfg(windows)]
    {
        // Native OpenProcess + GetExitCodeProcess liveness -- NO external
        // process. The daemon is GUI-subsystem on Windows, so the previous
        // `tasklist` probe (a console child spawned without CREATE_NO_WINDOW)
        // opened a visible, focus-stealing terminal window on every 15s
        // pidfile-reassert tick. See `replace::windows_native::pid_alive`
        // for the preserved exact-pid / fail-safe semantics.
        crate::replace::windows_native::pid_alive(pid)
    }
}

/// Read `/proc/<pid>/cmdline` (Linux) as a space-joined command line, or
/// `None` if the process is gone or exposes no argv.
///
/// Fork-free identity read: callers that already know a pid is alive can
/// confirm *which* process it is from the same `/proc` entry instead of
/// forking `ps`/`pgrep`, so the supervisor never spawns twice for one pid
/// (review finding #5, the identity-read piggyback).
///
/// Cross-user / hardening note: on a default Linux mount `/proc/<pid>/cmdline`
/// is world-readable, so this resolves another user's process too. For kernel
/// threads, not-yet-reaped zombies, and processes hidden by `hidepid`, the
/// blob is empty and this returns `None`. Callers MUST treat `None` as
/// "cannot confirm identity" and fail closed -- never as a match.
#[cfg(target_os = "linux")]
#[must_use]
pub fn proc_cmdline(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    join_proc_cmdline(&raw)
}

/// Join a raw `/proc/<pid>/cmdline` blob (NUL-separated argv, usually with a
/// trailing NUL) into a space-separated string. Returns `None` for an empty
/// or all-NUL blob so identity callers fail closed. Pure, so the parsing is
/// unit-testable without a live process (the cross-user/hidepid empty case is
/// covered by asserting the empty-blob branch).
#[cfg(target_os = "linux")]
fn join_proc_cmdline(raw: &[u8]) -> Option<String> {
    let joined = raw
        .split(|&b| b == 0)
        .filter(|seg| !seg.is_empty())
        .map(|seg| String::from_utf8_lossy(seg).into_owned())
        .collect::<Vec<String>>()
        .join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tc-pidfile-{}", std::process::id()));
        let rec = RunningDaemon {
            pid: std::process::id(),
            version: "0.1.14".into(),
            endpoint: "/tmp/x.sock".into(),
        };
        write_pidfile(&dir, &rec).unwrap();
        let got = read_pidfile(&dir).unwrap();
        assert_eq!(got, rec);
        remove_pidfile(&dir);
        assert!(read_pidfile(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_pidfile_raw_returns_dead_pid_contents() {
        let dir = std::env::temp_dir().join(format!("tc-raw-{}", std::process::id()));
        let rec = RunningDaemon {
            pid: 999_999_999,
            version: "0.1.0".into(),
            endpoint: "x".into(),
        };
        write_pidfile(&dir, &rec).unwrap();
        assert!(
            read_pidfile(&dir).is_none(),
            "read_pidfile still hides dead pids"
        );
        assert_eq!(
            read_pidfile_raw(&dir),
            Some(rec),
            "raw must return contents regardless of liveness"
        );
        remove_pidfile(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dead_pid_reads_as_absent() {
        let dir = std::env::temp_dir().join(format!("tc-pidfile-dead-{}", std::process::id()));
        let rec = RunningDaemon {
            pid: 999_999_999,
            version: "0.1.0".into(),
            endpoint: "x".into(),
        };
        write_pidfile(&dir, &rec).unwrap();
        assert!(
            read_pidfile(&dir).is_none(),
            "a pidfile with a dead pid must read as absent"
        );
        remove_pidfile(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- Finding #5: /proc liveness + fork-free identity read (Linux) ---

    #[cfg(target_os = "linux")]
    #[test]
    fn pid_alive_true_for_self_false_for_absent() {
        assert!(
            pid_alive(std::process::id()),
            "the running test process must read as alive"
        );
        assert!(
            !pid_alive(0xFFFF_FFF0),
            "an absent high pid must read as dead (no /proc entry)"
        );
    }

    // A daemon that crashed while its launcher (the MCP adapter) never waited
    // on it lingers as a zombie. It cannot serve anything, so it must read as
    // dead: otherwise the restarted daemon sees "a live daemon already serves
    // this endpoint", exits, and the adapter can never recover it.
    #[cfg(unix)]
    #[test]
    fn pid_alive_false_for_unreaped_zombie() {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .expect("spawn short-lived child");
        let pid = child.id();
        // Do not wait yet: once the child exits it stays a zombie until reaped.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while pid_alive(pid) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let alive = pid_alive(pid);
        let _ = child.wait();
        assert!(
            !alive,
            "an exited, unreaped child (zombie) must read as dead"
        );
    }

    #[cfg(windows)]
    #[test]
    fn pid_alive_true_for_self_false_for_absent_windows() {
        assert!(
            pid_alive(std::process::id()),
            "the running test process must read as alive (native OpenProcess)"
        );
        assert!(
            !pid_alive(0xFFFF_FFF0),
            "an absent pid must read as dead (OpenProcess invalid-parameter)"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_cmdline_reads_self_and_handles_absent() {
        let mine = proc_cmdline(std::process::id());
        assert!(mine.is_some(), "self cmdline must be readable on Linux");
        assert!(!mine.unwrap().is_empty(), "self cmdline must be non-empty");
        assert!(
            proc_cmdline(0xFFFF_FFF0).is_none(),
            "an absent pid yields no cmdline"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn join_proc_cmdline_parses_nul_separated_argv() {
        // argv as the kernel exposes it: NUL-separated, trailing NUL. A path
        // with regex/glob metacharacters must survive verbatim (no escaping).
        let raw = b"terminal-commanderd\x00--data-dir\x00/tmp/tc (run)+[v1]\x00";
        assert_eq!(
            join_proc_cmdline(raw).as_deref(),
            Some("terminal-commanderd --data-dir /tmp/tc (run)+[v1]")
        );
        // Empty / all-NUL blob == kernel thread, zombie, or hidepid-hidden
        // process => None, so identity callers fail closed (the cross-user
        // case we cannot spawn in CI is covered here at the parser).
        assert_eq!(join_proc_cmdline(b""), None);
        assert_eq!(join_proc_cmdline(b"\x00\x00"), None);
        // Single arg, no trailing NUL.
        assert_eq!(join_proc_cmdline(b"solo").as_deref(), Some("solo"));
    }
}
