// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Narrow update preflight for npm-managed Windows installs.
//
// The public `terminal-commander update` command runs from the Node
// wrapper. Before invoking npm it asks the currently installed native
// helper to stop old Terminal Commander processes whose image path is
// inside this exact npm platform package bin directory. This prevents
// Windows file-lock cleanup failures during package replacement without
// shelling out to taskkill, cmd.exe, PowerShell, or process-name-wide
// termination.

#![allow(clippy::redundant_pub_crate)]

use std::path::Path;

#[cfg(any(windows, test))]
const OWNED_BINARIES: &[&str] = &[
    "terminal-commander.exe",
    "terminal-commanderd.exe",
    "terminal-commander-mcp.exe",
];

#[derive(Debug, Default)]
pub(crate) struct UpdateLockResult {
    pub(crate) lines: Vec<String>,
    pub(crate) errors: usize,
}

#[cfg(windows)]
pub(crate) fn stop_installed_processes(bin_dir: &Path) -> UpdateLockResult {
    windows_impl::stop_installed_processes(bin_dir)
}

#[cfg(not(windows))]
pub(crate) fn stop_installed_processes(_bin_dir: &Path) -> UpdateLockResult {
    UpdateLockResult {
        lines: vec!["terminal-commander: update-lock preflight skipped on non-Windows.".to_owned()],
        errors: 0,
    }
}

#[cfg(any(windows, test))]
fn normalize_path(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_owned();
    }
    while s.ends_with('\\') && s.len() > 3 {
        s.pop();
    }
    s.to_lowercase()
}

#[cfg(any(windows, test))]
use std::path::PathBuf;

#[cfg(any(windows, test))]
fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(any(windows, test))]
fn is_owned_binary_name(name: &str) -> bool {
    OWNED_BINARIES
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

#[cfg(any(windows, test))]
fn image_is_inside_bin_dir(image: &Path, bin_dir: &Path) -> bool {
    let Some(parent) = image.parent() else {
        return false;
    };
    normalize_path(&canonical_or_original(parent))
        == normalize_path(&canonical_or_original(bin_dir))
}

#[cfg(windows)]
mod windows_impl {
    use super::{UpdateLockResult, image_is_inside_bin_dir, is_owned_binary_name, normalize_path};
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess,
        WaitForSingleObject,
    };

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn stop_installed_processes(bin_dir: &Path) -> UpdateLockResult {
        let mut result = UpdateLockResult::default();
        let bin_dir_norm = normalize_path(bin_dir);
        result.lines.push(format!(
            "terminal-commander: update-lock preflight scanning {bin_dir_norm}"
        ));

        let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
            Ok(handle) => OwnedHandle(handle),
            Err(err) => {
                result.errors += 1;
                result.lines.push(format!(
                    "terminal-commander: update-lock process snapshot failed: {err}"
                ));
                return result;
            }
        };

        let current_pid = unsafe { GetCurrentProcessId() };
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
                .expect("PROCESSENTRY32W size fits in u32"),
            ..Default::default()
        };

        let mut has_entry = unsafe { Process32FirstW(snapshot.0, &raw mut entry).is_ok() };
        let mut stopped = 0usize;
        while has_entry {
            let pid = entry.th32ProcessID;
            let name = exe_name_from_entry(&entry);
            if pid != current_pid
                && is_owned_binary_name(&name)
                && let Some(image) = process_image_path(pid)
                && image_is_inside_bin_dir(&image, bin_dir)
            {
                match terminate_process(pid) {
                    Ok(waited) => {
                        stopped += 1;
                        let wait_note = if waited { "stopped" } else { "terminate_sent" };
                        result.lines.push(format!(
                            "terminal-commander: {wait_note} pid {pid} ({})",
                            image.display()
                        ));
                    }
                    Err(err) => {
                        result.errors += 1;
                        result.lines.push(format!(
                            "terminal-commander: failed to stop pid {pid} ({}): {err}",
                            image.display()
                        ));
                    }
                }
            }
            has_entry = unsafe { Process32NextW(snapshot.0, &raw mut entry).is_ok() };
        }

        if stopped == 0 && result.errors == 0 {
            result
                .lines
                .push("terminal-commander: no owned running binaries found for update.".to_owned());
        }
        result
    }

    fn exe_name_from_entry(entry: &PROCESSENTRY32W) -> String {
        let end = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        String::from_utf16_lossy(&entry.szExeFile[..end])
    }

    fn process_image_path(pid: u32) -> Option<PathBuf> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
        let proc = OwnedHandle(handle);
        let mut buf16 = vec![0u16; 2048];
        let mut len = u32::try_from(buf16.len()).expect("static path buffer length fits in u32");
        let ok = unsafe {
            QueryFullProcessImageNameW(
                proc.0,
                PROCESS_NAME_FORMAT(1),
                windows::core::PWSTR(buf16.as_mut_ptr()),
                &raw mut len,
            )
            .is_ok()
        };
        if !ok || len == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf16[..len as usize],
        )))
    }

    fn terminate_process(pid: u32) -> Result<bool, String> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
        let handle = unsafe { OpenProcess(access, false, pid) }.map_err(|e| e.to_string())?;
        let proc = OwnedHandle(handle);
        unsafe { TerminateProcess(proc.0, 0) }.map_err(|e| e.to_string())?;
        let wait = unsafe { WaitForSingleObject(proc.0, 2000) };
        // M5: only WAIT_OBJECT_0 is a confirmed clean exit. Previously any value
        // other than WAIT_TIMEOUT (including WAIT_FAILED / WAIT_ABANDONED) was
        // reported as a clean stop. TerminateProcess already succeeded above, so
        // this is diagnostic accuracy: report whether we actually observed exit.
        match wait {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            WAIT_FAILED => Err(format!(
                "WaitForSingleObject failed after TerminateProcess: {}",
                std::io::Error::last_os_error()
            )),
            other => Err(format!(
                "WaitForSingleObject returned unexpected status 0x{:x} after TerminateProcess",
                other.0
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_binary_names_are_exact() {
        assert!(is_owned_binary_name("terminal-commanderd.exe"));
        assert!(is_owned_binary_name("TERMINAL-COMMANDER-MCP.EXE"));
        assert!(!is_owned_binary_name("terminal-commander-helper.exe"));
        assert!(!is_owned_binary_name("cmd.exe"));
    }

    #[test]
    fn image_scope_requires_same_bin_dir() {
        let root = PathBuf::from(
            r"C:\Users\me\.npm-global\node_modules\terminal-commander\node_modules\@terminal-commander\windows-x64\bin",
        );
        let image = root.join("terminal-commanderd.exe");
        let other = PathBuf::from(r"C:\Temp\terminal-commanderd.exe");
        assert!(image_is_inside_bin_dir(&image, &root));
        assert!(!image_is_inside_bin_dir(&other, &root));
    }
}
