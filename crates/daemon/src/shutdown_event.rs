// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// BRUCE_FLAG: a same-user, shutdown-only Windows event so a normal
// (non-elevated) `terminal-commander update` can ask an elevated daemon to
// exit. The event grants EVENT_MODIFY_STATE and nothing else. It is not the
// command pipe, and this process will not wait on an event it did not create.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::Arc;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE, HLOCAL,
    LocalFree, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, WaitForSingleObject,
};
use windows::core::PCWSTR;

use crate::state::DaemonState;

/// Owned kernel handle that may move to the waiter thread.
///
/// `HANDLE` is a raw pointer and is not `Send`. This wrapper is sent to exactly
/// one thread, which is the only user of the handle.
struct SendHandle(HANDLE);

// SAFETY: the handle is exclusively owned. It is not aliased, and `Drop`
// closes it exactly once on the thread that owns the wrapper.
unsafe impl Send for SendHandle {}

impl Drop for SendHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is an open event handle this wrapper owns.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Create the medium-integrity shutdown event and wait for it off the runtime
/// thread. Signaling it calls [`DaemonState::trigger_shutdown`], the same
/// sticky flag the Shutdown IPC uses.
// The module is crate-visible, so `pub(crate)` is the real visibility.
// `redundant_pub_crate` wants `pub` here; `unreachable_pub` rejects that.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn spawn_waiter(state: Arc<DaemonState>) {
    let name = terminal_commander_supervisor::shutdown_handoff::event_name(std::process::id());
    let handle = match create_event(&name) {
        Ok(Some(handle)) => handle,
        Ok(None) => {
            tracing::warn!(
                "shutdown event {name} already exists; not waiting on an event this process did not create"
            );
            return;
        }
        Err(err) => {
            tracing::warn!("shutdown event {name} was not created: {err}");
            return;
        }
    };
    let owned = SendHandle(handle);
    if let Err(err) = std::thread::Builder::new()
        .name("tc-shutdown-event".to_owned())
        .spawn(move || {
            if wait_signaled(&owned) {
                tracing::info!("shutdown event signaled; requesting daemon shutdown");
                state.trigger_shutdown();
            }
        })
    {
        tracing::warn!("shutdown event waiter thread failed: {err}");
    }
}

fn wait_signaled(handle: &SendHandle) -> bool {
    // SAFETY: `handle.0` is a live event handle. `INFINITE` waits until
    // `SetEvent` or process exit (which abandons the thread).
    let wait = unsafe { WaitForSingleObject(handle.0, INFINITE) };
    wait == WAIT_OBJECT_0
}

/// `Ok(Some)` is an event this call created. `Ok(None)` means the name was
/// already taken and the caller must not wait on it.
fn create_event(name: &str) -> std::io::Result<Option<HANDLE>> {
    let wide = wide_null(name);
    // SAFETY: `wide` is a NUL-terminated UTF-16 name. A successful open means
    // some other creator owns this name; the handle is closed before return.
    match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(wide.as_ptr())) } {
        Ok(existing) => {
            unsafe {
                let _ = CloseHandle(existing);
            }
            return Ok(None);
        }
        Err(err) if is_not_found(&err) => {}
        Err(err) => {
            return Err(std::io::Error::other(format!(
                "OpenEventW before create: {err}"
            )));
        }
    }

    let sid = crate::ipc::pipe_acl::current_user_sid()?;
    let sddl = terminal_commander_supervisor::shutdown_handoff::event_sddl(&sid)
        .map_err(std::io::Error::other)?;
    let wide_sddl = wide_null(&sddl);

    // SAFETY: both buffers are NUL-terminated and live for this call.
    // `ConvertStringSecurityDescriptorToSecurityDescriptorW` allocates `sd`,
    // which is freed with `LocalFree` on every path after `CreateEventW`.
    unsafe {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide_sddl.as_ptr()),
            1,
            &raw mut sd,
            None,
        )
        .map_err(|err| std::io::Error::other(format!("shutdown event SDDL: {err}")))?;

        let sa = SECURITY_ATTRIBUTES {
            #[allow(clippy::cast_possible_truncation)]
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let created = CreateEventW(Some(&raw const sa), false, false, PCWSTR(wide.as_ptr()));
        LocalFree(Some(HLOCAL(sd.0)));
        let handle =
            created.map_err(|err| std::io::Error::other(format!("CreateEventW: {err}")))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return Ok(None);
        }
        Ok(Some(handle))
    }
}

fn is_not_found(err: &windows::core::Error) -> bool {
    err.code() == ERROR_FILE_NOT_FOUND.to_hresult()
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
