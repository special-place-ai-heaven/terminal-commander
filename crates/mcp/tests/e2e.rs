// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! End-to-end MVP demo scenarios (TC30). Exercise the full pipeline:
//! create a bucket, append events, search registry, wait on bucket,
//! retrieve event_context, all through the MCP ToolSurface. Policy
//! gating is exercised throughout.

use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    BucketConfig, BucketId, BucketManager, Captures, ContextHint, ContextRingManager, EventDraft,
    EventSource, FrameId, JobManager, ProbeId, RuleDefinition, RuleStatus, RuleType, Severity,
    SourceFrame, SourcePointer, SourceStream, SourceType,
};
use terminal_commander_mcp::ToolSurface;
use terminal_commander_sifters::SifterRuntime;
use terminal_commanderd::{PolicyEngine, Router};

fn surface() -> (ToolSurface, Arc<ContextRingManager>) {
    let buckets = Arc::new(BucketManager::new());
    let rings = Arc::new(ContextRingManager::new());
    let jobs = Arc::new(JobManager::new());
    let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
    let router = Arc::new(Router::new(buckets, Arc::clone(&rings), jobs, sifter));
    let s = ToolSurface::new(router, PolicyEngine::default_engine());
    (s, rings)
}

fn draft(bid: BucketId, sev: Severity, kind: &str, package: &str) -> EventDraft {
    let mut caps = Captures::new();
    caps.insert("package".to_owned(), package.to_owned());
    EventDraft {
        bucket_id: bid,
        timestamp: time::OffsetDateTime::now_utc(),
        severity: sev,
        kind: kind.to_owned(),
        summary: format!("{kind}: missing {package}"),
        rule: None,
        source: EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::Process,
            stream: SourceStream::Stderr,
            job_id: None,
        },
        captures: Some(caps),
        pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
        pointer_unavailable_reason: None,
        tags: None,
        frame_truncated_bytes: 0,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

fn rule(id: &str, tag: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Draft,
        severity: Severity::Medium,
        event_kind: "kw_match".to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec!["needle".to_owned()]),
        captures: vec![],
        summary_template: "found needle".to_owned(),
        tags: vec![tag.to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn e2e_discover_then_create_and_read_bucket() {
    let (s, _) = surface();
    let d = s.system_discover();
    assert_eq!(d.mcp_spec, "2025-11-25");
    assert!(d.tools.contains(&"bucket_wait".to_owned()));
    let bid = BucketId::new();
    s.router
        .bucket_create(bid, BucketConfig::default())
        .unwrap();
    let _ = s
        .router
        .bucket_append(
            bid,
            draft(bid, Severity::High, "missing_package", "libssl-dev"),
        )
        .unwrap();
    let resp = s.bucket_events_since(bid, 0, None, None, None).unwrap();
    assert_eq!(resp.events.len(), 1);
    assert_eq!(resp.events[0].kind, "missing_package");
    let summary = s.bucket_summary(bid).unwrap();
    assert_eq!(summary.event_count, 1);
    assert_eq!(summary.by_severity.high, 1);
}

#[test]
fn e2e_dynamic_rule_create_search_round_trip() {
    let (s, _) = surface();
    let mut store = terminal_commander_store::EventStore::in_memory().unwrap();
    let v = s
        .registry_create(&mut store, &rule("e2e.rule", "demo"))
        .unwrap();
    assert_eq!(v, 1);
    let hits = s.registry_search(&mut store, "demo", None).unwrap();
    assert!(hits.iter().any(|h| h.rule_id == "e2e.rule"));
    let got = s.registry_get(&store, "e2e.rule").unwrap().unwrap();
    assert_eq!(got.id, "e2e.rule");
    // Activation also routes through policy (AllowWithAudit under
    // developer_local).
    s.registry_activate(&mut store, "e2e.rule", 1, Some("developer_local"))
        .unwrap();
}

#[test]
fn e2e_bucket_wait_returns_event_after_async_append() {
    let runtime = rt();
    runtime.block_on(async {
        let (s, _) = surface();
        let bid = BucketId::new();
        s.router
            .bucket_create(bid, BucketConfig::default())
            .unwrap();
        let s2 = Arc::new(s);
        let s_for_wait = Arc::clone(&s2);
        let waiter = tokio::spawn(async move {
            s_for_wait
                .bucket_wait(
                    bid,
                    0,
                    Some(Severity::High),
                    None,
                    None,
                    Duration::from_secs(2),
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        s2.router
            .bucket_append(bid, draft(bid, Severity::High, "compile_error", "x"))
            .unwrap();
        let resp = waiter.await.unwrap().unwrap();
        assert!(!resp.heartbeat);
        assert_eq!(resp.events.len(), 1);
        assert_eq!(resp.events[0].severity, Severity::High);
    });
}

#[test]
fn e2e_event_context_around_emitted_event() {
    let (s, rings) = surface();
    let pid = ProbeId::new();
    rings.create_ring_default(pid).unwrap();
    // Push three frames into the ring.
    let mut fids = Vec::new();
    for i in 0..3u32 {
        let line = format!("frame {i}");
        let f = SourceFrame::new(pid, SourceStream::Stdout, line).with_line(u64::from(i));
        fids.push(f.frame_id);
        rings.append_frame(pid, f).unwrap();
    }
    let resp = s.event_context(pid, fids[1], 1, 1, None).unwrap();
    assert!(!resp.anchor_missing);
    // Anchor in the middle: should return 3 frames (before+anchor+after).
    assert_eq!(resp.frames.len(), 3);
}

#[test]
fn e2e_file_read_window_caps_payload_for_large_file() {
    let (s, _) = surface();
    let p = std::env::temp_dir().join(format!("tc-e2e-frw-{}", std::process::id()));
    std::fs::write(&p, vec![b'a'; 200_000]).unwrap();
    let resp = s.file_read_window(&p, 0, 1_000_000).unwrap();
    assert!(resp.truncated);
    assert!(resp.content_utf8_lossy.len() <= 64 * 1024);
    let _ = std::fs::remove_file(&p);
}
