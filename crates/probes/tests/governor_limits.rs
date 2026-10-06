// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Resource governor enforcement (crates/probes/src/governor.rs).
//!
//! The allocation workload is THIS test binary re-invoked with
//! `--exact alloc_helper --nocapture` and `TC_TEST_ALLOC_MIB=<n>`: the helper
//! is a `#[test]` that returns at once unless the env var is set, then commits
//! and touches `n` MiB in 1 MiB chunks. No python, powershell or extra binary.
//!
//! Linux runs in whichever mode the host offers (cgroup sibling when the
//! daemon's parent cgroup is writable, else `RLIMIT_DATA`) and prints it.

use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{BucketId, ContextRingManager};
use terminal_commander_probes::governor::{GovernorMode, JobLimits, host_memory};
use terminal_commander_probes::{EventSink, InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;

const ALLOC_ENV: &str = "TC_TEST_ALLOC_MIB";
const LIMIT: u64 = 100 * 1024 * 1024;
const OVER_MIB: &str = "300";

/// Workload, not a check: idle unless `TC_TEST_ALLOC_MIB` is set.
#[test]
fn alloc_helper() {
    let Ok(mib) = std::env::var(ALLOC_ENV) else {
        return;
    };
    let mib: usize = mib.parse().expect("TC_TEST_ALLOC_MIB is a number");
    let mut chunks = Vec::with_capacity(mib);
    for _ in 0..mib {
        let mut chunk = vec![0u8; 1 << 20];
        for i in (0..chunk.len()).step_by(4096) {
            chunk[i] = 1;
        }
        chunks.push(chunk);
    }
    std::hint::black_box(&chunks);
    std::thread::sleep(Duration::from_millis(200));
    println!("ALLOC_OK {mib}");
}

fn deps() -> (
    Arc<ContextRingManager>,
    Arc<SifterRuntime>,
    Arc<dyn EventSink>,
) {
    let rings = Arc::new(ContextRingManager::new());
    let sifter = Arc::new(SifterRuntime::build(&[]).expect("empty sifter builds"));
    let sink: Arc<dyn EventSink> = Arc::new(InMemorySink::new());
    (rings, sifter, sink)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime builds")
}

fn helper_argv() -> Vec<String> {
    let exe = std::env::current_exe().expect("test exe path");
    vec![
        exe.to_string_lossy().into_owned(),
        "--exact".to_owned(),
        "alloc_helper".to_owned(),
        "--nocapture".to_owned(),
    ]
}

/// The helper wrapped in a shell so the allocating process is a GRANDCHILD.
fn forked_helper_argv() -> Vec<String> {
    let direct = helper_argv();
    #[cfg(windows)]
    {
        let mut argv = vec!["cmd".to_owned(), "/c".to_owned()];
        argv.extend(direct);
        argv
    }
    #[cfg(unix)]
    {
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "\"$0\" --exact alloc_helper --nocapture".to_owned(),
            direct[0].clone(),
        ]
    }
}

const fn governed() -> JobLimits {
    JobLimits {
        memory_bytes: Some(LIMIT),
        priority: None,
    }
}

/// Run `argv` under `limits` allocating `mib`; return (exit success, report).
fn run(
    argv: &[String],
    limits: JobLimits,
    mib: &str,
) -> (bool, terminal_commander_probes::governor::GovernorReport) {
    rt().block_on(async {
        let (rings, sifter, sink) = deps();
        let cfg = ProcessProbeConfig {
            env: vec![(ALLOC_ENV.into(), mib.into())],
            limits,
            ..ProcessProbeConfig::for_bucket(BucketId::new())
        };
        let mut probe = ProcessProbe::spawn(argv, &cfg, rings, sifter, sink).expect("spawn");
        let status = tokio::time::timeout(Duration::from_mins(2), probe.wait())
            .await
            .expect("child exits within 120s")
            .expect("wait ok");
        (status.success(), probe.governor_report())
    })
}

