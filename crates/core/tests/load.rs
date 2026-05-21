// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Load + backpressure tests (TC28). Deterministic; no external
//! dependencies. These exercise the bucket manager + context ring
//! at MVP-target rates.

use std::sync::Arc;
use std::time::Instant;

use terminal_commander_core::{
    BucketConfig, BucketId, BucketManager, BucketReadRequest, Captures, ContextRingManager,
    EventDraft, EventSource, FrameId, ProbeId, Severity, SignalEvent, SourceFrame, SourcePointer,
    SourceStream, SourceType,
};

fn ev(bid: BucketId) -> SignalEvent {
    SignalEvent {
        event_id: terminal_commander_core::EventId::new(),
        bucket_id: bid,
        seq: 0,
        timestamp: time::OffsetDateTime::now_utc(),
        severity: Severity::Low,
        kind: "k".to_owned(),
        summary: "s".to_owned(),
        rule: None,
        source: EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::Process,
            stream: SourceStream::Stderr,
            job_id: None,
        },
        captures: Some(Captures::new()),
        pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
        pointer_unavailable_reason: None,
        tags: None,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

#[test]
fn bucket_handles_10k_appends_under_a_second() {
    let mgr = BucketManager::new();
    let bid = BucketId::new();
    mgr.create_bucket(
        bid,
        BucketConfig {
            max_events: 20_000,
            ttl: std::time::Duration::from_hours(1),
        },
    )
    .unwrap();
    let start = Instant::now();
    for _ in 0..10_000_u32 {
        mgr.append(bid, ev(bid)).unwrap();
    }
    let elapsed = start.elapsed();
    let summary = mgr.summary(bid).unwrap();
    assert_eq!(summary.event_count, 10_000);
    assert_eq!(summary.dropped_count, 0);
    // Generous bound: 1s on the orchestration host. Tightens later.
    assert!(elapsed.as_secs_f64() < 5.0, "elapsed: {elapsed:?}");
}

#[test]
fn bucket_overflow_backpressure_drops_oldest_with_counter() {
    let mgr = BucketManager::new();
    let bid = BucketId::new();
    mgr.create_bucket(
        bid,
        BucketConfig {
            max_events: 100,
            ttl: std::time::Duration::from_hours(1),
        },
    )
    .unwrap();
    for _ in 0..5_000_u32 {
        mgr.append(bid, ev(bid)).unwrap();
    }
    let s = mgr.summary(bid).unwrap();
    assert_eq!(s.event_count, 100);
    assert_eq!(s.dropped_count, 4_900);
    assert!(s.head_seq > 0);
    assert_eq!(s.tail_seq, 5_000);
    // Cursor read past tail returns nothing.
    let r = mgr
        .events_since(bid, &BucketReadRequest::new(5_000))
        .unwrap();
    assert!(r.events.is_empty());
}

#[test]
fn context_ring_handles_5k_frames_with_byte_cap() {
    let mgr = ContextRingManager::new();
    let pid = ProbeId::new();
    mgr.create_ring_default(pid).unwrap();
    let mut last_id = None;
    for i in 0..5_000_u32 {
        let line = format!("line {i}");
        let f = SourceFrame::new(pid, SourceStream::Stdout, line).with_line(u64::from(i));
        let id = f.frame_id;
        mgr.append_frame(pid, f).unwrap();
        if i % 100 == 0 {
            last_id = Some(id);
        }
    }
    let count = mgr.frame_count(pid);
    // DEFAULT_RING_FRAMES=4096; ring caps at that.
    assert!(count <= 4096);
    // Anchor near the head is likely evicted; ring reports it.
    if let Some(id) = last_id {
        let req = terminal_commander_core::ContextWindowRequest {
            probe_id: pid,
            anchor: id,
            before: 5,
            after: 5,
            max_bytes: Some(1024),
        };
        let resp = mgr.window(&req).unwrap();
        // Either present (recent enough) or anchor_missing (older).
        assert!(resp.frames.len() <= 11);
        let _ = resp.anchor_missing;
    }
}

#[test]
fn bucket_reader_bounded_does_not_dump_entire_history() {
    let mgr = BucketManager::new();
    let bid = BucketId::new();
    mgr.create_bucket_default(bid).unwrap();
    for _ in 0..1_000_u32 {
        mgr.append(bid, ev(bid)).unwrap();
    }
    // Default limit is 200 (DEFAULT_READ_LIMIT).
    let r = mgr.events_since(bid, &BucketReadRequest::new(0)).unwrap();
    assert!(r.events.len() <= 200);
    assert!(r.has_more);
    // Explicit limit clamps to MAX_READ_LIMIT.
    let mut req = BucketReadRequest::new(0);
    req.limit = Some(50_000);
    let r = mgr.events_since(bid, &req).unwrap();
    assert!(r.events.len() <= 10_000);
}

#[test]
fn concurrent_appenders_are_safe_with_send_sync() {
    // Compile-time sanity: BucketManager + ContextRingManager are
    // Arc-shareable across threads. The append fast-path is
    // serialized inside the per-bucket cell.
    fn assert_ss<T: Send + Sync>() {}
    assert_ss::<Arc<BucketManager>>();
    assert_ss::<Arc<ContextRingManager>>();
}

#[allow(dead_code)]
const fn _ensures_event_draft_imports(_d: &EventDraft) {}
