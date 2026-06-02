// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Windows-only regression: daemon runtime spawns must not allocate a console
//! window.
//!
//! ## What the daemon promises
//!
//! GUI-subsystem daemon children spawned with `windows_silent`
//! (`CREATE_NO_WINDOW`) must not pop a visible console window (outward-filter
//! leakage), while ordinary console children still get one. This regression
//! proves both halves so a "no window" result is never vacuous.
//!
//! ## Why this uses `GetConsoleWindow`, not `AttachConsole`-success
//!
//! Empirically, on Windows 11 / ConPTY-hosted test hosts:
//!
//! * `AttachConsole(pid)` *succeeds* for BOTH a `CREATE_NO_WINDOW` child and an
//!   ordinary child. `CREATE_NO_WINDOW` suppresses the console *window*, not the
//!   console *screen buffer* — the silent child still owns a console object you
//!   can attach to. So "AttachConsole fails => no console" is NOT a reliable
//!   silent-vs-loud discriminator here; it returns "has console" for both.
//! * `GetConsoleProcessList` likewise lists BOTH children — also useless as a
//!   discriminator.
//! * `GetConsoleWindow()` (the HWND of the console *window*) is the one signal
//!   that separates them: it is a real top-level conhost window for the
//!   unflagged child (non-null) and `NULL` for the `CREATE_NO_WINDOW` child,
//!   deterministically across runs. That is exactly the product invariant —
//!   "no visible console window leaks".
//!
//! The detector therefore attaches to the child and inspects the console
//! *window handle*: non-null == has a visible console window; null == none.
//!
//! ## Timing
//!
//! An unflagged `cmd.exe` allocates its conhost window ~180-210ms after
//! `spawn()` returns; a fixed short sleep races the child's startup and
//! observes "no window yet". The detector polls (bounded) until the window
//! appears rather than sleeping a fixed interval, so the positive control is
//! not flaky.
//!
//! ## Concurrency
//!
//! `AttachConsole`/`FreeConsole` mutate process-wide console state. libtest
//! runs `#[test]`s on parallel threads by default, so the attach/detach
//! sections are serialized with a process-wide mutex; otherwise one test's
//! attach would corrupt another's spawn inheritance.

#![cfg(windows)]
// Win32 console FFI for AttachConsole/GetConsoleWindow/FreeConsole regression.
// Each call has a per-op SAFETY comment.
#![allow(unsafe_code)]

use std::process::Command;
use std::sync::{Arc, Mutex};

use terminal_commander_core::{BucketId, ContextRingManager, windows_silent};
use terminal_commander_probes::{DEFAULT_GRACE, InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;
use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};

/// Serializes the process-wide `AttachConsole`/`FreeConsole` sections so
/// libtest's default parallel threads do not corrupt each other's console
/// inheritance. Held only around an attach -> inspect -> detach window.
static CONSOLE_LOCK: Mutex<()> = Mutex::new(());

/// Detach the test runner's own console once so each test can `AttachConsole`
/// to a child PID. Without this, the test process already has a console
/// attached and `AttachConsole` returns 0 with `ERROR_ACCESS_DENIED`
/// regardless of the child's console state, making the positive control and
/// negative case indistinguishable. Idempotent.
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

/// Attach to `pid`'s console (if any) and report whether it owns a visible
/// console *window* (`GetConsoleWindow` non-null). Returns `None` when no
/// console can be attached at all (child has neither buffer nor window).
///
/// Serialized via `CONSOLE_LOCK` because attach/detach is process-wide state.
fn child_has_console_window(pid: u32) -> Option<bool> {
    let _guard = CONSOLE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: AttachConsole takes an OS pid; failure is reported via zero return
    // and we bail before touching console state.
    let attached = unsafe { AttachConsole(pid) };
    if attached == 0 {
        return None;
    }
    // SAFETY: we hold the attached console for this call; GetConsoleWindow has
    // no preconditions and returns the console window handle (NULL if none).
    let hwnd = unsafe { GetConsoleWindow() };
    // SAFETY: paired detach for the AttachConsole above.
    let _ = unsafe { FreeConsole() };
    Some(!hwnd.is_null())
}

/// Poll until the child is observed to OWN a visible console window, or the
/// budget elapses. Returns `true` if a window was ever seen. Used by the
/// positive control: an ordinary child's conhost window appears ~200ms after
/// spawn, so a fixed short sleep would race it.
fn poll_child_has_window(pid: u32, budget: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    loop {
        if child_has_console_window(pid) == Some(true) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Poll for the window-leak budget and assert the child NEVER owns a console
/// window. A silent child must never show a window at any point in its life,
/// so observing "no window" briefly is not enough — we watch for the same
/// budget the positive control uses, then require it stayed window-less.
fn assert_child_never_has_window(pid: u32, budget: std::time::Duration, ctx: &str) {
    let deadline = std::time::Instant::now() + budget;
    loop {
        assert_ne!(
            child_has_console_window(pid),
            Some(true),
            "{ctx}: child pid {pid} unexpectedly owns a visible console window \
             (CREATE_NO_WINDOW must suppress the window)"
        );
        if std::time::Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Window must appear within this budget for the positive control (~200ms
/// observed; 3s is generous headroom for slow/loaded hosts).
const WINDOW_BUDGET: std::time::Duration = std::time::Duration::from_secs(3);
/// How long to watch a silent child to be confident it never pops a window.
const NO_WINDOW_WATCH: std::time::Duration = std::time::Duration::from_millis(800);

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

/// Positive control: without `CREATE_NO_WINDOW`, the child gets a visible
/// console window. Proves the negative cases are not vacuous — the detector
/// CAN observe a window when one exists.
#[test]
fn positive_control_unflagged_spawn_allocates_console() {
    detach_test_console();
    let (mut child, pid) = spawn_cmd(false);
    let has_window = poll_child_has_window(pid, WINDOW_BUDGET);
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        has_window,
        "expected a visible console window on unflagged child (pid {pid}); \
         detector never observed GetConsoleWindow != NULL within {WINDOW_BUDGET:?}"
    );
}

/// Production helper: flagged spawn must not pop a console window.
#[test]
fn windows_silent_spawn_has_no_console() {
    detach_test_console();
    let (mut child, pid) = spawn_cmd(true);
    assert_child_never_has_window(pid, NO_WINDOW_WATCH, "windows_silent spawn");
    let _ = child.kill();
    let _ = child.wait();
}

/// S1 path: `ProcessProbe::spawn` applies `windows_silent` (no console window)
/// and still pipes stdio.
#[test]
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
        assert_child_never_has_window(pid, NO_WINDOW_WATCH, "ProcessProbe silent spawn");

        let _ = probe.wait().await.expect("probe wait");
        let m = probe.metrics();
        assert!(
            m.frames_stdout >= 1,
            "stdout frames captured with CREATE_NO_WINDOW: {}",
            m.frames_stdout
        );
    });
}
