// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Daemon runtime bootstrap (TC36) integration tests.
//!
//! Proves end-to-end that:
//! - Bootstrap creates `data_dir`, opens the store, applies the
//!   V0003 audit migration.
//! - The router uses the persistent audit sink (NOT InMemoryAudit).
//! - Audit rows survive store reopen.
//! - Config validation rejects clearly-broken values without leaking
//!   secrets in error messages.
//! - Self-check passes on a clean bootstrap.

use std::path::PathBuf;

use terminal_commander_store::{AuditReadRequest, EventStore};
use terminal_commanderd::{
    DaemonConfig, DaemonState, RuntimeMode, config::HARD_MAX_FILE_WINDOW_BYTES,
    config::HARD_MAX_READ_LIMIT, run_self_check,
};

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-daemon-bootstrap-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

#[test]
fn bootstrap_then_reopen_sees_audit_rows() {
    let data = temp_data_dir("reopen");

    // First boot: drive a router call, then drop the state.
    {
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let bid = terminal_commander_core::BucketId::new();
        state
            .router
            .bucket_create(bid, terminal_commander_core::BucketConfig::default())
            .unwrap();
    }

    // Reopen the DB independently and confirm the audit row survived.
    {
        let db = data.join("terminal-commander.db");
        let mut s = EventStore::with_writer(&db).unwrap();
        let rows = s.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter().any(|r| r.action == "bucket_create"),
            "router bucket_create must produce a persisted audit row; rows: {rows:?}"
        );
        // The row must be tagged with the router actor and the
        // closed-set 'info' decision.
        let row = rows.iter().find(|r| r.action == "bucket_create").unwrap();
        assert_eq!(row.actor.as_deref(), Some("router"));
        assert_eq!(row.decision, "info");
    }

    cleanup(&data);
}

#[test]
fn run_self_check_lands_self_check_row_in_persistent_store() {
    let data = temp_data_dir("selfcheck");
    let cfg = DaemonConfig::defaults_in(&data);
    let (state, rep) = run_self_check(cfg).unwrap();
    assert_eq!(rep.failures, 0, "self-check failures: {}", rep.render());

    let mut g = state.store.lock();
    let rows = g.audit_since(&AuditReadRequest::new(0)).unwrap();
    assert!(rows.iter().any(|r| r.action == "self_check"));
    drop(g);

    cleanup(&data);
}

#[test]
fn bootstrap_runtime_mode_defaults_to_self_check() {
    let data = temp_data_dir("mode");
    let cfg = DaemonConfig::defaults_in(&data);
    assert_eq!(cfg.daemon.runtime_mode, RuntimeMode::SelfCheck);
    cleanup(&data);
}

#[test]
fn config_load_rejects_empty_data_dir_without_leaking_values() {
    let s = r#"
        [daemon]
        data_dir = ""

        [policy]
        profile = "developer_local"
    "#;
    let err = DaemonConfig::from_toml(s).unwrap_err();
    let msg = format!("{err}");
    // The message names the field, not a secret-looking value.
    assert!(msg.contains("data_dir"));
}

#[test]
fn config_load_clamps_oversized_limits() {
    let s = r#"
        [daemon]
        data_dir = "/tmp/tc-clamp"

        [policy]
        profile = "developer_local"

        [limits]
        file_window_bytes = 999_999_999
        bucket_read_limit = 999_999_999
    "#;
    let cfg = DaemonConfig::from_toml(s).unwrap();
    assert_eq!(cfg.limits.file_window_bytes, HARD_MAX_FILE_WINDOW_BYTES);
    assert_eq!(cfg.limits.bucket_read_limit, HARD_MAX_READ_LIMIT);
}

#[test]
fn config_rejects_wsl_mnt_c_path() {
    let s = r#"
        [daemon]
        data_dir = "/mnt/c/Users/x/tcdata"

        [policy]
        profile = "developer_local"
    "#;
    let err = DaemonConfig::from_toml(s).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("9P") || msg.contains("/mnt/c"));
}
