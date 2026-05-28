// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Windows-only regression: daemon runtime spawns must not allocate a console.
//!
//! Uses `AttachConsole` + `GetConsoleWindow` (not desktop pixel checks). Includes a
//! positive control (spawn without `CREATE_NO_WINDOW`) so failures are not vacuous.

#![cfg(windows)]

use std::process::Command;
use std::sync::Arc;

use terminal_commander_core::{BucketId, ContextRingManager, windows_silent};
use terminal_commander_probes::{DEFAULT_GRACE, InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};

const ERROR_INVALID_HANDLE: u32 = 6;

/// ~1s child lifetime so we can attach before exit.
const LONG_LIVED_CMD: &[&str] = &["/c", "ping", "-n", "2", "127.0.0.1"];

fn child_console_hwnd(pid: u32) -> Option<isize> {
    // SAFETY: Win32 console attach/detach for an existing child PID.
    unsafe {
        if AttachConsole(pid) == 0 {
            return None;
        }
        let hwnd = GetConsoleWindow();
        let _ = FreeConsole();
        if hwnd == 0 { None } else { Some(hwnd) }
    }
}

fn child_has_no_console(pid: u32) -> bool {
    // SAFETY: Win32 console attach for an existing child PID.
    unsafe {
        if AttachConsole(pid) != 0 {
            let _ = FreeConsole();
            return false;
        }
        GetLastError() == ERROR_INVALID_HANDLE
    }
}

fn spawn_cmd(silent: bool) -> (std::process::Child, u32) {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(LONG_LIVED_CMD);
    if silent {
        windows_silent(&mut cmd);
    }
    let child = cmd.spawn().expect("spawn cmd");
    let pid = child.id();
    (child, pid)
}

/// Positive control: without `CREATE_NO_WINDOW`, the child gets a console.
#[test]
fn positive_control_unflagged_spawn_allocates_console() {
    let (mut child, pid) = spawn_cmd(false);
    std::thread::sleep(std::time::Duration::from_millis(100));
    let hwnd = child_console_hwnd(pid);
    assert!(
        hwnd.is_some(),
        "expected console HWND on unflagged child (pid {pid})"
    );
    let _ = child.kill();
    let _ = child.wait();
}

/// Production helper: flagged spawn must not attach a console.
#[test]
fn windows_silent_spawn_has_no_console() {
    let (mut child, pid) = spawn_cmd(true);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(
        child_has_no_console(pid),
        "flagged child pid {pid} must not have a console (AttachConsole -> ERROR_INVALID_HANDLE)"
    );
    let _ = child.kill();
    let _ = child.wait();
}

/// S1 path: `ProcessProbe::spawn` applies `windows_silent` and still pipes stdio.
#[test]
fn process_probe_spawn_silent_and_captures_stdio() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let rings = Arc::new(ContextRingManager::new());
        let bucket = BucketId::new();
        let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
        let sink: Arc<dyn terminal_commander_probes::EventSink> = Arc::new(InMemorySink::new());
        let argv = vec![
            "cmd.exe".to_owned(),
            "/c".to_owned(),
            "echo TC_STDIO_PROBE".to_owned(),
            "&".to_owned(),
            "ping".to_owned(),
            "-n".to_owned(),
            "2".to_owned(),
            "127.0.0.1".to_owned(),
        ];
        let mut probe = ProcessProbe::spawn(
            &argv,
            &ProcessProbeConfig {
                probe_id: None,
                bucket_id: bucket,
                cwd: None,
                env: Vec::new(),
                grace: DEFAULT_GRACE,
            },
            rings,
            sifter,
            sink,
        )
        .expect("spawn probe");

        let _ = probe.wait().await.expect("probe wait");
        let m = probe.metrics();
        assert!(
            m.frames_stdout >= 1,
            "stdout frames captured with CREATE_NO_WINDOW: {}",
            m.frames_stdout
        );
    });
}
