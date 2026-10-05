// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! spec 004 FR-004: `command_status` must never report counters it did not
//! observe.
//!
//! The job ledger is SHARED across the combed, PTY and watch lanes
//! (`DaemonState::bootstrap` mints one `JobManager` and clones it into all
//! three), and `JobManager::get` has no `source_type` filter. Before this fix
//! `CommandRuntime::status` therefore answered for jobs it did not own: real
//! `state`/`exit_code` from the shared ledger, but every counter zeroed --
//! because the metrics live in the OWNING runtime's private map -- and
//! `restarted: false` positively asserting those zeros were observed live.
//!
//! Measured against a live 0.1.86 daemon before the fix: a PTY `hostname` job
//! reported `frames_total: 0, bytes_total: 0` from `command_status` while
//! `command_output_tail` returned `["CRRR65734"]` for the SAME job id.
//!
//! These tests pin the guard so that cannot recur.

use std::path::PathBuf;

use terminal_commander_core::{BucketId, JobConfig, JobId, ProbeId, SourceType};
use terminal_commanderd::{CommandStartRequest, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-lane-own-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

/// Register a job in the SHARED ledger under a non-combed lane, exactly as the
/// PTY and watch runtimes do, without needing a real PTY or watched file.
fn register_foreign_lane_job(state: &DaemonState, source_type: SourceType) -> JobId {
    let job_id = JobId::new();
    state.jobs.start(JobConfig {
        job_id,
        argv: vec!["foreign-lane".to_owned()],
        bucket_id: BucketId::new(),
        probe_id: ProbeId::new(),
        source_type,
        grace_secs: 0,
    });
    state.jobs.mark_running(job_id);
    job_id
}

#[test]
fn combed_status_declines_a_pty_lane_job_instead_of_reporting_zeroes() {
    let data = tmp_data_dir("pty-decline");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let job_id = register_foreign_lane_job(&state, SourceType::Terminal);

    let result = state.command.status(job_id);

    // The regression: this used to return Ok with every counter zeroed and
    // `outcome_trust`-equivalent trust asserting a live observation. The combed
    // runtime does not own this job's metrics, so it must decline and let the
    // caller route to the lane that does.
    assert!(
        result.is_err(),
        "combed runtime must not answer for a PTY-lane job it does not own; \
         answering means reporting counters it never observed"
    );

    cleanup(&data);
}

#[test]
fn combed_status_declines_a_watch_lane_job_instead_of_reporting_zeroes() {
    let data = tmp_data_dir("watch-decline");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let job_id = register_foreign_lane_job(&state, SourceType::File);

    assert!(
        state.command.status(job_id).is_err(),
        "the watch lane has the same shared-ledger exposure as PTY"
    );

    cleanup(&data);
}

#[test]
fn combed_status_still_answers_for_its_own_lane() {
    // The guard must reject foreign lanes WITHOUT over-rejecting: a real combed
    // job still gets a status, and it is marked as a live observation.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("own-lane");
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
            .expect("combed start ok");

        let status = state
            .command
            .status(started.job_id)
            .expect("combed runtime must answer for its OWN job");

        assert_eq!(
            status.outcome_trust,
            terminal_commanderd::OutcomeTrust::Observed,
            "a live combed job is observed, not reconstructed"
        );
        assert!(
            !status.restarted,
            "`restarted` is derived from outcome_trust and must agree with it"
        );

        cleanup(&data);
    });
}

#[test]
fn unknown_id_is_still_unknown_in_every_lane() {
    // The guard must not turn "never heard of it" into something else.
    let data = tmp_data_dir("unknown");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    assert!(state.command.status(JobId::new()).is_err());

    cleanup(&data);
}

/// Register a COMBED-lane job in the shared ledger with no live binding, which
/// is what a future `CommandRuntime.live` eviction (BACKLOG TCD-8) will
/// produce for a finished job.
fn register_combed_job_without_binding(state: &DaemonState) -> JobId {
    let job_id = JobId::new();
    state.jobs.start(JobConfig {
        job_id,
        argv: vec!["evicted".to_owned()],
        bucket_id: BucketId::new(),
        probe_id: ProbeId::new(),
        source_type: SourceType::Process,
        grace_secs: 0,
    });
    job_id
}

#[test]
fn a_combed_job_with_no_live_binding_is_declined_not_certified_observed() {
    // spec 004 review (composer MEDIUM, grok/kimi LOW). The combed lane used
    // `unwrap_or_default()` on a live-map miss and then stamped
    // `outcome_trust: Observed` -- certifying zeros nobody witnessed, which is
    // the exact over-claim this feature exists to delete.
    //
    // Unreachable while TCD-8 keeps terminal bindings forever. This is the
    // guard that stops TCD-8's eventual fix from silently reintroducing it.
    let data = tmp_data_dir("evicted-binding");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let job_id = register_combed_job_without_binding(&state);
    state.jobs.mark_running(job_id);

    assert!(
        state.command.status(job_id).is_err(),
        "with no live binding there is no observation; the lane must decline \
         so the caller reconstructs from the receipt, never certify zeroes"
    );

    cleanup(&data);
}

#[test]
fn the_start_window_still_answers_while_the_binding_is_being_inserted() {
    // The decline above must NOT swallow the documented window between ledger
    // registration and live-map insertion in `start_combed_inner`. A job that
    // has genuinely just started has truthful zeroes, and reporting it as
    // unknown would be a regression of its own.
    let data = tmp_data_dir("start-window");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&data)).unwrap();

    let job_id = register_combed_job_without_binding(&state);
    // deliberately NOT marked running: still `Starting`.

    let s = state
        .command
        .status(job_id)
        .expect("a starting job must still answer");
    assert_eq!(s.frames_total, 0, "nothing has happened yet");

    cleanup(&data);
}
