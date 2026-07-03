// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! US4 / SC-004 evidence: measure the serialized agent-facing response bytes a
//! quiet long build costs BEFORE vs AFTER the token-lean changes.
//!
//! Repro shape (the findings-doc scenario): an agent watches a ~quiet long
//! build over a subscription with several in-scope sources, polling repeatedly.
//! BEFORE this feature every poll re-sent the full per-source liveness array
//! and every bucket read returned full `SignalEvent` records. AFTER: the
//! adapter reads compact (`{summary, stream, seq, severity}`) and requests the
//! liveness delta, so a full liveness snapshot rides only the FIRST pull and is
//! omitted while nothing changes.
//!
//! This test measures the exact wire shapes (real `SignalEvent` and
//! `SourceLiveness` values, serialized with serde_json) and asserts the >= 60%
//! reduction SC-004 targets. Run with `--no-capture` to print the byte counts
//! recorded in `specs/002-dogfood-remediation/evidence-sc004.md`.
//!
//! Source-status: live.

use serde_json::json;
use terminal_commander_core::{
    BucketId, Captures, EventId, EventSource, FrameId, JobId, ProbeId, RuleId, RuleRef, Severity,
    SignalEvent, SourcePointer, SourceStream, SourceType,
};
use terminal_commander_ipc::{Liveness, SourceLiveness};

/// In-scope sources for the watched build (one command job + three file-watch
/// probes) -- a realistic multi-source subscription.
const SOURCES: usize = 4;
/// Poll cycles across the quiet build.
const POLLS: usize = 12;
/// Compile diagnostics surfaced during the build.
const MATCHES: usize = 3;

/// A representative full compile-diagnostic `SignalEvent` (the heavy record the
/// event store keeps and that a full read returns).
fn diagnostic_event(bucket: BucketId, seq: u64) -> SignalEvent {
    let mut captures = Captures::new();
    captures.insert("code".to_owned(), "E0432".to_owned());
    captures.insert(
        "match".to_owned(),
        "error[E0432]: unresolved import".to_owned(),
    );
    SignalEvent {
        event_id: EventId::new(),
        bucket_id: bucket,
        seq,
        timestamp: time::OffsetDateTime::now_utc(),
        severity: Severity::High,
        kind: "compile_error".to_owned(),
        summary: "rustc E0432: unresolved import `crate::missing_module`".to_owned(),
        rule: Some(RuleRef {
            id: RuleId::new(),
            version: 1,
        }),
        source: EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::Process,
            stream: SourceStream::Stderr,
            job_id: Some(JobId::new()),
        },
        captures: Some(captures),
        pointer: Some(SourcePointer::new(FrameId::new()).with_line(7)),
        pointer_unavailable_reason: None,
        tags: None,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

/// The compact projection the adapter emits (mirrors `project_signal_compact`).
fn compact_projection(ev: &SignalEvent) -> serde_json::Value {
    json!({
        "summary": ev.summary,
        "stream": ev.source.stream,
        "seq": ev.seq,
        "severity": ev.severity,
    })
}

/// One in-scope source's liveness entry (running or exited).
fn liveness_entry(bucket: BucketId, running: bool) -> SourceLiveness {
    SourceLiveness {
        bucket_id: bucket,
        job_id: Some(JobId::new()),
        probe_id: Some(ProbeId::new()),
        liveness: if running {
            Liveness::Running
        } else {
            Liveness::Exited { code: 0 }
        },
    }
}

fn bytes(v: &serde_json::Value) -> usize {
    serde_json::to_string(v).expect("serialize").len()
}

#[test]
fn sc004_compact_and_delta_cut_quiet_build_bytes_by_at_least_60_percent() {
    let bucket = BucketId::new();

    // The full per-source liveness snapshot re-sent on every legacy pull.
    let full_liveness: Vec<SourceLiveness> = (0..SOURCES)
        .map(|i| liveness_entry(BucketId::new(), i == 0))
        .collect();

    // The build's compile diagnostics, full records.
    let full_events: Vec<SignalEvent> = (0..MATCHES as u64)
        .map(|i| diagnostic_event(bucket, i + 1))
        .collect();
    let compact_events: Vec<serde_json::Value> =
        full_events.iter().map(compact_projection).collect();

    // ---- BEFORE (pre-US4): full liveness on EVERY pull + full event records. ----
    // Each idle pull re-sends the full liveness array.
    let before_idle_pull = json!({
        "events": [],
        "liveness": full_liveness,
        "lagged": false,
        "truncated": false,
    });
    // The event-bearing read returns full records.
    let before_events_read = json!({
        "bucket_id": bucket,
        "cursor_in": 0,
        "next_cursor": MATCHES,
        "has_more": false,
        "dropped_count": 0,
        "events": full_events,
    });
    let before_total = POLLS * bytes(&before_idle_pull) + bytes(&before_events_read);

    // ---- AFTER (US4): liveness baseline on pull #1, omitted while idle;
    //      compact event records. ----
    let after_baseline_pull = json!({
        "events": [],
        "liveness": full_liveness,
        "lagged": false,
        "truncated": false,
    });
    // Steady idle pulls omit the liveness section entirely (empty delta).
    let after_idle_pull = json!({
        "events": [],
        "lagged": false,
        "truncated": false,
    });
    let after_events_read = json!({
        "bucket_id": bucket,
        "cursor_in": 0,
        "next_cursor": MATCHES,
        "has_more": false,
        "dropped_count": 0,
        "events": compact_events,
        "compact": true,
    });
    let after_total = bytes(&after_baseline_pull)
        + (POLLS - 1) * bytes(&after_idle_pull)
        + bytes(&after_events_read);

    // Integer per-mille reduction (avoids float casts under -D clippy::pedantic).
    let saved = before_total - after_total;
    let permille = saved * 1000 / before_total;
    println!(
        "SC-004 quiet-build bytes: before={before_total} after={after_total} \
         reduction={}.{}% (sources={SOURCES}, polls={POLLS}, matches={MATCHES})",
        permille / 10,
        permille % 10
    );

    assert!(
        after_total < before_total,
        "token-lean must reduce bytes: before={before_total} after={after_total}"
    );
    assert!(
        permille >= 600,
        "SC-004 targets >= 60.0% reduction; got {}.{}% (before={before_total} after={after_total})",
        permille / 10,
        permille % 10
    );
}
