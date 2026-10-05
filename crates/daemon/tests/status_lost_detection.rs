// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! spec 004 US4 / FR-009 / FR-010 / FR-011: a job the daemon durably recorded
//! STARTING but never recorded finishing must be distinguishable from an
//! identifier it has no record of.
//!
//! The trap this guards: the original remediation plan keyed lost-detection on
//! a `job_start` audit row emitted by `Router::job_start`. That function has NO
//! production caller -- all three runtimes call `JobManager::start` directly --
//! so the predicate would have matched nothing, forever, while looking correct
//! in review.
//!
//! The first test below therefore starts a REAL command and asserts detection
//! against the row the production path actually wrote. A test that hand-seeded
//! an audit row would have passed against the broken predicate, which is
//! precisely the failure mode being defended against.

use std::path::PathBuf;

use terminal_commander_store::AuditEntry;
use terminal_commanderd::{AuditSink, CommandStartRequest, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-lost-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

#[test]
fn a_really_started_job_is_detected_via_the_production_audit_row() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("real-start");
        let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();
        let exe = std::env::current_exe()
            .expect("current test binary path")
            .to_string_lossy()
            .into_owned();

        let started = state
            .command
            .start_combed(CommandStartRequest {
                argv: vec![exe, "--list".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace: None,
                tag: None,
                dedup_nonce: None,
                receipt_shape: None,
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        let wire = started.job_id.to_wire_string();
        assert!(
            state
                .store
                .job_start_recorded(&wire)
                .expect("lookup must not error"),
            "starting a real command MUST leave a durable, job-id-subject start \
             record that lost-detection can find. If this fails the predicate is \
             keyed on an action nothing emits."
        );

        cleanup(&data);
    });
}

#[test]
fn an_identifier_never_started_has_no_start_record() {
    let data = tmp_data_dir("never");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let fabricated = terminal_commander_core::JobId::new().to_wire_string();
    assert!(
        !state
            .store
            .job_start_recorded(&fabricated)
            .expect("lookup must not error"),
        "an id the daemon never saw must not read as lost"
    );

    cleanup(&data);
}

#[test]
fn detection_covers_every_lane_that_records_a_start() {
    // FR-010. Each lane audits its start under a DIFFERENT action, all with the
    // job id as subject. A combed-only predicate would misclassify a lost PTY,
    // session or watch job as "never heard of it".
    //
    // These rows go through the SAME `AuditSink::emit` the runtimes use, so this
    // exercises the real write path rather than a hand-rolled INSERT.
    let data = tmp_data_dir("lanes");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    for action in [
        "command_start",
        "pty_command_start",
        "shell_session_start",
        "file_watch_start",
    ] {
        let wire = terminal_commander_core::JobId::new().to_wire_string();
        state
            .audit
            .emit(&AuditEntry::new(action, wire.clone(), "allow"))
            .expect("audit emit");
        assert!(
            state.store.job_start_recorded(&wire).expect("lookup"),
            "lane action `{action}` must be covered by lost-detection"
        );
    }

    cleanup(&data);
}

#[test]
fn a_denied_or_failed_start_does_not_read_as_a_lost_job() {
    // The `decision = 'allow'` filter is load-bearing, not cosmetic: the combed
    // lane writes a `command_start` row with decision `error` when the spawn
    // fails. A job that never started must never be reported as one that
    // started and got lost.
    let data = tmp_data_dir("denied");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let errored = terminal_commander_core::JobId::new().to_wire_string();
    state
        .audit
        .emit(&AuditEntry::new("command_start", errored.clone(), "error"))
        .expect("audit emit");
    assert!(
        !state.store.job_start_recorded(&errored).expect("lookup"),
        "a failed start must not be reported as a lost job"
    );

    let denied = terminal_commander_core::JobId::new().to_wire_string();
    state
        .audit
        .emit(&AuditEntry::new(
            "pty_command_start",
            denied.clone(),
            "deny",
        ))
        .expect("audit emit");
    assert!(
        !state.store.job_start_recorded(&denied).expect("lookup"),
        "a denied start must not be reported as a lost job"
    );

    cleanup(&data);
}

/// spec 004 US4 acceptance 3 / T2: a job killed mid-run can NEVER read as a
/// successful terminal outcome.
///
/// The chain has two links, and this test pins the first:
///   1. a non-terminal job has NO receipt (here), and
///   2. a start record with no receipt classifies as lost, never as a terminal
///      (`a_really_started_job_is_detected_via_the_production_audit_row` plus
///      the wire-level assertions in `ipc::handlers::command`).
///
/// Together they mean an abruptly-killed daemon cannot leave behind anything a
/// later status read could mistake for success -- there is simply no receipt to
/// reconstruct from. Structurally this holds because `drive_to_exit` only
/// yields `Exited` AFTER `probe.wait()` reaps the child, and both persist sites
/// sit after that reap.
///
/// Deterministic by construction: no real process is killed, so there is no
/// race to lose.
#[test]
fn a_non_terminal_job_has_no_receipt_to_reconstruct_from() {
    let data = tmp_data_dir("running");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let job_id = terminal_commander_core::JobId::new();
    state.jobs.start(terminal_commander_core::JobConfig {
        job_id,
        argv: vec!["still-running".to_owned()],
        bucket_id: terminal_commander_core::BucketId::new(),
        probe_id: terminal_commander_core::ProbeId::new(),
        source_type: terminal_commander_core::SourceType::Process,
        grace_secs: 0,
    });
    state.jobs.mark_running(job_id);

    assert!(
        state.command.reconstructed_status(job_id).is_none(),
        "a job that has not reached a terminal transition MUST have no receipt; \
         if one existed, an abrupt daemon death could surface it as a completed \
         outcome"
    );

    let _ = &state;
    cleanup(&data);
}
