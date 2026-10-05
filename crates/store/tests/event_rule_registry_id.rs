// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! The human registry rule id rides on stored events (V0010) and databases
//! written before that column existed stay readable.

use terminal_commander_core::{
    BucketId, EventId, EventSource, FrameId, ProbeId, RuleId, RuleRef, Severity, SignalEvent,
    SourcePointer, SourceStream, SourceType,
};
use terminal_commander_store::{EventStore, StoreBucketConfig};
use time::OffsetDateTime;

fn event(bid: BucketId, registry_id: Option<&str>) -> SignalEvent {
    SignalEvent {
        event_id: EventId::new(),
        bucket_id: bid,
        seq: 0,
        timestamp: OffsetDateTime::now_utc(),
        severity: Severity::High,
        kind: "compile_error".to_owned(),
        summary: "rustc E0432".to_owned(),
        rule: Some(RuleRef {
            id: RuleId::new(),
            version: 3,
            registry_id: registry_id.map(str::to_owned),
        }),
        source: EventSource {
            probe_id: ProbeId::new(),
            source_type: SourceType::Process,
            stream: SourceStream::Stderr,
            job_id: None,
        },
        captures: None,
        pointer: Some(SourcePointer::new(FrameId::new()).with_line(1)),
        pointer_unavailable_reason: None,
        tags: None,
        count: 1,
        first_seen: None,
        last_seen: None,
        suppressed: false,
    }
}

fn db_path(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!(
        "tc-registry-id-{tag}-{}-{nanos}.db",
        std::process::id()
    ));
    p
}

#[test]
fn registry_id_round_trips_through_the_store() {
    let mut s = EventStore::in_memory().unwrap();
    let bid = BucketId::new();
    s.ensure_bucket(bid, &StoreBucketConfig::default()).unwrap();
    let ev = event(bid, Some("cargo.compile-error"));
    let id = ev.event_id;
    s.append(ev).unwrap();
    let back = s.get_event(id).unwrap();
    let rule = back.rule.expect("rule ref");
    assert_eq!(rule.registry_id.as_deref(), Some("cargo.compile-error"));
    assert_eq!(rule.version, 3);
}

#[test]
fn database_without_the_column_stays_readable_by_reader_and_writer() {
    let path = db_path("legacy");
    let bid = BucketId::new();
    let old_id;
    {
        let mut s = EventStore::with_writer(&path).unwrap();
        s.ensure_bucket(bid, &StoreBucketConfig::default()).unwrap();
        let ev = event(bid, Some("cargo.compile-error"));
        old_id = ev.event_id;
        s.append(ev).unwrap();
    }
    // Turn it into a pre-V0010 database: no column, no migration record.
    {
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute_batch(
            "ALTER TABLE events DROP COLUMN rule_registry_id;
             DELETE FROM schema_migrations WHERE version = 10;",
        )
        .unwrap();
    }
    // A reader never migrates; it must still decode the row, with no registry id.
    let reader = EventStore::with_reader(&path).unwrap();
    let rule = reader.get_event(old_id).unwrap().rule.unwrap();
    assert_eq!(rule.registry_id, None);
    assert_eq!(rule.version, 3);
    drop(reader);
    // The next writer open migrates; old rows stay None, new rows carry the id.
    let mut w = EventStore::with_writer(&path).unwrap();
    assert_eq!(w.get_event(old_id).unwrap().rule.unwrap().registry_id, None);
    let ev = event(bid, Some("cargo.compile-error"));
    let new_id = ev.event_id;
    w.append(ev).unwrap();
    assert_eq!(
        w.get_event(new_id)
            .unwrap()
            .rule
            .unwrap()
            .registry_id
            .as_deref(),
        Some("cargo.compile-error")
    );
    drop(w);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn legacy_rule_ref_json_without_registry_id_deserializes() {
    let rr: RuleRef =
        serde_json::from_str(r#"{"id":"rul_018f0000000000000000000000000007","version":2}"#)
            .unwrap();
    assert_eq!(rr.registry_id, None);
    assert!(
        !serde_json::to_string(&rr).unwrap().contains("registry_id"),
        "absent registry_id must stay off the wire"
    );
}
