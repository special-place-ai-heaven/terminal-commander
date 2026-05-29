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

/// Write the pidfile atomically (tmp + rename).
pub fn write_pidfile(state_dir: &Path, rec: &RunningDaemon) -> std::io::Result<()> {
    std::fs::create_dir_all(state_dir)?;
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
/// On Linux this reads `/proc/<pid>` existence -- fork-free, so the
/// liveness checks on the daemon bring-up and reap paths no longer pay a
/// `fork`+`exec` per call (review finding #5).
///
/// Cross-user semantics (Linux). `/proc/<pid>` exists for a live process
/// (and for a not-yet-reaped zombie) regardless of its owner, so unlike the
/// previous `kill -0` probe this never reports another user's live process
/// as dead via `EPERM`. The one environment where the two diverge in the
/// other direction is `hidepid=2`, under which another user's `/proc/<pid>`
/// is hidden and this reports dead -- matching `kill -0`'s `EPERM` result.
/// In every case the supervisor's own daemon is the same user, so the
/// existence answer is identical to the old one for the pids we actually
/// act on; the kill paths are independently identity-gated, so a cross-user
/// pid reported alive is never killed.
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        // A live process or unreaped zombie always has a /proc/<pid> dir.
        // `exists()` returns false on any stat error, preserving the old
        // "treat as dead on failure" default.
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        // macOS / *BSD have no /proc; keep the portable, fork-based probe.
        // `kill -0 <pid>` exits 0 iff the process exists and is signalable;
        // non-zero otherwise. No signal is delivered. (Out of scope for the
        // /proc optimization, which is Linux-only.)
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // `tasklist /FO CSV /NH` emits one quoted-CSV row per matching
        // process: "Image Name","PID","Session Name","Session#","Mem Usage".
        // Parse the PID column (index 1) and compare it EXACTLY -- a bare
        // substring match would falsely match the pid digits appearing in
        // the memory-usage column, the session id, or a superstring pid.
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                csv_row_has_exact_pid(&stdout, pid)
            })
            .unwrap_or(false)
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

/// Parse `tasklist /FO CSV /NH` output and return true iff some row's PID
/// column (the second quoted field) equals `pid` exactly. Tolerant of the
/// "INFO: No tasks..." line tasklist prints when nothing matches.
#[cfg(windows)]
fn csv_row_has_exact_pid(stdout: &str, pid: u32) -> bool {
    let want = pid.to_string();
    stdout.lines().any(|line| {
        // Fields are quoted; the PID is the second field.
        line.split(',')
            .nth(1)
            .map(|f| f.trim().trim_matches('"') == want)
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn csv_pid_match_is_exact_not_substring() {
        // Real-ish tasklist /FO CSV /NH row. PID column is field index 1.
        let row = "\"terminal-commanderd.exe\",\"1234\",\"Console\",\"1\",\"12,345 K\"";
        assert!(csv_row_has_exact_pid(row, 1234), "exact pid must match");
        // 123 is a substring of the pid 1234 AND of the mem-usage "12,345";
        // the old substring check matched it -- the column-exact check must not.
        assert!(
            !csv_row_has_exact_pid(row, 123),
            "substring of the pid/mem column must NOT match"
        );
        // 12 appears in mem usage "12,345 K" but is not the PID column.
        assert!(
            !csv_row_has_exact_pid(row, 12),
            "digits in the mem-usage column must NOT match"
        );
        // The "INFO: No tasks" line tasklist prints on no match.
        assert!(!csv_row_has_exact_pid("INFO: No tasks are running.", 1234));
    }

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
