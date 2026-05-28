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

/// Cross-platform "is this pid alive" check. Uses OS tools rather than
/// a libc dependency so the supervisor crate stays dep-light and the
/// same code path works on Windows and Unix.
#[must_use]
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // `kill -0 <pid>` exits 0 iff the process exists and is
        // signalable; non-zero otherwise. No signal is delivered.
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
        let rec = RunningDaemon { pid: 999_999_999, version: "0.1.0".into(), endpoint: "x".into() };
        write_pidfile(&dir, &rec).unwrap();
        assert!(read_pidfile(&dir).is_none(), "read_pidfile still hides dead pids");
        assert_eq!(read_pidfile_raw(&dir), Some(rec), "raw must return contents regardless of liveness");
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
}
