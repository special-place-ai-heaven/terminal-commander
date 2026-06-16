// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC-3 daemon IPC integration tests for `command_stop` (Phase 6a).
//!
//! Stands up the real UDS IPC server in a temp dir on its own socket
//! (never the live daemon) and exercises the force-kill surface plus
//! its security-critical ordering:
//!
//! 1. `command_stop` kills a running combed command and the job
//!    reaches a terminal `Cancelled` state; an `allow` audit row lands
//!    whose subject is the job_id wire string.
//! 2. A `ReadOnlyObserver` caller is `PolicyDenied` BEFORE any live-map
//!    lookup: the deny audit row's subject is the PEER identity (never a
//!    job_id / `job_...`), and a denied caller learns nothing about job
//!    existence (no `UnknownJob` oracle leak).
//! 3. A second stop on an already-terminal job is a no-op that returns
//!    the terminal state (no error, no double kill / double event).
//! 4. An unknown job under an allowed profile returns `UnknownJob`.
//!
//! Linux/WSL only (UDS).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    CommandStatusParams, CommandStopParams, DaemonClient, DaemonConfig, DaemonState, IpcErrorCode,
    IpcRequest, IpcResponse, IpcServer, PolicyProfile, ServerHandle,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cmd-stop-{tag}-{pid}-{nanos}-{n}"));
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

/// Build a TEST server on its own temp data dir + socket with an
/// explicit policy profile so the deny path can be exercised.
fn build_server_with_profile(
    data: &std::path::Path,
    profile: PolicyProfile,
) -> (Arc<DaemonState>, ServerHandle) {
    let mut cfg = DaemonConfig::defaults_in(data);
    cfg.policy.profile = profile;
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (state, handle)
}

fn small_start_params(argv: &[&str]) -> terminal_commanderd::CommandStartParams {
    terminal_commanderd::CommandStartParams {
        environment: None,
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: Vec::new(),
        bucket_config: None,
        rules: Vec::new(),
        grace_ms: Some(2_000),
        tag: None,
        dedup_nonce: None,
        strip_ansi: true,
    }
}

