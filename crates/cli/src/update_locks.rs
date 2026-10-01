// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Narrow update preflight for npm-managed Windows installs.
//
// The public `terminal-commander update` command runs from the Node
// wrapper. Before invoking npm it asks the currently installed native
// helper to stop old Terminal Commander processes whose image path is
// anywhere inside the npm package scope dir passed by the shim. Daemons
// are asked to exit themselves first (shutdown event, then Shutdown IPC).
// `TerminateProcess` is the fallback, retried when Windows returns
// Access Denied. The preflight exits non-zero if a process is still
// running, so npm does not run and the installed version stays put.
//
// BRUCE_FLAG: this stops in-scope Terminal Commander processes only. It
// does not elevate, enable SeDebugPrivilege, or widen the command-pipe
// ACL. The shutdown event is a separate same-user signal.
//
// The scope is the `node_modules` directory that contains the
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
const STOP_ATTEMPTS: u32 = 3;

/// Pipe names this preflight is willing to send `Shutdown` to.
///
/// The pidfile is ours, but its endpoint string is still data. Only the
/// product's `\\.\pipe\terminal-commander-…` namespace is accepted.
#[cfg(any(windows, test))]
pub(crate) fn accepted_shutdown_pipe(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix(r"\\.\pipe\terminal-commander-") else {
        return false;
    };
    !rest.is_empty()
        && rest.len() <= 200
        && !rest.contains('\\')
        && !rest.contains('/')
        && !rest.contains('\0')
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopKind {
    Daemon,
    Other,
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct AccessDenied {
    win32: u32,
    hresult: u32,
    detail: String,
}

#[cfg(any(windows, test))]
#[derive(Debug)]
enum EventTry {
    /// No event. Older daemons never created one; that is not a failure.
    Missing,
    Signaled,
    Failed(String),
}

#[cfg(any(windows, test))]
#[derive(Debug)]
enum Graceful {
    Exited,
    /// Self-exit did not finish. `reason` is operator-facing detail.
    Continue(String),
}

#[cfg(any(windows, test))]
#[derive(Debug)]
enum Terminate {
    Exited,
    StillRunning,
    AccessDenied(AccessDenied),
    Failed(String),
}

#[cfg(any(windows, test))]
struct StopOne {
    lines: Vec<String>,
    failed: bool,
}

/// Combine the shutdown event and the pipe `Shutdown` into one outcome.
///
/// A missing event is normal (the running daemon predates it) and is not
/// reported. Either channel exiting the process is success.
#[cfg(any(windows, test))]
fn graceful_outcome(
    event: EventTry,
    ipc: Result<(), String>,
    event_exited: bool,
    ipc_exited: bool,
) -> Graceful {
    if event_exited || ipc_exited {
        return Graceful::Exited;
    }
    let mut why = Vec::new();
    match event {
        EventTry::Missing => {}
        EventTry::Signaled => {
            why.push("shutdown event signaled but pid still running".to_owned());
        }
        EventTry::Failed(detail) => why.push(format!("shutdown event: {detail}")),
    }
    match ipc {
        Ok(()) => why.push("daemon pipe shutdown did not exit the pid".to_owned()),
        Err(detail) => why.push(detail),
    }
    let reason = if why.is_empty() {
        "no shutdown channel".to_owned()
    } else {
        why.join("; ")
    };
    Graceful::Continue(reason)
}

#[cfg(any(windows, test))]
fn push_recovery(lines: &mut Vec<String>, pid: u32, image: &str, kind: StopKind) {
    lines.push(format!(
        "terminal-commander: update stopped; installed version unchanged (pid {pid}, {image})"
    ));
    lines.push(
        "terminal-commander: next: close the program that started this process, then run `terminal-commander update` again."
            .to_owned(),
    );
    if kind == StopKind::Daemon {
        lines.push(
            "terminal-commander: if a daemon was started in another terminal, run `terminal-commander session reap --all` there, then retry `terminal-commander update`."
                .to_owned(),
        );
    }
    lines.push(format!(
        "terminal-commander: last resort: from an elevated terminal stop pid {pid}, then run `terminal-commander update` as your normal user."
    ));
}

/// Stop one owned process. Daemons get a self-exit attempt first. Terminate
/// is retried on Access Denied and when the process is still alive. A process
/// that is still running is a failure: the caller must not continue the update.
#[cfg(any(windows, test))]
fn stop_owned_process(
    pid: u32,
    image: &str,
    kind: StopKind,
    mut graceful: impl FnMut(u32) -> Graceful,
    mut terminate: impl FnMut(u32) -> Terminate,
    mut pause: impl FnMut(),
) -> StopOne {
    let mut lines = Vec::new();
    if kind == StopKind::Daemon {
        match graceful(pid) {
            Graceful::Exited => {
                lines.push(format!(
                    "terminal-commander: stopped pid {pid} ({image}) via daemon shutdown"
                ));
                return StopOne {
                    lines,
                    failed: false,
                };
            }
            Graceful::Continue(reason) => {
                lines.push(format!(
                    "terminal-commander: daemon shutdown did not stop pid {pid}: {reason}"
                ));
            }
        }
    }

    for attempt in 1..=STOP_ATTEMPTS {
        match terminate(pid) {
            Terminate::Exited => {
                let line = if attempt == 1 {
                    format!("terminal-commander: stopped pid {pid} ({image})")
                } else {
                    format!(
                        "terminal-commander: stopped pid {pid} ({image}) after {attempt} terminate attempts"
                    )
                };
                lines.push(line);
                return StopOne {
                    lines,
                    failed: false,
                };
            }
            Terminate::StillRunning if attempt == STOP_ATTEMPTS => {
                lines.push(format!(
                    "terminal-commander: pid {pid} ({image}) is still running after terminate"
                ));
                push_recovery(&mut lines, pid, image, kind);
                return StopOne {
                    lines,
                    failed: true,
                };
            }
            Terminate::AccessDenied(info) if attempt == STOP_ATTEMPTS => {
                let win32 = info.win32;
                let hresult = info.hresult;
                let detail = info.detail;
                lines.push(format!(
                    "terminal-commander: failed to stop pid {pid} ({image}): Access is denied (win32 {win32}, HRESULT 0x{hresult:08x}). {detail}"
                ));
                push_recovery(&mut lines, pid, image, kind);
                return StopOne {
                    lines,
                    failed: true,
                };
            }
            Terminate::Failed(msg) => {
                lines.push(format!(
                    "terminal-commander: failed to stop pid {pid} ({image}): {msg}"
                ));
                push_recovery(&mut lines, pid, image, kind);
                return StopOne {
                    lines,
                    failed: true,
                };
            }
            Terminate::StillRunning | Terminate::AccessDenied(_) => pause(),
        }
    }

    lines.push(format!(
        "terminal-commander: internal stop loop ended without a result for pid {pid} ({image})"
    ));
    StopOne {
        lines,
        failed: true,
    }
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
    use super::{
        AccessDenied, EventTry, Graceful, StopKind, Terminate, UpdateLockResult, graceful_outcome,
        image_is_inside_scope, is_owned_binary_name, normalize_path, stop_owned_process,
    };
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        EVENT_MODIFY_STATE, GetCurrentProcessId, OpenEventW, OpenProcess, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW, SetEvent,
        TerminateProcess,
    };
    use windows::core::PCWSTR;

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
                let report = stop_one(pid, &image, &name);
                result.lines.extend(report.lines);
                if report.failed {
                    result.errors += 1;
                } else {
                    stopped += 1;
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

    fn stop_one(pid: u32, image: &Path, exe_name: &str) -> super::StopOne {
        let kind = if exe_name.eq_ignore_ascii_case("terminal-commanderd.exe") {
            StopKind::Daemon
        } else {
            StopKind::Other
        };
        let image_s = image.display().to_string();
        stop_owned_process(
            pid,
            &image_s,
            kind,
            &mut graceful_for,
            &mut terminate_once,
            &mut || std::thread::sleep(std::time::Duration::from_millis(200)),
        )
    }

    /// Shutdown event first (works against an elevated daemon that created it),
    /// then Shutdown IPC on the recorded pipe. Either one has to actually
    /// exit the pid before we call it success.
    fn graceful_for(pid: u32) -> Graceful {
        // Pipe drain and the lifecycle drain are each capped at 10s. A signaled
        // daemon can hold the image until both finish, so the wait is that
        // budget plus a second. It returns as soon as the pid is gone.
        let budget = std::time::Duration::from_secs(21);
        let event = signal_shutdown_event(pid);
        let event_exited = matches!(event, EventTry::Signaled) && wait_until_stopped(pid, budget);
        if event_exited {
            return Graceful::Exited;
        }
        let ipc = crate::request_daemon_shutdown(pid);
        let ipc_exited = ipc.is_ok() && wait_until_stopped(pid, budget);
        graceful_outcome(event, ipc, event_exited, ipc_exited)
    }

    fn signal_shutdown_event(pid: u32) -> EventTry {
        let name = terminal_commander_supervisor::shutdown_handoff::event_name(pid);
        let wide = wide_null(&name);
        // SAFETY: `wide` is NUL-terminated. The event is opened for
        // EVENT_MODIFY_STATE only; the handle is closed by `OwnedHandle`.
        match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(wide.as_ptr())) } {
            Ok(handle) => {
                let owned = OwnedHandle(handle);
                // SAFETY: `owned.0` is the event just opened.
                match unsafe { SetEvent(owned.0) } {
                    Ok(()) => EventTry::Signaled,
                    Err(err) => EventTry::Failed(format!("SetEvent: {err}")),
                }
            }
            Err(err) if err.code() == ERROR_FILE_NOT_FOUND.to_hresult() => EventTry::Missing,
            Err(err) => EventTry::Failed(err.to_string()),
        }
    }

    /// `PROCESS_TERMINATE` alone. Combining it with `PROCESS_SYNCHRONIZE`
    /// makes `OpenProcess` fail when that extra right is missing, which
    /// surfaced as Access Denied even for a process we could otherwise stop.
    /// Exit is polled with `PROCESS_QUERY_LIMITED_INFORMATION`, the right that
    /// already succeeded when we read the image path.
    fn terminate_once(pid: u32) -> Terminate {
        if !process_is_running(pid) {
            return Terminate::Exited;
        }
        // SAFETY: `OpenProcess` takes an access mask, an inherit flag, and a
        // pid. The handle, when returned, is owned by `OwnedHandle`.
        let handle = match unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) } {
            Ok(handle) => handle,
            Err(err) => return classify_terminate_error(pid, &err),
        };
        let proc = OwnedHandle(handle);
        // SAFETY: `proc.0` was opened with `PROCESS_TERMINATE`.
        if let Err(err) = unsafe { TerminateProcess(proc.0, 0) } {
            return classify_terminate_error(pid, &err);
        }
        drop(proc);
        // TerminateProcess is a hard kill; two seconds is enough to observe it.
        if wait_until_stopped(pid, std::time::Duration::from_secs(2)) {
            Terminate::Exited
        } else {
            Terminate::StillRunning
        }
    }

    fn classify_terminate_error(pid: u32, err: &windows::core::Error) -> Terminate {
        if err.code() == windows::Win32::Foundation::ERROR_ACCESS_DENIED.to_hresult() {
            // The process can die between the snapshot and the open. A denial
            // against a pid that is already gone is a stop, not a failure.
            if !process_is_running(pid) {
                return Terminate::Exited;
            }
            return Terminate::AccessDenied(AccessDenied {
                win32: 5,
                hresult: err.code().0.cast_unsigned(),
                detail: err.to_string(),
            });
        }
        Terminate::Failed(err.to_string())
    }

    fn process_is_running(pid: u32) -> bool {
        terminal_commander_supervisor::pidfile::pid_alive(pid)
    }

    fn wait_until_stopped(pid: u32, budget: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if !process_is_running(pid) {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return !process_is_running(pid);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
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

    fn denied() -> Terminate {
        Terminate::AccessDenied(AccessDenied {
            win32: 5,
            hresult: 0x8007_0005,
            detail: "Access is denied. (0x80070005)".to_owned(),
        })
    }

    const IMAGE: &str = r"C:\Users\me\AppData\Roaming\npm\node_modules\@terminal-commander\windows-x64\bin\terminal-commanderd.exe";

    #[test]
    fn daemon_shutdown_skips_terminate() {
        let mut graceful_calls = 0;
        let mut terminate_calls = 0;
        let report = stop_owned_process(
            53132,
            IMAGE,
            StopKind::Daemon,
            |_| {
                graceful_calls += 1;
                Graceful::Exited
            },
            |_| {
                terminate_calls += 1;
                Terminate::Exited
            },
            || panic!("pause"),
        );
        assert!(!report.failed);
        assert_eq!(graceful_calls, 1);
        assert_eq!(terminate_calls, 0);
        let text = report.lines.join("\n");
        assert!(text.contains("stopped pid 53132"));
        assert!(text.contains("via daemon shutdown"));
    }

    #[test]
    fn access_denied_retries_then_stops() {
        let mut attempts = 0;
        let mut pauses = 0;
        let report = stop_owned_process(
            53132,
            IMAGE,
            StopKind::Daemon,
            |_| Graceful::Continue("no live session pidfile for pid 53132".to_owned()),
            |_| {
                attempts += 1;
                if attempts < 3 {
                    denied()
                } else {
                    Terminate::Exited
                }
            },
            || pauses += 1,
        );
        assert!(!report.failed);
        assert_eq!(attempts, 3);
        assert_eq!(pauses, 2);
        assert!(
            report
                .lines
                .join("\n")
                .contains("after 3 terminate attempts")
        );
    }

    #[test]
    fn access_denied_after_retries_leaves_the_install_unchanged() {
        let mut attempts = 0;
        let report = stop_owned_process(
            53132,
            IMAGE,
            StopKind::Daemon,
            |_| Graceful::Continue("shutdown pipe: Access is denied. (os error 5)".to_owned()),
            |_| {
                attempts += 1;
                denied()
            },
            || {},
        );
        assert!(report.failed);
        assert_eq!(attempts, 3);
        let text = report.lines.join("\n");
        assert!(text.contains("pid 53132"));
        assert!(text.contains(IMAGE));
        assert!(text.contains("win32 5"));
        assert!(text.contains("0x80070005"));
        assert!(text.contains("shutdown pipe: Access is denied"));
        assert!(text.contains("installed version unchanged"));
        let next = text
            .find("next: close the program")
            .expect("non-admin next step");
        let reap = text.find("session reap --all").expect("same-session reap");
        let last = text.find("last resort:").expect("elevation is last");
        assert!(next < reap && reap < last, "{text}");
        assert!(text.contains("as your normal user"));
    }

    #[test]
    fn still_running_after_terminate_is_not_success() {
        let report = stop_owned_process(
            53132,
            IMAGE,
            StopKind::Daemon,
            |_| Graceful::Continue("pid still running after shutdown".to_owned()),
            |_| Terminate::StillRunning,
            || {},
        );
        assert!(report.failed);
        let text = report.lines.join("\n");
        assert!(text.contains("still running after terminate"));
        assert!(text.contains("installed version unchanged"));
        assert!(!text.contains("terminate_sent"));
    }

    #[test]
    fn helper_stop_does_not_ask_the_daemon_to_shut_down() {
        let mut graceful_calls = 0;
        let report = stop_owned_process(
            9,
            r"C:\npm\terminal-commander-mcp.exe",
            StopKind::Other,
            |_| {
                graceful_calls += 1;
                Graceful::Exited
            },
            |_| Terminate::Exited,
            || panic!("pause"),
        );
        assert!(!report.failed);
        assert_eq!(graceful_calls, 0);
        assert!(report.lines[0].contains("stopped pid 9"));
    }

    #[test]
    fn helper_access_denied_does_not_suggest_session_reap() {
        let report = stop_owned_process(
            9,
            r"C:\npm\terminal-commander-mcp.exe",
            StopKind::Other,
            |_| Graceful::Exited,
            |_| denied(),
            || {},
        );
        assert!(report.failed);
        let text = report.lines.join("\n");
        assert!(text.contains("next: close the program"));
        assert!(!text.contains("session reap"));
        assert!(text.contains("last resort:"));
    }

    #[test]
    fn non_access_denied_terminate_error_does_not_retry() {
        let mut attempts = 0;
        let mut pauses = 0;
        let report = stop_owned_process(
            9,
            r"C:\npm\terminal-commander-mcp.exe",
            StopKind::Other,
            |_| Graceful::Exited,
            |_| {
                attempts += 1;
                Terminate::Failed("invalid parameter".to_owned())
            },
            || pauses += 1,
        );
        assert!(report.failed);
        assert_eq!(attempts, 1);
        assert_eq!(pauses, 0);
        assert!(report.lines[0].contains("invalid parameter"));
    }

    #[test]
    fn missing_shutdown_event_still_counts_a_pipe_exit() {
        let outcome = graceful_outcome(EventTry::Missing, Ok(()), false, true);
        assert!(matches!(outcome, Graceful::Exited));
    }

    #[test]
    fn signaled_event_that_exits_the_daemon_skips_the_pipe_error() {
        let outcome = graceful_outcome(
            EventTry::Signaled,
            Err("shutdown pipe: Access is denied".to_owned()),
            true,
            false,
        );
        assert!(matches!(outcome, Graceful::Exited));
    }

    #[test]
    fn both_shutdown_channels_denied_keeps_both_reasons() {
        let outcome = graceful_outcome(
            EventTry::Failed("Access is denied. (0x80070005)".to_owned()),
            Err("no live session pidfile for pid 53132".to_owned()),
            false,
            false,
        );
        let Graceful::Continue(reason) = outcome else {
            panic!("expected continue");
        };
        assert!(reason.contains("shutdown event: Access is denied"));
        assert!(reason.contains("no live session pidfile"));
    }

    #[test]
    fn accepted_shutdown_pipe_is_the_product_namespace_only() {
        assert!(accepted_shutdown_pipe(r"\\.\pipe\terminal-commander-poslj"));
        assert!(!accepted_shutdown_pipe(r"\\.\pipe\terminal-commander"));
        assert!(!accepted_shutdown_pipe(
            r"\\.\pipe\other-terminal-commander-x"
        ));
        assert!(!accepted_shutdown_pipe(
            r"\\.\pipe\terminal-commander-..\evil"
        ));
        assert!(!accepted_shutdown_pipe(r"C:\Users\me\daemon.sock"));
    }
}
