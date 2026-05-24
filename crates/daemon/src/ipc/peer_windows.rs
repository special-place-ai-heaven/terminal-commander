// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Resolve the Windows named-pipe client's SID, PID, and image path.

#![cfg(windows)]
// Win32 FFI inherently requires unsafe; this module is the single
// containment point for all unsafe Win32 calls in the daemon.
#![allow(unsafe_code)]

use std::path::PathBuf;
use terminal_commander_supervisor::identity::PeerIdentity;
use tokio::net::windows::named_pipe::NamedPipeServer;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};

pub fn peer_identity_for(server: &NamedPipeServer) -> PeerIdentity {
    use std::os::windows::io::AsRawHandle;
    // as_raw_handle() returns *mut c_void; HANDLE wraps *mut c_void.
    let raw_handle = server.as_raw_handle();
    let handle = HANDLE(raw_handle);
    let mut pid: u32 = 0;
    let pid_opt = unsafe {
        if GetNamedPipeClientProcessId(handle, &mut pid).is_ok() && pid != 0 {
            Some(pid)
        } else {
            None
        }
    };
    let (sid, image) = match pid_opt.and_then(resolve_sid_and_image) {
        Some(pair) => pair,
        None => return PeerIdentity::unknown_because("could not resolve peer SID"),
    };
    if sid.is_empty() {
        return PeerIdentity::unknown_because("empty SID returned by Win32");
    }
    PeerIdentity::Windows {
        sid,
        pid: pid_opt,
        image,
    }
}

fn resolve_sid_and_image(pid: u32) -> Option<(String, Option<PathBuf>)> {
    unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        if OpenProcessToken(proc, TOKEN_QUERY, &mut token).is_err() {
            let _ = CloseHandle(proc);
            return None;
        }

        // First call: get required buffer size.
        let mut needed: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        let mut buf = vec![0u8; needed as usize];

        if GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .is_err()
        {
            let _ = CloseHandle(token);
            let _ = CloseHandle(proc);
            return None;
        }

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = windows::core::PWSTR::null();
        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str).is_err() {
            let _ = CloseHandle(token);
            let _ = CloseHandle(proc);
            return None;
        }
        let sid = sid_str.to_string().unwrap_or_default();
        // LocalFree the string buffer allocated by ConvertSidToStringSidW.
        // sid_str.0 is *mut u16; HLOCAL wraps *mut c_void.
        LocalFree(HLOCAL(sid_str.0.cast::<core::ffi::c_void>()));

        let mut buf16 = vec![0u16; 1024];
        let mut len = buf16.len() as u32;
        // PROCESS_NAME_FORMAT(0) = PROCESS_NAME_NATIVE returns NT device
        // paths (`\Device\HarddiskVolume3\...`) which are operator-hostile
        // in audit logs. Use the Win32 form (`C:\Program Files\...`).
        let image = if QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_FORMAT(1),
            windows::core::PWSTR(buf16.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
        {
            Some(PathBuf::from(String::from_utf16_lossy(
                &buf16[..len as usize],
            )))
        } else {
            None
        };

        let _ = CloseHandle(token);
        let _ = CloseHandle(proc);
        Some((sid, image))
    }
}
