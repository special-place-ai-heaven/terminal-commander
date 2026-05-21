// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC39 daemon signal API integration tests.
//!
//! Unix-only. End-to-end pipeline under test:
//!
//! ```text
//! CommandRuntime::start_combed
//!   -> ProcessProbe -> Router::bucket_append (PersistentAudit)
//! IPC client -> bucket_events_since   (cursor read)
//! IPC client -> bucket_wait           (Notify-backed, no busy poll)
//! IPC client -> bucket_summary        (counters only)
//! IPC client -> event_context         (bounded window by event_id)
//! ```
//!
//! No raw stdout/stderr is returned by any endpoint. Heartbeat is a
//! typed flag, never a stream dump.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ContextHint, JobState, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    BucketEventsSinceParams, BucketSummaryParams, BucketWaitParams, CommandStartRequest,
    ContextUnavailableReason, DaemonClient, DaemonConfig, DaemonState, EventContextParams,
    IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-ipc-bucket-{tag}-{pid}-{nanos}"));
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

fn build_state_and_server(
    data: &std::path::Path,
) -> (Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let cfg = DaemonConfig::defaults_in(data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let server = IpcServer::new(Arc::clone(&state), socket);
    let handle = server.spawn().unwrap();
    (state, handle)
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

/// Build a python3 argv that prints several stdout lines plus one
/// matching MARKER on stderr.
fn py_argv_with_marker(marker: &str, count: u32) -> Vec<String> {
    let m = marker.replace('\'', "\\'");
    let script = format!(
        "import sys\nfor i in range({count}):\n    print('quiet line', i)\nprint('{m}: hit', file=sys.stderr)\nfor i in range(2):\n    print('after line', i)\n"
    );
    vec!["python3".to_owned(), "-c".to_owned(), script]
}

#[test]
fn bucket_events_since_returns_structured_events_no_raw_text() {
    rt().block_on(async {
        let data = tmp_data_dir("bes");
        let (state, handle) = build_state_and_server(&data);

        // Run a non-shell command that emits noise + one MARKER.
        let rule = keyword_rule("test.marker", "MARKER", Severity::High, "kw_marker");
        let start = state
            .command
            .start_combed(CommandStartRequest {
                argv: py_argv_with_marker("MARKER", 3),
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![rule],
                grace: None,
            })
            .unwrap();

        // Wait for lifecycle completion.
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(start.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
            ) {
                break;
            }
        }

        // IPC client request.
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let resp = client
            .call(
                1,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: start.bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                }),
            )
            .await
            .unwrap();
        match resp {
            IpcResponse::BucketEventsSince(b) => {
                assert!(!b.events.is_empty());
                // kw_marker AND a lifecycle event are present.
                let kinds: Vec<&str> = b.events.iter().map(|e| e.kind.as_str()).collect();
                assert!(kinds.contains(&"kw_marker"), "kinds: {kinds:?}");
                assert!(
                    kinds.contains(&"command_exited") || kinds.contains(&"command_failed"),
                    "kinds: {kinds:?}"
                );
                // No raw stdout body leaked: only the rule summary
                // template renders for keyword-matched events.
                for e in &b.events {
                    if e.kind == "command_exited" || e.kind == "command_failed" {
                        continue;
                    }
                    assert!(
                        !e.summary.contains("quiet line"),
                        "raw stdout leaked into event summary: {}",
                        e.summary
                    );
                }
            }
            other => panic!("unexpected response: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_wait_returns_heartbeat_when_no_events_arrive() {
    rt().block_on(async {
        let data = tmp_data_dir("hb");
        let (state, handle) = build_state_and_server(&data);

        // Create an empty bucket via the router (no command needed).
        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let resp = client
            .call(
                1,
                IpcRequest::BucketWait(BucketWaitParams {
                    bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                    timeout_ms: Some(100),
                }),
            )
            .await
            .unwrap();
        match resp {
            IpcResponse::BucketWait(w) => {
                assert!(w.heartbeat, "expected heartbeat on timeout: {w:?}");
                assert!(w.events.is_empty());
                assert_eq!(w.next_cursor, w.cursor_in);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_wait_wakes_on_command_event() {
    rt().block_on(async {
        let data = tmp_data_dir("wake");
        let (state, handle) = build_state_and_server(&data);

        // Pre-create the bucket so the client can wait on it.
        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();

        // Spawn the wait client BEFORE the event arrives.
        let socket = handle.socket_path().to_path_buf();
        let client = DaemonClient::new(socket).with_timeout(Duration::from_secs(5));
        let waiter = tokio::spawn(async move {
            client
                .call(
                    11,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id,
                        cursor: 0,
                        severity_min: Some(Severity::Medium),
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(3_000),
                    }),
                )
                .await
        });

        // Give the waiter a moment to register.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Append a High-severity event into the bucket through the
        // router. Use a synthetic draft so we don't have to spawn a
        // child process here.
        let mut caps = terminal_commander_core::Captures::new();
        caps.insert("pkg".to_owned(), "libssl-dev".to_owned());
        let draft = terminal_commander_core::EventDraft {
            bucket_id,
            timestamp: time::OffsetDateTime::now_utc(),
            severity: Severity::High,
            kind: "missing_package".to_owned(),
            summary: "APT could not locate libssl-dev".to_owned(),
            rule: None,
            source: terminal_commander_core::EventSource {
                probe_id: terminal_commander_core::ProbeId::new(),
                source_type: terminal_commander_core::SourceType::Process,
                stream: terminal_commander_core::SourceStream::Stderr,
                job_id: None,
            },
            captures: Some(caps),
            pointer:
                Some(
                    terminal_commander_core::SourcePointer::new(
                        terminal_commander_core::FrameId::new(),
                    )
                    .with_line(1),
                ),
            pointer_unavailable_reason: None,
            tags: None,
            frame_truncated_bytes: 0,
            count: 1,
            first_seen: None,
            last_seen: None,
            suppressed: false,
        };
        state.router.bucket_append(bucket_id, draft).unwrap();

        // The waiter must wake up.
        let resp = waiter.await.unwrap().unwrap();
        match resp {
            IpcResponse::BucketWait(w) => {
                assert!(!w.heartbeat, "expected wake, not heartbeat");
                assert!(!w.events.is_empty());
                assert_eq!(w.events[0].kind, "missing_package");
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_summary_reports_counts_only() {
    rt().block_on(async {
        let data = tmp_data_dir("sum");
        let (state, handle) = build_state_and_server(&data);

        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let resp = client
            .call(
                2,
                IpcRequest::BucketSummary(BucketSummaryParams { bucket_id }),
            )
            .await
            .unwrap();
        match resp {
            IpcResponse::BucketSummary(s) => {
                assert_eq!(s.bucket_id, bucket_id);
                assert_eq!(s.event_count, 0);
                assert_eq!(s.by_severity.high, 0);
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn event_context_returns_bounded_window_around_event_pointer() {
    rt().block_on(async {
        let data = tmp_data_dir("ctx");
        let (state, handle) = build_state_and_server(&data);

        // Run a command that produces several stdout frames and one
        // matching stderr line (the matched event will carry a
        // pointer into the context ring).
        let rule = keyword_rule("test.boom", "BOOM", Severity::High, "kw_boom");
        let start = state
            .command
            .start_combed(CommandStartRequest {
                argv: py_argv_with_marker("BOOM", 4),
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![rule],
                grace: None,
            })
            .unwrap();

        // Wait for exit.
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(start.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
            ) {
                break;
            }
        }

        // Find the kw_boom event_id through bucket_events_since.
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let bes = client
            .call(
                3,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: start.bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                }),
            )
            .await
            .unwrap();
        let event_id = match bes {
            IpcResponse::BucketEventsSince(b) => b
                .events
                .iter()
                .find(|e| e.kind == "kw_boom")
                .map(|e| e.event_id)
                .expect("kw_boom should be present"),
            other => panic!("unexpected: {other:?}"),
        };

        // event_context by id.
        let resp = client
            .call(
                4,
                IpcRequest::EventContext(EventContextParams {
                    bucket_id: start.bucket_id,
                    event_id,
                    before: Some(2),
                    after: Some(2),
                    max_bytes: None,
                }),
            )
            .await
            .unwrap();
        match resp {
            IpcResponse::EventContext(c) => {
                assert!(!c.anchor_missing, "anchor missing: {c:?}");
                assert!(c.unavailable_reason.is_none());
                assert!(
                    !c.frames.is_empty(),
                    "expected non-empty context frames; got {c:?}"
                );
                // Bounded: 2 before + 1 anchor + 2 after = 5 max.
                assert!(c.frames.len() <= 5, "frames: {}", c.frames.len());
                // Total bytes accounted for.
                let computed_bytes: usize = c.frames.iter().map(|f| f.text.len()).sum();
                assert_eq!(c.total_bytes, computed_bytes);
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn event_context_returns_no_pointer_for_below_medium_event() {
    rt().block_on(async {
        let data = tmp_data_dir("ctx-low");
        let (state, handle) = build_state_and_server(&data);

        // Create a bucket, append a low-severity event with NO
        // pointer (allowed by TC02 for < Medium).
        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();
        let draft = terminal_commander_core::EventDraft {
            bucket_id,
            timestamp: time::OffsetDateTime::now_utc(),
            severity: Severity::Low,
            kind: "command_exited".to_owned(),
            summary: "low-severity lifecycle".to_owned(),
            rule: None,
            source: terminal_commander_core::EventSource {
                probe_id: terminal_commander_core::ProbeId::new(),
                source_type: terminal_commander_core::SourceType::Process,
                stream: terminal_commander_core::SourceStream::Meta,
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
        let ev = state.router.bucket_append(bucket_id, draft).unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let resp = client
            .call(
                5,
                IpcRequest::EventContext(EventContextParams {
                    bucket_id,
                    event_id: ev.event_id,
                    before: None,
                    after: None,
                    max_bytes: None,
                }),
            )
            .await
            .unwrap();
        match resp {
            IpcResponse::EventContext(c) => {
                assert!(c.frames.is_empty());
                assert_eq!(
                    c.unavailable_reason,
                    Some(ContextUnavailableReason::NoPointer)
                );
                assert!(!c.anchor_missing);
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_events_since_unknown_bucket_returns_typed_error() {
    rt().block_on(async {
        let data = tmp_data_dir("nobkt");
        let (_state, handle) = build_state_and_server(&data);

        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                7,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: terminal_commander_core::BucketId::new(),
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::BucketNotFound);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn event_context_unknown_event_returns_typed_error() {
    rt().block_on(async {
        let data = tmp_data_dir("noev");
        let (state, handle) = build_state_and_server(&data);

        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let err = client
            .call(
                9,
                IpcRequest::EventContext(EventContextParams {
                    bucket_id,
                    event_id: terminal_commander_core::EventId::new(),
                    before: None,
                    after: None,
                    max_bytes: None,
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, IpcErrorCode::EventNotFound);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_events_since_clamps_oversized_limit() {
    rt().block_on(async {
        let data = tmp_data_dir("clamp");
        let (state, handle) = build_state_and_server(&data);

        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        // Asking for 999_999 events MUST be silently clamped, NOT
        // rejected, and the response is bounded.
        let resp = client
            .call(
                12,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: Some(999_999),
                }),
            )
            .await
            .unwrap();
        match resp {
            IpcResponse::BucketEventsSince(b) => {
                assert!(b.events.len() <= terminal_commanderd::MAX_BUCKET_READ_LIMIT);
            }
            other => panic!("unexpected: {other:?}"),
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn ipc_bucket_methods_emit_persistent_audit_rows() {
    rt().block_on(async {
        let data = tmp_data_dir("audit");
        let (state, handle) = build_state_and_server(&data);

        let bucket_id = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bucket_id, terminal_commander_core::BucketConfig::default())
            .unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf());
        let _ = client
            .call(
                21,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                }),
            )
            .await
            .unwrap();
        let _ = client
            .call(
                22,
                IpcRequest::BucketSummary(BucketSummaryParams { bucket_id }),
            )
            .await
            .unwrap();
        let _ = client
            .call(
                23,
                IpcRequest::BucketWait(BucketWaitParams {
                    bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                    timeout_ms: Some(50),
                }),
            )
            .await
            .unwrap();

        let rows = {
            let mut g = state.store.lock();
            g.audit_since(&AuditReadRequest::new(0)).unwrap()
        };
        let actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
        assert!(actions.contains(&"ipc_bucket_events_since"));
        assert!(actions.contains(&"ipc_bucket_summary"));
        assert!(actions.contains(&"ipc_bucket_wait"));

        handle.shutdown().await;
        cleanup(&data);
    });
}
