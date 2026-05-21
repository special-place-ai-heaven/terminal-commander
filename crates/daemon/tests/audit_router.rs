// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Router + persistent audit sink (TC35) integration tests.
//!
//! Proves the end-to-end persistence story: a Router constructed
//! over a `PersistentAudit` backed by a file-backed `EventStore`
//! records rows that survive store reopen.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use terminal_commander_core::{
    BucketConfig, BucketId, BucketManager, BucketReadRequest, Captures, ContextRingManager,
    EventDraft, EventSource, FrameId, JobConfig, JobManager, ProbeId, Severity, SourcePointer,
    SourceStream, SourceType,
};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::{AuditReadRequest, EventStore};
use terminal_commanderd::{PersistentAudit, Router};

fn tmp_db_path(suffix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-audit-router-{pid}-{nanos}-{suffix}.db"));
    p
}

fn cleanup(p: &PathBuf) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(p.with_extension("db-wal"));
    let _ = std::fs::remove_file(p.with_extension("db-shm"));
}

fn make_draft(bid: BucketId, pid: ProbeId, sev: Severity, kind: &str) -> EventDraft {
    let mut caps = Captures::new();
    caps.insert("k".to_owned(), "v".to_owned());
    EventDraft {
        bucket_id: bid,
        timestamp: time::OffsetDateTime::now_utc(),
        severity: sev,
        kind: kind.to_owned(),
        summary: "s".to_owned(),
        rule: None,
        source: EventSource {
            probe_id: pid,
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

fn build_router_with_persistent_audit(store: Arc<Mutex<EventStore>>) -> Router {
    let buckets = Arc::new(BucketManager::new());
    let rings = Arc::new(ContextRingManager::new());
    let jobs = Arc::new(JobManager::new());
    let sifter = Arc::new(SifterRuntime::build(&[]).unwrap());
    let audit = Arc::new(PersistentAudit::new(store));
    audit.ensure_migration().unwrap();
    Router::with_sink(buckets, rings, jobs, sifter, audit)
}

#[test]
fn router_audit_persists_across_store_reopen() {
    let p = tmp_db_path("router-reopen");

    // First open: drive a few router calls.
    {
        let store = Arc::new(Mutex::new(EventStore::with_writer(&p).unwrap()));
        let r = build_router_with_persistent_audit(Arc::clone(&store));
        let bid = BucketId::new();
        r.bucket_create(bid, BucketConfig::default()).unwrap();
        r.bucket_append(
            bid,
            make_draft(bid, ProbeId::new(), Severity::High, "kind1"),
        )
        .unwrap();
        r.bucket_append(bid, make_draft(bid, ProbeId::new(), Severity::Low, "kind2"))
            .unwrap();
        let _ = r
            .bucket_events_since(bid, &BucketReadRequest::new(0))
            .unwrap();
        // 4 audit rows emitted so far: 1 bucket_create + 2 bucket_append + 1 bucket_events_since.
        assert_eq!(r.audit_len(), 4);
    }

    // Reopen: prove rows survived.
    {
        let mut s = EventStore::with_writer(&p).unwrap();
        let rows = s.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert_eq!(rows.len(), 4);
        let actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
        assert_eq!(
            actions,
            vec![
                "bucket_create",
                "bucket_append",
                "bucket_append",
                "bucket_events_since",
            ]
        );
        // Every router-emitted row uses the closed-set `info` decision.
        assert!(rows.iter().all(|r| r.decision == "info"));
        // Every router-emitted row carries the `router` actor.
        assert!(rows.iter().all(|r| r.actor.as_deref() == Some("router")));
    }

    cleanup(&p);
}

#[test]
fn router_job_lifecycle_persists_audit() {
    let p = tmp_db_path("router-jobs");
    {
        let store = Arc::new(Mutex::new(EventStore::with_writer(&p).unwrap()));
        let r = build_router_with_persistent_audit(Arc::clone(&store));
        let bid = BucketId::new();
        let pid = ProbeId::new();
        let cfg = JobConfig::new(vec!["echo".to_owned()], bid, pid);
        let id = r.job_start(cfg);
        let _ = r.job_finish(id, Some(0), None);
    }
    {
        let mut s = EventStore::with_writer(&p).unwrap();
        let rows = s.audit_since(&AuditReadRequest::new(0)).unwrap();
        let kinds: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
        assert!(kinds.contains(&"job_start"));
        assert!(kinds.contains(&"job_finish"));
    }
    cleanup(&p);
}
