// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

#![cfg(any(unix, windows))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{BucketId, ContextRingManager, ProbeId};
use terminal_commander_probes::{InMemorySink, ProcessCleanup, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;

struct Fixture {
    directory: PathBuf,
    leader: u32,
    descendant: u32,
}

impl Fixture {
    async fn start() -> (Self, ProcessProbe) {
        Self::start_with_grace(Duration::from_millis(100)).await
    }

    async fn start_with_grace(grace: Duration) -> (Self, ProcessProbe) {
        let directory = std::env::temp_dir().join(format!("tc-ownership-{}", ProbeId::new()));
        std::fs::create_dir(&directory).unwrap();
        let child = "import pathlib,os,sys,time,signal; signal.signal(signal.SIGTERM, signal.SIG_IGN); pathlib.Path(sys.argv[1]).write_text(str(os.getpid())); time.sleep(60)";
        let leader = "import pathlib,subprocess,sys,time,os\nd=pathlib.Path(sys.argv[1])\np=subprocess.Popen([sys.executable,'-c',sys.argv[2],str(d/'descendant')])\nwhile not (d/'descendant').exists(): time.sleep(.005)\n(d/'ready').touch()\nwhile not (d/'exit').exists(): time.sleep(.005)\nos._exit(0)";
        let mut config = ProcessProbeConfig::for_bucket(BucketId::new());
        config.grace = grace;
        let probe = ProcessProbe::spawn(
            &[
                "python3".into(),
                "-c".into(),
                leader.into(),
                directory.display().to_string(),
                child.into(),
            ],
            &config,
            Arc::new(ContextRingManager::new()),
            Arc::new(SifterRuntime::build(&[]).unwrap()),
            Arc::new(InMemorySink::new()),
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while !directory.join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("leader and descendant readiness handshake");
        let descendant = std::fs::read_to_string(directory.join("descendant"))
            .unwrap()
            .parse()
            .unwrap();
        let fixture = Self {
            directory,
            leader: probe.child_pid(),
            descendant,
        };
        assert!(running(fixture.leader));
        assert!(running(fixture.descendant));
        #[cfg(unix)]
        assert_eq!(probe.identity().process_group_id, Some(fixture.leader));
        (fixture, probe)
    }

    async fn assert_gone(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while running(self.leader)
                || running(self.descendant)
                || (cfg!(target_os = "linux")
                    && std::path::Path::new(&format!("/proc/{}/stat", self.leader)).exists())
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("leader and descendant must be stopped");
        #[cfg(unix)]
        assert!(
            !std::path::Path::new(&format!("/proc/{}/stat", self.leader)).exists(),
            "direct child must be reaped"
        );
    }

    fn assert_gone_without_runtime(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while running(self.leader) || running(self.descendant) {
            assert!(
                Instant::now() < deadline,
                "native cleanup must survive runtime shutdown"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[cfg(target_os = "linux")]
fn running(pid: u32) -> bool {
    // An orphan zombie has exited; its new parent owns reaping. The direct
    // leader is additionally checked for /proc removal after probe completion.
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(") ")
            .is_some_and(|(_, fields)| !fields.starts_with('Z'))
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
fn running(pid: u32) -> bool {
    // SAFETY: signal zero observes existence without signaling the process.
    unsafe { libc::kill(i32::try_from(pid).unwrap(), 0) == 0 }
}

#[cfg(windows)]
fn running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    // SAFETY: opened process handle is used only for a zero-time wait and closed exactly once.
    unsafe {
        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        alive
    }
}

#[tokio::test]
async fn explicit_cancel_is_idempotent_and_stops_the_descendant() {
    let (fixture, mut probe) = Fixture::start().await;
    probe.cancel();
    probe.cancel();
    let report = probe.wait_report().await.unwrap();
    assert!(report.cancelled);
    assert_eq!(report.cleanup, ProcessCleanup::Complete);
    fixture.assert_gone().await;
}

#[tokio::test]
async fn owner_drop_stops_the_descendant_with_a_live_runtime() {
    let (fixture, probe) = Fixture::start().await;
    drop(probe);
    fixture.assert_gone().await;
}

#[test]
fn runtime_shutdown_kills_the_tree_and_reaps_without_tokio() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (fixture, probe) = runtime.block_on(Fixture::start());
    let metrics = probe.metrics_handle();
    drop(runtime);
    fixture.assert_gone_without_runtime();
    let deadline = Instant::now() + Duration::from_secs(5);
    while metrics.lock().cleanup == ProcessCleanup::Reaping {
        assert!(Instant::now() < deadline, "native reaping must finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(metrics.lock().cleanup, ProcessCleanup::Complete);
    #[cfg(target_os = "linux")]
    assert!(!std::path::Path::new(&format!("/proc/{}/stat", fixture.leader)).exists());
    drop(probe);
}

#[tokio::test]
async fn aborting_the_wait_owner_stops_the_tree() {
    let (fixture, mut probe) = Fixture::start().await;
    let owner = tokio::spawn(async move { probe.wait_report().await });
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    fixture.assert_gone().await;
}

#[test]
fn shutdown_before_the_lifecycle_task_is_polled_still_reaps_the_child() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let directory = std::env::temp_dir().join(format!("tc-unpolled-{}", ProbeId::new()));
    std::fs::create_dir(&directory).unwrap();
    let ready = directory.join("ready");
    let probe = {
        let _entered = runtime.enter();
        ProcessProbe::spawn(
            &[
                "python3".into(),
                "-c".into(),
                "import pathlib,sys,time; pathlib.Path(sys.argv[1]).touch(); time.sleep(60)".into(),
                ready.display().to_string(),
            ],
            &ProcessProbeConfig::for_bucket(BucketId::new()),
            Arc::new(ContextRingManager::new()),
            Arc::new(SifterRuntime::build(&[]).unwrap()),
            Arc::new(InMemorySink::new()),
        )
        .unwrap()
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    // The OS child runs, but the current-thread Tokio runtime has never polled.
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "unpolled child readiness handshake"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = probe.child_pid();
    let metrics = probe.metrics_handle();
    drop(runtime);
    let deadline = Instant::now() + Duration::from_secs(5);
    while running(pid) || metrics.lock().cleanup == ProcessCleanup::Reaping {
        assert!(
            Instant::now() < deadline,
            "unpolled owner must reap independently"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(metrics.lock().cleanup, ProcessCleanup::Complete);
    drop(probe);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn exited_leader_with_descendant_holding_pipe_has_bounded_completion() {
    let (fixture, mut probe) = Fixture::start().await;
    std::fs::write(fixture.directory.join("exit"), []).unwrap();
    let report = tokio::time::timeout(Duration::from_secs(5), probe.wait_report())
        .await
        .unwrap()
        .unwrap();
    assert!(report.exit_status.success());
    assert!(!report.cancelled);
    assert!(report.observation.is_complete());
    assert_eq!(report.cleanup, ProcessCleanup::Complete);
    fixture.assert_gone().await;
}

#[tokio::test]
#[cfg(unix)]
async fn natural_signal_exit_is_preserved_in_the_report() {
    use std::os::unix::process::ExitStatusExt;
    let mut probe = ProcessProbe::spawn(
        &[
            "python3".into(),
            "-c".into(),
            "import os,signal; os.kill(os.getpid(),signal.SIGTERM)".into(),
        ],
        &ProcessProbeConfig::for_bucket(BucketId::new()),
        Arc::new(ContextRingManager::new()),
        Arc::new(SifterRuntime::build(&[]).unwrap()),
        Arc::new(InMemorySink::new()),
    )
    .unwrap();
    let report = probe.wait_report().await.unwrap();
    assert_eq!(report.exit_status.signal(), Some(libc::SIGTERM));
    assert!(!report.cancelled);
    assert!(report.observation.is_complete());
}

#[cfg(target_os = "linux")]
async fn assert_leader_remains_waitable(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while running(fixture.leader) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("leader exit handshake");
    // WNOWAIT checks the child is still ours without consuming its ownership anchor.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            fixture.leader,
            &raw mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    assert_eq!(
        result,
        0,
        "leader must remain unreaped while descendants are owned: {}",
        std::io::Error::last_os_error()
    );
    assert_eq!(
        unsafe { info.si_pid() },
        i32::try_from(fixture.leader).unwrap()
    );
    assert!(
        running(fixture.descendant),
        "descendant must retain the ownership window"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn natural_exit_retains_the_group_anchor_until_final_cleanup() {
    let (fixture, mut probe) = Fixture::start_with_grace(Duration::from_secs(2)).await;
    std::fs::write(fixture.directory.join("exit"), []).unwrap();
    assert_leader_remains_waitable(&fixture).await;
    probe.cancel();
    tokio::time::timeout(Duration::from_secs(5), probe.wait_report())
        .await
        .unwrap()
        .unwrap();
    fixture.assert_gone().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancellation_retains_the_group_anchor_through_descendant_grace() {
    let (fixture, mut probe) = Fixture::start_with_grace(Duration::from_secs(2)).await;
    probe.cancel();
    assert_leader_remains_waitable(&fixture).await;
    tokio::time::timeout(Duration::from_secs(5), probe.wait_report())
        .await
        .unwrap()
        .unwrap();
    fixture.assert_gone().await;
}
