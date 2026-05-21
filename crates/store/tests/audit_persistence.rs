// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Persistent audit log (TC35) integration tests.
//!
//! These tests exercise the durable path: write rows, close the
//! store, reopen against the same on-disk file, and confirm the
//! rows survive. They also confirm bounded metadata caps and the
//! closed-set decision validator.

use std::path::PathBuf;

use terminal_commander_store::{
    AuditEntry, AuditReadRequest, EventStore, MAX_AUDIT_METADATA_BYTES, MAX_AUDIT_REASON_BYTES,
    MAX_AUDIT_SUBJECT_BYTES,
};

fn tmp_db_path(suffix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-audit-{pid}-{nanos}-{suffix}.db"));
    p
}

fn cleanup(p: &PathBuf) {
    let _ = std::fs::remove_file(p);
    let _ = std::fs::remove_file(p.with_extension("db-wal"));
    let _ = std::fs::remove_file(p.with_extension("db-shm"));
}

#[test]
fn audit_rows_survive_store_reopen() {
    let p = tmp_db_path("reopen");

    // First open: migrate + insert four rows.
    {
        let mut s = EventStore::with_writer(&p).unwrap();
        s.record_audit(
            &AuditEntry::new("bucket_create", "bkt_a", "info")
                .with_actor("router")
                .with_profile("developer_local"),
        )
        .unwrap();
        s.record_audit(&AuditEntry::new("bucket_append", "bkt_a", "info").with_actor("router"))
            .unwrap();
        s.record_audit(
            &AuditEntry::new("registry_activate", "rule_x", "allow_with_audit")
                .with_profile("admin_debug")
                .with_actor("mcp"),
        )
        .unwrap();
        s.record_audit(
            &AuditEntry::new("file_read", "/etc/hostname", "allow")
                .with_actor("mcp")
                .with_reason("FileRead allowed under developer_local"),
        )
        .unwrap();
        assert_eq!(s.audit_count().unwrap(), 4);
    }

    // Reopen against the same path.
    {
        let mut s = EventStore::with_writer(&p).unwrap();
        let rows = s.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert_eq!(rows.len(), 4, "audit rows must survive reopen");
        // audit_id assigned by AUTOINCREMENT is strictly increasing.
        for win in rows.windows(2) {
            assert!(win[0].audit_id < win[1].audit_id);
        }
        // Decisions preserved.
        assert!(rows.iter().any(|r| r.decision == "allow_with_audit"));
        assert!(rows.iter().any(|r| r.decision == "allow"));
        assert!(rows.iter().any(|r| r.decision == "info"));
        // Reason preserved.
        let fr = rows.iter().find(|r| r.action == "file_read").unwrap();
        assert!(fr.reason.as_deref().unwrap().contains("developer_local"));
        // Cursor read after the last row returns empty.
        let last = rows.last().unwrap().audit_id;
        let none = s.audit_since(&AuditReadRequest::new(last)).unwrap();
        assert!(none.is_empty());
    }

    cleanup(&p);
}

#[test]
fn migration_v0003_idempotent_across_reopen() {
    let p = tmp_db_path("idemp");
    {
        let mut s = EventStore::with_writer(&p).unwrap();
        s.ensure_audit().unwrap();
        s.ensure_audit().unwrap(); // explicit idempotency
    }
    {
        let mut s = EventStore::with_writer(&p).unwrap();
        // ensure_audit runs lazily; record_audit forces it through.
        s.record_audit(&AuditEntry::new("bucket_create", "x", "info"))
            .unwrap();
        assert_eq!(s.audit_count().unwrap(), 1);
    }
    cleanup(&p);
}

#[test]
fn rejects_unknown_decision() {
    let mut s = EventStore::in_memory().unwrap();
    let err = s
        .record_audit(&AuditEntry::new("anything", "x", "bogus"))
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("closed set"),
        "expected closed-set error, got: {msg}"
    );
}

#[test]
fn rejects_empty_action() {
    let mut s = EventStore::in_memory().unwrap();
    let err = s
        .record_audit(&AuditEntry::new("", "x", "info"))
        .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("action"), "expected action error, got: {msg}");
}

