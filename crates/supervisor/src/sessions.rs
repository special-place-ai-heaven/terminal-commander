//! Session enumeration for the supervisor CLI. Pure filesystem read: the base
//! dir's pidfile is the "default" session; each immediate subdir with a pidfile
//! is a seeded session labeled by its token. No daemon connection here.

use std::path::{Path, PathBuf};

use crate::pidfile::{pid_alive, pidfile_path, read_pidfile_raw};

/// One enumerated session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    /// "default" for the base-dir session, else the token (subdir name).
    pub label: String,
    pub state_dir: PathBuf,
    pub pid: u32,
    pub version: String,
    pub endpoint: String,
    /// True iff the recorded pid is currently alive.
    pub alive: bool,
}

/// Enumerate sessions under `base_dir`: the base pidfile (label "default") plus
/// every immediate subdir containing a pidfile (label = subdir name).
///
/// Liveness is identity-gated by the real
/// [`crate::replace::pid_belongs_to_daemon`]: an entry is `alive` only when its
/// pid is running AND that pid is a TC daemon bound to its own state dir, so a
/// recycled PID does not masquerade as a live session.
#[must_use]
pub fn enumerate(base_dir: &Path) -> Vec<SessionEntry> {
    enumerate_with(base_dir, &crate::replace::pid_belongs_to_daemon)
}

/// [`enumerate`] with an injectable identity check. Production calls
/// [`enumerate`] (which passes the real `pid_belongs_to_daemon`); the seam
/// exists so tests can supply a stub instead of spawning a live daemon to act
/// as the identity oracle. `identity(pid, state_dir) -> bool`.
#[must_use]
pub fn enumerate_with(base_dir: &Path, identity: &dyn Fn(u32, &Path) -> bool) -> Vec<SessionEntry> {
    let mut out = Vec::new();
    if let Some(e) = entry_for(base_dir, "default", identity) {
        out.push(e);
    }
    if let Ok(rd) = std::fs::read_dir(base_dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() && pidfile_path(&p).exists() {
                let label = ent.file_name().to_string_lossy().into_owned();
                if let Some(e) = entry_for(&p, &label, identity) {
                    out.push(e);
                }
            }
        }
    }
    out
}

/// Build a [`SessionEntry`] for one state dir, deciding liveness via the
/// injected `identity` check.
///
/// `alive` is true only when the recorded pid is BOTH currently running AND
/// confirmed to belong to a TC daemon for THIS `state_dir`. The identity gate
/// closes a recycled-PID hole: a dead session's pid can be reassigned by the
/// OS to an unrelated process, which `pid_alive` alone would report as "alive",
/// leaving the stale pidfile uncleaned (`reap_one` only cleans on `!alive`).
///
/// `identity` is injectable purely for testability — production callers pass
/// [`crate::replace::pid_belongs_to_daemon`] (see [`enumerate`]); tests inject
/// a stub so they need not spawn a real daemon. The signature mirrors
/// `pid_belongs_to_daemon`: `(pid, state_dir) -> bool`.
fn entry_for(
    state_dir: &Path,
    label: &str,
    identity: &dyn Fn(u32, &Path) -> bool,
) -> Option<SessionEntry> {
    let rec = read_pidfile_raw(state_dir)?;
    let alive = pid_alive(rec.pid) && identity(rec.pid, state_dir);
    Some(SessionEntry {
        label: label.to_owned(),
        state_dir: state_dir.to_path_buf(),
        pid: rec.pid,
        version: rec.version,
        endpoint: rec.endpoint,
        alive,
    })
}

