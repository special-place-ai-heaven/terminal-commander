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
    DaemonClient, DaemonConfig, DaemonState, FileReadWindowParams, FileSearchParams,
    FileWatchStartParams, FileWatchStopParams, IpcErrorCode, IpcRequest, IpcResponse, IpcServer,
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
