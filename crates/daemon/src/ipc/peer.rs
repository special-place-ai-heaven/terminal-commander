// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Peer credential extraction for accepted UDS connections (TC37).
//!
//! Backed by `tokio::net::UnixStream::peer_cred()`, which wraps the
//! platform-appropriate syscall:
//! - Linux / Android: `SO_PEERCRED` (returns uid, gid, pid)
//! - macOS / BSD: `getpeereid` (returns uid, gid; pid is None on BSDs)
//!
//! Credential lookup has no `unsafe`; the platform distinction lives
//! inside tokio. macOS image/parent lookups call libproc (`unsafe` FFI).
//!
//! Fail-closed: if `peer_cred()` returns an error, we return `None`
//! and the IPC server treats that as a peer-credential failure on
//! Linux/WSL and refuses the connection.

use serde::{Deserialize, Serialize};

/// Peer credentials captured at accept time. Fields are platform-
/// dependent; `pid` may be `None` on BSDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerCred {
    pub uid: u32,
    pub gid: u32,
    pub pid: Option<i32>,
}

impl PeerCred {
    /// Render for an audit metadata blob.
    #[must_use]
    pub fn to_audit_string(&self) -> String {
        self.pid.map_or_else(
            || format!("uid={};gid={}", self.uid, self.gid),
            |p| format!("uid={};gid={};pid={}", self.uid, self.gid, p),
        )
    }
}

#[cfg(unix)]
/// Resolve peer credentials for a tokio `UnixStream`.
///
/// Returns `None` if the OS does not return credentials or the
/// syscall fails. The IPC server is responsible for converting
/// `None` into a peer-credential failure on Linux/WSL.
pub fn resolve(stream: &tokio::net::UnixStream) -> Option<PeerCred> {
    let ucred = stream.peer_cred().ok()?;
    Some(PeerCred {
        uid: ucred.uid(),
        gid: ucred.gid(),
        pid: ucred.pid(),
    })
}

/// `true` when `uid` is the user this daemon runs as. The daemon runs any
/// command for whoever drives it, so only its own user may.
#[cfg(unix)]
#[must_use]
#[allow(unsafe_code)]
pub fn same_user(uid: u32) -> bool {
    // SAFETY: geteuid takes no arguments and cannot fail.
    uid == unsafe { libc::geteuid() }
}

/// Longest parent chain [`descends_from`] walks before giving up.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) const MAX_ANCESTRY_DEPTH: usize = 64;

/// `true` when `ancestor` is a strict ancestor of `pid`. An unreadable
/// parent ends the walk as `false`.
///
/// ponytail: parent-chain walk only. A descendant whose intermediate
/// parent already exited was reparented and escapes; Win32 job-object
/// membership would catch it on Windows.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn descends_from(pid: u32, ancestor: u32) -> bool {
    let mut cur = pid;
    for _ in 0..MAX_ANCESTRY_DEPTH {
        match parent_pid(cur) {
            Some(parent) if parent == ancestor => return true,
            Some(parent) if parent > 1 && parent != cur => cur = parent,
            _ => return false,
        }
    }
    false
}

#[cfg(windows)]
pub fn descends_from(pid: u32, ancestor: u32) -> bool {
    super::peer_windows::descends_from(pid, ancestor)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub const fn descends_from(_pid: u32, _ancestor: u32) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn parent_pid(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))?
        .trim()
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
fn parent_pid(pid: u32) -> Option<u32> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    // SAFETY: `info` is an aligned `proc_bsdinfo` of exactly `size` bytes;
    // proc_pidinfo writes at most `size` bytes and returns the count written.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if written != size {
        return None;
    }
    // SAFETY: the kernel filled all `size` bytes, and the zeroed start is
    // already a valid `proc_bsdinfo` (plain integers and char arrays).
    Some(unsafe { info.assume_init() }.pbi_ppid)
}

/// Executable path of `pid` via `proc_pidpath` (macOS has no `/proc`).
#[cfg(target_os = "macos")]
pub fn image_path(pid: i32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut buf = vec![0u8; usize::try_from(libc::PROC_PIDPATHINFO_MAXSIZE).ok()?];
    let cap = u32::try_from(buf.len()).ok()?;
    // SAFETY: `buf` is a live, writable buffer of `cap` bytes; proc_pidpath
    // writes at most `cap` bytes and returns the length written (<= 0 on error).
    let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), cap) };
    buf.truncate(usize::try_from(len).ok().filter(|&n| n > 0)?);
    Some(std::ffi::OsString::from_vec(buf).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_string_with_pid() {
        let p = PeerCred {
            uid: 1000,
            gid: 1000,
            pid: Some(12345),
        };
        assert_eq!(p.to_audit_string(), "uid=1000;gid=1000;pid=12345");
    }

    #[test]
    fn audit_string_without_pid() {
        let p = PeerCred {
            uid: 1000,
            gid: 1000,
            pid: None,
        };
        assert_eq!(p.to_audit_string(), "uid=1000;gid=1000");
    }
}
