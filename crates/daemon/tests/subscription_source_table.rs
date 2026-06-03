// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Task 4 integration test: the bucket source side-table is reachable on
//! `DaemonState` and records source identity at probe start.
//!
//! Unit coverage of `BucketSourceTable` (record/get/snapshot/dirty epoch)
//! lives in `crates/daemon/src/subscriptions/source.rs`; this file verifies
//! the table is constructed on `DaemonState` and bumps its dirty epoch when a
//! source is recorded, exercising the public daemon API.

use terminal_commander_core::{BucketId, JobId, ProbeId};
use terminal_commander_ipc::ProbeKind;
use terminal_commanderd::{BucketSource, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> std::path::PathBuf {
    static C: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = C.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-subsrc-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

#[test]
fn daemon_state_carries_source_table_that_records_and_bumps_epoch() {
    let data = tmp_data_dir("record");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).unwrap();

    let bucket = BucketId::new();
    let job = JobId::new();
    let probe = ProbeId::new();
    let src = BucketSource {
        kind: ProbeKind::Command,
        job_id: Some(job),
        probe_id: Some(probe),
        path: None,
        tag: None,
    };

    assert!(state.sources.get(bucket).is_none(), "no source yet");
    let before = state.sources.dirty_epoch();

    state.sources.record(bucket, src.clone());

    assert_eq!(state.sources.get(bucket), Some(src), "source round-trips");
    assert!(
        state.sources.dirty_epoch() > before,
        "record bumps the dirty epoch on the daemon-owned table"
    );

    cleanup(&data);
}