fn assert_governed_failure(ok: bool, report: &terminal_commander_probes::governor::GovernorReport) {
    println!("governor report: {report:?} exit_ok={ok}");
    assert!(!ok, "300 MiB alloc under a 100 MiB limit must fail");
    assert_eq!(report.memory_limit_bytes, Some(LIMIT));
    match report.mode.as_ref().expect("governed job has a mode") {
        GovernorMode::JobObject => {
            let peak = report.peak_memory_bytes.expect("job object reports peak");
            assert!(peak >= LIMIT, "peak {peak} < limit {LIMIT}");
            assert!(report.memory_limit_hit);
        }
        GovernorMode::Cgroup => {
            println!("linux governor mode: cgroup");
            assert!(report.memory_limit_hit, "cgroup oom_kill must be counted");
        }
        GovernorMode::Rlimit => {
            println!("linux governor mode: rlimit");
            assert!(!report.memory_limit_hit, "rlimit cannot know the hit");
        }
        GovernorMode::Unavailable(reason) => panic!("governor unavailable: {reason}"),
    }
}

#[cfg(any(windows, unix))]
#[test]
fn over_limit_allocation_is_stopped() {
    let (ok, report) = run(&helper_argv(), governed(), OVER_MIB);
    assert_governed_failure(ok, &report);
}

/// Tree containment: the allocating process is a grandchild. Job Object and
/// cgroup sum the tree; rlimit is inherited and still enforced per process.
#[cfg(any(windows, unix))]
#[test]
fn forked_child_allocation_is_stopped() {
    let (ok, report) = run(&forked_helper_argv(), governed(), OVER_MIB);
    assert_governed_failure(ok, &report);
}

#[test]
fn default_limits_are_ungoverned() {
    let (ok, report) = run(&helper_argv(), JobLimits::default(), OVER_MIB);
    assert!(ok, "ungoverned 300 MiB alloc must succeed");
    assert_eq!(report.mode, None);
    assert_eq!(
        report,
        terminal_commander_probes::governor::GovernorReport::default()
    );
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn host_memory_reports_total() {
    let host = host_memory().expect("host memory available");
    assert!(host.total_bytes > 0);
    #[cfg(windows)]
    assert!(host.commit_limit_bytes.is_some());
    let mode = terminal_commander_probes::governor::available_mode();
    println!("available_mode: {mode:?}");
    #[cfg(windows)]
    assert_eq!(mode, GovernorMode::JobObject);
    #[cfg(target_os = "linux")]
    assert!(matches!(mode, GovernorMode::Cgroup | GovernorMode::Rlimit));
}

/// Windows PTY lane: the ConPTY child is governed by its own Job Object.
#[cfg(windows)]
#[test]
fn conpty_child_over_limit_is_stopped() {
    use terminal_commander_probes::{PtyExitOutcome, PtyProbe, PtyProbeConfig};
    rt().block_on(async {
        let (rings, sifter, sink) = deps();
        let cfg = PtyProbeConfig {
            env: vec![(ALLOC_ENV.into(), OVER_MIB.into())],
            limits: governed(),
            ..PtyProbeConfig::for_bucket(BucketId::new())
        };
        let mut probe = PtyProbe::spawn(&helper_argv(), &cfg, rings, sifter, sink).expect("spawn");
        let completion = probe.take_completion().expect("completion receiver");
        let outcome = tokio::time::timeout(Duration::from_mins(2), completion)
            .await
            .expect("child exits within 120s")
            .expect("outcome sent");
        let report = probe.governor_report();
        println!("conpty outcome {outcome:?} report {report:?}");
        assert!(
            matches!(outcome, PtyExitOutcome::Exited { code: Some(c), .. } if c != 0),
            "governed ConPTY child must exit abnormally"
        );
        assert_eq!(report.mode, Some(GovernorMode::JobObject));
        assert_eq!(report.memory_limit_bytes, Some(LIMIT));
        let peak = report.peak_memory_bytes.expect("peak reported");
        assert!(peak >= LIMIT, "peak {peak} < limit {LIMIT}");
        assert!(report.memory_limit_hit);
    });
}
