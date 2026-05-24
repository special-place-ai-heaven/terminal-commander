// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Build a security descriptor string (SDDL) that restricts a Windows
// named pipe to LocalSystem, Administrators, and the current user.

#![cfg(windows)]
#![allow(unsafe_code)]

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub fn build_sddl_for_current_user() -> std::io::Result<String> {
    let user_sid = current_user_sid()?;
    // Owner: current user. DACL: LocalSystem full, Admins full,
    // current user full. Everyone denied implicitly (no allow entry).
    let sddl = format!("O:{user_sid}D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{user_sid})");
    Ok(sddl)
}

fn current_user_sid() -> std::io::Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token)
            .map_err(|e| std::io::Error::other(format!("OpenProcessToken: {e}")))?;

        // First call: get required buffer size.
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &raw mut needed);
        let mut buf = vec![0u8; needed as usize];

        if let Err(e) = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &raw mut needed,
        ) {
            let _ = CloseHandle(token);
            return Err(std::io::Error::other(format!("GetTokenInformation: {e}")));
        }

        // SAFETY: Windows allocates the buffer aligned for TOKEN_USER.
        #[allow(clippy::cast_ptr_alignment, clippy::ptr_as_ptr)]
        let token_user = &*(buf.as_ptr().cast::<TOKEN_USER>());
        let mut sid_str = windows::core::PWSTR::null();
        if let Err(e) = ConvertSidToStringSidW(token_user.User.Sid, &raw mut sid_str) {
            let _ = CloseHandle(token);
            return Err(std::io::Error::other(format!(
                "ConvertSidToStringSidW: {e}"
            )));
        }
        let s = sid_str.to_string().unwrap_or_default();
        // LocalFree the string buffer allocated by ConvertSidToStringSidW.
        // sid_str.0 is *mut u16; HLOCAL wraps *mut c_void.
        LocalFree(HLOCAL(sid_str.0.cast::<core::ffi::c_void>()));
        let _ = CloseHandle(token);
        Ok(s)
    }
}

pub fn create_named_pipe_with_sddl(
    name: &str,
    sddl: &str,
    first: bool,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, NAMED_PIPE_MODE, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::core::PCWSTR;

    let wide_name: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_sddl: Vec<u16> = OsStr::new(sddl)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide_sddl.as_ptr()),
            1, // SDDL_REVISION_1
            &raw mut sd,
            None,
        )
        .map_err(|e| std::io::Error::other(format!("ConvertStringSecurityDescriptor: {e}")))?;

        let sa = SECURITY_ATTRIBUTES {
            // SECURITY_ATTRIBUTES is a small fixed struct; the cast is always safe.
            #[allow(clippy::cast_possible_truncation)]
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };

        // dwOpenMode bits:
        //   PIPE_ACCESS_DUPLEX         — read/write.
        //   FILE_FLAG_OVERLAPPED       — required for tokio
        //                                NamedPipeServer::from_raw_handle.
        //   FILE_FLAG_FIRST_PIPE_INSTANCE — only on the first iteration.
        let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        if first {
            open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }

        // PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT as NAMED_PIPE_MODE.
        let pipe_mode = NAMED_PIPE_MODE(PIPE_TYPE_BYTE.0 | PIPE_READMODE_BYTE.0 | PIPE_WAIT.0);

        let handle = CreateNamedPipeW(
            PCWSTR(wide_name.as_ptr()),
            open_mode,
            pipe_mode,
            255, // max instances
            4096,
            4096,
            0,
            Some(&raw const sa),
        );

        // LocalFree the security descriptor allocated by
        // ConvertStringSecurityDescriptorToSecurityDescriptorW.
        LocalFree(HLOCAL(sd.0));

        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        // SAFETY: handle was created with FILE_FLAG_OVERLAPPED; tokio
        // NamedPipeServer::from_raw_handle requires an OVERLAPPED handle.
        tokio::net::windows::named_pipe::NamedPipeServer::from_raw_handle(handle.0.cast())
    }
}
