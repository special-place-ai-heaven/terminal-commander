// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC44 daemon IPC tests for the PTY command surface.
//!
//! Covers `pty_command_start`, `pty_command_write_stdin`,
//! `pty_command_stop`, `pty_command_list` plus the typed error
//! paths: ShellInterpreterDenied, ArgvInvalid, OversizedRequest,
//! SecretInputDenied, UnknownJob.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{ContextHint, RuleDefinition, RuleStatus, RuleType, Severity};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
    PtyCommandStartParams, PtyCommandStopParams, PtyCommandWriteStdinParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-pty-ipc-{tag}-{pid}-{nanos}-{n}"));
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
    for candidate in ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"] {
        if std::path::Path::new(candidate).exists() {
            return true;
        }
    }
    false
}

fn keyword_rule(id: &str, kw: &str, severity: Severity, kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity,
        event_kind: kind.to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec![kw.to_owned()]),
        captures: vec![],
        summary_template: format!("matched {kw}"),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// Poll the live list until `job_id` appears, so a stdin write never races
/// the PTY spawn. Panics past the deadline.
async fn wait_until_live(client: &DaemonClient, job_id: terminal_commander_core::JobId) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut seq = 900u64;
    loop {
        let listed = match client
            .call(seq, IpcRequest::PtyCommandList)
            .await
            .expect("list")
        {
            IpcResponse::PtyCommandList(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        seq += 1;
        if listed.entries.iter().any(|e| e.job_id == job_id) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pty job never became live"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn build_server() -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("server");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

#[test]
fn pty_command_start_then_stop_returns_metrics() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let py = r#"
import sys, time
print("pty hello", flush=True)
time.sleep(0.2)
print("pty bye", flush=True)
"#;
        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        py.to_owned(),
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
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // M2: poll until the short-lived script has exited (drops out of the
        // live list) instead of a fixed 800ms sleep that races slow PTY spawn /
        // CI load. Once exited, both frames have been emitted and counted.
        let mut seq = 2u64;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let listed = match client
                .call(seq, IpcRequest::PtyCommandList)
                .await
                .expect("pty list")
            {
                IpcResponse::PtyCommandList(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            seq += 1;
            let still_live = listed.entries.iter().any(|e| e.job_id == started.job_id);
            if !still_live || std::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let stop = client
            .call(
                seq,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await
            .expect("pty stop");
        let stopped = match stop {
            IpcResponse::PtyCommandStop(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(stopped.job_id, started.job_id);
        assert!(
            stopped.frames_total >= 2,
            "frames: {}",
            stopped.frames_total
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn pty_command_rejects_shell_interpreter() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf());

        let err = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec!["bash".to_owned()],
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
            .expect_err("shell interpreter must be denied");
        assert_eq!(err.code, IpcErrorCode::ShellInterpreterDenied);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn pty_command_rejects_empty_argv() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![],
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
            .expect_err("empty argv rejected");
        assert_eq!(err.code, IpcErrorCode::ArgvInvalid);
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn pty_write_stdin_oversized_is_rejected() {
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
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        "import time; time.sleep(2)".to_owned(),
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
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Oversized stdin.
        let huge = "x".repeat(16_000);
        let err = client
            .call(
                2,
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: started.job_id,
                    bytes: huge,
                    cursor: None,
                    wait_ms: None,
                }),
            )
            .await
            .expect_err("oversized stdin");
        assert_eq!(err.code, IpcErrorCode::OversizedRequest);

        let _ = client
            .call(
                3,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await;

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn pty_write_stdin_unknown_job_returns_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                1,
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: terminal_commander_core::JobId::new(),
                    bytes: "hello".to_owned(),
                    cursor: None,
                    wait_ms: None,
                }),
            )
            .await
            .expect_err("unknown job");
        assert_eq!(err.code, IpcErrorCode::UnknownJob);
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // cohesive end-to-end secret-prompt flow; the M2 poll added lines
fn pty_write_stdin_denied_during_secret_prompt() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        // Emit a `[sudo] password for dev:` line via python — the
        // prompt detector flags this as SudoPassword (secret).
        let py = r#"
import sys, time
sys.stdout.write("[sudo] password for dev: ")
sys.stdout.flush()
time.sleep(2)
"#;
        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        py.to_owned(),
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
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // M2: poll the read-only live list until the PtyProbe has consumed the
        // prompt line and flagged it secret, instead of a fixed 500ms sleep that
        // races prompt consumption under load. Polling a read-only signal (not
        // speculative writes) means no stdin is ever sent before the flag is set.
        let mut seq = 2u64;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let listed = match client
                .call(seq, IpcRequest::PtyCommandList)
                .await
                .expect("pty list")
            {
                IpcResponse::PtyCommandList(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            seq += 1;
            let flagged = listed
                .entries
                .iter()
                .find(|e| e.job_id == started.job_id)
                .is_some_and(|e| e.secret_prompt_active);
            if flagged {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "secret prompt never flagged active within deadline; entries: {:?}",
                listed.entries
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let err = client
            .call(
                seq,
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: started.job_id,
                    bytes: "should-not-be-sent\n".to_owned(),
                    cursor: None,
                    wait_ms: None,
                }),
            )
            .await
            .expect_err("secret prompt must reject stdin");
        assert_eq!(err.code, IpcErrorCode::SecretInputDenied);

        // Audit row exists, decision=deny, metadata does NOT carry
        // the typed payload.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let deny_row = rows
            .iter()
            .find(|r| r.action == "pty_command_write_stdin" && r.decision == "deny")
            .expect("deny audit row");
        let metadata = deny_row.metadata_json.as_deref().unwrap_or("");
        assert!(
            !metadata.contains("should-not-be-sent"),
            "audit metadata must NOT carry the typed payload; got: {metadata}"
        );
        assert!(
            metadata.contains("\"prompt_kind\":\"secret\""),
            "audit metadata must record prompt_kind=secret; got: {metadata}"
        );

        let _ = client
            .call(
                3,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await;

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn pty_command_list_reflects_live_then_stopped_state() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        // Long-lived job so it is reliably live when we list it.
        let py = "import time\ntime.sleep(2.0)\n";
        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        py.to_owned(),
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
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // The running job must appear in the snapshot.
        let resp = client
            .call(2, IpcRequest::PtyCommandList)
            .await
            .expect("pty list");
        let listed = match resp {
            IpcResponse::PtyCommandList(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            listed.entries.iter().any(|e| e.job_id == started.job_id),
            "running pty job {} must appear in pty_command_list; got {:?}",
            started.job_id,
            listed.entries
        );

        // Stop it.
        let resp = client
            .call(
                3,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await
            .expect("pty stop");
        match resp {
            IpcResponse::PtyCommandStop(_) => {}
            other => panic!("unexpected: {other:?}"),
        }

        // After stop the job must no longer be listed as live.
        let resp = client
            .call(4, IpcRequest::PtyCommandList)
            .await
            .expect("pty list after stop");
        let listed = match resp {
            IpcResponse::PtyCommandList(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            !listed.entries.iter().any(|e| e.job_id == started.job_id),
            "stopped pty job {} must not appear in pty_command_list; got {:?}",
            started.job_id,
            listed.entries
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

// US5 / FR-041: `pty_command_write_stdin` gains an optional bounded
// `wait_ms` settle window that returns the combed signals the write
// provoked in the SAME call (cursor in, signals + next cursor out) --
// the same shape family as `shell_session_exec`. Omitted = today's
// immediate return, byte-identical.

#[test]
fn pty_stdin_wait_ms_returns_combed_signals_with_cursor() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(8));

        // A REPL that emits a rule-matching line for each stdin line.
        let py =
            "import sys\nfor line in sys.stdin:\n    print('RESULT', eval(line.strip()), flush=True)\n";
        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        py.to_owned(),
                    ],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![keyword_rule(
                        "test.result",
                        "RESULT",
                        Severity::Medium,
                        "kw_result",
                    )],
                    rows: None,
                    cols: None,
                    tag: None,
                }),
            )
            .await
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        wait_until_live(&client, started.job_id).await;

        // ONE call: write stdin + settle-read the signals it provoked.
        let resp = client
            .call(
                2,
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: started.job_id,
                    bytes: "1+1\n".to_owned(),
                    cursor: Some(0),
                    wait_ms: Some(5000),
                }),
            )
            .await
            .expect("pty stdin wait");
        let r = match resp {
            IpcResponse::PtyCommandWriteStdin(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(r.bytes_written > 0, "bytes must be written");
        assert_eq!(r.cursor_in, Some(0), "cursor_in echoes the requested cursor");
        let events = r.events.expect("wait_ms must return an events batch");
        assert!(
            events.iter().any(|e| e.kind == "kw_result"),
            "expected kw_result signal from the REPL; got {events:?}"
        );
        let next = r.next_cursor.expect("wait_ms must return next_cursor");
        assert!(next > 0, "next cursor must advance past the head");
        assert!(r.has_more.is_some(), "has_more present when waited");
        assert!(r.dropped_count.is_some(), "dropped_count present when waited");

        let _ = client
            .call(
                3,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await;
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn pty_stdin_without_wait_is_byte_identical_to_today() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(6));

        let py = "import sys\nfor line in sys.stdin:\n    sys.stdout.write('echoed: ' + line)\n    sys.stdout.flush()\n";
        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        py.to_owned(),
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
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        wait_until_live(&client, started.job_id).await;

        // No cursor / wait_ms -> immediate return, today's legacy shape.
        let resp = client
            .call(
                2,
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: started.job_id,
                    bytes: "hello\n".to_owned(),
                    cursor: None,
                    wait_ms: None,
                }),
            )
            .await
            .expect("pty stdin no wait");
        let r = match resp {
            IpcResponse::PtyCommandWriteStdin(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.bytes_written, 6);
        assert!(r.cursor_in.is_none());
        assert!(r.next_cursor.is_none());
        assert!(r.has_more.is_none());
        assert!(r.dropped_count.is_none());
        assert!(r.events.is_none());

        // Byte-identical wire: the serialized response is EXACTLY today's
        // three keys -- the new optional fields skip-serialize when absent.
        let json = serde_json::to_value(&r).expect("serialize response");
        let keys: std::collections::BTreeSet<&str> = json
            .as_object()
            .expect("response is an object")
            .keys()
            .map(String::as_str)
            .collect();
        let expected: std::collections::BTreeSet<&str> =
            ["job_id", "bytes_written", "secret_prompt_active"]
                .into_iter()
                .collect();
        assert_eq!(
            keys, expected,
            "no-wait response must serialize byte-identically to today"
        );

        let _ = client
            .call(
                3,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await;
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)] // cohesive end-to-end secret-prompt flow
fn pty_stdin_secret_prompt_denial_unchanged_by_wait() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(6));

        // Emit a `[sudo] password for dev:` prompt (flagged secret).
        let py = r#"
import sys, time
sys.stdout.write("[sudo] password for dev: ")
sys.stdout.flush()
time.sleep(2)
"#;
        let resp = client
            .call(
                1,
                IpcRequest::PtyCommandStart(PtyCommandStartParams {
                    environment: None,
                    argv: vec![
                        "python3".to_owned(),
                        "-u".to_owned(),
                        "-c".to_owned(),
                        py.to_owned(),
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
            .expect("pty start");
        let started = match resp {
            IpcResponse::PtyCommandStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Poll the read-only live list until the prompt is flagged secret.
        let mut seq = 2u64;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let listed = match client
                .call(seq, IpcRequest::PtyCommandList)
                .await
                .expect("pty list")
            {
                IpcResponse::PtyCommandList(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            seq += 1;
            let flagged = listed
                .entries
                .iter()
                .find(|e| e.job_id == started.job_id)
                .is_some_and(|e| e.secret_prompt_active);
            if flagged {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "secret prompt never flagged active within deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Denial fires BEFORE the write and is unchanged by wait_ms/cursor:
        // no settle read happens, the typed error is identical to today.
        let err = client
            .call(
                seq,
                IpcRequest::PtyCommandWriteStdin(PtyCommandWriteStdinParams {
                    job_id: started.job_id,
                    bytes: "should-not-be-sent\n".to_owned(),
                    cursor: Some(0),
                    wait_ms: Some(3000),
                }),
            )
            .await
            .expect_err("secret prompt must reject stdin even with wait_ms");
        assert_eq!(err.code, IpcErrorCode::SecretInputDenied);

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let deny_row = rows
            .iter()
            .find(|r| r.action == "pty_command_write_stdin" && r.decision == "deny")
            .expect("deny audit row");
        let metadata = deny_row.metadata_json.as_deref().unwrap_or("");
        assert!(
            !metadata.contains("should-not-be-sent"),
            "audit metadata must NOT carry the typed payload; got: {metadata}"
        );

        let _ = client
            .call(
                3,
                IpcRequest::PtyCommandStop(PtyCommandStopParams {
                    job_id: started.job_id,
                }),
            )
            .await;
        handle.shutdown().await;
        cleanup(&data);
    });
}
