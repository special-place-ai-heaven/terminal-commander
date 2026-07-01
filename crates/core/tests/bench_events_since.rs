// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Auto Research Engineer scoring harness for `BucketManager::events_since`.
//!
//! LOCKED FILE — see research/score.md. Do not edit the fixture, the
//! query, the sample count, or the correctness assertion. Only the
//! ASSET (`crates/core/src/bucket.rs`) may change between rounds.

use std::time::Instant;

use terminal_commander_core::{
    BucketConfig, BucketId, BucketManager, BucketReadRequest, Captures, EventSource, FrameId,
    ProbeId, Severity, SignalEvent, SourcePointer, SourceStream, SourceType,
};

const EVENT_COUNT: u64 = 5_000;
const SAMPLES: usize = 10;

fn fixture_event(bid: BucketId, seq: u64) -> SignalEvent {
    let severity = match seq % 3 {
        0 => Severity::Low,
        1 => Severity::Medium,
        _ => Severity::High,
    };
    let kind = if seq % 2 == 0 { "a" } else { "b" };
    SignalEvent {
        event_id: terminal_commander_core::EventId::new(),
        bucket_id: bid,
        seq: 0,
        timestamp: time::OffsetDateTime::now_utc(),
        severity,
        kind: kind.to_owned(),
        summary: "bench fixture event".to_owned(),
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

fn build_fixture() -> (BucketManager, BucketId) {
    let mgr = BucketManager::new();
    let bid = BucketId::new();
    mgr.create_bucket(bid, BucketConfig::default())
        .expect("fresh bucket create must succeed");
    for seq in 1..=EVENT_COUNT {
        mgr.append(bid, fixture_event(bid, seq))
            .expect("append must succeed under default cap/TTL");
    }
    (mgr, bid)
}

fn query() -> BucketReadRequest {
    BucketReadRequest {
        cursor: 0,
        severity_min: Some(Severity::Low),
        kind_filter: None,
        limit: Some(2_000),
    }
}

/// Identity-independent projection of a response: `event_id`,
/// `bucket_id`, and `timestamp` are freshly minted per fixture build
/// (UUIDv7 / now_utc) and are NOT part of the behavior under test.
/// Everything that reflects `events_since`'s filtering/ordering logic
/// is compared instead.
fn project(resp: &terminal_commander_core::BucketReadResponse) -> Vec<(u64, String, String, u32)> {
    resp.events
        .iter()
        .map(|e| (e.seq, e.severity.as_str().to_owned(), e.kind.clone(), e.count))
        .collect()
}

#[test]
fn bench_events_since() {
    // Warm-up call, discarded.
    let (mgr, bid) = build_fixture();
    let warm = mgr.events_since(bid, &query()).expect("warm-up read");
    let expected_events = project(&warm);
    let expected_next_cursor = warm.next_cursor;
    let expected_has_more = warm.has_more;
    let expected_dropped_count = warm.dropped_count;

    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let (mgr, bid) = build_fixture();
        let start = Instant::now();
        let resp = mgr.events_since(bid, &query()).expect("scored read");
        let elapsed = start.elapsed();

        // Correctness gate: a faster-but-different result is not a win.
        assert_eq!(project(&resp), expected_events, "output events changed");
        assert_eq!(resp.next_cursor, expected_next_cursor, "next_cursor changed");
        assert_eq!(resp.has_more, expected_has_more, "has_more changed");
        assert_eq!(
            resp.dropped_count, expected_dropped_count,
            "dropped_count changed"
        );

        samples.push(elapsed.as_nanos());
    }

    samples.sort_unstable();
    let median = samples[SAMPLES / 2];

    println!("bench_events_since samples (ns): {samples:?}");
    println!("bench_events_since median (ns): {median}");
}
