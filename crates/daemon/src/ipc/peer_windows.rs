// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
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
        if GetNamedPipeClientProcessId(handle, &raw mut pid).is_ok() && pid != 0 {
            Some(pid)
        } else {
            None
        }
    };
    let Some((sid, image)) = pid_opt.and_then(resolve_sid_and_image) else {
        return PeerIdentity::unknown_because("could not resolve peer SID");
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
        if OpenProcessToken(proc, TOKEN_QUERY, &raw mut token).is_err() {
            let _ = CloseHandle(proc);
            return None;
        }

        // First call: get required buffer size.
        let mut needed: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &raw mut needed);
        // Allocate an 8-byte-aligned buffer big enough for `needed` bytes.
        // align_of::<TOKEN_USER>() is at most align_of::<u64>() = 8 on x64.
        // Vec<u8> is only byte-aligned; casting to *const TOKEN_USER from it
        // is undefined behavior. Vec<u64> guarantees 8-byte alignment.
        let aligned_len = (needed as usize).div_ceil(std::mem::size_of::<u64>());
        let mut buf: Vec<u64> = vec![0u64; aligned_len.max(1)];

        if GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast::<core::ffi::c_void>()),
            needed,
            &raw mut needed,
        )
        .is_err()
        {
            let _ = CloseHandle(token);
            let _ = CloseHandle(proc);
            return None;
        }

        // SAFETY: buf is Vec<u64>, so it is 8-byte aligned, which matches
        // align_of::<TOKEN_USER>() on x64 (contains a *mut SID pointer).
        // Windows filled the buffer with a valid TOKEN_USER + appended SID bytes.
        let token_user = &*(buf.as_ptr().cast::<TOKEN_USER>());
        let mut sid_str = windows::core::PWSTR::null();
        if ConvertSidToStringSidW(token_user.User.Sid, &raw mut sid_str).is_err() {
            let _ = CloseHandle(token);
            let _ = CloseHandle(proc);
            return None;
        }
        let sid = sid_str.to_string().unwrap_or_default();
        // LocalFree the string buffer allocated by ConvertSidToStringSidW.
        // sid_str.0 is *mut u16; HLOCAL wraps *mut c_void.
        LocalFree(HLOCAL(sid_str.0.cast::<core::ffi::c_void>()));

        let mut buf16 = vec![0u16; 1024];
        // SAFETY: buf16.len() is 1024, well within u32 range.
        #[allow(clippy::cast_possible_truncation)]
        let mut len = buf16.len() as u32;
        // PROCESS_NAME_FORMAT(0) = PROCESS_NAME_NATIVE returns NT device
        // paths (`\Device\HarddiskVolume3\...`) which are operator-hostile
        // in audit logs. Use the Win32 form (`C:\Program Files\...`).
        let image = if QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_FORMAT(1),
            windows::core::PWSTR(buf16.as_mut_ptr()),
            &raw mut len,
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
