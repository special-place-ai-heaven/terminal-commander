// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Resource governor enforcement (crates/probes/src/governor.rs).
//!
//! The allocation workload is THIS test binary re-invoked with
//! `--exact alloc_helper --nocapture` and `TC_TEST_ALLOC_MIB=<n>`: the helper
//! is a `#[test]` that returns at once unless the env var is set, then commits
//! and touches `n` MiB in 1 MiB chunks. No python, powershell or extra binary.
//! Optional helper env: `TC_TEST_ALLOC_ONESHOT` (one `n` MiB allocation
//! instead of chunks; a refused one exits 3), `TC_TEST_READY_FILE` (written
//! after the allocation),
//! `TC_TEST_HOLD_FILE` (hold the memory until it exists), `TC_TEST_HOLD_MS`
//! (hold it for a plain sleep: no syscall that could charge memory),
//! `TC_TEST_OOM_VICTIM` (Linux: raise `oom_score_adj` so the memcg OOM killer
//! picks this process), `TC_TEST_EXPECT_PRIORITY` (Windows: panic unless the
//! own priority class equals this value).
//!
//! Linux runs in whichever mode the host offers (cgroup sibling when the
//! daemon's parent cgroup is writable, else `RLIMIT_DATA`) and prints it.
//! Every test runs in its own process under nextest, so the process-global
//! host ceiling is installed at most once per process.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{BucketId, ContextRingManager};
use terminal_commander_probes::governor::{
    GovernorMode, GovernorReport, JobLimits, JobPriority, available_mode, host_ceiling,
    host_memory, install_host_ceiling,
};
use terminal_commander_probes::{EventSink, InMemorySink, ProcessProbe, ProcessProbeConfig};
use terminal_commander_sifters::SifterRuntime;

const ALLOC_ENV: &str = "TC_TEST_ALLOC_MIB";
const READY_ENV: &str = "TC_TEST_READY_FILE";
const HOLD_ENV: &str = "TC_TEST_HOLD_FILE";
const HOLD_MS_ENV: &str = "TC_TEST_HOLD_MS";
const OOM_VICTIM_ENV: &str = "TC_TEST_OOM_VICTIM";
#[cfg(windows)]
const EXPECT_PRIORITY_ENV: &str = "TC_TEST_EXPECT_PRIORITY";
const NESTED_ENV: &str = "TC_TEST_NESTED";
const ONESHOT_ENV: &str = "TC_TEST_ALLOC_ONESHOT";
#[cfg(target_os = "linux")]
const REMOVED_HOST_ENV: &str = "TC_TEST_REMOVED_HOST";
const MIB: u64 = 1024 * 1024;
const LIMIT: u64 = 100 * MIB;
const HOST_LIMIT: u64 = 150 * MIB;
const OVER_MIB: &str = "300";

