// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! spec 004 review regression: an operator STOP must stay readable.
//!
//! Cross-model review of the 004 branch (grok BLOCKER, kimi-k3 HIGH-1) found
//! that the branch persisted receipts on the happy path only. A deliberately
//! stopped PTY job or file watch removed its live binding, flipped the ledger,
//! and persisted nothing -- so the new lane routing fell all the way through:
//!
//!   combed declines (not `SourceType::Process`)
//!     -> PTY/watch live maps no longer hold it
//!     -> no receipt exists
//!     -> `job_start_recorded` finds the start audit row
//!     -> `JobLost`
//!
//! `JobLost` means "the daemon recorded this starting and never recorded it
//! finishing" -- i.e. the daemon died mid-run. Reporting that for a session the
//! operator cleanly stopped is a false positive on the branch's own new
//! diagnostic, and it breaches FR-005: an outcome that was readable before
//! (`cancelled`, with misleading zeros) must not become unreadable.
//!
//! The watch lane additionally recorded `finish(watch_id, Some(0), None)` --
//! fabricating a SUCCESSFUL exit for a cancelled job, which is the exact
//! false-green class this feature exists to delete.
//!
//! These tests drive the real `WatchRuntime` (pure filesystem, no PTY backend
//! required) and pin both guarantees.

use std::path::PathBuf;

use terminal_commander_core::{BucketConfig, JobState};
use terminal_commanderd::{DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-stop-readback-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

/// Start a real file watch on a temp file inside the daemon data dir.
fn start_watch(state: &DaemonState, data: &std::path::Path) -> terminal_commander_core::JobId {
    let watched = data.join("watched.log");
    std::fs::write(&watched, b"seed\n").expect("seed watched file");
    let canonical = std::fs::canonicalize(&watched).expect("canonicalize");
    let (watch_id, _bucket, _probe) = state
        .watch
        .start(canonical, BucketConfig::default(), vec![], false, None)
        .expect("watch start");
    watch_id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_watch_stays_readable_instead_of_reporting_lost() {
    let data = tmp_data_dir("stop-readable");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let watch_id = start_watch(&state, &data);
    state.watch.stop(watch_id).expect("stop");

    // The live binding is gone -- that part is intended.
    assert!(
        state.live_lane_status(watch_id).is_none(),
        "stop removes the live binding; if this changes the test below is \
         no longer exercising the fallthrough it was written for"
    );

    // ...but the outcome MUST still be reconstructable. Before the fix this
    // returned None, and the handler then reported `JobLost` for a job the
    // operator had just stopped on purpose.
    let recon = state
        .command
        .reconstructed_status(watch_id)
        .expect("a stopped watch must remain readable, never report JobLost");

    assert_eq!(
        recon.state,
        JobState::Cancelled,
        "an operator stop is a cancellation"
    );
    assert_eq!(
        recon.exit_code, None,
        "a cancelled watch has no exit status; inventing one is the \
         false-green this feature removes"
    );

    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_a_watch_never_records_a_successful_exit() {
    // The pre-fix code called `finish(watch_id, Some(0), None)` with the
    // rationale "the cancel is a clean stop". A cancelled job that reads back
    // as `exited 0` is indistinguishable from a real success.
    let data = tmp_data_dir("no-fake-zero");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let watch_id = start_watch(&state, &data);
    state.watch.stop(watch_id).expect("stop");

    let rec = state.jobs.get(watch_id).expect("ledger record");
    assert_ne!(
        rec.state,
        JobState::Exited,
        "a stopped watch must not be recorded as a clean exit"
    );
    assert_eq!(
        rec.exit_info.as_ref().and_then(|e| e.exit_code),
        None,
        "no exit code may be fabricated for a cancellation"
    );

    cleanup(&data);
}

/// A running watch that has captured a line reports how long ago it did.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_watch_reports_last_output_age() {
    let data = tmp_data_dir("watch-age");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    // Follow from the beginning so the seed line is captured without racing
    // an append against the probe's initial seek to the end.
    let watched = data.join("watched.log");
    std::fs::write(&watched, b"seed\n").expect("seed watched file");
    let canonical = std::fs::canonicalize(&watched).expect("canonicalize");
    let (watch_id, _bucket, _probe) = state
        .watch
        .start(canonical, BucketConfig::default(), vec![], true, None)
        .expect("watch start");

    let mut seen = None;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let status = state.watch.status(watch_id).expect("live watch status");
        if status.frames_total > 0 {
            seen = Some(status);
            break;
        }
    }
    state.watch.stop(watch_id).expect("stop");
    let seen = seen.expect("watch must capture a line");
    assert!(seen.elapsed_ms.is_some(), "{seen:?}");
    assert!(seen.last_output_age_ms.is_some(), "{seen:?}");

    cleanup(&data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_watch_carries_its_real_counters_into_the_receipt() {
    // Readability alone is not enough -- the whole point of 004 is that the
    // preserved outcome carries evidence rather than zeroes.
    let data = tmp_data_dir("stop-evidence");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let watch_id = start_watch(&state, &data);
    state.watch.stop(watch_id).expect("stop");

    let recon = state
        .command
        .reconstructed_status(watch_id)
        .expect("readable after stop");

    // The probe_id is the cheapest non-defaultable evidence field: a zeroed
    // receipt cannot produce the real one.
    assert_ne!(
        recon.probe_id,
        terminal_commander_core::ProbeId::new(),
        "probe_id must come from the persisted evidence, not a fresh default"
    );

    cleanup(&data);
}
