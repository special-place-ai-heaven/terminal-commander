// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! terminal-commander#284 wire compatibility for `bucket_wait`.

use std::time::Duration;

use terminal_commander_core::{BucketId, BucketWakeReason, WakeMode};
use terminal_commander_ipc::{BucketWaitParams, BucketWaitResponse};

#[test]
fn bucket_wait_params_old_payload_deserializes() {
    // A pre-#284 client sends no `wake_mode`.
    let id = BucketId::new();
    let old = serde_json::json!({ "bucket_id": id, "cursor": 7, "timeout_ms": 600_000 });
    let p: BucketWaitParams = serde_json::from_value(old).unwrap();
    assert_eq!(p.wake_mode, None, "absent = any");
    assert_eq!(p.requested_timeout(), Duration::from_mins(10));
    // The request is honoured up to the ceiling, not the old 30 s constant.
    assert_eq!(
        p.effective_timeout(3_600_000),
        Duration::from_mins(10)
    );
    assert_eq!(p.effective_timeout(30_000), Duration::from_secs(30));
    // A new client's mode round-trips; a default request stays byte-lean.
    let new = serde_json::json!({ "bucket_id": id, "cursor": 0, "wake_mode": "match" });
    let p: BucketWaitParams = serde_json::from_value(new).unwrap();
    assert_eq!(p.wake_mode, Some(WakeMode::Match));
    let plain = serde_json::json!({ "bucket_id": id, "cursor": 0 });
    let p: BucketWaitParams = serde_json::from_value(plain).unwrap();
    assert!(!serde_json::to_string(&p).unwrap().contains("wake_mode"));
    // An old daemon's response (no new fields) still decodes.
    let resp = serde_json::json!({
        "bucket_id": id, "cursor_in": 0, "next_cursor": 0,
        "heartbeat": true, "dropped_count": 0, "events": []
    });
    let r: BucketWaitResponse = serde_json::from_value(resp).unwrap();
    assert_eq!(r.timeout_clamped_to_ms, None);
    assert!(r.job_exit.is_none());
    assert_eq!(r.wake_reason, BucketWakeReason::Timeout);
}