/// Workload, not a check: idle unless `TC_TEST_ALLOC_MIB` is set.
#[test]
fn alloc_helper() {
    let Ok(mib) = std::env::var(ALLOC_ENV) else {
        return;
    };
    #[cfg(windows)]
    if let Ok(want) = std::env::var(EXPECT_PRIORITY_ENV) {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetPriorityClass};
        // SAFETY: the pseudo handle from GetCurrentProcess is always valid;
        // GetPriorityClass only reads it.
        let have = unsafe { GetPriorityClass(GetCurrentProcess()) };
        assert_eq!(have.to_string(), want, "priority class");
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os(OOM_VICTIM_ENV).is_some() {
        let _ = std::fs::write("/proc/self/oom_score_adj", "1000");
    }
    let mib: usize = mib.parse().expect("TC_TEST_ALLOC_MIB is a number");
    let mut chunks = Vec::with_capacity(mib);
    if std::env::var_os(ONESHOT_ENV).is_some() {
        let mut buf: Vec<u8> = Vec::new();
        if buf.try_reserve_exact(mib << 20).is_err() {
            eprintln!("ALLOC_REFUSED {mib}");
            std::process::exit(3);
        }
        buf.resize(mib << 20, 1);
        chunks.push(buf);
    }
    while chunks.len() < mib && std::env::var_os(ONESHOT_ENV).is_none() {
        let mut chunk = vec![0u8; 1 << 20];
        for i in (0..chunk.len()).step_by(4096) {
            chunk[i] = 1;
        }
        chunks.push(chunk);
    }
    std::hint::black_box(&chunks);
    if let Some(ready) = std::env::var_os(READY_ENV) {
        std::fs::write(ready, b"").expect("write ready file");
    }
    if let Some(hold) = std::env::var_os(HOLD_ENV) {
        let deadline = Instant::now() + Duration::from_mins(2);
        while !Path::new(&hold).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    if let Ok(ms) = std::env::var(HOLD_MS_ENV) {
        std::thread::sleep(Duration::from_millis(ms.parse().expect("hold ms")));
    }
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

fn self_argv(test: &str) -> Vec<String> {
    let exe = std::env::current_exe().expect("test exe path");
    vec![
        exe.to_string_lossy().into_owned(),
        "--exact".to_owned(),
        test.to_owned(),
        "--nocapture".to_owned(),
    ]
}

fn helper_argv() -> Vec<String> {
    self_argv("alloc_helper")
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
        join_host_ceiling: false,
    }
}

fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
}

fn config(limits: JobLimits, env: Vec<(OsString, OsString)>) -> ProcessProbeConfig {
    ProcessProbeConfig {
        env,
        limits,
        ..ProcessProbeConfig::for_bucket(BucketId::new())
    }
}

async fn finish(probe: &mut ProcessProbe) -> (bool, GovernorReport) {
    let status = tokio::time::timeout(Duration::from_mins(2), probe.wait())
        .await
        .expect("child exits within 120s")
        .expect("wait ok");
    (status.success(), probe.governor_report())
}

/// Run `argv` under `cfg`; return (exit success, report).
fn run_cfg(argv: &[String], cfg: &ProcessProbeConfig) -> (bool, GovernorReport) {
    rt().block_on(async {
        let (rings, sifter, sink) = deps();
        let mut probe = ProcessProbe::spawn(argv, cfg, rings, sifter, sink).expect("spawn");
        finish(&mut probe).await
    })
}

/// Run `argv` under `limits` allocating `mib`; return (exit success, report).
fn run(argv: &[String], limits: JobLimits, mib: &str) -> (bool, GovernorReport) {
    run_cfg(argv, &config(limits, env(&[(ALLOC_ENV, mib)])))
}