#[test]
fn caps_metadata_json_size() {
    let mut s = EventStore::in_memory().unwrap();
    // Just at the cap: allowed.
    let ok_meta = "a".repeat(MAX_AUDIT_METADATA_BYTES);
    s.record_audit(&AuditEntry::new("bucket_append", "bkt_x", "info").with_metadata_json(ok_meta))
        .unwrap();
    // One byte over the cap: rejected.
    let too_big = "a".repeat(MAX_AUDIT_METADATA_BYTES + 1);
    let err = s
        .record_audit(
            &AuditEntry::new("bucket_append", "bkt_x", "info").with_metadata_json(too_big),
        )
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("MAX_AUDIT_METADATA_BYTES"),
        "expected metadata cap error, got: {msg}"
    );
}

#[test]
fn caps_subject_size() {
    let mut s = EventStore::in_memory().unwrap();
    let too_big_subject = "s".repeat(MAX_AUDIT_SUBJECT_BYTES + 1);
    let err = s
        .record_audit(&AuditEntry::new("bucket_create", too_big_subject, "info"))
        .unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("MAX_AUDIT_SUBJECT_BYTES"),
        "expected subject cap error, got: {msg}"
    );
}

#[test]
fn truncates_oversized_reason_without_failing() {
    let mut s = EventStore::in_memory().unwrap();
    // Insert succeeds; reason is truncated to the cap on disk.
    let huge = "z".repeat(MAX_AUDIT_REASON_BYTES * 4);
    s.record_audit(&AuditEntry::new("bucket_create", "x", "info").with_reason(huge))
        .unwrap();
    let rows = s.audit_since(&AuditReadRequest::new(0)).unwrap();
    assert_eq!(rows.len(), 1);
    let stored = rows[0].reason.as_deref().unwrap();
    assert!(
        stored.len() <= MAX_AUDIT_REASON_BYTES,
        "reason should be capped: len={}",
        stored.len()
    );
}

#[test]
fn filters_by_action_and_decision() {
    let mut s = EventStore::in_memory().unwrap();
    s.record_audit(&AuditEntry::new("bucket_create", "bkt_x", "info"))
        .unwrap();
    s.record_audit(&AuditEntry::new("bucket_append", "bkt_x", "info"))
        .unwrap();
    s.record_audit(
        &AuditEntry::new("registry_activate", "rule_y", "allow_with_audit").with_actor("mcp"),
    )
    .unwrap();
    let only_create = s
        .audit_since(&AuditReadRequest {
            cursor: 0,
            action_filter: Some("bucket_create".to_owned()),
            decision_filter: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(only_create.len(), 1);
    let only_audit_decision = s
        .audit_since(&AuditReadRequest {
            cursor: 0,
            action_filter: None,
            decision_filter: Some("allow_with_audit".to_owned()),
            limit: None,
        })
        .unwrap();
    assert_eq!(only_audit_decision.len(), 1);
    assert_eq!(only_audit_decision[0].action, "registry_activate");
}

/// Documented invariant: the audit log MUST NEVER store raw stream
/// content. We do not have a raw-stream column; this test pins that.
/// A future schema change that added a raw blob column would break.
#[test]
fn schema_does_not_have_raw_or_blob_columns() {
    let mut s = EventStore::in_memory().unwrap();
    let cols = s.audit_table_columns().unwrap();
    let bad = cols.iter().find(|(name, ty)| {
        name.to_ascii_lowercase().contains("stream")
            || name.to_ascii_lowercase().contains("raw")
            || ty.eq_ignore_ascii_case("BLOB")
    });
    assert!(
        bad.is_none(),
        "audit_records schema must not contain raw-stream / blob columns: {bad:?}"
    );
}

#[test]
fn raw_stream_text_rejected_via_metadata_cap() {
    // Even if a buggy caller tried to stuff raw stdout into metadata,
    // the MAX_AUDIT_METADATA_BYTES cap and the no-blob schema mean
    // the audit log cannot become a raw-output bypass. This test
    // makes the property explicit at the integration level.
    let mut s = EventStore::in_memory().unwrap();
    let huge_raw = vec![b'x'; MAX_AUDIT_METADATA_BYTES + 1024];
    let huge_str = String::from_utf8(huge_raw).unwrap();
    let err = s
        .record_audit(
            &AuditEntry::new("bucket_append", "bkt_x", "info").with_metadata_json(huge_str),
        )
        .unwrap_err();
    assert!(format!("{err}").contains("MAX_AUDIT_METADATA_BYTES"));
}
