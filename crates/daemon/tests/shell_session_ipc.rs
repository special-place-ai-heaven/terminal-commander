// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! P1 / TC50 daemon IPC tests for the persistent shell-session surface.
//!
//! Covers the omni O-02 gate end-to-end through the daemon (NOT the MCP
//! adapter): start a session, `cd /tmp`, then `pwd`, and confirm the
//! combed signal reports `/tmp` WITHOUT the agent re-passing the cwd.
//! Also covers status (cwd reported), graceful stop, and the
//! default-deny denial when the `allow_session` cap is off.
//!
//! The session is a long-lived login-shell PTY, so it is unix-only.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{ContextHint, RuleDefinition, RuleStatus, RuleType, Severity};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    PolicyProfile, SessionState, ShellSessionExecParams, ShellSessionStartParams,
    ShellSessionStatusParams, ShellSessionStopParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-session-ipc-{tag}-{pid}-{nanos}-{n}"));
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

fn bash_available() -> bool {
    std::path::Path::new("/bin/bash").exists()
}

/// Live daemon on `full_access`: the loader preset flips every cap
/// (including `allow_session`) ON, so the session lane is allowed with
/// audit. Caps are config-only, never an MCP/IPC flag.
fn build_server_full_access() -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("server");
    let mut cfg = DaemonConfig::defaults_in(&data);
    cfg.policy.profile = PolicyProfile::FullAccess;
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

/// Live daemon on the DEFAULT profile (`developer_local`): caps default
/// false, so `allow_session` is OFF and the session lane is denied.
fn build_server_default() -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("default");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

/// Inline keyword rule that fires on any line containing `tmp`, so the
/// combed `pwd` output line `/tmp` surfaces as a structured signal in the
/// session bucket. Without an active rule the session emits no signal for
/// arbitrary output (combed, never raw).
fn tmp_rule() -> RuleDefinition {
    RuleDefinition {
        id: "session.cwd.tmp".to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Info,
        event_kind: "cwd".to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec!["tmp".to_owned()]),
        captures: vec![],
        summary_template: "cwd line: ${line}".to_owned(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// O-02: start a session, `cd /tmp`, then `pwd`; the combed signal must
/// report `/tmp` WITHOUT the agent re-passing cwd. Status reports cwd.
/// Stop is graceful (terminal state Exited).
#[test]
#[allow(clippy::too_many_lines)] // cohesive end-to-end O-02 flow
fn session_cd_then_pwd_reports_tmp_then_status_and_stop() {
    if !bash_available() {
        eprintln!("skipping: /bin/bash not present");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_full_access();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Start the session with the tmp rule bound to its bucket.
        let started = match client
            .call(
                1,
                IpcRequest::ShellSessionStart(ShellSessionStartParams {
                    shell: None,
                    cwd: None,
                    env: vec![],
                    rules: vec![tmp_rule()],
                    bucket_config: None,
                    tag: None,
                }),
            )
            .await
            .expect("session start")
        {
            IpcResponse::ShellSessionStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(matches!(
            started.state,
            SessionState::Live | SessionState::Starting
        ));

        // Give the interactive shell a moment to finish reading its rc
        // files before the first command (a write that races startup can be
        // dropped by the line discipline).
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Send `cd /tmp` (no combed signal expected — `cd` is silent).
        let _ = client
            .call(
                2,
                IpcRequest::ShellSessionExec(ShellSessionExecParams {
                    session_id: started.session_id,
                    line: "cd /tmp".to_owned(),
                    cursor: 0,
                    wait_ms: Some(800),
                }),
            )
            .await
            .expect("exec cd");

        // Send `pwd` on each poll. The shell prints `/tmp` in the cwd set by
        // the prior `cd` line (sticky cwd) — the agent never re-passes the
        // directory. Re-sending each iteration is robust against a single
        // send racing shell startup; the bucket cursor advances so a hit is
        // observed exactly once.
        let mut found_tmp = false;
        let mut cursor = 0u64;
        let mut seen: Vec<String> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut seq = 3u64;
        while std::time::Instant::now() < deadline {
            let resp = match client
                .call(
                    seq,
                    IpcRequest::ShellSessionExec(ShellSessionExecParams {
                        session_id: started.session_id,
                        line: "pwd".to_owned(),
                        cursor,
                        wait_ms: Some(800),
                    }),
                )
                .await
                .expect("exec pwd")
            {
                IpcResponse::ShellSessionExec(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            seq += 1;
            cursor = resp.next_cursor;
            for e in &resp.events {
                seen.push(format!("[{}] {}", e.kind, e.summary));
            }
            if resp.events.iter().any(|e| e.summary.contains("/tmp")) {
                found_tmp = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            found_tmp,
            "combed session signal must report /tmp from `pwd` after `cd /tmp` \
             (sticky cwd, O-02); events seen: {seen:?}"
        );

        // Status reports the tracked cwd (/tmp from the `cd` line).
        let status = match client
            .call(
                seq,
                IpcRequest::ShellSessionStatus(ShellSessionStatusParams {
                    session_id: started.session_id,
                }),
            )
            .await
            .expect("session status")
        {
            IpcResponse::ShellSessionStatus(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        seq += 1;
        assert_eq!(status.cwd.as_deref(), Some("/tmp"), "status cwd");
        assert!(matches!(status.state, SessionState::Live));

        // Graceful stop -> terminal state Exited.
        let stopped = match client
            .call(
                seq,
                IpcRequest::ShellSessionStop(ShellSessionStopParams {
                    session_id: started.session_id,
                }),
            )
            .await
            .expect("session stop")
        {
            IpcResponse::ShellSessionStop(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(stopped.state, SessionState::Exited);

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Default-deny: on the default `developer_local` profile the
/// `allow_session` cap is OFF, so `shell_session_start` is denied at the
/// `SessionStart` policy gate (PolicyDenied), never a synthetic session.
#[test]
fn session_start_denied_on_default_profile() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_default();
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                1,
                IpcRequest::ShellSessionStart(ShellSessionStartParams {
                    shell: None,
                    cwd: None,
                    env: vec![],
                    rules: vec![],
                    bucket_config: None,
                    tag: None,
                }),
            )
            .await
            .expect_err("session start must be denied when allow_session is off");
        assert_eq!(err.code, IpcErrorCode::PolicyDenied);
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Terminal-state guard: exec on an unknown session fails loudly with
/// UnknownSession, never hangs.
#[test]
fn session_exec_unknown_session_fails_loudly() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_full_access();
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                1,
                IpcRequest::ShellSessionExec(ShellSessionExecParams {
                    session_id: terminal_commander_core::SessionId::new(),
                    line: "pwd".to_owned(),
                    cursor: 0,
                    wait_ms: Some(200),
                }),
            )
            .await
            .expect_err("exec on unknown session must fail");
        assert_eq!(err.code, IpcErrorCode::UnknownSession);
        handle.shutdown().await;
        cleanup(&data);
    });
}