/// A fresh scratch dir for ready/release marker files.
fn scratch_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("tc-gov-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn assert_governed_failure(ok: bool, report: &GovernorReport) {
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

/// ONE large allocation the kernel refuses outright: the hit must come from
/// the kernel's own limit signal (Windows: the job's memory-limit message on
/// the completion port).
#[test]
fn single_refused_allocation_is_a_limit_hit() {
    let cfg = config(
        governed(),
        env(&[(ALLOC_ENV, OVER_MIB), (ONESHOT_ENV, "1")]),
    );
    let (ok, report) = run_cfg(&helper_argv(), &cfg);
    println!("one-shot report: {report:?} exit_ok={ok}");
    assert!(!ok, "one 300 MiB alloc under a 100 MiB limit must fail");
    match report.mode.as_ref().expect("governed job has a mode") {
        // Observed live: `PeakJobMemoryUsed` records the refused charge too,
        // so the peak is reported but not asserted here.
        GovernorMode::JobObject => {
            assert!(report.memory_limit_hit, "kernel limit message counted");
        }
        GovernorMode::Cgroup => assert!(report.memory_limit_hit, "cgroup oom counted"),
        GovernorMode::Rlimit => assert!(!report.memory_limit_hit, "rlimit cannot know"),
        GovernorMode::Unavailable(reason) => panic!("governor unavailable: {reason}"),
    }
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
    assert_eq!(report, GovernorReport::default());
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn host_memory_reports_total() {
    let host = host_memory().expect("host memory available");
    assert!(host.total_bytes > 0);
    #[cfg(windows)]
    assert!(host.commit_limit_bytes.is_some());
    let mode = available_mode();
    println!("available_mode: {mode:?}");
    #[cfg(windows)]
    assert_eq!(mode, GovernorMode::JobObject);
    #[cfg(target_os = "linux")]
    assert!(matches!(mode, GovernorMode::Cgroup | GovernorMode::Rlimit));
}

/// Mode, limit and host flag are final right after spawn; peak only at exit.
#[test]
fn mode_is_final_before_exit() {
    let dir = scratch_dir("mode");
    let release = dir.join("release");
    let release_s = release.to_string_lossy().into_owned();
    let cfg = config(governed(), env(&[(ALLOC_ENV, "1"), (HOLD_ENV, &release_s)]));
    rt().block_on(async {
        let (rings, sifter, sink) = deps();
        let mut probe =
            ProcessProbe::spawn(&helper_argv(), &cfg, rings, sifter, sink).expect("spawn");
        let early = probe.governor_report();
        println!("report before exit: {early:?}");
        #[cfg(windows)]
        assert_eq!(early.mode, Some(GovernorMode::JobObject));
        #[cfg(target_os = "linux")]
        assert_eq!(early.mode, Some(available_mode()));
        assert_eq!(early.memory_limit_bytes, Some(LIMIT));
        assert_eq!(early.peak_memory_bytes, None);
        assert!(!early.memory_limit_hit);
        assert!(!early.host_ceiling_joined);
        std::fs::write(&release, b"").expect("release");
        let (ok, late) = finish(&mut probe).await;
        println!("report after exit: {late:?}");
        assert!(ok, "1 MiB under 100 MiB succeeds");
        assert_eq!(late.mode, early.mode);
        #[cfg(windows)]
        assert!(late.peak_memory_bytes.is_some());
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// Two jobs, each under its 100 MiB per-job limit, both join a 150 MiB host
/// ceiling. The first holds 90 MiB; the second's 90 MiB pushes the host past
/// its ceiling and fails. Run twice in the same process: the second round
/// must be detected too, after the host ceiling was already reached once (a
/// never-resetting peak cannot see it). Rlimit mode has no aggregate
/// primitive: install returns `Err` and the scenario stops there.
fn host_ceiling_scenario() {
    let installed = install_host_ceiling(HOST_LIMIT);
    println!(
        "available_mode {:?} install_host_ceiling {installed:?}",
        available_mode()
    );
    #[cfg(target_os = "linux")]
    if available_mode() == GovernorMode::Rlimit {
        println!("linux governor mode: rlimit (no host ceiling)");
        assert!(installed.is_err(), "rlimit mode must refuse a host ceiling");
        assert_eq!(host_ceiling(), None);
        return;
    }
    let mode = installed.expect("host ceiling installs");
    #[cfg(windows)]
    assert_eq!(mode, GovernorMode::JobObject);
    #[cfg(target_os = "linux")]
    assert_eq!(mode, GovernorMode::Cgroup);
    assert_eq!(host_ceiling(), Some(HOST_LIMIT));
    assert!(
        install_host_ceiling(HOST_LIMIT).is_err(),
        "second install is refused"
    );

    let joined = JobLimits {
        join_host_ceiling: true,
        ..governed()
    };
    for round in 1..=2 {
        println!("host ceiling round {round}");
        host_ceiling_round(joined);
    }
    // The host dir lives for the daemon's life; this process is the daemon.
    #[cfg(target_os = "linux")]
    if let Some(parent) = test_parent_cgroup() {
        let _ = std::fs::remove_dir(parent.join(format!("tc-jobs-{}", std::process::id())));
    }
}

fn host_ceiling_round(joined: JobLimits) {
    let dir = scratch_dir("host");
    let ready = dir.join("ready").to_string_lossy().into_owned();
    rt().block_on(async {
        let (rings, sifter, sink) = deps();
        // The holder sleeps instead of polling: on Linux, any kernel
        // allocation it made while the OOM-killed pusher is still releasing
        // its memory triggers a second memcg OOM that picks the holder
        // (observed live in the kernel log).
        let holder_cfg = config(
            joined,
            env(&[
                (ALLOC_ENV, "90"),
                (READY_ENV, &ready),
                (HOLD_MS_ENV, "5000"),
            ]),
        );
        let mut holder = ProcessProbe::spawn(
            &helper_argv(),
            &holder_cfg,
            Arc::clone(&rings),
            Arc::clone(&sifter),
            Arc::clone(&sink),
        )
        .expect("spawn holder");
        assert!(holder.governor_report().host_ceiling_joined);
        let deadline = Instant::now() + Duration::from_mins(1);
        while !Path::new(&ready).exists() {
            assert!(Instant::now() < deadline, "holder never allocated 90 MiB");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let pusher_cfg = config(joined, env(&[(ALLOC_ENV, "90"), (OOM_VICTIM_ENV, "1")]));
        let mut pusher = ProcessProbe::spawn(&helper_argv(), &pusher_cfg, rings, sifter, sink)
            .expect("spawn pusher");
        let (pusher_ok, pusher_report) = finish(&mut pusher).await;
        assert_eq!(
            holder.governor_report().peak_memory_bytes,
            None,
            "holder still holding (peak is filled at exit) when the pusher ended"
        );
        let (holder_ok, holder_report) = finish(&mut holder).await;
        println!("holder ok={holder_ok} {holder_report:?}");
        println!("pusher ok={pusher_ok} {pusher_report:?}");
        assert!(holder_ok, "the holder (90 MiB, under both limits) survives");
        assert!(
            !pusher_ok,
            "the second 90 MiB must hit the 150 MiB host ceiling"
        );
        assert!(holder_report.host_ceiling_joined);
        assert!(pusher_report.host_ceiling_joined);
        assert!(
            pusher_report.host_ceiling_hit,
            "the failing job names the host ceiling"
        );
        assert!(
            !holder_report.host_ceiling_hit,
            "a successful job is never flagged"
        );
        assert!(
            !pusher_report.memory_limit_hit,
            "its own 100 MiB limit was not the one reached"
        );
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn host_ceiling_caps_jobs_together() {
    host_ceiling_scenario();
}

/// Inner half of the nested-job test: idle unless `TC_TEST_NESTED` is set.
#[test]
fn host_ceiling_helper() {
    if std::env::var_os(NESTED_ENV).is_some() {
        host_ceiling_scenario();
    }
}

/// The "daemon" (this binary, re-invoked) itself runs inside a governed job;
/// the host job and the per-job jobs nest one level deeper.
#[cfg(windows)]
#[test]
fn host_ceiling_nests_when_daemon_is_in_a_job() {
    let outer = JobLimits {
        memory_bytes: Some(280 * MIB),
        ..governed()
    };
    let (ok, report) = run_cfg(
        &self_argv("host_ceiling_helper"),
        &config(outer, env(&[(NESTED_ENV, "1")])),
    );
    println!("outer report {report:?}");
    assert_eq!(report.mode, Some(GovernorMode::JobObject));
    assert!(ok, "nested host-ceiling scenario passes inside a job");
}

/// `Normal` inherits the parent's priority (no `PRIORITY_CLASS` flag);
/// `Idle` is applied.
#[cfg(windows)]
#[test]
fn normal_priority_inherits() {
    use windows_sys::Win32::System::Threading::{
        BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        SetPriorityClass,
    };
    let with_priority = |p| JobLimits {
        priority: Some(p),
        ..governed()
    };
    // SAFETY: the pseudo handle is always valid; this process owns its class.
    let set = |class| unsafe { SetPriorityClass(GetCurrentProcess(), class) };
    assert_ne!(set(BELOW_NORMAL_PRIORITY_CLASS), 0);
    let below = BELOW_NORMAL_PRIORITY_CLASS.to_string();
    let inherited = run_cfg(
        &helper_argv(),
        &config(
            with_priority(JobPriority::Normal),
            env(&[(ALLOC_ENV, "1"), (EXPECT_PRIORITY_ENV, &below)]),
        ),
    );
    assert_ne!(set(NORMAL_PRIORITY_CLASS), 0);
    println!("normal under below_normal parent: {inherited:?}");
    assert!(
        inherited.0,
        "Normal must inherit BELOW_NORMAL, not force NORMAL"
    );
    let idle = IDLE_PRIORITY_CLASS.to_string();
    let applied = run_cfg(
        &helper_argv(),
        &config(
            with_priority(JobPriority::Idle),
            env(&[(ALLOC_ENV, "1"), (EXPECT_PRIORITY_ENV, &idle)]),
        ),
    );
    assert!(applied.0, "Idle must be applied by the job");
}

/// A fixed limit the job cannot be put under: the report says what really
/// happened. Windows: a 1-byte job limit either governs (child fails) or is
/// refused (kill-only job retried, `Unavailable` with the Win32 code, child
/// runs). Linux: only reachable for real on a host without a writable cgroup,
/// where the job still runs under `Rlimit`.
#[test]
fn ungovernable_limit_is_reported_honestly() {
    #[cfg(windows)]
    {
        let tiny = JobLimits {
            memory_bytes: Some(1),
            ..governed()
        };
        let (ok, report) = run(&helper_argv(), tiny, "1");
        println!("1-byte limit: ok={ok} {report:?}");
        match report.mode.expect("governed") {
            GovernorMode::JobObject => assert!(!ok, "an enforced 1-byte limit fails the child"),
            GovernorMode::Unavailable(reason) => {
                assert!(ok, "an unenforced limit leaves the child running");
                assert!(reason.bytes().any(|b| b.is_ascii_digit()), "{reason}");
                assert!(!reason.contains('\\') && !reason.contains('/'), "{reason}");
            }
            other => panic!("unexpected mode {other:?}"),
        }
    }
    #[cfg(target_os = "linux")]
    {
        if available_mode() != GovernorMode::Rlimit {
            println!("cgroup host: no real Unavailable path without mocks; skipped");
            return;
        }
        let (ok, report) = run(&helper_argv(), governed(), "1");
        println!("rlimit host: ok={ok} {report:?}");
        assert!(ok);
        assert_eq!(report.mode, Some(GovernorMode::Rlimit));
    }
}

// ------------------------------------------------------------------ Linux --

/// This process's parent cgroup dir (where the governor creates job dirs).
#[cfg(target_os = "linux")]
fn test_parent_cgroup() -> Option<PathBuf> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let own = text.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
    let rel = Path::new(own.strip_prefix('/')?).parent()?;
    Some(Path::new("/sys/fs/cgroup").join(rel))
}

/// Field 19 of `/proc/self/stat`: this process's nice value.
#[cfg(target_os = "linux")]
fn own_nice() -> i32 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("stat");
    let rest = &stat[stat.rfind(')').expect("comm end") + 1..];
    rest.split_whitespace()
        .nth(16)
        .expect("nice field")
        .parse()
        .expect("nice is a number")
}

#[cfg(target_os = "linux")]
fn sh(script: &str, arg0: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        arg0.to_owned(),
    ]
}

#[cfg(target_os = "linux")]
#[test]
fn normal_priority_inherits() {
    let parent = own_nice().to_string();
    let limits = JobLimits {
        priority: Some(JobPriority::Normal),
        ..governed()
    };
    let (ok, report) = run_cfg(
        &sh(r#"[ "$(nice)" = "$0" ]"#, &parent),
        &config(limits, Vec::new()),
    );
    println!("parent nice {parent}: ok={ok} {report:?}");
    assert!(ok, "Normal must leave the child's nice unchanged");
}

/// Memory `None` + priority: nice only, no cgroup dir anywhere.
#[cfg(target_os = "linux")]
#[test]
fn priority_only_is_nice_without_cgroup_dir() {
    let id = terminal_commander_core::ProbeId::new();
    let want = own_nice().max(10).to_string();
    let cfg = ProcessProbeConfig {
        probe_id: Some(id),
        ..config(
            JobLimits {
                memory_bytes: None,
                priority: Some(JobPriority::BelowNormal),
                join_host_ceiling: false,
            },
            Vec::new(),
        )
    };
    rt().block_on(async {
        let (rings, sifter, sink) = deps();
        let argv = sh(r#"[ "$(nice)" = "$0" ] || exit 3; sleep 1"#, &want);
        let mut probe = ProcessProbe::spawn(&argv, &cfg, rings, sifter, sink).expect("spawn");
        if let Some(parent) = test_parent_cgroup() {
            let pid = std::process::id();
            assert!(!parent.join(format!("tc-job-{pid}-{id}")).exists());
            assert!(
                !parent
                    .join(format!("tc-jobs-{pid}"))
                    .join(id.to_string())
                    .exists()
            );
        }
        let (ok, report) = finish(&mut probe).await;
        println!("nice-only: ok={ok} {report:?}");
        assert!(ok, "child nice must be {want}");
        assert_eq!(report.mode, Some(GovernorMode::Rlimit));
    });
}

/// A grandchild that outlives the job keeps its cgroup populated; finish
/// kills it (`cgroup.kill`) and the dir is gone.
#[cfg(target_os = "linux")]
#[test]
fn outliving_grandchild_is_killed_and_dir_removed() {
    if available_mode() != GovernorMode::Cgroup {
        println!("linux governor mode: rlimit (no cgroup dir to clean); skipped");
        return;
    }
    let parent = test_parent_cgroup().expect("cgroup parent");
    let id = terminal_commander_core::ProbeId::new();
    let dir = scratch_dir("orphan");
    let pidfile = dir.join("pid").to_string_lossy().into_owned();
    let cfg = ProcessProbeConfig {
        probe_id: Some(id),
        ..config(governed(), Vec::new())
    };
    let argv = sh(
        r#"sleep 30 </dev/null >/dev/null 2>&1 & echo $! > "$0"; exit 0"#,
        &pidfile,
    );
    let (ok, report) = run_cfg(&argv, &cfg);
    println!("orphan job: ok={ok} {report:?}");
    assert!(ok);
    assert_eq!(report.mode, Some(GovernorMode::Cgroup));
    // The EBUSY retry runs on the blocking pool, off the async worker.
    let job_dir = parent.join(format!("tc-job-{}-{id}", std::process::id()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while job_dir.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!job_dir.exists(), "job cgroup dir must be removed");
    let pid = std::fs::read_to_string(&pidfile).expect("pid file");
    let proc_dir = PathBuf::from(format!("/proc/{}", pid.trim()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while proc_dir.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!proc_dir.exists(), "the outliving grandchild was killed");
    let _ = std::fs::remove_dir_all(dir);
}

/// After boot found cgroups usable, a per-job create failure (here: the
/// host dir removed under the daemon) is `Unavailable`, never a run-time
/// switch to the rlimit lane; the job still runs. Runs in a re-exec of this
/// binary: it installs the process-wide host ceiling, which a shared-process
/// `cargo test` run has already installed for `host_ceiling_caps_jobs_together`.
#[cfg(target_os = "linux")]
#[test]
fn removed_host_dir_is_unavailable_not_rlimit() {
    if available_mode() != GovernorMode::Cgroup {
        println!("linux governor mode: rlimit (no cgroup lane); skipped");
        return;
    }
    let argv = self_argv("removed_host_dir_helper");
    let out = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .env(REMOVED_HOST_ENV, "1")
        .output()
        .expect("re-exec test binary");
    println!("{}", String::from_utf8_lossy(&out.stdout));
    assert!(
        out.status.success(),
        "removed-host-dir scenario failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Inner half of `removed_host_dir_is_unavailable_not_rlimit`: idle unless
/// `TC_TEST_REMOVED_HOST` is set.
#[cfg(target_os = "linux")]
#[test]
fn removed_host_dir_helper() {
    if std::env::var_os(REMOVED_HOST_ENV).is_none() {
        return;
    }
    install_host_ceiling(HOST_LIMIT).expect("host ceiling installs");
    let joined = JobLimits {
        join_host_ceiling: true,
        ..governed()
    };
    let (ok, first) = run(&helper_argv(), joined, "1");
    println!("before removal: ok={ok} {first:?}");
    assert!(ok);
    assert_eq!(first.mode, Some(GovernorMode::Cgroup));
    let host = test_parent_cgroup()
        .expect("cgroup parent")
        .join(format!("tc-jobs-{}", std::process::id()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::fs::remove_dir(&host).is_err() {
        assert!(Instant::now() < deadline, "host dir never emptied");
        std::thread::sleep(Duration::from_millis(20));
    }
    let (ok, second) = run(&helper_argv(), joined, "1");
    println!("after removal: ok={ok} {second:?}");
    assert!(ok, "an ungoverned job still runs");
    match second.mode {
        Some(GovernorMode::Unavailable(reason)) => {
            assert!(!reason.contains('/'), "reason carries no path: {reason}");
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
    assert!(!second.host_ceiling_joined);
}

/// Boot sweep removes empty dirs of DEAD daemons only; a live daemon's dirs
/// (here: this process's) stay. Ignored by default: it rmdirs in the shared
/// parent cgroup. Run alone: `--run-ignored only -E 'test(sweep_)'`.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "mutates the shared parent cgroup; run alone"]
fn sweep_removes_stale_dirs() {
    use terminal_commander_core::ProbeId;
    if available_mode() != GovernorMode::Cgroup {
        println!("linux governor mode: rlimit; skipped");
        return;
    }
    let parent = test_parent_cgroup().expect("cgroup parent");
    // A pid that really existed and is now dead.
    let mut child = std::process::Command::new("true").spawn().expect("true");
    let dead = child.id();
    child.wait().expect("reap");
    let live = std::process::id();
    let stale = parent.join(format!("tc-job-{dead}-{}", ProbeId::new()));
    let stale_host = parent.join(format!("tc-jobs-{dead}"));
    let stale_child = stale_host.join(ProbeId::new().to_string());
    let live_job = parent.join(format!("tc-job-{live}-{}", ProbeId::new()));
    std::fs::create_dir(&stale).expect("stale sibling");
    std::fs::create_dir(&stale_host).expect("stale host dir");
    std::fs::create_dir(&stale_child).expect("stale host child");
    std::fs::create_dir(&live_job).expect("live job dir");
    let removed = terminal_commander_probes::governor::sweep_stale_job_dirs();
    println!("swept {removed}");
    let live_kept = live_job.exists();
    let _ = std::fs::remove_dir(&live_job);
    assert!(removed >= 3);
    assert!(!stale.exists() && !stale_child.exists() && !stale_host.exists());
    assert!(live_kept, "a live daemon's dir is never swept");
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
        assert_eq!(
            probe.governor_report().mode,
            Some(GovernorMode::JobObject),
            "mode final before exit"
        );
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
