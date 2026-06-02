// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Windows-only regression: daemon runtime spawns must not allocate a console.
//!
//! Uses `AttachConsole` + `GetConsoleWindow` (not desktop pixel checks). Includes a
//! positive control (spawn without `CREATE_NO_WINDOW`) so failures are not vacuous.

#![cfg(windows)]
// Win32 console FFI for AttachConsole/GetConsoleWindow/FreeConsole regression.
// Each call has a per-op SAFETY comment.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::process::Command;
use std::sync::Arc;

use terminal_commander_core::{BucketId, ContextRingManager, windows_silent};
use terminal_commander_probes::{DEFAULT_GRACE, InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};

const ERROR_INVALID_HANDLE: u32 = 6;

/// Detach the test runner's own console once so each test can `AttachConsole`
/// to a child PID. Without this, the test process already has a console attached
/// and `AttachConsole` returns 0 with `ERROR_ACCESS_DENIED` regardless of the
/// child's console state, making the positive control and negative case
/// indistinguishable. Idempotent.
fn detach_test_console() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: FreeConsole detaches the calling process from its console.
        // The test runner does not require its console after this point.
        let _ = unsafe { FreeConsole() };
    });
}

/// ~1s child lifetime so we can attach before exit.
const LONG_LIVED_CMD: &[&str] = &["/c", "ping", "-n", "2", "127.0.0.1"];

fn child_console_hwnd(pid: u32) -> Option<usize> {
    // SAFETY: AttachConsole takes an OS pid; failure is reported via zero return
    // and we bail before touching console state. GetConsoleWindow has no
    // preconditions. FreeConsole is paired with the prior AttachConsole.
    let attached = unsafe { AttachConsole(pid) };
    if attached == 0 {
        return None;
    }
    // SAFETY: see comment above; we hold the attached console for this call.
    let hwnd = unsafe { GetConsoleWindow() };
    // SAFETY: paired detach for the AttachConsole above.
    let _ = unsafe { FreeConsole() };
    if hwnd.is_null() {
        None
    } else {
        Some(hwnd as usize)
    }
}

fn child_has_no_console(pid: u32) -> bool {
    // SAFETY: AttachConsole takes an OS pid; we only inspect the success/failure
    // return code and clean up via FreeConsole on the success branch.
    let attached = unsafe { AttachConsole(pid) };
    if attached != 0 {
        // SAFETY: paired detach for the AttachConsole above.
        let _ = unsafe { FreeConsole() };
        return false;
    }
    // SAFETY: GetLastError is thread-local and has no preconditions.
    let err = unsafe { GetLastError() };
    err == ERROR_INVALID_HANDLE
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
#[ignore = "AttachConsole behavior depends on the test runner's console session; CI-only via cargo test -- --ignored"]
fn positive_control_unflagged_spawn_allocates_console() {
    detach_test_console();
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
#[ignore = "AttachConsole behavior depends on the test runner's console session; CI-only via cargo test -- --ignored"]
fn windows_silent_spawn_has_no_console() {
    detach_test_console();
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
#[ignore = "AttachConsole behavior depends on the test runner's console session; CI-only via cargo test -- --ignored"]
fn process_probe_spawn_silent_and_captures_stdio() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    detach_test_console();
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

        let pid = probe.child_pid();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            child_has_no_console(pid),
            "ProcessProbe child pid {pid} must not have a console (AttachConsole -> ERROR_INVALID_HANDLE)"
        );

        let _ = probe.wait().await.expect("probe wait");
        let m = probe.metrics();
        assert!(
            m.frames_stdout >= 1,
            "stdout frames captured with CREATE_NO_WINDOW: {}",
            m.frames_stdout
        );
    });
}

/// Regression (the env-overlay bug): a NON-EMPTY env must NOT strip the
/// OS-essential environment from the child. Before the overlay fix the spawn
/// did `env_clear()` then set only the supplied vars, so the child lost
/// `SystemRoot` (and `PATH`); a Windows binary like `node` then aborted at
/// startup (exit 134, crypto/AES init) or, with `SystemRoot` gone, exited 1.
///
/// We spawn `node -e "process.exit(process.env.SystemRoot ? 0 : 1)"` with a
/// supplied `TCENV` entry. Under overlay the child's environment block inherits
/// the parent env, so `SystemRoot` is present -> exit 0. Under the old REPLACE
/// the child env block held only `TCENV`, so `SystemRoot` was gone -> exit 1
/// (or a 134 crash in Node's crypto init). (argv[0] program resolution uses the
/// parent process PATH either way; the bug was the stripped child env BLOCK.)
///
/// Skips (does not fail) when `node` is not on PATH, so a box or CI runner
/// without Node degrades gracefully instead of red-failing.
#[test]
fn env_overlay_preserves_windows_system_env() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping env_overlay_preserves_windows_system_env: `node` not on PATH");
        return;
    }
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
            "node".to_owned(),
            "-e".to_owned(),
            "process.exit(process.env.SystemRoot ? 0 : 1)".to_owned(),
        ];
        let mut probe = ProcessProbe::spawn(
            &argv,
            &ProcessProbeConfig {
                probe_id: None,
                bucket_id: bucket,
                cwd: None,
                env: vec![(OsString::from("TCENV"), OsString::from("bar"))],
                grace: DEFAULT_GRACE,
            },
            rings,
            sifter,
            sink,
        )
        .expect("spawn node probe");

        let status = probe.wait().await.expect("probe wait");
        assert!(
            status.success(),
            "non-empty env must OVERLAY (inherit parent env): child should see \
             SystemRoot and exit 0, got {status:?} (REPLACE would give exit 1 or a 134 crash)"
        );
    });
}
