// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

#![cfg(any(target_os = "linux", windows))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use terminal_commander_core::{BucketId, ContextRingManager, ProbeId};
use terminal_commander_probes::job_cpu::{JobCpuAccounting, JobCpuState, JobCpuUnknown};
use terminal_commander_probes::{InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("tc-job-cpu-{}", ProbeId::new()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        // Also releases descendants if an assertion panics before probe cleanup.
        let _ = std::fs::write(self.0.join("release"), b"");
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture_child(role: &str) -> std::process::Child {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "cpu_fixture_entry", "--nocapture"]);
    command.env("TC_CPU_FIXTURE_ROLE", role);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    command.spawn().unwrap()
}

fn wait_file_sync(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(Instant::now() < deadline, "fixture handshake timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Reentered only by subprocesses launched by the tests below.
#[test]
// The leader-exit fixture deliberately leaves its descendant alive; ProcessProbe owns cleanup.
#[allow(clippy::zombie_processes)]
fn cpu_fixture_entry() {
    let Ok(role) = std::env::var("TC_CPU_FIXTURE_ROLE") else {
        return;
    };
    let dir = PathBuf::from(std::env::var_os("TC_CPU_FIXTURE_DIR").unwrap());
    match role.as_str() {
        "leader" => {
            let mut middle = fixture_child("middle");
            if std::env::var_os("TC_CPU_LEADER_EXIT").is_some() {
                wait_file_sync(&dir.join("leader-exit"));
            } else {
                if std::env::var_os("TC_CPU_ONCE").is_some() {
                    wait_file_sync(&dir.join("release"));
                }
                // In the continuous-work case this is a blocking kernel wait:
                // the leader itself performs no periodic polling or CPU work.
                assert!(middle.wait().unwrap().success());
            }
        }
        "middle" => {
            let mut busy = fixture_child("busy");
            assert!(busy.wait().unwrap().success());
            std::fs::write(dir.join("descendants-exited"), b"").unwrap();
        }
        "busy" => {
            std::fs::write(dir.join("ready"), std::process::id().to_string()).unwrap();
            wait_file_sync(&dir.join("start-work"));
            let once = std::env::var_os("TC_CPU_ONCE").is_some();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                for value in 0_u64..1_000_000 {
                    std::hint::black_box(value.wrapping_mul(value));
                }
                if once || dir.join("release").exists() || Instant::now() >= deadline {
                    break;
                }
            }
        }
        _ => panic!("invalid CPU fixture role"),
    }
}

fn spawn_fixture(dir: &Path, leader_exit: bool, once: bool) -> ProcessProbe {
    let mut config = ProcessProbeConfig::for_bucket(BucketId::new());
    // The leader-exit case samples during the intentional pipe-drain grace;
    // it must outlast the sampler's 100 ms minimum retained interval.
    config.grace = if leader_exit {
        Duration::from_secs(3)
    } else {
        Duration::from_millis(50)
    };
    config.env = vec![
        ("TC_CPU_FIXTURE_ROLE".into(), "leader".into()),
        ("TC_CPU_FIXTURE_DIR".into(), dir.as_os_str().to_owned()),
    ];
    if leader_exit {
        config.env.push(("TC_CPU_LEADER_EXIT".into(), "1".into()));
    }
    if once {
        config.env.push(("TC_CPU_ONCE".into(), "1".into()));
    }
    ProcessProbe::spawn(
        &[
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            "--exact".into(),
            "cpu_fixture_entry".into(),
            "--nocapture".into(),
        ],
        &config,
        Arc::new(ContextRingManager::new()),
        Arc::new(SifterRuntime::build(&[]).unwrap()),
        Arc::new(InMemorySink::new()),
    )
    .unwrap()
}

async fn wait_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("fixture readiness handshake");
}

async fn wait_busy(probe: &ProcessProbe) -> terminal_commander_core::job_cpu::JobCpuSample {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let sample = probe.cpu_sample();
            if matches!(sample.state, JobCpuState::Known { percent } if percent > 0.0) {
                return sample;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("busy descendant must contribute to whole-job CPU")
}

#[tokio::test]
async fn idle_leader_reports_silent_busy_grandchild() {
    let dir = FixtureDir::new();
    let mut probe = spawn_fixture(&dir.0, false, false);
    wait_file(&dir.0.join("ready")).await;
    let first = probe.cpu_sample();
    assert_eq!(
        first.state,
        JobCpuState::Unknown(JobCpuUnknown::FirstSample)
    );
    assert!(first.interval.is_none());
    std::fs::write(dir.0.join("start-work"), b"").unwrap();
    let sample = wait_busy(&probe).await;
    assert_eq!(sample.probe_id, first.probe_id);
    assert_eq!(sample.leader_pid, probe.child_pid());
    assert!(sample.interval.unwrap() >= Duration::from_millis(100));
    #[cfg(target_os = "linux")]
    {
        assert_eq!(sample.accounting, JobCpuAccounting::LinuxProcessGroup);
        assert!(sample.leader_start_ticks.is_some());
    }
    #[cfg(windows)]
    assert_eq!(sample.accounting, JobCpuAccounting::WindowsJobObject);
    std::fs::write(dir.0.join("release"), b"").unwrap();
    assert!(probe.wait().await.unwrap().success());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn exited_leader_does_not_hide_a_busy_descendant() {
    let dir = FixtureDir::new();
    let mut probe = spawn_fixture(&dir.0, true, false);
    wait_file(&dir.0.join("ready")).await;
    let _ = probe.cpu_sample();
    std::fs::write(dir.0.join("start-work"), b"").unwrap();
    wait_busy(&probe).await;
    std::fs::write(dir.0.join("leader-exit"), b"").unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", probe.child_pid()))
                .expect("the exited leader remains as the process-group identity anchor");
            let state = stat.rsplit_once(')').unwrap().1.split_whitespace().next();
            if matches!(state, Some("Z" | "X")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("leader must exit while descendant keeps pipes open");
    wait_busy(&probe).await;
    std::fs::write(dir.0.join("release"), b"").unwrap();
    assert!(probe.wait().await.unwrap().success());
}

#[cfg(windows)]
#[tokio::test]
async fn job_object_retains_cpu_of_exited_descendants() {
    let dir = FixtureDir::new();
    let mut probe = spawn_fixture(&dir.0, false, true);
    wait_file(&dir.0.join("ready")).await;
    assert_eq!(
        probe.cpu_sample().state,
        JobCpuState::Unknown(JobCpuUnknown::FirstSample)
    );
    std::fs::write(dir.0.join("start-work"), b"").unwrap();
    wait_file(&dir.0.join("descendants-exited")).await;
    // Both descendants have exited and the leader only waits on a file. Their
    // consumed CPU still belongs to the Job Object's retained interval.
    let sample = wait_busy(&probe).await;
    assert_eq!(sample.accounting, JobCpuAccounting::WindowsJobObject);
    std::fs::write(dir.0.join("release"), b"").unwrap();
    assert!(probe.wait().await.unwrap().success());
}
