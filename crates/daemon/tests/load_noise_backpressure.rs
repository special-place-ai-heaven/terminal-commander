// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC47 load / noise / backpressure gate.
//!
//! Stress / quality gate, NOT feature-building. Every test in this
//! file generates noisy input at runtime (no committed fixtures),
//! drives the existing runtimes through their public IPC surface,
//! and asserts the LLM-visible contract holds under load:
//!
//! - Large noisy stdout never surfaces raw in the BUCKET (only
//!   structured EventDrafts produced by sifter rules reach the
//!   bucket; lifecycle events carry argv metadata, not stdout body).
//!   The one sanctioned exception is the TCE-ERG-1 no-silence receipt,
//!   which rides on `command_status` (NOT the bucket) and is populated
//!   only when ZERO rules matched. The bucket itself never carries raw
//!   stdout, with or without the receipt.
//! - Matching signal lines still emit signal events.
//! - `bucket_events_since` and `bucket_wait` payloads stay under
//!   `MAX_BUCKET_READ_LIMIT` (10_000) events per call.
//! - `event_context` windows stay under `MAX_CONTEXT_FRAMES` /
//!   `MAX_CONTEXT_BYTES`.
//! - `bucket_wait` does not busy-poll: heartbeat returns AFTER the
//!   timeout window when no events arrive.
//! - Multiple concurrent probes do not cross-talk: bucket A only
//!   contains events from job A.
//! - `runtime_state` / `probe_list` stay bounded under live load.
//! - Registry activation during an active stream (TC42b rebind path)
//!   stays consistent under noise.
//! - `BucketSummary.dropped_count` surfaces when the bucket retention
//!   cap evicts events.
//!
//! No `frames_suppressed` counter exists in the daemon today. The
//! noise-reduction ratio is derived from `frames_total` /
//! `events_emitted` where the test owns both input volume and the
//! matching rule. The missing explicit `frames_suppressed` is filed
//! as a backlog item in the TC47 final report.