/// Poll `command_status` until the job reaches a terminal state (or the
/// deadline lapses). Returns the final observed status.
async fn poll_until_terminal(
    client: &DaemonClient,
    job_id: terminal_commander_core::JobId,
    mut seq: u64,
) -> terminal_commanderd::CommandStatusResponse {
    use terminal_commander_core::JobState;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status = match client
            .call(
                seq,
                IpcRequest::CommandStatus(CommandStatusParams { job_id }),
            )
            .await
            .expect("status")
        {
            IpcResponse::CommandStatus(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };
        seq += 1;
        if matches!(
            status.state,
            JobState::Exited | JobState::Cancelled | JobState::Failed
        ) || std::time::Instant::now() >= deadline
        {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// (1) `command_stop` kills a running command and it becomes terminal
/// (`Cancelled`); an `allow` audit row lands with the job_id subject.
#[test]
fn command_stop_kills_running_command_and_audits_allow() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("kill");
        let (state, handle) = build_server_with_profile(&data, PolicyProfile::DeveloperLocal);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Long-lived command so the job is reliably live when we stop it.
        let start = match client
            .call(
                1,
                IpcRequest::CommandStartCombed(small_start_params(&["sleep", "30"])),
            )
            .await
            .expect("start")
        {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };

        let stop = match client
            .call(
                2,
                IpcRequest::CommandStop(CommandStopParams {
                    job_id: start.job_id,
                }),
            )
            .await
            .expect("command_stop")
        {
            IpcResponse::CommandStop(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(stop.job_id, start.job_id, "stop echoes the job_id");
        assert_eq!(stop.bucket_id, start.bucket_id, "stop echoes the bucket_id");

        // The kill drives the job to a terminal Cancelled state.
        let status = poll_until_terminal(&client, start.job_id, 3).await;
        assert_eq!(
            status.state,
            terminal_commander_core::JobState::Cancelled,
            "a stopped job must reach Cancelled, got {:?}",
            status.state
        );

        // An `allow` audit row with the job_id wire-string subject landed.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let job_wire = start.job_id.to_wire_string();
        let allow = rows
            .iter()
            .find(|r| r.action == "command_stop" && r.decision == "allow")
            .expect("command_stop allow audit row must land");
        assert_eq!(
            allow.subject, job_wire,
            "the allow row's subject must be the job_id wire string"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// (2) A `ReadOnlyObserver` stop is `PolicyDenied` BEFORE the live-map
/// lookup: the deny row's subject is the PEER identity (NOT a job_id),
/// and the denied caller cannot tell whether the job exists (no
/// `UnknownJob` leak). We pass an arbitrary `JobId::new()` that does not
/// exist; an allowed profile would return `UnknownJob`, but the deny
/// must fire first, so the caller only ever sees `PolicyDenied`.
#[test]
fn command_stop_read_only_observer_is_denied_with_peer_subject_no_oracle() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny");
        let (state, handle) = build_server_with_profile(&data, PolicyProfile::ReadOnlyObserver);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let bogus = terminal_commander_core::JobId::new();
        let err = client
            .call(
                1,
                IpcRequest::CommandStop(CommandStopParams { job_id: bogus }),
            )
            .await
            .expect_err("read_only_observer must be policy-denied");
        // Deny fires BEFORE the live-map lookup -> the caller gets
        // PolicyDenied, never UnknownJob. This is the no-oracle invariant.
        assert_eq!(
            err.code,
            IpcErrorCode::PolicyDenied,
            "a denied caller must see PolicyDenied, not UnknownJob (no existence oracle)"
        );

        // A `command_stop` / `deny` audit row landed; its subject is the
        // PEER identity string, never a job_id wire string / "job_...".
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let deny = rows
            .iter()
            .find(|r| r.action == "command_stop" && r.decision == "deny")
            .expect("command_stop deny audit row must land");
        let bogus_wire = bogus.to_wire_string();
        assert_ne!(
            deny.subject, bogus_wire,
            "deny subject must NOT be the job_id wire string"
        );
        assert!(
            !deny.subject.starts_with("job_"),
            "deny subject must be the peer identity, not a job_id; got: {}",
            deny.subject
        );
        // The peer-identity audit subject is `uid=..:pid=..` (Unix) /
        // `sid=..:pid=..` (Windows) / `unknown_peer`. Assert it looks like
        // a peer record, not a job record.
        assert!(
            deny.subject.contains("uid=")
                || deny.subject.contains("sid=")
                || deny.subject == "unknown_peer",
            "deny subject must be the peer identity string; got: {}",
            deny.subject
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// (3) A second stop on an already-terminal job is a no-op that returns
/// the terminal state with no error and no second kill/double event.
#[test]
fn command_stop_second_stop_on_terminal_job_is_noop() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("noop");
        let (state, handle) = build_server_with_profile(&data, PolicyProfile::DeveloperLocal);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let start = match client
            .call(
                1,
                IpcRequest::CommandStartCombed(small_start_params(&["sleep", "30"])),
            )
            .await
            .expect("start")
        {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };

        // First stop: kills it.
        let _ = match client
            .call(
                2,
                IpcRequest::CommandStop(CommandStopParams {
                    job_id: start.job_id,
                }),
            )
            .await
            .expect("first command_stop")
        {
            IpcResponse::CommandStop(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };
        let status = poll_until_terminal(&client, start.job_id, 3).await;
        assert_eq!(
            status.state,
            terminal_commander_core::JobState::Cancelled,
            "job must be terminal after the first stop"
        );

        // Second stop: a no-op that still returns Ok with the terminal
        // bucket/job ids. The job stays Cancelled (no resurrection).
        let stop2 = match client
            .call(
                100,
                IpcRequest::CommandStop(CommandStopParams {
                    job_id: start.job_id,
                }),
            )
            .await
            .expect("second command_stop must be Ok (no-op), not an error")
        {
            IpcResponse::CommandStop(s) => s,
            other => panic!("unexpected response: {other:?}"),
        };
        assert_eq!(stop2.job_id, start.job_id);
        assert_eq!(stop2.bucket_id, start.bucket_id);

        // Exactly ONE allow row: the no-op second stop does not emit a
        // second job-id allow audit (it returns terminal state before the
        // allow-audit + kill).
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let allow_rows = rows
            .iter()
            .filter(|r| r.action == "command_stop" && r.decision == "allow")
            .count();
        assert_eq!(
            allow_rows, 1,
            "the no-op second stop must NOT emit a second command_stop/allow row"
        );

        // Still Cancelled (no double-kill / resurrection).
        let status2 = poll_until_terminal(&client, start.job_id, 200).await;
        assert_eq!(
            status2.state,
            terminal_commander_core::JobState::Cancelled,
            "the job must stay Cancelled after the no-op second stop"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// (4) An unknown job under an allowed profile returns `UnknownJob`.
#[test]
fn command_stop_unknown_job_under_allowed_profile_returns_unknown_job() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("unknown");
        let (_state, handle) = build_server_with_profile(&data, PolicyProfile::DeveloperLocal);
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let bogus = terminal_commander_core::JobId::new();
        let err = client
            .call(
                1,
                IpcRequest::CommandStop(CommandStopParams { job_id: bogus }),
            )
            .await
            .expect_err("unknown job under an allowed profile must be rejected");
        assert_eq!(err.code, IpcErrorCode::UnknownJob);

        handle.shutdown().await;
        cleanup(&data);
    });
}
