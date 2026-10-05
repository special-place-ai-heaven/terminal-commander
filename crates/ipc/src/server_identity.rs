// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Who serves the endpoint, checked before the client sends anything.
//!
//! Endpoint names are predictable (`terminal-commander-<USERNAME>`, or a
//! session token that sits in harness config and command lines), so another
//! local user can create the pipe or socket first. Requests can carry an
//! owner's password (`credential provide`), so a client only talks to a
//! server running as its own user.

#![allow(unsafe_code)]
// Same convention as the CLI modules: explicit crate visibility on a private module.
#![allow(clippy::redundant_pub_crate)]

/// `true` when the daemon on the other end of `stream` runs as this user.
#[cfg(unix)]
pub(crate) fn served_by_current_user(stream: &tokio::net::UnixStream) -> bool {
    // SAFETY: geteuid takes no arguments and cannot fail.
    let own = unsafe { libc::geteuid() };
    stream.peer_cred().is_ok_and(|cred| cred.uid() == own)
}

/// `true` when the process serving `pipe` runs as this user (same user SID;
/// an elevated server keeps it). An unreadable server is not this user's.
#[cfg(windows)]
pub(crate) fn served_by_current_user(
    pipe: &tokio::net::windows::named_pipe::NamedPipeClient,
) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::EqualSid;
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: the pipe handle is live for this call; every handle opened
    // here is closed before returning; the SID pointers point into buffers
    // that outlive the comparison.
    unsafe {
        let mut pid = 0u32;
        if GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &raw mut pid).is_err() {
            return false;
        }
        let Ok(server) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let theirs = token_user(server);
        let _ = CloseHandle(server);
        let ours = token_user(GetCurrentProcess());
        match (theirs, ours) {
            (Some(a), Some(b)) => EqualSid(sid_of(&a), sid_of(&b)).is_ok(),
            _ => false,
        }
    }
}

/// The `TOKEN_USER` of `process`, in an 8-byte-aligned buffer.
#[cfg(windows)]
unsafe fn token_user(process: windows::Win32::Foundation::HANDLE) -> Option<Vec<u64>> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TokenUser};
    use windows::Win32::System::Threading::OpenProcessToken;

    // SAFETY: the caller passes a live process handle; the token handle is
    // closed on every path; the buffer is sized by the first call.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &raw mut token).ok()?;
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &raw mut needed);
        let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        let read = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &raw mut needed,
        );
        let _ = CloseHandle(token);
        read.ok()?;
        Some(buf)
    }
}

#[cfg(windows)]
const fn sid_of(token_user: &[u64]) -> windows::Win32::Security::PSID {
    // SAFETY: `token_user` holds a TOKEN_USER written by GetTokenInformation
    // into a u64 buffer, which satisfies its alignment.
    unsafe {
        (*token_user
            .as_ptr()
            .cast::<windows::Win32::Security::TOKEN_USER>())
        .User
        .Sid
    }
}