/// Remove a stale pidfile, but ONLY if it STILL names `classified_pid` at
/// delete time. Closes the race where a daemon restarts (writing a fresh
/// pidfile with a new pid) between stale classification and cleanup. Returns
/// true iff a file was removed.
#[must_use]
pub fn cleanup_stale(state_dir: &Path, classified_pid: u32) -> bool {
    match read_pidfile_raw(state_dir) {
        Some(rec) if rec.pid == classified_pid => {
            crate::pidfile::remove_pidfile(state_dir);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pidfile::{RunningDaemon, write_pidfile};

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tc-sessions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn enumerate_finds_default_and_seeded_with_stale_classification() {
        let base = tmp();
        write_pidfile(
            &base,
            &RunningDaemon {
                pid: 999_999_999,
                version: "0.1.0".into(),
                endpoint: "base.sock".into(),
            },
        )
        .unwrap();
        let seeded = base.join("agent-1");
        write_pidfile(
            &seeded,
            &RunningDaemon {
                pid: std::process::id(),
                version: "0.1.1".into(),
                endpoint: "agent.sock".into(),
            },
        )
        .unwrap();

        // Inject an identity oracle: the test runner's own pid stands in for
        // a live TC daemon (we cannot spawn a real one here). Liveness is now
        // identity-gated (`pid_alive && identity`), so without this stub the
        // seeded entry -- whose pid IS this process -- would be gated to
        // not-alive even though `pid_alive` reports it running. Production
        // passes the real `pid_belongs_to_daemon`; this stub matches the
        // seeded fixture exactly (only this process's pid is "the daemon").
        let me = std::process::id();
        let identity = move |pid: u32, _dir: &Path| pid == me;
        let mut got = enumerate_with(&base, &identity);
        got.sort_by(|a, b| a.label.cmp(&b.label));
        assert_eq!(got.len(), 2);
        let agent = got.iter().find(|e| e.label == "agent-1").unwrap();
        assert!(agent.alive, "seeded session with this pid must be alive");
        assert_eq!(agent.version, "0.1.1");
        let def = got.iter().find(|e| e.label == "default").unwrap();
        assert!(!def.alive, "default session with dead pid must be stale");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recycled_pid_is_reported_not_alive_and_reaped() {
        // L5 regression: liveness used to be `pid_alive(rec.pid)` with NO identity
        // check. When a dead session's pid is RECYCLED by the OS to an unrelated
        // live process, `pid_alive` reports it running, so the entry was
        // classified `alive` and its stale pidfile was NEVER cleaned (`reap_one`
        // only cleans on `!alive`). The identity gate fixes this: a recycled pid
        // that does NOT belong to a TC daemon for this state dir is reported
        // not-alive, so the stale pidfile gets reaped.
        let base = tmp();
        let recycled = std::process::id(); // alive (this process) but NOT our daemon.
        write_pidfile(
            &base,
            &RunningDaemon {
                pid: recycled,
                version: "0".into(),
                endpoint: "x".into(),
            },
        )
        .unwrap();

        // Identity oracle says "not the daemon" for the recycled pid (mirrors
        // `pid_belongs_to_daemon` returning false because the live process is
        // unrelated to this state dir).
        let identity = |_pid: u32, _dir: &Path| false;
        let entries = enumerate_with(&base, &identity);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.pid, recycled, "pid is the recycled (live) pid");
        assert!(
            !e.alive,
            "a live-but-non-daemon recycled pid must be reported NOT alive so it is reapable"
        );

        // The reap path (`reap_one`'s `!alive` branch) cleans the stale pidfile
        // via compare-before-delete. Verify the stale pidfile is now removable.
        assert!(
            cleanup_stale(&e.state_dir, e.pid),
            "stale pidfile for a recycled pid must be cleaned"
        );
        assert!(
            read_pidfile_raw(&base).is_none(),
            "stale pidfile must be gone after reap"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cleanup_stale_removes_only_matching_dead_pid() {
        let base = tmp();
        write_pidfile(
            &base,
            &RunningDaemon {
                pid: 999_999_999,
                version: "0".into(),
                endpoint: "x".into(),
            },
        )
        .unwrap();
        // Stale (dead pid) -> removed.
        assert!(
            cleanup_stale(&base, 999_999_999),
            "matching dead pid must be cleaned"
        );
        assert!(read_pidfile_raw(&base).is_none(), "pidfile must be gone");

        // Race guard: pidfile now names a DIFFERENT pid than the one we classified.
        write_pidfile(
            &base,
            &RunningDaemon {
                pid: std::process::id(),
                version: "0".into(),
                endpoint: "x".into(),
            },
        )
        .unwrap();
        assert!(
            !cleanup_stale(&base, 999_999_999),
            "must NOT delete when current pid differs from classified"
        );
        assert!(
            read_pidfile_raw(&base).is_some(),
            "live pidfile must survive"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
