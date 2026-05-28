// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Narrow update preflight for npm-managed Windows installs.
//
// The public `terminal-commander update` command runs from the Node
// wrapper. Before invoking npm it asks the currently installed native
// helper to stop old Terminal Commander processes whose image path is
// anywhere inside the npm package scope dir passed by the shim. The
// scope is the `node_modules` directory that contains the
// `terminal-commander` package, so the preflight reaps owned binaries
// loaded from:
//
//   - the currently installed package
//   - npm-renamed leftover siblings (`.terminal-commander-RAND`)
//   - any staged in-progress install under the same scope
//
// Binaries from unrelated installs (a different node version, a
// per-user install elsewhere) are left alone. This prevents Windows
// file-lock cleanup failures during package replacement without
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
pub(crate) fn stop_installed_processes(scope_dir: &Path) -> UpdateLockResult {
    windows_impl::stop_installed_processes(scope_dir)
}

#[cfg(not(windows))]
pub(crate) fn stop_installed_processes(_scope_dir: &Path) -> UpdateLockResult {
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
fn image_is_inside_scope(image: &Path, scope_dir: &Path) -> bool {
    let image_norm = normalize_path(&canonical_or_original(image));
    let mut scope_norm = normalize_path(&canonical_or_original(scope_dir));
    if !scope_norm.ends_with('\\') {
        scope_norm.push('\\');
    }
    image_norm.starts_with(&scope_norm)
}

#[cfg(windows)]
mod windows_impl {
    use super::{UpdateLockResult, image_is_inside_scope, is_owned_binary_name, normalize_path};
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

    pub(super) fn stop_installed_processes(scope_dir: &Path) -> UpdateLockResult {
        let mut result = UpdateLockResult::default();
        let scope_dir_norm = normalize_path(scope_dir);
        result.lines.push(format!(
            "terminal-commander: update-lock preflight scope {scope_dir_norm}"
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
                && image_is_inside_scope(&image, scope_dir)
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
        // PROCESS_NAME_FORMAT(0) = win32 (`C:\...`), (1) = NT-native (`\Device\HarddiskVolumeN\...`).
        // The scope dir arrives as a win32 path from the JS shim, so we MUST ask for
        // the same shape here; mismatch makes every in-scope process look out-of-scope.
        let ok = unsafe {
            QueryFullProcessImageNameW(
                proc.0,
                PROCESS_NAME_FORMAT(0),
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
    fn image_in_scope_dir_matches() {
        let scope = PathBuf::from(r"C:\Users\me\.npm-global\node_modules");
        let active = scope.join(r"terminal-commander\node_modules\@terminal-commander\windows-x64\bin\terminal-commanderd.exe");
        let staged = scope.join(r".terminal-commander-jZv5xAZ5\node_modules\@terminal-commander\windows-x64\bin\terminal-commander-mcp.exe");
        let unrelated = PathBuf::from(r"C:\Temp\terminal-commanderd.exe");
        assert!(image_is_inside_scope(&active, &scope));
        assert!(image_is_inside_scope(&staged, &scope));
        assert!(!image_is_inside_scope(&unrelated, &scope));
    }

    #[test]
    fn image_in_scope_rejects_sibling_prefix_collision() {
        // Sibling whose path-string starts with scope's chars but is not actually
        // under scope. Without the trailing separator on scope, a naive
        // starts_with would false-positive.
        let scope = PathBuf::from(r"C:\Users\me\nm\terminal-commander");
        let sibling =
            PathBuf::from(r"C:\Users\me\nm\terminal-commander-evil\bin\terminal-commanderd.exe");
        assert!(!image_is_inside_scope(&sibling, &scope));
    }

    #[test]
    fn nt_device_path_does_not_match_win32_scope() {
        // Regression: QueryFullProcessImageNameW(PROCESS_NAME_FORMAT=1) returns
        // an NT-native path like `\Device\HarddiskVolumeN\Users\...`, which can
        // never satisfy a win32 `C:\Users\...` scope. The earlier preflight
        // build asked for that format and silently skipped every running
        // owned binary, so `terminal-commander update` looked successful while
        // npm hit EBUSY a moment later. This test guards the gate behavior
        // even if a future refactor reverts to format 1; the win32 callsite
        // in `process_image_path` is the actual fix.
        let scope = PathBuf::from(r"C:\Users\me\nm\terminal-commander");
        let nt_image = PathBuf::from(
            r"\Device\HarddiskVolume3\Users\me\nm\terminal-commander\bin\terminal-commanderd.exe",
        );
        assert!(!image_is_inside_scope(&nt_image, &scope));
    }
}
