// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Cross-process advisory file lock used to single-flight daemon
//! bring-up (H6).
//!
//! `ensure_daemon` and `replace_if_stale` each do a probe-then-spawn
//! (and probe-then-kill-then-spawn) sequence with no inter-process
//! coordination. Two adapters cold-starting in the same window each
//! reach the spawn branch; on Unix the daemon's bind unconditionally
//! `remove_file`s + rebinds the socket, orphaning the first daemon
//! (which keeps the DB open) while clients reach only the second.
//!
//! This module wraps the standard-library advisory file lock
//! (`std::fs::File::try_lock`, stabilized in Rust 1.89) in a small RAII
//! guard. The OS lock is held for the lifetime of the [`ProcessLock`]
//! and released on drop. The kernel also releases the lock when the
//! holding process exits, so the lock *file* on disk is a pure rendezvous
//! point and carries no liveness meaning: a leftover lock file from a
//! crashed process is harmless and immediately re-lockable. We therefore
//! never delete the lock file — deleting it would defeat the rendezvous
//! (a concurrent peer could create a fresh inode and lock that instead,
//! and both would believe they hold the lock).
//!
//! No `unsafe` is used: the supervisor crate keeps `unsafe_code = forbid`.

use std::fs::{File, TryLockError};
use std::io;
use std::path::Path;

/// RAII handle to a held cross-process advisory lock.
///
/// The wrapped [`File`] keeps the OS lock held; dropping this guard
/// (or exiting the process) releases it. There is no explicit unlock
/// method by design — release is tied to the guard's lifetime so a lock
/// can never be released early by accident while a critical section is
/// still in flight.
#[derive(Debug)]
pub struct ProcessLock {
    // Held purely for its Drop (= unlock). Never read after construction.
    _file: File,
}

/// Outcome of a non-blocking [`try_acquire`] attempt.
#[derive(Debug)]
pub enum TryLockResult {
    /// The lock was free and is now held by the returned guard.
    Acquired(ProcessLock),
    /// Another process (or another handle in this process) holds the lock.
    Contended,
}

/// Try to take the cross-process advisory lock at `path` without
/// blocking.
///
/// Opens (creating if needed) the lock file with read + write access —
/// Windows requires both for `LockFileEx`-backed locking — and attempts
/// a non-blocking advisory lock:
///
/// * lock free  -> [`TryLockResult::Acquired`] holding the guard;
/// * lock held  -> [`TryLockResult::Contended`] (the std
///   `TryLockError::WouldBlock` case);
/// * real error -> `Err` (open failure or a non-contention lock error).
///
/// The lock file is intentionally left on disk; see the module docs.
pub fn try_acquire(path: &Path) -> io::Result<TryLockResult> {
    let file = File::options()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(TryLockResult::Acquired(ProcessLock { _file: file })),
        Err(TryLockError::WouldBlock) => Ok(TryLockResult::Contended),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T1: two acquisitions of the same lock path contend; releasing the
    /// first frees it; a leftover lock *file* is re-lockable.
    ///
    /// `std::fs::File::try_lock` is backed by `flock`/`LockFileEx`. Two
    /// separate `File` opens of the same path within one process DO
    /// contend with each other (the lock is associated with the open file
    /// description / handle, not the thread), so this is a valid
    /// single-process exercise of the cross-process semantics.
    #[test]
    fn second_acquire_is_contended_then_reacquirable_after_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.lock");

        // 1. First acquire succeeds.
        let first = match try_acquire(&path).unwrap() {
            TryLockResult::Acquired(g) => g,
            TryLockResult::Contended => panic!("first acquire should succeed"),
        };

        // 2. Second acquire on the same path contends.
        match try_acquire(&path).unwrap() {
            TryLockResult::Contended => {}
            TryLockResult::Acquired(_) => panic!("second acquire should be contended"),
        }

        // 3. Drop the first guard -> lock released -> third acquire succeeds.
        drop(first);
        let third = match try_acquire(&path).unwrap() {
            TryLockResult::Acquired(g) => g,
            TryLockResult::Contended => panic!("acquire after drop should succeed"),
        };
        drop(third);

        // 4. The lock FILE is still present (we never delete it) and is
        //    immediately re-lockable — a stale lock file is harmless.
        assert!(path.exists(), "lock file should persist on disk");
        match try_acquire(&path).unwrap() {
            TryLockResult::Acquired(_) => {}
            TryLockResult::Contended => panic!("stale lock file should be re-lockable"),
        }
    }
}
