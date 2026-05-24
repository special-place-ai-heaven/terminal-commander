// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Peer credential extraction for accepted UDS connections (TC37).
//!
//! Backed by `tokio::net::UnixStream::peer_cred()`, which wraps the
//! platform-appropriate syscall:
//! - Linux / Android: `SO_PEERCRED` (returns uid, gid, pid)
//! - macOS / BSD: `getpeereid` (returns uid, gid; pid is None on BSDs)
//!
//! No `unsafe` code; workspace lints forbid it. The platform
//! distinction lives inside tokio.
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
