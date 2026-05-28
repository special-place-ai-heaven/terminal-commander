// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC41 daemon IPC integration tests for `command_start_combed` and
//! `command_status`.
//!
//! Stands up the real UDS IPC server in a temp dir and exercises:
//!
//! - happy-path argv start: a small program (`true`) is accepted, the
//!   response carries bounded job/bucket/probe ids only, an
//!   `ipc_command_start_combed` audit row lands, the `command_start`
//!   row from the command runtime also lands with `allow`.
//! - shell-bridge guard: argv `["sh", "-c", "..."]` is rejected with
//!   the typed `shell_interpreter_denied` IPC error and a
//!   `command_rejected` / `deny` audit row, and no process is spawned.
//! - empty argv: typed `argv_invalid` error.
//! - `command_status` happy path: after the job exits, status returns
//!   the lifecycle state and frame counters; never raw text.
//! - `command_status` for an unknown job: typed `unknown_job` error.
//!
//! Linux/WSL only (UDS).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    CommandStartParams, CommandStatusParams, DaemonClient, DaemonConfig, DaemonState, IpcErrorCode,
    IpcRequest, IpcResponse, IpcServer,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-ipc-cmd-{tag}-{pid}-{nanos}-{n}"));
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

fn build_server(data: &std::path::Path) -> (Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let server = IpcServer::new(Arc::clone(&state), socket);
    let handle = server.spawn().unwrap();
    (state, handle)
}

fn small_start_params(argv: &[&str]) -> CommandStartParams {
    CommandStartParams {
        environment: None,
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: Vec::new(),
        bucket_config: None,
        rules: Vec::new(),
        grace_ms: Some(2_000),
    }
}

/// Audit-row predicate used by the M2 poll in the happy-path test.
/// Hoisted to module scope so it does not trip `items_after_statements`
/// when placed inside the async-block body.
fn audit_rows_have_both_start_rows(rows: &[terminal_commander_store::AuditRow]) -> bool {
    rows.iter().any(|r| r.action == "ipc_command_start_combed")
        && rows
            .iter()
            .any(|r| r.action == "command_start" && r.decision == "allow")
}

#[test]
fn command_start_combed_happy_path_returns_bounded_ids_and_audits_through_ipc() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("happy");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let resp = client
            .call(
                1,
                IpcRequest::CommandStartCombed(small_start_params(&["true"])),
            )
            .await
            .expect("command_start_combed call");
        let start = match resp {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };
        // Bounded identifiers only; no stdout/stderr fields exist on
        // this wire type.
        assert!(
            start.job_id.to_wire_string().starts_with("job_"),
            "job_id must use wire form"
        );
        assert!(
            start.bucket_id.to_wire_string().starts_with("bkt_"),
            "bucket_id must use wire form"
        );
        assert!(
            start.probe_id.to_wire_string().starts_with("prb_"),
            "probe_id must use wire form"
        );
        assert_eq!(start.cursor, 0);

        // M2: poll the audit log until both expected rows appear (or a generous
        // deadline), instead of a fixed sleep that races slow job exit under load.
        // (`audit_rows_have_both_start_rows` is hoisted to module scope above to
        // satisfy clippy::items_after_statements on Linux/rust 1.95.)
        let read_rows = || {
            let mut g = state.store.lock();
            g.audit_since(&AuditReadRequest::new(0)).unwrap()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut rows = read_rows();
        while !audit_rows_have_both_start_rows(&rows) && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
            rows = read_rows();
        }
        assert!(
            rows.iter().any(|r| r.action == "ipc_command_start_combed"),
            "ipc-side audit row missing; rows: {rows:?}"
        );
        assert!(
            rows.iter()
                .any(|r| r.action == "command_start" && r.decision == "allow"),
            "runtime allow audit row missing; rows: {rows:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn command_start_combed_denies_shell_interpreter_and_audits() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("sh");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let err = client
            .call(
                7,
                IpcRequest::CommandStartCombed(small_start_params(&["sh", "-c", "echo nope"])),
            )
            .await
            .expect_err("shell interpreter must be denied");
        assert_eq!(err.code, IpcErrorCode::ShellInterpreterDenied);
        assert!(
            err.message.contains("sh") || err.message.contains("shell"),
            "error message should name the shell, got: {}",
            err.message
        );

        let rows = {
            let mut g = state.store.lock();
            g.audit_since(&AuditReadRequest::new(0)).unwrap()
        };
        assert!(
            rows.iter()
                .any(|r| r.action == "command_rejected" && r.decision == "deny"),
            "runtime must record a deny row for the shell attempt; rows: {rows:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn command_start_combed_rejects_empty_argv_with_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("emptyargv");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(11, IpcRequest::CommandStartCombed(small_start_params(&[])))
            .await
            .expect_err("empty argv must be rejected");
        assert_eq!(err.code, IpcErrorCode::ArgvInvalid);
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn command_status_returns_lifecycle_counters_after_exit() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("status");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let start = match client
            .call(
                21,
                IpcRequest::CommandStartCombed(small_start_params(&["true"])),
            )
            .await
            .expect("start")
        {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };

        // M2: poll command_status until the job reaches a terminal state (or a
        // deadline), instead of a fixed sleep that races slow exit under load.
        let query_status = |seq: u64| {
            let client = &client;
            let job_id = start.job_id;
            async move {
                match client
                    .call(
                        seq,
                        IpcRequest::CommandStatus(CommandStatusParams { job_id }),
                    )
                    .await
                    .expect("status")
                {
                    IpcResponse::CommandStatus(s) => s,
                    other => panic!("unexpected response: {other:?}"),
                }
            }
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut status = query_status(22).await;
        let mut seq = 23;
        while !matches!(
            status.state,
            terminal_commander_core::JobState::Exited
                | terminal_commander_core::JobState::Cancelled
                | terminal_commander_core::JobState::Failed
        ) && std::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
            status = query_status(seq).await;
            seq += 1;
        }
        assert_eq!(status.job_id, start.job_id);
        assert_eq!(status.bucket_id, start.bucket_id);
        assert_eq!(status.probe_id, start.probe_id);
        // The test name says "after exit": require a terminal state. The poll
        // above waits for it; if the deadline lapsed with state still Running,
        // a `true` command that didn't exit in 30s is a real failure, so do NOT
        // accept Running here.
        assert!(
            matches!(
                status.state,
                terminal_commander_core::JobState::Exited
                    | terminal_commander_core::JobState::Cancelled
                    | terminal_commander_core::JobState::Failed
            ),
            "command must reach a terminal state after exit, got {:?}",
            status.state
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn command_status_for_unknown_job_returns_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("unk");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let bogus = terminal_commander_core::JobId::new();
        let err = client
            .call(
                33,
                IpcRequest::CommandStatus(CommandStatusParams { job_id: bogus }),
            )
            .await
            .expect_err("unknown job must be rejected");
        assert_eq!(err.code, IpcErrorCode::UnknownJob);
        handle.shutdown().await;
        cleanup(&data);
    });
}
