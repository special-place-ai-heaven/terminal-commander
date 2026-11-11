// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! terminal-commander#284: `bucket_wait` honours the caller's timeout up to a
//! policy ceiling, wakes on a match, and returns when the watched job ends.
//!
//! Unix-only (the jobs are `python3` children, like `ipc_bucket.rs`). The
//! 8-minute acceptance check lives as a paused-clock unit test in
//! `terminal-commander-core` (`bucket_wait_returns_once_on_late_match`); here
//! the daemon half is proved with scaled real time.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{
    BucketConfig, BucketId, BucketWakeReason, JobState, Severity, WakeMode,
};
use terminal_commander_ipc::{BucketWaitParams, BucketWaitResponse};
use terminal_commanderd::{
    CommandStartRequest, DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse,
    IpcServer, PolicyProfile,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "tc-bucket-wait-wake-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ))
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

type Served = (
    Arc<DaemonState>,
    terminal_commanderd::ServerHandle,
    DaemonClient,
);

fn serve(cfg: DaemonConfig) -> Served {
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    let client =
        DaemonClient::new(handle.socket_path().to_path_buf()).with_timeout(Duration::from_secs(20));
    (state, handle, client)
}

fn start_job(state: &DaemonState, script: &str) -> terminal_commanderd::CommandStartResponse {
    state
        .command
        .start_combed(CommandStartRequest {
            argv: vec!["python3".to_owned(), "-c".to_owned(), script.to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            receipt_shape: None,
            strip_ansi: true,
            peer_discriminator: None,
            limits: None,
        })
        .unwrap()
}

const fn wait_params(bucket_id: BucketId, timeout_ms: u64) -> BucketWaitParams {
    BucketWaitParams {
        bucket_id,
        cursor: 0,
        severity_min: None,
        kind_filter: None,
        limit: None,
        timeout_ms: Some(timeout_ms),
        wake_mode: None,
    }
}

async fn call_wait(client: &DaemonClient, params: BucketWaitParams) -> BucketWaitResponse {
    match client
        .call(1, IpcRequest::BucketWait(params))
        .await
        .unwrap()
    {
        IpcResponse::BucketWait(w) => w,
        other => panic!("unexpected response: {other:?}"),
    }
}

fn empty_bucket(state: &DaemonState) -> BucketId {
    let bucket_id = BucketId::new();
    state
        .router
        .bucket_create(bucket_id, BucketConfig::default())
        .unwrap();
    bucket_id
}

#[test]
fn bucket_wait_match_mode_returns_on_exit() {
    rt().block_on(async {
        let data = tmp_data_dir("exit");
        let (state, handle, client) = serve(DaemonConfig::defaults_in(&data));
        // 500 lines the filter never matches (no rules, so no events), then a
        // clean exit after a pause so the wait is parked when the job ends.
        let start = start_job(
            &state,
            "import time\ntime.sleep(1.5)\nfor i in range(500):\n    print('noise', i)\n",
        );
        let mut p = wait_params(start.bucket_id, 600_000);
        p.severity_min = Some(Severity::Medium);
        p.wake_mode = Some(WakeMode::Match);
        let t0 = Instant::now();
        let w = call_wait(&client, p).await;
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "did not return at exit"
        );
        assert_eq!(w.wake_reason, BucketWakeReason::Exit, "{w:?}");
        assert!(!w.heartbeat, "{w:?}");
        assert!(w.events.is_empty(), "{w:?}");
        assert_eq!(w.timeout_clamped_to_ms, None);
        let exit = w.job_exit.expect("job_exit on an exit wake");
        assert_eq!(exit.job_id, start.job_id);
        assert_eq!(exit.state, JobState::Exited);
        assert_eq!(exit.exit_code, Some(0));
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
fn bucket_wait_hardened_ceiling_clamps() {
    // The hardened default is the 30 s the issue names.
    let mut hardened = DaemonConfig::defaults_in(tmp_data_dir("h0"));
    hardened.policy.profile = PolicyProfile::DeveloperLocal;
    assert_eq!(hardened.bucket_wait_ceiling_ms(), 30_000);
    assert_eq!(
        DaemonConfig::defaults_in(tmp_data_dir("h1")).bucket_wait_ceiling_ms(),
        3_600_000,
        "full_access default"
    );
    // Scaled: a hardened daemon with a 300 ms ceiling, asked for 10 minutes,
    // waits 300 ms and says so (same code path as the 30 s ceiling).
    rt().block_on(async {
        let data = tmp_data_dir("clamp");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.policy.profile = PolicyProfile::DeveloperLocal;
        cfg.limits.bucket_wait_max_ms = Some(300);
        let (state, handle, client) = serve(cfg);
        let bucket = empty_bucket(&state);
        let t0 = Instant::now();
        let w = call_wait(&client, wait_params(bucket, 600_000)).await;
        assert!(t0.elapsed() < Duration::from_secs(10), "clamp not applied");
        assert!(w.heartbeat, "{w:?}");
        assert_eq!(w.wake_reason, BucketWakeReason::Timeout);
        assert_eq!(w.timeout_clamped_to_ms, Some(300), "{w:?}");
        // Under the ceiling: no clamp note.
        let w = call_wait(&client, wait_params(bucket, 100)).await;
        assert_eq!(w.timeout_clamped_to_ms, None, "{w:?}");
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
fn bucket_wait_default_mode_unchanged() {
    rt().block_on(async {
        let data = tmp_data_dir("default");
        let (state, handle, client) = serve(DaemonConfig::defaults_in(&data));
        let bucket = empty_bucket(&state);
        // No wake_mode: a filtered-out append still wakes the wait and
        // returns it empty (the historical behaviour), tagged `append`.
        let mut p = wait_params(bucket, 5_000);
        p.severity_min = Some(Severity::High);
        let waiter = {
            let client = client.clone();
            tokio::spawn(async move { call_wait(&client, p).await })
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        let low = terminal_commander_core::EventDraft {
            bucket_id: bucket,
            timestamp: time::OffsetDateTime::now_utc(),
            severity: Severity::Low,
            kind: "noise".to_owned(),
            summary: "noise".to_owned(),
            rule: None,
            source: terminal_commander_core::EventSource {
                probe_id: terminal_commander_core::ProbeId::new(),
                source_type: terminal_commander_core::SourceType::Process,
                stream: terminal_commander_core::SourceStream::Stderr,
                job_id: None,
            },
            captures: None,
            pointer: None,
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        };
        state.router.bucket_append(bucket, low).unwrap();
        let w = waiter.await.unwrap();
        assert_eq!(w.wake_reason, BucketWakeReason::Append, "{w:?}");
        assert!(w.events.is_empty());
        assert_eq!(w.timeout_clamped_to_ms, None);
        assert!(w.job_exit.is_none());
        // Timeout still reports a heartbeat with wake_reason timeout.
        let w = call_wait(&client, wait_params(bucket, 100)).await;
        assert!(w.heartbeat);
        assert_eq!(w.wake_reason, BucketWakeReason::Timeout);
        handle.shutdown().await;
        let _ = std::fs::remove_dir_all(&data);
    });
}

#[test]
fn policy_status_reports_wait_ceiling() {
    rt().block_on(async {
        for (tag, profile, bucket_ms, rw_ms) in [
            ("ps-full", PolicyProfile::FullAccess, 3_600_000, 3_600_000),
            ("ps-hard", PolicyProfile::DeveloperLocal, 30_000, 60_000),
        ] {
            let data = tmp_data_dir(tag);
            let mut cfg = DaemonConfig::defaults_in(&data);
            cfg.policy.profile = profile;
            let (_state, handle, client) = serve(cfg);
            let resp = client.call(1, IpcRequest::PolicyStatus).await.unwrap();
            let IpcResponse::PolicyStatus(p) = resp else {
                panic!("unexpected response: {resp:?}");
            };
            assert_eq!(p.bucket_wait_max_ms, Some(bucket_ms), "{tag}");
            assert_eq!(p.run_and_watch_max_wait_ms, Some(rw_ms), "{tag}");
            handle.shutdown().await;
            let _ = std::fs::remove_dir_all(&data);
        }
    });
}
