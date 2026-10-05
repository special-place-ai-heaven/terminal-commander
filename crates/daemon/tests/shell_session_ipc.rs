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
use terminal_commanderd::ipc::protocol::{AuditSinceParams, AuditSinceResponse};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    PolicyProfile, SessionState, ShellSessionExecParams, ShellSessionStartParams,
    ShellSessionStatusParams, ShellSessionStopParams, WorkspaceSnapshotApplyParams,
    WorkspaceSnapshotCreateParams,
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
/// `developer_local`: sessions stay off unless `allow_session` is set. The
/// default `full_access` profile presets it on.
fn build_server_hardened() -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("hardened");
    let mut cfg = DaemonConfig::defaults_in(&data);
    cfg.policy.profile = terminal_commanderd::PolicyProfile::DeveloperLocal;
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

/// The os_guard failsafe binds `shell_session_exec` too: a line deleting
/// under a protected root is refused with the typed code and never reaches
/// the shell, including a relative operand after a tracked `cd`. TEST
/// SAFETY: refused targets do not exist; the allowed control deletes a
/// non-existent child of the data dir.
#[test]
fn session_exec_os_guard_refuses_protected_deletion_and_allows_control() {
    if !bash_available() {
        eprintln!("skipping: /bin/bash not present");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_full_access();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        let started = match client
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
            .expect("session start")
        {
            IpcResponse::ShellSessionStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        let exec = |line: String| {
            IpcRequest::ShellSessionExec(ShellSessionExecParams {
                session_id: started.session_id,
                line,
                cursor: 0,
                wait_ms: Some(50),
            })
        };

        let err = client
            .call(2, exec("rm -rf /usr/lib/tc-guard-nonexistent".to_owned()))
            .await
            .expect_err("protected deletion must be refused");
        assert_eq!(err.code, IpcErrorCode::OsCriticalPathProtected, "{err:?}");

        // The refusal writes the same kind of audit row the shell_exec lane's
        // deny path writes (command_shell_rejected): the session lane has no
        // audit sink of its own, so it reuses PtyRuntime's.
        let audit = client
            .call(
                99,
                IpcRequest::AuditSince(AuditSinceParams {
                    cursor: 0,
                    action_filter: Some("shell_session_exec_rejected".to_owned()),
                    decision_filter: None,
                    limit: Some(10),
                }),
            )
            .await
            .expect("audit_since");
        let IpcResponse::AuditSince(AuditSinceResponse { rows, .. }) = audit else {
            panic!("unexpected: {audit:?}");
        };
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].decision, "deny");
        assert!(
            rows[0]
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("TC's one failsafe")),
            "{:?}",
            rows[0].reason
        );

        // A tracked `cd` moves the cwd relative operands resolve against.
        client
            .call(3, exec("cd /usr/lib".to_owned()))
            .await
            .expect("exec cd");
        let err = client
            .call(4, exec("rm -rf tc-guard-nonexistent".to_owned()))
            .await
            .expect_err("relative deletion under /usr/lib must be refused");
        assert_eq!(err.code, IpcErrorCode::OsCriticalPathProtected, "{err:?}");

        let victim = data.join("tc-guard-nonexistent");
        client
            .call(5, exec(format!("rm -rf '{}'", victim.display())))
            .await
            .expect("ordinary deletion runs");

        handle.shutdown().await;
        cleanup(&data);
    });
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
fn session_start_denied_on_developer_local() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_hardened();
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

