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

use crate::protocol::IpcError;

/// The refusal for an endpoint served by another user: both identities and
/// the fix, never anything sent.
pub(crate) fn foreign_server(endpoint: &str, server: &str, client: &str) -> IpcError {
    IpcError::transport_not_connected(format!(
        "{endpoint} is served by {server}, but this client runs as {client}; nothing was \
         sent. Run the client as the user that owns the daemon, or point it at your own \
         daemon (unset TC_SOCKET, drop --socket)."
    ))
}

/// `uid N (name)`, or `uid N` when the account database has no entry.
#[cfg(unix)]
#[must_use]
pub fn uid_label(uid: u32) -> String {
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buf = vec![0 as libc::c_char; 4096];
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: every pointer refers to a live local buffer of the stated
    // size; `pw_name` is read only when getpwuid_r reports a match, and it
    // points into `buf`, which outlives the read.
    let name = unsafe {
        let rc = libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
            &raw mut found,
        );
        (rc == 0 && !found.is_null()).then(|| {
            std::ffi::CStr::from_ptr((*found).pw_name)
                .to_string_lossy()
                .into_owned()
        })
    };
    name.map_or_else(|| format!("uid {uid}"), |n| format!("uid {uid} ({n})"))
}

/// `Err((server, client))` labels when the daemon on the other end of
/// `stream` runs as another user.
#[cfg(unix)]
pub(crate) fn check_server_user(stream: &tokio::net::UnixStream) -> Result<(), (String, String)> {
    // SAFETY: geteuid takes no arguments and cannot fail.
    let own = unsafe { libc::geteuid() };
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == own => Ok(()),
        Ok(cred) => Err((uid_label(cred.uid()), uid_label(own))),
        Err(_) => Err((
            "a process whose user cannot be read".to_owned(),
            uid_label(own),
        )),
    }
}

/// `Err((server, client))` account names when the process serving `pipe`
/// runs as another user (same user SID passes; an elevated server keeps it).
#[cfg(windows)]
pub(crate) fn check_server_user(
    pipe: &tokio::net::windows::named_pipe::NamedPipeClient,
) -> Result<(), (String, String)> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::EqualSid;
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let ours = own_token_user();
    // SAFETY: the pipe handle is live for this call; every handle opened
    // here is closed before returning; the SID pointers point into buffers
    // that outlive the comparison.
    let theirs = unsafe {
        let mut pid = 0u32;
        GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &raw mut pid)
            .ok()
            .and_then(|()| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok())
            .and_then(|server| {
                let user = token_user(server);
                let _ = CloseHandle(server);
                user
            })
    };
    // SAFETY: both buffers hold TOKEN_USER structures (see `sid_of`).
    if let (Some(a), Some(b)) = (&theirs, &ours)
        && unsafe { EqualSid(sid_of(a), sid_of(b)) }.is_ok()
    {
        return Ok(());
    }
    let label = |t: &Option<Vec<u64>>| {
        t.as_deref()
            .and_then(account_name)
            .unwrap_or_else(|| "an account that cannot be read".to_owned())
    };
    Err((label(&theirs), label(&ours)))
}

/// This process's account, `DOMAIN\name`, for an access-denied refusal.
#[cfg(windows)]
pub(crate) fn own_account() -> String {
    own_token_user()
        .as_deref()
        .and_then(account_name)
        .unwrap_or_else(|| "this account".to_owned())
}

#[cfg(windows)]
fn own_token_user() -> Option<Vec<u64>> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close.
    unsafe { token_user(windows::Win32::System::Threading::GetCurrentProcess()) }
}

/// `DOMAIN\name` for the SID in a `TOKEN_USER` buffer.
#[cfg(windows)]
fn account_name(token_user: &[u64]) -> Option<String> {
    use windows::Win32::Security::{LookupAccountSidW, SID_NAME_USE};
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut name_len, mut domain_len) = (256u32, 256u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: the buffers and their lengths match; the SID points into
    // `token_user`, which outlives the call.
    unsafe {
        LookupAccountSidW(
            windows::core::PCWSTR::null(),
            sid_of(token_user),
            Some(windows::core::PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(windows::core::PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut kind,
        )
        .ok()?;
    }
    let name = String::from_utf16_lossy(&name[..name_len as usize]);
    let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
    Some(if domain.is_empty() {
        name
    } else {
        format!("{domain}\\{name}")
    })
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_refusal_names_both_identities_and_the_fix() {
        let e = super::foreign_server("/run/tc.sock", "uid 1001 (bob)", "uid 0 (root)");
        assert!(
            e.message.contains("served by uid 1001 (bob)"),
            "{}",
            e.message
        );
        assert!(e.message.contains("runs as uid 0 (root)"), "{}", e.message);
        assert!(
            e.message
                .contains("Run the client as the user that owns the daemon")
        );
        assert!(e.message.contains("nothing was sent"));
    }

    #[cfg(unix)]
    #[test]
    fn a_uid_is_labelled_with_its_account_name() {
        assert_eq!(super::uid_label(0), "uid 0 (root)");
        assert_eq!(super::uid_label(4_000_000_000), "uid 4000000000");
    }

    #[cfg(windows)]
    #[test]
    fn this_account_has_a_readable_name() {
        let me = super::own_account();
        let user = std::env::var("USERNAME").unwrap();
        assert!(
            me.to_ascii_lowercase()
                .ends_with(&user.to_ascii_lowercase()),
            "{me} vs {user}"
        );
    }
}
