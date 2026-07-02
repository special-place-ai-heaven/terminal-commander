// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC43 daemon IPC tests for the file probe surface.
//!
//! Covers `file_read_window`, `file_search`, `file_watch_start` +
//! `file_watch_stop`, plus the typed error paths: PathDenied,
//! FileNotFound, FileBinary, OversizedRequest, UnknownWatch.

#![cfg(unix)]

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{ContextHint, RuleDefinition, RuleStatus, RuleType, Severity};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, DirEntryKind, FileListDirParams, FileReadWindowParams,
    FileSearchParams, FileWatchStartParams, FileWatchStopParams, FileWriteParams,
    FileWriteResponse, IpcErrorCode, IpcRequest, IpcResponse, IpcServer, MAX_FILE_WRITE_BYTES,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    // Unique per call: pid + nanos + a process-local atomic counter. nanos alone
    // is NOT enough — two concurrent build_server() calls in the same test
    // binary (same pid) can resolve to the same timestamp and collide on one
    // SQLite data dir, which then fails migrations with a UNIQUE constraint on
    // schema_migrations.version. The counter guarantees distinctness.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    p.push(format!("tc-file-ipc-{tag}-{pid}-{nanos}-{n}"));
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

fn write_text(path: &std::path::Path, text: &str) {
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn build_server() -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("server");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

fn kw_rule(id: &str, keyword: &str, event_kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: event_kind.to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec![keyword.to_owned()]),
        captures: vec![],
        summary_template: "matched".to_owned(),
        tags: vec!["tc43".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[test]
fn file_read_window_returns_bounded_lines() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let tmp = data.join("read.txt");
        write_text(&tmp, "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Full read.
        let resp = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: tmp.clone(),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect("read");
        let r = match resp {
            IpcResponse::FileReadWindow(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.lines.len(), 6);
        assert_eq!(r.lines[0].text, "alpha");
        assert_eq!(r.lines[0].line, 1);

        // Bounded window: start_line=3, max_lines=2.
        let resp = client
            .call(
                2,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: tmp.clone(),
                    start_line: Some(3),
                    max_lines: Some(2),
                    max_bytes: None,
                }),
            )
            .await
            .expect("read window");
        let r = match resp {
            IpcResponse::FileReadWindow(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.lines.len(), 2);
        assert_eq!(r.lines[0].text, "gamma");
        assert_eq!(r.lines[1].text, "delta");
        assert!(r.truncated);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_read_window_denies_default_deny_path() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: PathBuf::from("/etc/shadow"),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect_err("default-deny path must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_read_window_missing_file_returns_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: data.join("does-not-exist.txt"),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect_err("missing file must be rejected");
        assert_eq!(err.code, IpcErrorCode::FileNotFound);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_search_returns_bounded_matches() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let tmp = data.join("search.txt");
        write_text(
            &tmp,
            "alpha\nneedle here\nbeta\nNEEDLE upper\nneedle again\n",
        );
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Case-sensitive: 2 matches.
        let resp = client
            .call(
                1,
                IpcRequest::FileSearch(FileSearchParams {
                    path: tmp.clone(),
                    query: "needle".to_owned(),
                    case_insensitive: None,
                    max_matches: None,
                    max_snippet_bytes: None,
                }),
            )
            .await
            .expect("search");
        let r = match resp {
            IpcResponse::FileSearch(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.matches.len(), 2);
        assert_eq!(r.matches[0].line, 2);
        assert_eq!(r.matches[1].line, 5);
        assert!(r.matches[0].snippet.contains("needle"));

        // Case-insensitive: 3 matches.
        let resp = client
            .call(
                2,
                IpcRequest::FileSearch(FileSearchParams {
                    path: tmp.clone(),
                    query: "needle".to_owned(),
                    case_insensitive: Some(true),
                    max_matches: None,
                    max_snippet_bytes: None,
                }),
            )
            .await
            .expect("search ci");
        let r = match resp {
            IpcResponse::FileSearch(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.matches.len(), 3);

        // Cap via max_matches.
        let resp = client
            .call(
                3,
                IpcRequest::FileSearch(FileSearchParams {
                    path: tmp.clone(),
                    query: "needle".to_owned(),
                    case_insensitive: Some(true),
                    max_matches: Some(1),
                    max_snippet_bytes: None,
                }),
            )
            .await
            .expect("search cap");
        let r = match resp {
            IpcResponse::FileSearch(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.matches.len(), 1);
        assert!(r.truncated);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_search_rejects_empty_query() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let tmp = data.join("q.txt");
        write_text(&tmp, "anything\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileSearch(FileSearchParams {
                    path: tmp.clone(),
                    query: String::new(),
                    case_insensitive: None,
                    max_matches: None,
                    max_snippet_bytes: None,
                }),
            )
            .await
            .expect_err("empty query rejected");
        assert_eq!(err.code, IpcErrorCode::OversizedRequest);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_read_window_rejects_binary() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let tmp = data.join("bin.dat");
        // Invalid UTF-8 bytes.
        std::fs::write(&tmp, [0xff, 0xfe, 0xfa, 0x00, 0x01, 0x02]).unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: tmp.clone(),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect_err("binary content rejected");
        assert_eq!(err.code, IpcErrorCode::FileBinary);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_watch_start_then_append_emits_events_when_rule_active() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        let tmp = data.join("watch.log");
        write_text(&tmp, "preexisting\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        // Start watch with inline rule so we don't need to activate.
        let resp = client
            .call(
                1,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: tmp.clone(),
                    bucket_config: None,
                    rules: vec![kw_rule("kw-watch", "needle", "needle_match")],
                    follow_from_beginning: Some(false),
                    tag: None,
                }),
            )
            .await
            .expect("watch start");
        let ws = match resp {
            IpcResponse::FileWatchStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Settle before appending. NOTE (review): polling FileWatchList is NOT
        // a valid readiness signal here — WatchRuntime::start inserts the live
        // entry right after FileProbe::spawn returns, but the spawned probe task
        // opens the file and seeks to EOF (follow_from_beginning:false) only
        // later. If we appended before that seek, the EOF position would already
        // include our line and it would be skipped as "already at EOF" — a lost
        // event, not a late one (no deadline can recover it). There is no
        // daemon-side "probe positioned" signal, so a short fixed settle is the
        // honest mechanism (the sibling mcp/tests/file_tools_live_e2e.rs does the
        // same). The post-append wait below IS a real poll. seq starts at 2.
        let mut seq = 2u64;
        tokio::time::sleep(Duration::from_millis(250)).await;

        // Append matching content.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&tmp).unwrap();
            writeln!(f, "needle appears here").unwrap();
        }

        // M2: poll bucket_events_since until the needle_match lands (or deadline),
        // instead of a fixed 800ms sleep that races watcher pickup under load.
        // Generous deadline: the early-exit fires the instant the condition is
        // met, so this only matters under pathological parallel-test contention
        // where the file-watch notify -> sift -> bucket pipeline is slow. 30s
        // absorbs heavy CI load without reintroducing a wall-clock race.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let r = loop {
            let resp = client
                .call(
                    seq,
                    IpcRequest::BucketEventsSince(terminal_commanderd::BucketEventsSinceParams {
                        bucket_id: ws.bucket_id,
                        cursor: 0,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                    }),
                )
                .await
                .expect("events");
            seq += 1;
            let r = match resp {
                IpcResponse::BucketEventsSince(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            if r.events.iter().any(|e| e.kind == "needle_match")
                || std::time::Instant::now() >= deadline
            {
                break r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        assert!(
            r.events.iter().any(|e| e.kind == "needle_match"),
            "expected needle_match in bucket; got {:?}",
            r.events
        );

        // Stop the watch.
        let resp = client
            .call(
                seq,
                IpcRequest::FileWatchStop(FileWatchStopParams {
                    watch_id: ws.watch_id,
                }),
            )
            .await
            .expect("stop");
        let stopped = match resp {
            IpcResponse::FileWatchStop(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(stopped.watch_id, ws.watch_id);
        assert!(stopped.events_emitted >= 1);

        // Audit row landed for the start.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter().any(|r| r.action == "file_watch_start"),
            "expected file_watch_start audit row"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_watch_stop_unknown_id_returns_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let err = client
            .call(
                1,
                IpcRequest::FileWatchStop(FileWatchStopParams {
                    watch_id: terminal_commander_core::JobId::new(),
                }),
            )
            .await
            .expect_err("unknown watch must be rejected");
        assert_eq!(err.code, IpcErrorCode::UnknownWatch);
        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_watch_denies_default_deny_path() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let err = client
            .call(
                1,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: PathBuf::from("/etc/shadow"),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: None,
                    tag: None,
                }),
            )
            .await
            .expect_err("sensitive path watch must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Build a daemon whose `[policy.probes] deny_kinds` is set to `deny`, leaving
/// every other knob at defaults (developer_local, no path allow-lists). Used to
/// prove the TC22 A2 probe-kind gate blocks the REAL `file_watch_start` op, not
/// just `evaluate()` in isolation.
fn build_server_with_probe_deny(
    deny: &[&str],
) -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("probe-deny");
    let mut cfg = DaemonConfig::defaults_in(&data);
    cfg.policy.probes = Some(terminal_commanderd::PolicyProbesSection {
        allow_kinds: vec![],
        deny_kinds: deny.iter().map(|s| (*s).to_owned()).collect(),
    });
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

#[test]
fn file_watch_start_denied_when_probe_kind_denied() {
    // TC22 A2: with deny_kinds = ["file_watch"], a file_watch_start on an
    // otherwise-allowed path is DENIED with the POLICY.md `probe_kind_denied`
    // substring, and NO probe is created (the live-watch list stays empty).
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server_with_probe_deny(&["file_watch"]);
        let tmp = data.join("denied.log");
        write_text(&tmp, "preexisting\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let err = client
            .call(
                1,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: tmp.clone(),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: Some(false),
                    tag: None,
                }),
            )
            .await
            .expect_err("file_watch probe kind is denied");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("probe_kind_denied"),
            "deny must carry the POLICY.md substring; got: {}",
            err.message
        );

        // No probe was created: the live-watch snapshot is empty.
        let resp = client
            .call(2, IpcRequest::FileWatchList)
            .await
            .expect("watch list");
        let list = match resp {
            IpcResponse::FileWatchList(l) => l,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            list.entries.is_empty(),
            "a denied file_watch_start must create NO probe; got {:?}",
            list.entries
        );

        // The deny was audited under the file_watch_start action.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "file_watch_start" && r.decision == "deny"),
            "expected a file_watch_start deny audit row"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_watch_start_allowed_when_probe_kind_not_denied() {
    // TC22 A2 control: with an EMPTY deny_kinds the SAME file_watch_start on the
    // SAME path SUCCEEDS and creates a live probe. Proves the gate is the cause
    // of the deny above, not an unrelated failure.
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server_with_probe_deny(&[]);
        let tmp = data.join("allowed.log");
        write_text(&tmp, "preexisting\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        let resp = client
            .call(
                1,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: tmp.clone(),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: Some(false),
                    tag: None,
                }),
            )
            .await
            .expect("watch start should succeed when probe kind is not denied");
        let ws = match resp {
            IpcResponse::FileWatchStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // The probe is live.
        let resp = client
            .call(2, IpcRequest::FileWatchList)
            .await
            .expect("watch list");
        let list = match resp {
            IpcResponse::FileWatchList(l) => l,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            list.entries.iter().any(|w| w.watch_id == ws.watch_id),
            "the allowed watch must be live; got {:?}",
            list.entries
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn file_watch_list_reflects_live_then_stopped_state() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let tmp = data.join("listed.log");
        write_text(&tmp, "preexisting\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(3));

        // Start a watch (no inline rules needed — we only assert listing).
        let resp = client
            .call(
                1,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: tmp.clone(),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: Some(false),
                    tag: None,
                }),
            )
            .await
            .expect("watch start");
        let ws = match resp {
            IpcResponse::FileWatchStart(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // The live watch must appear in the snapshot.
        let resp = client
            .call(2, IpcRequest::FileWatchList)
            .await
            .expect("watch list");
        let listed = match resp {
            IpcResponse::FileWatchList(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            listed.entries.iter().any(|e| e.watch_id == ws.watch_id),
            "live watch {} must appear in file_watch_list; got {:?}",
            ws.watch_id,
            listed.entries
        );

        // Stop it.
        let resp = client
            .call(
                3,
                IpcRequest::FileWatchStop(FileWatchStopParams {
                    watch_id: ws.watch_id,
                }),
            )
            .await
            .expect("watch stop");
        match resp {
            IpcResponse::FileWatchStop(_) => {}
            other => panic!("unexpected: {other:?}"),
        }

        // After stop the watch must no longer be listed as live.
        let resp = client
            .call(4, IpcRequest::FileWatchList)
            .await
            .expect("watch list after stop");
        let listed = match resp {
            IpcResponse::FileWatchList(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            !listed.entries.iter().any(|e| e.watch_id == ws.watch_id),
            "stopped watch {} must not appear in file_watch_list; got {:?}",
            ws.watch_id,
            listed.entries
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Build a default-deny target on disk (a file whose canonical path ends
/// with a sensitive suffix) and an innocently-named symlink pointing at
/// it. Returns `(secret_target, symlink)`.
#[cfg(unix)]
fn make_symlink_to_secret(data: &std::path::Path) -> (PathBuf, PathBuf) {
    // `.ssh/id_rsa` is in DEFAULT_DENY_PATH_SUFFIXES.
    let ssh_dir = data.join(".ssh");
    std::fs::create_dir_all(&ssh_dir).unwrap();
    let secret = ssh_dir.join("id_rsa");
    write_text(&secret, "BEGIN OPENSSH PRIVATE KEY\n");
    let link = data.join("innocent.txt");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    (secret, link)
}

/// BUG 1 (security): a symlink whose own name is innocuous must NOT
/// bypass the default-deny suffix check by resolving to a sensitive
/// target. `file_read_window` canonicalizes BEFORE the policy check, so
/// the symlink is denied on the real target, not allowed on the raw name.
#[cfg(unix)]
#[test]
fn file_read_window_denies_symlink_to_default_deny_target() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let (_secret, link) = make_symlink_to_secret(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: link.clone(),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect_err("symlink to a default-deny target must be denied");
        assert_eq!(
            err.code,
            IpcErrorCode::PathDenied,
            "symlink bypass: expected PathDenied, got {err:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// BUG 1 (security): same symlink-bypass guarantee for `file_search`.
#[cfg(unix)]
#[test]
fn file_search_denies_symlink_to_default_deny_target() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let (_secret, link) = make_symlink_to_secret(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileSearch(FileSearchParams {
                    path: link.clone(),
                    query: "PRIVATE".to_owned(),
                    case_insensitive: None,
                    max_matches: None,
                    max_snippet_bytes: None,
                }),
            )
            .await
            .expect_err("symlink to a default-deny target must be denied");
        assert_eq!(
            err.code,
            IpcErrorCode::PathDenied,
            "symlink bypass: expected PathDenied, got {err:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// BUG 1 (security): same symlink-bypass guarantee for `file_watch_start`.
#[cfg(unix)]
#[test]
fn file_watch_denies_symlink_to_default_deny_target() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let (_secret, link) = make_symlink_to_secret(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileWatchStart(FileWatchStartParams {
                    path: link.clone(),
                    bucket_config: None,
                    rules: vec![],
                    follow_from_beginning: None,
                    tag: None,
                }),
            )
            .await
            .expect_err("symlink to a default-deny target must be denied");
        assert_eq!(
            err.code,
            IpcErrorCode::PathDenied,
            "symlink bypass: expected PathDenied, got {err:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// BUG 2 (trust/correctness): a relative path is rejected with a teaching
/// `PathDenied`, not silently resolved against the daemon's CWD. (The
/// cross-platform variant lives in `common.rs` lib unit tests; this is
/// the end-to-end dispatch-path assertion.)
#[test]
fn file_read_window_rejects_relative_path() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: PathBuf::from("Cargo.toml"),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect_err("relative path must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("must be absolute"),
            "teaching message expected, got: {}",
            err.message
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

// =====================================================================
// TC22 A3: file_write through-daemon integration tests (constitution VI:
// >=1 integration test per new tool). Covers the happy path (file exists
// with EXACT content), the deny path (policy error AND no file created),
// the oversize bound, and the audit-before-write proof.
// =====================================================================

/// AC: file_write to an allowed path -> file exists with the EXACT content,
/// the response reports the canonical path + byte count, and an
/// audit-before-write `file_write` row with decision `allow` landed.
#[test]
fn file_write_to_allowed_path_writes_exact_content_and_audits() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        let target = data.join("subdir/out.txt");
        // Parent does not exist yet -> exercise create_dirs within an allowed
        // path (developer_local zero-config allows the temp tree).
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let content = "alpha\nbeta\ngamma\n";
        let resp = client
            .call(
                1,
                IpcRequest::FileWrite(FileWriteParams {
                    path: target.clone(),
                    content: content.to_owned(),
                    create_dirs: true,
                }),
            )
            .await
            .expect("write");
        let r: FileWriteResponse = match resp {
            IpcResponse::FileWrite(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.bytes_written, content.len() as u64);

        // The file exists on disk with the EXACT bytes (not via the daemon).
        let on_disk = std::fs::read_to_string(&target).expect("target file must exist");
        assert_eq!(on_disk, content, "written content must be exact");

        // Audit-before-write proof: a `file_write` row with decision `allow`
        // exists. (The handler emits this BEFORE the bytes land.)
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.action == "file_write" && row.decision == "allow"),
            "expected a file_write allow audit row; got {:?}",
            rows.iter()
                .map(|r| (r.action.clone(), r.decision.clone()))
                .collect::<Vec<_>>()
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC: file_write to a DENIED path (default-deny sensitive suffix) returns a
/// PathDenied policy error AND creates no file.
#[test]
fn file_write_to_denied_path_errors_and_creates_no_file() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        // A path whose canonical form ends with a default-deny suffix. We put
        // it under the (existing) data dir so the PARENT canonicalizes, and the
        // file name `.ssh/id_rsa` tail is matched by the suffix deny. Build the
        // parent dir so canonicalize-parent succeeds and the DENY is purely the
        // policy verdict, not a missing-parent error.
        let ssh_dir = data.join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        let secret = ssh_dir.join("id_rsa");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileWrite(FileWriteParams {
                    path: secret.clone(),
                    content: "BEGIN OPENSSH PRIVATE KEY\n".to_owned(),
                    create_dirs: false,
                }),
            )
            .await
            .expect_err("default-deny write target must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);

        // No file created: the deny precedes any write.
        assert!(
            !secret.exists(),
            "denied write must not create the target file"
        );

        // A `file_write` deny audit row was recorded.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.action == "file_write" && row.decision == "deny"),
            "expected a file_write deny audit row"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC: oversize content (> MAX_FILE_WRITE_BYTES) is rejected with a bounded
/// OversizedRequest error before any filesystem touch.
#[test]
fn file_write_oversize_content_returns_bounded_error() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let target = data.join("big.txt");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let oversize = "x".repeat(MAX_FILE_WRITE_BYTES + 1);
        let err = client
            .call(
                1,
                IpcRequest::FileWrite(FileWriteParams {
                    path: target.clone(),
                    content: oversize,
                    create_dirs: false,
                }),
            )
            .await
            .expect_err("oversize content must be rejected");
        assert_eq!(err.code, IpcErrorCode::OversizedRequest);
        assert!(
            !target.exists(),
            "oversize-rejected write must not create the target file"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC: a relative write path is rejected with a teaching PathDenied (the
/// daemon has no workspace root), and no file is created.
#[test]
fn file_write_rejects_relative_path() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileWrite(FileWriteParams {
                    path: PathBuf::from("out.txt"),
                    content: "x".to_owned(),
                    create_dirs: false,
                }),
            )
            .await
            .expect_err("relative write path must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("must be absolute"),
            "teaching message expected, got: {}",
            err.message
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC (write_allow enforcement, A3): with a configured write_allow, a write
/// OUTSIDE the allow-list is denied (no_allow_rule) and creates no file, while
/// a write INSIDE is allowed with exact content. Proves the new tool finally
/// makes write_allow enforceable end-to-end.
#[test]
fn file_write_enforces_write_allow_list() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("write-allow");
        // Build a server whose policy.paths.write_allow confines writes to an
        // `allowed/**` subtree of the data dir. Canonicalize the data dir so
        // the glob matches the same canonical form the engine evaluates.
        let mut cfg = DaemonConfig::defaults_in(&data);
        let allowed_dir = data.join("allowed");
        std::fs::create_dir_all(&allowed_dir).unwrap();
        let canon_data = std::fs::canonicalize(&data).unwrap();
        let glob = format!("{}/allowed/**", canon_data.display());
        cfg.policy.paths = Some(terminal_commanderd::PolicyPathsSection {
            write_allow: vec![glob],
            ..Default::default()
        });
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Inside the allow-list -> allowed, exact content.
        let inside = allowed_dir.join("ok.txt");
        let resp = client
            .call(
                1,
                IpcRequest::FileWrite(FileWriteParams {
                    path: inside.clone(),
                    content: "ok\n".to_owned(),
                    create_dirs: false,
                }),
            )
            .await
            .expect("in-allow write");
        match resp {
            IpcResponse::FileWrite(_) => {}
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "ok\n");

        // Outside the allow-list -> denied (no_allow_rule), no file.
        let outside = data.join("denied.txt");
        let err = client
            .call(
                2,
                IpcRequest::FileWrite(FileWriteParams {
                    path: outside.clone(),
                    content: "nope\n".to_owned(),
                    create_dirs: false,
                }),
            )
            .await
            .expect_err("off-write-allow write must be denied");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("no_allow_rule"),
            "off-allow-list write must carry no_allow_rule: {}",
            err.message
        );
        assert!(!outside.exists(), "denied write must not create a file");

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// FIX 1 (Medium finding): a write target containing `..` is rejected BEFORE
/// any directory or file is created, on BOTH `create_dirs: true` and `false`.
///
/// Threat closed: previously the step-1 gate saw the RAW parent (carrying the
/// literal `..`), and `create_dir_all` would honor the `..` and build a
/// directory OUTSIDE the allow-list (e.g. `data/escaped`) before the final
/// canonical gate denied the content -- a create-then-deny asymmetry that left
/// an out-of-allow-list directory artifact on disk. The fix rejects `..` up
/// front, so no escaped artifact is ever created.
///
/// Evidence: the call returns `PathDenied` with a `..` teaching reason AND the
/// would-be-escaped directory `data/escaped` does NOT exist afterward.
#[test]
fn file_write_rejects_dotdot_target_before_creating_artifact() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Target whose `..` would climb out of `data/inner` into a SIBLING
        // `data/escaped` directory. If `create_dir_all` ever honored the `..`,
        // it would materialize `data/escaped` outside the intended tree.
        let escaped_dir = data.join("escaped");
        let dotdot_target = data
            .join("inner")
            .join("..")
            .join("escaped")
            .join("out.txt");
        assert!(
            !escaped_dir.exists(),
            "precondition: escaped sibling dir must not pre-exist"
        );

        for create_dirs in [true, false] {
            let err = client
                .call(
                    1,
                    IpcRequest::FileWrite(FileWriteParams {
                        path: dotdot_target.clone(),
                        content: "escape\n".to_owned(),
                        create_dirs,
                    }),
                )
                .await
                .expect_err("`..` write target must be rejected");
            assert_eq!(
                err.code,
                IpcErrorCode::PathDenied,
                "`..` rejection must be PathDenied (create_dirs={create_dirs})"
            );
            assert!(
                err.message.contains(".."),
                "teaching `..` reason expected (create_dirs={create_dirs}), got: {}",
                err.message
            );
            // The escaped sibling directory was NOT created -- the `..` reject
            // precedes any `create_dir_all` (proves create-then-deny is closed).
            assert!(
                !escaped_dir.exists(),
                "no out-of-allow-list artifact may be created (create_dirs={create_dirs})"
            );
            assert!(
                !dotdot_target.exists(),
                "the target file must not exist (create_dirs={create_dirs})"
            );
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// FIX 2 (audit-stream completeness): a non-PathDenied refusal -- here an
/// OVERSIZE write -- emits a domain `file_write` DENY audit row, not just the
/// dispatch-level `ipc_file_write` row. The `file_write` audit stream is now
/// self-complete for every refusal. Audit-before-any-write is preserved: the
/// row exists and no file was written.
#[test]
fn file_write_oversize_emits_domain_deny_audit_row() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        let target = data.join("big.txt");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let oversize = "x".repeat(MAX_FILE_WRITE_BYTES + 1);
        let err = client
            .call(
                1,
                IpcRequest::FileWrite(FileWriteParams {
                    path: target.clone(),
                    content: oversize,
                    create_dirs: false,
                }),
            )
            .await
            .expect_err("oversize content must be rejected");
        assert_eq!(err.code, IpcErrorCode::OversizedRequest);
        assert!(
            !target.exists(),
            "oversize-rejected write must not create a file"
        );

        // A domain `file_write` DENY row was recorded for the oversize refusal.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.action == "file_write" && row.decision == "deny"),
            "expected a file_write deny audit row for the oversize refusal; got {:?}",
            rows.iter()
                .map(|r| (r.action.clone(), r.decision.clone()))
                .collect::<Vec<_>>()
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

// =====================================================================
// US3 (FR-020/021): file_list_dir — bounded, policy-gated single-level
// directory listing. Same read-path gate + audit as file_read_window.
// =====================================================================

/// AC1: a readable directory lists its entries with name/kind/size/mtime,
/// sorted dirs-first then files/symlinks, each group lexicographic. Symlinks
/// are reported by kind and NEVER followed (size absent).
#[test]
fn file_list_dir_returns_sorted_bounded_entries() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, state, handle) = build_server();
        let dir = data.join("listing");
        std::fs::create_dir_all(&dir).unwrap();
        // Two subdirs + two files, created OUT of lexicographic order so the
        // deterministic sort is actually exercised.
        std::fs::create_dir(dir.join("bdir")).unwrap();
        std::fs::create_dir(dir.join("adir")).unwrap();
        write_text(&dir.join("zfile.txt"), "zz\n"); // 3 bytes
        write_text(&dir.join("afile.txt"), "a\n"); // 2 bytes
        // A symlink (to a file) sorts in the second group by name and its kind
        // is `symlink` — proving symlink_metadata is used (never followed).
        std::os::unix::fs::symlink(dir.join("afile.txt"), dir.join("mlink")).unwrap();

        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        let resp = client
            .call(
                1,
                IpcRequest::FileListDir(FileListDirParams {
                    path: dir.to_string_lossy().into_owned(),
                    max_entries: None,
                }),
            )
            .await
            .expect("list");
        let r = match resp {
            IpcResponse::FileListDir(r) => r,
            other => panic!("unexpected: {other:?}"),
        };

        // Dirs first (adir, bdir), then files/symlinks lexicographic
        // (afile.txt, mlink, zfile.txt).
        let names: Vec<&str> = r.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["adir", "bdir", "afile.txt", "mlink", "zfile.txt"]
        );
        assert_eq!(r.entries[0].kind, DirEntryKind::Dir);
        assert_eq!(r.entries[1].kind, DirEntryKind::Dir);
        assert_eq!(r.entries[2].kind, DirEntryKind::File);
        assert_eq!(r.entries[3].kind, DirEntryKind::Symlink);
        assert_eq!(r.entries[4].kind, DirEntryKind::File);
        // Files carry size; dirs and symlinks omit it (symlink never followed).
        assert_eq!(r.entries[2].size_bytes, Some(2));
        assert_eq!(r.entries[4].size_bytes, Some(3));
        assert!(r.entries[0].size_bytes.is_none(), "dir omits size");
        assert!(
            r.entries[3].size_bytes.is_none(),
            "symlink size omitted (never followed)"
        );
        assert!(r.entries[2].mtime_ms.is_some(), "file carries mtime");
        assert_eq!(r.total_entries, 5);
        assert!(!r.truncated);

        // FR-021: the listing is audited at dispatch level like other file ops.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter().any(|row| row.action == "ipc_file_list_dir"),
            "expected an ipc_file_list_dir audit row; got {:?}",
            rows.iter().map(|r| r.action.clone()).collect::<Vec<_>>()
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC2: a directory with more entries than the requested cap is
/// truncation-flagged with the TRUE total, never silently partial. The
/// returned entries are the deterministic head of the sorted list.
#[test]
fn file_list_dir_truncates_with_total_count_over_cap() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let dir = data.join("many");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"] {
            write_text(&dir.join(name), "x\n");
        }
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        // Cap of 2 over 5 entries -> truncated, total_entries = 5.
        let resp = client
            .call(
                1,
                IpcRequest::FileListDir(FileListDirParams {
                    path: dir.to_string_lossy().into_owned(),
                    max_entries: Some(2),
                }),
            )
            .await
            .expect("list capped");
        let r = match resp {
            IpcResponse::FileListDir(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(r.entries.len(), 2);
        assert_eq!(r.total_entries, 5);
        assert!(r.truncated, "over-cap listing must be truncation-flagged");
        // Deterministic head of the sorted files: a.txt, b.txt.
        assert_eq!(r.entries[0].name, "a.txt");
        assert_eq!(r.entries[1].name, "b.txt");

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC3 (FR-021): a policy-denied path returns the SAME denial shape (code AND
/// message) that `file_read_window` returns for that path — because both route
/// through the identical `resolve_and_authorize_file` read gate.
#[test]
fn file_list_dir_denies_policy_path_same_shape_as_read() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));
        // A default-deny path (same one the file_read deny test uses).
        let denied = std::path::PathBuf::from("/etc/shadow");

        let read_err = client
            .call(
                1,
                IpcRequest::FileReadWindow(FileReadWindowParams {
                    path: denied.clone(),
                    start_line: None,
                    max_lines: None,
                    max_bytes: None,
                }),
            )
            .await
            .expect_err("read of a default-deny path must be rejected");

        let list_err = client
            .call(
                2,
                IpcRequest::FileListDir(FileListDirParams {
                    path: denied.to_string_lossy().into_owned(),
                    max_entries: None,
                }),
            )
            .await
            .expect_err("list of a default-deny path must be rejected");

        assert_eq!(list_err.code, IpcErrorCode::PathDenied);
        assert_eq!(
            list_err.code, read_err.code,
            "list denial code must match file_read"
        );
        assert_eq!(
            list_err.message, read_err.message,
            "list denial message must be byte-identical to file_read (same policy gate)"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC4: listing a path that is a FILE (not a directory) errors with a teaching
/// message naming the files `read` action as the remedy.
#[test]
fn file_list_dir_on_file_teaches_read_action() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let file = data.join("afile.txt");
        write_text(&file, "hello\n");
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileListDir(FileListDirParams {
                    path: file.to_string_lossy().into_owned(),
                    max_entries: None,
                }),
            )
            .await
            .expect_err("listing a regular file must be rejected with a teaching error");
        assert_eq!(err.code, IpcErrorCode::FileNotFound);
        assert!(
            err.message.contains("not a directory"),
            "must explain the target is not a directory; got: {}",
            err.message
        );
        assert!(
            err.message.contains("read"),
            "must name the `read` action as the remedy; got: {}",
            err.message
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// AC (absolute-only, mirrors file_read): a relative path is rejected with the
/// existing teaching `PathDenied` (the daemon has no workspace root).
#[test]
fn file_list_dir_rejects_relative_path() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        let err = client
            .call(
                1,
                IpcRequest::FileListDir(FileListDirParams {
                    path: "some/dir".to_owned(),
                    max_entries: None,
                }),
            )
            .await
            .expect_err("relative path must be rejected");
        assert_eq!(err.code, IpcErrorCode::PathDenied);
        assert!(
            err.message.contains("must be absolute"),
            "teaching message expected, got: {}",
            err.message
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