/// F-003 (security): a session started with a secret-shaped env value must NOT
/// surface that secret verbatim anywhere. Start with env
/// `[("TOKEN", "supersecretvalue")]`, snapshot the workspace, and assert the
/// literal secret appears NOWHERE in (a) the persisted snapshot row / `env_json`
/// and (b) the `shell_session_status` response. The session runtime masks
/// secret-shaped values at capture time via `command::redact_env_pairs`, so
/// `<redacted>` is what reaches both surfaces.
#[test]
fn session_env_secret_is_redacted_in_snapshot_and_status() {
    const SECRET: &str = "supersecretvalue";
    if !bash_available() {
        eprintln!("skipping: /bin/bash not present");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server_full_access();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Start a session carrying a secret-keyed env var.
        let started = match client
            .call(
                1,
                IpcRequest::ShellSessionStart(ShellSessionStartParams {
                    shell: None,
                    cwd: None,
                    env: vec![("TOKEN".to_owned(), SECRET.to_owned())],
                    rules: vec![],
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

        // (a) Persisted snapshot row: create via IPC, then read the row back
        // straight from the store actor and inspect the serialized env_json.
        let snap = match client
            .call(
                2,
                IpcRequest::WorkspaceSnapshotCreate(WorkspaceSnapshotCreateParams {
                    session_id: started.session_id,
                    name: Some("redaction-probe".to_owned()),
                }),
            )
            .await
            .expect("snapshot create")
        {
            IpcResponse::WorkspaceSnapshotCreate(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        let row = state
            .store
            .get_workspace_snapshot(&snap.snapshot_id)
            .expect("store read")
            .expect("snapshot row must exist");
        // The TOKEN key stays visible; only the value is masked.
        let env_pairs: Vec<String> = row.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let env_json = serde_json::to_string(&row.env).expect("serialize env");
        assert!(
            !env_json.contains(SECRET),
            "secret value must NOT be persisted verbatim in env_json: {env_json}"
        );
        assert!(
            env_pairs.iter().any(|p| p.starts_with("TOKEN=")),
            "the TOKEN key must survive (only its value is masked): {env_pairs:?}"
        );
        assert!(
            env_pairs.iter().any(|p| p.contains("<redacted>")),
            "the secret value must be replaced with <redacted>: {env_pairs:?}"
        );

        // (b) Status response: the env_snapshot it returns must also be masked.
        let status = match client
            .call(
                3,
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
        let status_env: Vec<String> = status
            .env_snapshot
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        assert!(
            !status_env.iter().any(|p| p.contains(SECRET)),
            "secret value must NOT appear in shell_session_status: {status_env:?}"
        );
        assert!(
            status_env.iter().any(|p| p.contains("<redacted>")),
            "status env value must be masked: {status_env:?}"
        );

        let _ = client
            .call(
                4,
                IpcRequest::ShellSessionStop(ShellSessionStopParams {
                    session_id: started.session_id,
                }),
            )
            .await;
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// A snapshot holds `<redacted>` for a masked value (F-003), so applying it
/// must not export that marker over the real value: the key is skipped and
/// named in the response.
#[test]
#[allow(clippy::too_many_lines)] // one start -> snapshot -> apply -> read flow
fn snapshot_apply_skips_masked_values_instead_of_exporting_the_marker() {
    if !bash_available() {
        eprintln!("skipping: /bin/bash not present");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_full_access();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));
        let mut rule = tmp_rule();
        rule.id = "session.valmark".to_owned();
        rule.keywords = Some(vec!["VALMARK".to_owned()]);
        let started = match client
            .call(
                1,
                IpcRequest::ShellSessionStart(ShellSessionStartParams {
                    shell: None,
                    cwd: None,
                    env: vec![
                        ("FOO_TOKEN".to_owned(), "real".to_owned()),
                        ("PLAIN_VAR".to_owned(), "kept".to_owned()),
                    ],
                    rules: vec![rule],
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
        tokio::time::sleep(Duration::from_millis(300)).await;

        let snap = match client
            .call(
                2,
                IpcRequest::WorkspaceSnapshotCreate(WorkspaceSnapshotCreateParams {
                    session_id: started.session_id,
                    name: None,
                }),
            )
            .await
            .expect("snapshot create")
        {
            IpcResponse::WorkspaceSnapshotCreate(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        let applied = match client
            .call(
                3,
                IpcRequest::WorkspaceSnapshotApply(WorkspaceSnapshotApplyParams {
                    snapshot_id: snap.snapshot_id,
                    session_id: started.session_id,
                }),
            )
            .await
            .expect("snapshot apply")
        {
            IpcResponse::WorkspaceSnapshotApply(r) => r,
            other => panic!("unexpected: {other:?}"),
        };

        // Read FOO_TOKEN back from the shell.
        let mut cursor = 0u64;
        let mut seen: Vec<String> = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut seq = 4u64;
        while std::time::Instant::now() < deadline
            && !seen
                .iter()
                .any(|s| s.contains("VALMARK:") && !s.contains('$'))
        {
            let resp = match client
                .call(
                    seq,
                    IpcRequest::ShellSessionExec(ShellSessionExecParams {
                        session_id: started.session_id,
                        line: "echo \"VALMARK:$FOO_TOKEN:$PLAIN_VAR:\"".to_owned(),
                        cursor,
                        wait_ms: Some(800),
                    }),
                )
                .await
                .expect("exec echo")
            {
                IpcResponse::ShellSessionExec(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            seq += 1;
            cursor = resp.next_cursor;
            seen.extend(resp.events.iter().map(|e| e.summary.clone()));
        }
        assert!(
            !seen.iter().any(|s| s.contains("VALMARK:<redacted>:")),
            "the marker was exported over the real value: {seen:?}"
        );
        assert!(
            seen.iter().any(|s| s.contains("VALMARK:real:kept:")),
            "FOO_TOKEN must keep its real value and PLAIN_VAR must be restored: {seen:?}"
        );
        assert_eq!(applied.skipped_redacted, vec!["FOO_TOKEN".to_owned()]);

        let _ = client
            .call(
                seq,
                IpcRequest::ShellSessionStop(ShellSessionStopParams {
                    session_id: started.session_id,
                }),
            )
            .await;
        handle.shutdown().await;
        cleanup(&data);
    });
}