#![cfg(unix)]
#![allow(
    clippy::too_many_lines,
    clippy::unreadable_literal,
    clippy::needless_continue,
    clippy::literal_string_with_formatting_args,
    clippy::uninlined_format_args
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{
    BucketConfig, ContextHint, ProbeId, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commanderd::{
    BucketEventsSinceParams, BucketWaitParams, CommandStartParams, DaemonClient, DaemonConfig,
    DaemonState, EventContextParams, IpcRequest, IpcResponse, IpcServer, MAX_BUCKET_READ_LIMIT,
    MAX_CONTEXT_BYTES, MAX_CONTEXT_FRAMES, MAX_RESPONSE_BYTES, ProbeStatusParams,
    RegistryActivateParams, RegistryUpsertParams,
};

fn python3_available() -> bool {
    for c in ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"] {
        if std::path::Path::new(c).exists() {
            return true;
        }
    }
    false
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc47-load-{tag}-{pid}-{nanos}-{n}"));
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

fn build_server() -> (PathBuf, Arc<DaemonState>, terminal_commanderd::ServerHandle) {
    let data = tmp_data_dir("server");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (data, state, handle)
}

fn needle_rule() -> RuleDefinition {
    RuleDefinition {
        id: "tc47-needle".to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "needle_match".to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec!["TC47-NEEDLE".to_owned()]),
        captures: vec![],
        summary_template: "needle observed".to_owned(),
        tags: vec!["tc47".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// Regex needle whose `line` capture varies per emit so TC11 dedupe
/// does not collapse volume under a tiny bucket cap (Variant B).
fn needle_rule_distinct_dedupe() -> RuleDefinition {
    RuleDefinition {
        id: "tc47-needle".to_owned(),
        version: 1,
        kind: RuleType::Regex,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "needle_match".to_owned(),
        stream: None,
        description: None,
        pattern: Some(r"TC47-NEEDLE line (?P<line>\d+)".to_owned()),
        keywords: None,
        captures: vec!["line".to_owned()],
        summary_template: "needle at line ${line}".to_owned(),
        tags: vec!["tc47".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// Megabyte-scale noisy emitter. Prints `total_lines` lines of
/// noise, sprinkling exactly `needle_count` matching lines evenly.
/// Uses `python3 -u -c` so the output is line-buffered.
fn noisy_argv(total_lines: u32, needle_count: u32) -> Vec<String> {
    let py = format!(
        r#"
import sys
TOTAL = {total_lines}
NEEDLES = {needle_count}
step = max(1, TOTAL // max(1, NEEDLES))
for i in range(TOTAL):
    if i % step == 0 and (i // step) < NEEDLES:
        print(f"TC47-NEEDLE line {{i}}", flush=False)
    else:
        # ~100 ASCII bytes per noise line.
        print(f"noise-{{i:08d}} " + "x" * 80, flush=False)
sys.stdout.flush()
"#
    );
    vec!["python3".to_owned(), "-u".to_owned(), "-c".to_owned(), py]
}

#[test]
#[allow(clippy::too_many_lines)]
fn megabyte_scale_noisy_stdout_emits_signal_without_raw_leak() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(15));

        // Upsert + activate the needle rule globally so the spawn
        // sifter picks it up.
        let _ = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: needle_rule(),
                }),
            )
            .await
            .expect("upsert");
        let _ = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "tc47-needle".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");

        // 10_000 lines * ~100 bytes/line = ~1 MB stdout. 7 needles.
        let total_lines = 10_000u32;
        let needle_count = 7u32;
        let start_resp = client
            .call(
                3,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(total_lines, needle_count),
                    cwd: None,
                    env: vec![],
                    bucket_config: Some(BucketConfig {
                        max_events: 5_000,
                        ttl: Duration::from_mins(1),
                    }),
                    rules: vec![],
                    grace_ms: Some(30_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start");
        let started = match start_resp {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Drain the bucket until the lifecycle event arrives or 10s
        // elapse. Each `bucket_wait` call is bounded; total events
        // observed in any single response must stay within
        // `MAX_BUCKET_READ_LIMIT`.
        let mut cursor: u64 = 0;
        let mut needles_seen: u32 = 0;
        let mut command_exited = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !command_exited && Instant::now() < deadline {
            let resp = client
                .call(
                    4,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(1_000),
                    }),
                )
                .await
                .expect("bucket_wait");
            let r = match resp {
                IpcResponse::BucketWait(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            assert!(
                r.events.len() <= MAX_BUCKET_READ_LIMIT,
                "bucket_wait returned {} events > MAX_BUCKET_READ_LIMIT={}",
                r.events.len(),
                MAX_BUCKET_READ_LIMIT
            );
            for e in &r.events {
                match e.kind.as_str() {
                    "needle_match" => needles_seen += 1,
                    "command_exited" | "command_failed" => command_exited = true,
                    _ => {}
                }
            }
            cursor = r.next_cursor;
            if r.heartbeat {
                continue;
            }
        }

        assert!(
            command_exited,
            "command did not exit within deadline (needles_seen={needles_seen})"
        );
        assert!(
            needles_seen >= 1,
            "expected at least one needle_match signal under 1MB noise; got {needles_seen}"
        );

        // No raw stream leak: walk every event JSON and confirm no
        // `noise-` line text appears outside argv/summary metadata.
        let resp = client
            .call(
                5,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: started.bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: Some(MAX_BUCKET_READ_LIMIT),
                }),
            )
            .await
            .expect("bucket_events_since");
        let dump = match resp {
            IpcResponse::BucketEventsSince(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        let json = serde_json::to_string(&dump.events).unwrap();
        assert!(
            json.len() <= MAX_RESPONSE_BYTES,
            "bucket_events_since payload {} > MAX_RESPONSE_BYTES {}",
            json.len(),
            MAX_RESPONSE_BYTES
        );
        // The noise payload is `noise-XXXXXXXX x x x ...`; that
        // string only appears in raw stdout. Lifecycle events carry
        // the argv (which contains the python script source — the
        // noise template itself), so we look for a noise line that
        // would only exist if the actual emitted noise text leaked.
        // Use a high-line-number marker that exists in stdout but
        // never in the argv source code.
        assert!(
            !json.contains("noise-00009999"),
            "raw noise line leaked into bucket payload"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_wait_heartbeat_respects_timeout_without_busy_poll() {
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let bid = terminal_commander_core::BucketId::new();
        // Create an empty bucket and ensure bucket_wait blocks until
        // the timeout (heartbeat=true) rather than spinning.
        let _ = client
            .call(
                1,
                IpcRequest::BucketSummary(terminal_commanderd::BucketSummaryParams {
                    bucket_id: bid,
                }),
            )
            .await; // ignore — bucket likely missing, that's expected here.

        // Use a real command bucket so the wait path is identical.
        let start = client
            .call(
                2,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: vec!["sleep".to_owned(), "5".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(10_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start");
        let started = match start {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        let begin = Instant::now();
        let resp = client
            .call(
                3,
                IpcRequest::BucketWait(BucketWaitParams {
                    bucket_id: started.bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                    timeout_ms: Some(800),
                }),
            )
            .await
            .expect("bucket_wait");
        let elapsed = begin.elapsed();
        let r = match resp {
            IpcResponse::BucketWait(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        // Heartbeat or events allowed. The critical assertion is
        // that the call took close to the timeout, NOT 0 ms (which
        // would indicate busy-polling).
        assert!(
            elapsed >= Duration::from_millis(700),
            "bucket_wait returned in {elapsed:?}; expected ~800ms (no busy-poll)"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "bucket_wait took {elapsed:?}; cap should keep it well under 3s"
        );
        let _ = r;

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_events_since_limit_clamps_to_max() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Generate 200 needles by activating the rule + emitting
        // 200 matching lines.
        let _ = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: needle_rule(),
                }),
            )
            .await
            .expect("upsert");
        let _ = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "tc47-needle".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");

        let start = client
            .call(
                3,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(200, 200),
                    cwd: None,
                    env: vec![],
                    bucket_config: Some(BucketConfig {
                        max_events: 1_000,
                        ttl: Duration::from_mins(1),
                    }),
                    rules: vec![],
                    grace_ms: Some(15_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start");
        let started = match start {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Wait for the command to exit.
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut done = false;
        let mut cursor = 0u64;
        while !done && Instant::now() < deadline {
            let r = client
                .call(
                    4,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(800),
                    }),
                )
                .await
                .expect("bucket_wait");
            if let IpcResponse::BucketWait(w) = r {
                cursor = w.next_cursor;
                if w.events
                    .iter()
                    .any(|e| e.kind == "command_exited" || e.kind == "command_failed")
                {
                    done = true;
                }
            }
        }
        assert!(done, "command did not exit before limit-clamp test");

        // Ask for way more than MAX_BUCKET_READ_LIMIT; dispatcher
        // must clamp.
        let resp = client
            .call(
                5,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: started.bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: Some(MAX_BUCKET_READ_LIMIT * 10),
                }),
            )
            .await
            .expect("bucket_events_since oversized limit");
        let r = match resp {
            IpcResponse::BucketEventsSince(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            r.events.len() <= MAX_BUCKET_READ_LIMIT,
            "events.len()={} > MAX_BUCKET_READ_LIMIT={}",
            r.events.len(),
            MAX_BUCKET_READ_LIMIT
        );
        // Envelope budget too.
        let payload = serde_json::to_string(&r.events).unwrap();
        assert!(
            payload.len() <= MAX_RESPONSE_BYTES,
            "events payload {} > MAX_RESPONSE_BYTES {}",
            payload.len(),
            MAX_RESPONSE_BYTES
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn concurrent_probes_buckets_do_not_cross_talk() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));
        // Upsert + activate the needle rule globally.
        let _ = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: needle_rule(),
                }),
            )
            .await
            .expect("upsert");
        let _ = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "tc47-needle".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");

        // Job A emits 10 needles. Job B emits 0 needles.
        let a = client
            .call(
                3,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(500, 10),
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(10_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start a");
        let started_a = match a {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        let b = client
            .call(
                4,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(500, 0),
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(10_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start b");
        let started_b = match b {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Wait both to exit.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut a_done = false;
        let mut b_done = false;
        let mut ca = 0u64;
        let mut cb = 0u64;
        let mut events_a: Vec<terminal_commander_core::SignalEvent> = Vec::new();
        let mut events_b: Vec<terminal_commander_core::SignalEvent> = Vec::new();
        while (!a_done || !b_done) && Instant::now() < deadline {
            if !a_done {
                let r = client
                    .call(
                        5,
                        IpcRequest::BucketWait(BucketWaitParams {
                            bucket_id: started_a.bucket_id,
                            cursor: ca,
                            severity_min: None,
                            kind_filter: None,
                            limit: None,
                            timeout_ms: Some(500),
                        }),
                    )
                    .await
                    .expect("wait a");
                if let IpcResponse::BucketWait(w) = r {
                    ca = w.next_cursor;
                    if w.events
                        .iter()
                        .any(|e| matches!(e.kind.as_str(), "command_exited" | "command_failed"))
                    {
                        a_done = true;
                    }
                    events_a.extend(w.events);
                }
            }
            if !b_done {
                let r = client
                    .call(
                        6,
                        IpcRequest::BucketWait(BucketWaitParams {
                            bucket_id: started_b.bucket_id,
                            cursor: cb,
                            severity_min: None,
                            kind_filter: None,
                            limit: None,
                            timeout_ms: Some(500),
                        }),
                    )
                    .await
                    .expect("wait b");
                if let IpcResponse::BucketWait(w) = r {
                    cb = w.next_cursor;
                    if w.events
                        .iter()
                        .any(|e| matches!(e.kind.as_str(), "command_exited" | "command_failed"))
                    {
                        b_done = true;
                    }
                    events_b.extend(w.events);
                }
            }
        }
        assert!(
            a_done && b_done,
            "both commands must exit (a={a_done}, b={b_done})"
        );

        // No cross-talk: every event in bucket A has source.probe_id
        // equal to started_a.probe_id; same for B. (Sifter-emitted
        // events carry probe_id, not job_id, by design — job_id is
        // only stamped on synthetic lifecycle events. The bucket_id
        // on each event is the authoritative routing key here.)
        for e in &events_a {
            assert_eq!(
                e.source.probe_id, started_a.probe_id,
                "bucket A leaked a non-A event: {e:?}"
            );
            assert_eq!(
                e.bucket_id, started_a.bucket_id,
                "bucket A event has wrong bucket_id: {e:?}"
            );
        }
        for e in &events_b {
            assert_eq!(
                e.source.probe_id, started_b.probe_id,
                "bucket B leaked a non-B event: {e:?}"
            );
            assert_eq!(
                e.bucket_id, started_b.bucket_id,
                "bucket B event has wrong bucket_id: {e:?}"
            );
        }

        // Bucket B has zero needles by construction.
        assert!(
            events_b.iter().all(|e| e.kind != "needle_match"),
            "bucket B should have no needle_match events"
        );
        // Bucket A has at least one needle.
        assert!(
            events_a.iter().any(|e| e.kind == "needle_match"),
            "bucket A should have at least one needle_match"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn runtime_state_stays_bounded_under_live_load() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Spawn 3 concurrent noisy jobs.
        let mut probe_ids: Vec<ProbeId> = Vec::new();
        for i in 0..3 {
            let r = client
                .call(
                    1,
                    IpcRequest::CommandStartCombed(CommandStartParams {
                        environment: None,
                        argv: noisy_argv(2_000, 5),
                        cwd: None,
                        env: vec![],
                        bucket_config: None,
                        rules: vec![],
                        grace_ms: Some(10_000),
                        tag: None,
                        // TC-2: each is a DISTINCT logical job; without a
                        // distinct nonce the same-peer nonce-less fallback
                        // would collapse all three identical starts to one.
                        dedup_nonce: Some(format!("rt-load-{i}")),
                        strip_ansi: true,
                    }),
                )
                .await
                .expect("start");
            if let IpcResponse::CommandStartCombed(s) = r {
                probe_ids.push(s.probe_id);
            }
        }

        // runtime_state must be bounded and list all 3.
        let rs = client
            .call(
                2,
                IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
            )
            .await
            .expect("runtime_state");
        let snap = match rs {
            IpcResponse::RuntimeState(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        let payload = serde_json::to_string(&snap).unwrap();
        assert!(
            payload.len() <= MAX_RESPONSE_BYTES,
            "runtime_state payload {} > MAX_RESPONSE_BYTES {}",
            payload.len(),
            MAX_RESPONSE_BYTES
        );
        assert_eq!(snap.command_jobs, 3, "probes: {:?}", snap.probes);

        // probe_status on a known probe is bounded.
        let ps = client
            .call(
                3,
                IpcRequest::ProbeStatus(ProbeStatusParams {
                    probe_id: probe_ids[0],
                }),
            )
            .await
            .expect("probe_status");
        let payload = serde_json::to_string(&ps).unwrap();
        assert!(payload.len() <= MAX_RESPONSE_BYTES);

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn event_context_window_stays_bounded() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Activate the needle rule + emit needles.
        let _ = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: needle_rule(),
                }),
            )
            .await
            .expect("upsert");
        let _ = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "tc47-needle".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");
        let start = client
            .call(
                3,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(2_000, 3),
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(10_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start");
        let started = match start {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Wait until at least one needle event is in the bucket.
        let mut needle_event_id = None;
        let mut cursor = 0u64;
        let deadline = Instant::now() + Duration::from_secs(8);
        while needle_event_id.is_none() && Instant::now() < deadline {
            let r = client
                .call(
                    4,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(800),
                    }),
                )
                .await
                .expect("bucket_wait");
            if let IpcResponse::BucketWait(w) = r {
                cursor = w.next_cursor;
                for e in &w.events {
                    if e.kind == "needle_match" {
                        needle_event_id = Some(e.event_id);
                        break;
                    }
                }
            }
        }
        let event_id = needle_event_id.expect("at least one needle should fire");

        // Request a max-cap context window. Must stay under the
        // documented caps even if we ask for more.
        let resp = client
            .call(
                5,
                IpcRequest::EventContext(EventContextParams {
                    bucket_id: started.bucket_id,
                    event_id,
                    before: Some(MAX_CONTEXT_FRAMES + 100),
                    after: Some(MAX_CONTEXT_FRAMES + 100),
                    max_bytes: Some(MAX_CONTEXT_BYTES * 4),
                }),
            )
            .await
            .expect("event_context");
        let r = match resp {
            IpcResponse::EventContext(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            u32::try_from(r.frames.len()).unwrap_or(u32::MAX) <= MAX_CONTEXT_FRAMES * 2,
            "context frames {} exceeded MAX_CONTEXT_FRAMES*2 budget",
            r.frames.len()
        );
        assert!(
            r.total_bytes <= MAX_CONTEXT_BYTES,
            "context total_bytes {} > MAX_CONTEXT_BYTES {}",
            r.total_bytes,
            MAX_CONTEXT_BYTES
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn bucket_dropped_count_visible_when_retention_evicts() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(10));

        // Activate needle rule + create a TINY bucket so retention
        // evicts. Regex `line` capture varies per emit (Variant B)
        // so TC11 dedupe does not collapse 100 needles to one event.
        let _ = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: needle_rule_distinct_dedupe(),
                }),
            )
            .await
            .expect("upsert");
        let _ = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "tc47-needle".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");
        let start = client
            .call(
                3,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(2_000, 100),
                    cwd: None,
                    env: vec![],
                    // Cap the bucket at 8 events; 100 needles + a
                    // lifecycle event will force eviction.
                    bucket_config: Some(BucketConfig {
                        max_events: 8,
                        ttl: Duration::from_mins(1),
                    }),
                    rules: vec![],
                    grace_ms: Some(15_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start");
        let started = match start {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // Wait for the command to exit.
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut done = false;
        let mut cursor = 0u64;
        while !done && Instant::now() < deadline {
            let r = client
                .call(
                    4,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(500),
                    }),
                )
                .await
                .expect("wait");
            if let IpcResponse::BucketWait(w) = r {
                cursor = w.next_cursor;
                if w.events
                    .iter()
                    .any(|e| matches!(e.kind.as_str(), "command_exited" | "command_failed"))
                {
                    done = true;
                }
            }
        }
        assert!(done, "command did not exit");

        // BucketSummary must report dropped_count > 0.
        let r = client
            .call(
                5,
                IpcRequest::BucketSummary(terminal_commanderd::BucketSummaryParams {
                    bucket_id: started.bucket_id,
                }),
            )
            .await
            .expect("summary");
        let s = match r {
            IpcResponse::BucketSummary(s) => s,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(
            s.dropped_count > 0,
            "expected dropped_count > 0 under 8-event cap; got {:?}",
            s
        );
        assert!(
            s.event_count <= 8,
            "event_count {} > max_events 8",
            s.event_count
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn registry_activate_during_active_stream_rebinds_without_raw_leak() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let (data, _state, handle) = build_server();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(15));

        // Upsert but do NOT activate yet.
        let _ = client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: needle_rule(),
                }),
            )
            .await
            .expect("upsert");

        // Start a long-running noisy command. Use ~3s emission via
        // python sleep so the activation lands mid-stream.
        let py = r#"
import sys, time
for i in range(120):
    if i % 30 == 0:
        print(f"TC47-NEEDLE mid {i}", flush=True)
    else:
        print(f"noise-{i:08d}", flush=True)
    time.sleep(0.03)
"#;
        let start = client
            .call(
                2,
                IpcRequest::CommandStartCombed(CommandStartParams {
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
                    grace_ms: Some(15_000),
                    tag: None,
                    dedup_nonce: None,
                    strip_ansi: true,
                }),
            )
            .await
            .expect("start");
        let started = match start {
            IpcResponse::CommandStartCombed(s) => s,
            other => panic!("unexpected: {other:?}"),
        };

        // M2 N/A here (verified by tracing command.rs::status + drive_to_exit):
        // there is NO mid-run progress signal to poll. The live JobBinding.metrics
        // stays ProcessProbeMetrics::default() (zeros) for the whole run — the
        // probe accumulates frame counts internally and only surfaces them as
        // final_metrics at exit — so command_status / probe_list / runtime_state
        // all report frames_total == 0 until the child exits, then the final
        // total. And the bucket is empty pre-activation (no rule active yet), so
        // there is nothing to bucket_wait on either. Confirmed empirically: a
        // frames_total>=1 poll only unblocked at frames_total == 120 (job done),
        // landing activation AFTER every needle — zero matches.
        // So "let the stream flow, then activate mid-stream" can only be a
        // wall-clock delay. The 15s drain deadline below absorbs scheduling slack.
        tokio::time::sleep(Duration::from_millis(400)).await;

        // Activate mid-stream (TC42b rebind path).
        let _ = client
            .call(
                3,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "tc47-needle".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate mid-stream");

        // Drain to completion. We expect at least one needle_match
        // produced by frames emitted AFTER the activation.
        let mut cursor = 0u64;
        let mut needles = 0u32;
        let mut done = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !done && Instant::now() < deadline {
            let r = client
                .call(
                    4,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(500),
                    }),
                )
                .await
                .expect("wait");
            if let IpcResponse::BucketWait(w) = r {
                cursor = w.next_cursor;
                for e in &w.events {
                    if e.kind == "needle_match" {
                        needles += 1;
                    }
                    if matches!(e.kind.as_str(), "command_exited" | "command_failed") {
                        done = true;
                    }
                }
            }
        }
        assert!(done, "command must finish (needles={needles})");
        assert!(
            needles >= 1,
            "mid-stream activation should fire at least one needle_match"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
