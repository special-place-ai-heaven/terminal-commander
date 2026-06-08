// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC45 daemon IPC tests for the aggregate runtime view:
//! `runtime_state`, `probe_list`, `probe_status`. Read-only.

#![cfg(unix)]

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, FileWatchStartParams, IpcErrorCode, IpcRequest,
    IpcResponse, IpcServer, Liveness, ProbeKind, ProbeStatusParams, PtyCommandStartParams,
    ServerHandle,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-runtime-state-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn python3_available() -> bool {
    for c in ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"] {
        if std::path::Path::new(c).exists() {
            return true;
        }
    }
    false
}

fn build_server() -> (PathBuf, Arc<DaemonState>, ServerHandle) {
    let data = tmp_data_dir("server");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

#[test]
fn runtime_state_empty_when_no_runtimes_live() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let resp = client
            .call(
                1,
                IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
            )
            .await
            .expect("runtime_state");
        let r = match resp {
            IpcResponse::RuntimeState(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.command_jobs, 0);
        assert_eq!(r.pty_jobs, 0);
        assert_eq!(r.file_watches, 0);
        assert_eq!(r.bucket_count, 0);
        assert!(r.probes.is_empty());
        assert!(r.buckets.is_empty());
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)]
fn runtime_state_aggregates_command_pty_and_filewatch() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let log_path = data.join("watch.log");
        std::fs::create_dir_all(&data).unwrap();
        {
            let mut f = std::fs::File::create(&log_path).unwrap();
            writeln!(f, "preexisting").unwrap();
        }
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        // Spawn a command_start_combed job.
        let _ = client
            .call(
                1,
                IpcRequest::CommandStartCombed(terminal_commanderd::CommandStartParams {
                    environment: None,
                    argv: vec!["sleep".to_owned(), "1".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(2_000),
                    tag: None,
                    dedup_nonce: None,
                }),
            )
            .await
            .expect("command_start_combed");

        // Spawn a file watch.
        let watch_resp = client
            .call(
                2,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: log_path.clone(),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: Some(false),
                    tag: None,
                }),
            )
            .await
            .expect("file_watch_start");
        let _watch_probe = match watch_resp {
            IpcResponse::FileWatchStart(s) => s.probe_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Spawn a PTY job.
        let pty_resp = client
            .call(
                3,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        "import time; print('pty up'); time.sleep(1)".to_owned(),
                    ],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    rows: None,
                    cols: None,
                    tag: None,
                }),
            )
            .await
            .expect("pty_command_start");
        let pty_probe_id = match pty_resp {
            IpcResponse::PtyCommandStart(s) => s.probe_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Aggregate runtime state must show all three.
        let resp = client
            .call(
                4,
                IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
            )
            .await
            .expect("runtime_state");
        let r = match resp {
            IpcResponse::RuntimeState(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.command_jobs, 1, "probes: {:?}", r.probes);
        assert_eq!(r.file_watches, 1);
        assert_eq!(r.pty_jobs, 1);
        assert_eq!(r.probes.len(), 3);
        // At least three buckets (one per probe).
        assert!(r.bucket_count >= 3, "buckets: {}", r.bucket_count);
        // Kinds are populated correctly.
        assert!(
            r.probes
                .iter()
                .any(|p| matches!(p.kind, ProbeKind::Command))
        );
        assert!(
            r.probes
                .iter()
                .any(|p| matches!(p.kind, ProbeKind::FileWatch))
        );
        assert!(r.probes.iter().any(|p| matches!(p.kind, ProbeKind::Pty)));
        // FileWatch probe carries the path.
        let fw = r
            .probes
            .iter()
            .find(|p| matches!(p.kind, ProbeKind::FileWatch))
            .unwrap();
        assert_eq!(fw.path.as_deref(), Some(log_path.as_path()));

        // probe_list returns the same flat list.
        let resp = client
            .call(
                5,
                IpcRequest::ProbeList(terminal_commanderd::ListLimitParams::default()),
            )
            .await
            .expect("probe_list");
        let pl = match resp {
            IpcResponse::ProbeList(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(pl.probes.len(), 3);

        // probe_status by id resolves PTY probe.
        let resp = client
            .call(
                6,
                IpcRequest::ProbeStatus(ProbeStatusParams {
                    probe_id: pty_probe_id,
                }),
            )
            .await
            .expect("probe_status");
        let ps = match resp {
            IpcResponse::ProbeStatus(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(matches!(ps.probe.kind, ProbeKind::Pty));
        assert_eq!(ps.probe.probe_id, pty_probe_id);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn probe_status_unknown_probe_returns_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                1,
                IpcRequest::ProbeStatus(ProbeStatusParams {
                    probe_id: terminal_commander_core::ProbeId::new(),
                }),
            )
            .await
            .expect_err("unknown probe must be rejected");
        assert_eq!(err.code, IpcErrorCode::UnknownProbe);
        handle.shutdown().await;
        cleanup(&data);
    });
}

// MUST-ADD #3: an exited command must report `Exited{code}` derived from
// the job ledger (JobState), NOT `Running` from lingering live-map
// presence (command bindings are never removed from the live map after
// exit). `/usr/bin/true` exits 0 immediately.
#[test]
fn exited_command_reports_exited_liveness_from_jobstate() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let _ = client
            .call(
                1,
                IpcRequest::CommandStartCombed(terminal_commanderd::CommandStartParams {
                    environment: None,
                    argv: vec!["true".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(2_000),
                    tag: None,
                    dedup_nonce: None,
                }),
            )
            .await
            .expect("command_start_combed");

        // Poll runtime_state until the lingering command probe reports a
        // terminal liveness. Presence in the live map is unconditional;
        // the liveness must flip to Exited once the waiter calls finish().
        let mut found: Option<Liveness> = None;
        for i in 2..60 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let resp = client
                .call(
                    i,
                    IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
                )
                .await
                .expect("runtime_state");
            let r = match resp {
                IpcResponse::RuntimeState(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            if let Some(p) = r
                .probes
                .iter()
                .find(|p| matches!(p.kind, ProbeKind::Command))
            {
                // The probe lingers regardless of state; wait for terminal.
                if !matches!(p.liveness, Liveness::Starting | Liveness::Running) {
                    found = Some(p.liveness.clone());
                    break;
                }
            }
        }

        assert_eq!(
            found,
            Some(Liveness::Exited { code: 0 }),
            "exited `true` must report Exited code 0 (not Running from live-map presence)"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

// Item #1: a PTY that exits naturally must report `Exited{code}` derived
// from the job ledger (JobState), NOT `Running` from lingering live-map
// presence. The PTY binding lingers in the live map after exit (like the
// command runtime's), and a lifecycle waiter flips the ledger via
// `jobs.finish`, so `collect_probes`/`PtyRuntime::liveness` surfaces the
// terminal state. PTY cannot run a shell interpreter, so use python3 with an
// explicit exit code.
#[test]
fn exited_pty_reports_exited_liveness_from_jobstate() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    // Exit 0 promptly; `finish` maps a clean exit to Exited{0}.
                    argv: vec![
                        "python3".to_owned(),
                        "-c".to_owned(),
                        "import sys; sys.exit(0)".to_owned(),
                    ],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    rows: None,
                    cols: None,
                    tag: None,
                }),
            )
            .await
            .expect("pty_command_start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Poll runtime_state until the lingering PTY probe reports a terminal
        // liveness. Presence in the live map is unconditional; the liveness
        // must flip to Exited once the lifecycle waiter calls finish().
        let mut found: Option<Liveness> = None;
        for i in 2..80 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let resp = client
                .call(
                    i,
                    IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
                )
                .await
                .expect("runtime_state");
            let r = match resp {
                IpcResponse::RuntimeState(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            if let Some(p) = r
                .probes
                .iter()
                .find(|p| matches!(p.kind, ProbeKind::Pty) && p.job_id == started.job_id)
                && !matches!(p.liveness, Liveness::Starting | Liveness::Running)
            {
                found = Some(p.liveness.clone());
                break;
            }
        }

        assert_eq!(
            found,
            Some(Liveness::Exited { code: 0 }),
            "an exited PTY must report Exited code 0 (not Running from live-map presence)"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
