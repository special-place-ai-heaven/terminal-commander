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
#[must_use]
pub fn enumerate(base_dir: &Path) -> Vec<SessionEntry> {
    let mut out = Vec::new();
    if let Some(e) = entry_for(base_dir, "default") {
        out.push(e);
    }
    if let Ok(rd) = std::fs::read_dir(base_dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() && pidfile_path(&p).exists() {
                let label = ent.file_name().to_string_lossy().into_owned();
                if let Some(e) = entry_for(&p, &label) {
                    out.push(e);
                }
            }
        }
    }
    out
}

fn entry_for(state_dir: &Path, label: &str) -> Option<SessionEntry> {
    let rec = read_pidfile_raw(state_dir)?;
    Some(SessionEntry {
        label: label.to_owned(),
        state_dir: state_dir.to_path_buf(),
        pid: rec.pid,
        version: rec.version,
        endpoint: rec.endpoint,
        alive: pid_alive(rec.pid),
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
    use crate::pidfile::{write_pidfile, RunningDaemon};

    fn tmp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tc-sessions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn enumerate_finds_default_and_seeded_with_stale_classification() {
        let base = tmp();
        write_pidfile(&base, &RunningDaemon { pid: 999_999_999, version: "0.1.0".into(), endpoint: "base.sock".into() }).unwrap();
        let seeded = base.join("agent-1");
        write_pidfile(&seeded, &RunningDaemon { pid: std::process::id(), version: "0.1.1".into(), endpoint: "agent.sock".into() }).unwrap();

        let mut got = enumerate(&base);
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
    fn cleanup_stale_removes_only_matching_dead_pid() {
        let base = tmp();
        write_pidfile(&base, &RunningDaemon { pid: 999_999_999, version: "0".into(), endpoint: "x".into() }).unwrap();
        // Stale (dead pid) -> removed.
        assert!(cleanup_stale(&base, 999_999_999), "matching dead pid must be cleaned");
        assert!(read_pidfile_raw(&base).is_none(), "pidfile must be gone");

        // Race guard: pidfile now names a DIFFERENT pid than the one we classified.
        write_pidfile(&base, &RunningDaemon { pid: std::process::id(), version: "0".into(), endpoint: "x".into() }).unwrap();
        assert!(!cleanup_stale(&base, 999_999_999), "must NOT delete when current pid differs from classified");
        assert!(read_pidfile_raw(&base).is_some(), "live pidfile must survive");
        let _ = std::fs::remove_dir_all(&base);
    }
}
