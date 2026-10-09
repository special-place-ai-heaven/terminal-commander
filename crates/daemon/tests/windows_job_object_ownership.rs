// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! spec 004 T5 / review tripwire: a child killed by `KILL_ON_JOB_CLOSE` must
//! never persist `terminal_state: "exited", exit_code: 0`.
//!
//! ## Why this is a static tripwire and not a fault-injected behavioural test
//!
//! The concern is real: Windows children run inside a Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. If any ordering let the job handle
//! close while a waiter was still alive to observe `probe.wait()`, that waiter
//! could see a plausible `Ok(status)` for a child that was actually killed, and
//! persist a receipt claiming `exited`.
//!
//! It cannot happen, and the reason is OWNERSHIP rather than sequencing:
//!
//! 1. The lifecycle task owns `OwnedProcess`, which holds an `Arc<JobHandle>`
//!    through cleanup, governor accounting, and the exit report.
//! 2. `KILL_ON_JOB_CLOSE` fires only when the LAST `Arc` drops, via
//!    `JobHandle::Drop -> CloseHandle`.
//! 3. Runtime-loss cleanup transfers an Arc to its native reaper until cleanup
//!    verification. The normal daemon waiter owns its probe across `wait()`.
//!
//! The task producing the exit report retains the live job handle. Native
//! cleanup retains it independently of the Tokio runtime. A metrics/CPU handle
//! alone cannot keep a process alive after its lifecycle owner is gone.
//!
//! Forcing the failure would require production code to expose a seam that only
//! a test uses, which constitution VI (NON-NEGOTIABLE) forbids: "Production code
//! paths MUST NOT reach into test-only logic." `CONTRIBUTING.md` §6.1 sanctions
//! exactly this fallback -- "record the ownership argument as a documented
//! invariant with a review tripwire" -- for Windows `cfg` sentinels headless CI
//! cannot exercise live.
//!
//! These assertions therefore guard the three structural facts the argument
//! rests on. If a refactor breaks any of them the argument no longer holds, and
//! this test fails loudly rather than the guarantee silently evaporating.
//!
//! Source-status: `partial` -- structural guard, not live fault injection.

#![cfg(windows)]

fn read_repo_file(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Facts 1 + 2: lifecycle and native cleanup retain the shared job handle.
#[test]
fn lifecycle_task_and_native_reaper_own_the_job_handle_arc() {
    let source = read_repo_file("../probes/src/process.rs");
    let owned = source
        .split_once("struct OwnedProcess {")
        .unwrap()
        .1
        .split_once("impl OwnedProcess {")
        .unwrap()
        .0;
    assert!(
        owned.contains("job: Option<Arc<JobHandle>>"),
        "the lifecycle owner must retain the Windows Job Object handle"
    );

    let spawn = source
        .split_once("pub fn spawn_with_environment(")
        .unwrap()
        .1;
    let (before_task, task) = spawn
        .split_once("runtime_handle.spawn(async move {")
        .unwrap();
    assert!(
        before_task.contains("owned.job.clone_from(&job)"),
        "the Job Object Arc must reach OwnedProcess before the task can run"
    );
    let task = task.split_once("Ok(Self {").unwrap().0;
    let finished = task.find("owned.finish().await").unwrap();
    let accounted = task.find("owned.job.as_deref()").unwrap();
    let reported = task.find("Ok(ProcessProbeReport {").unwrap();
    assert!(
        finished < accounted && accounted < reported,
        "the task must retain the job through cleanup, accounting and the exit report"
    );

    let reaper = source.split_once("impl Drop for OwnedProcess {").unwrap().1;
    let reaper = reaper.split_once("fn signal_process_group(").unwrap().0;
    let cloned = reaper.find("let job = self.job.clone()").unwrap();
    let moved = reaper.find(".spawn(move || {").unwrap();
    let verified = reaper.find("job_quiescent(job.as_deref())").unwrap();
    assert!(
        cloned < moved && moved < verified,
        "runtime-loss cleanup must retain the Arc in its native reaper until verification"
    );

    let handle_drop = source.split_once("impl Drop for JobHandle {").unwrap().1;
    let handle_drop = handle_drop
        .split_once("pub struct ProcessProbe {")
        .unwrap()
        .0;
    assert!(
        handle_drop.contains("CloseHandle(self.0 as HANDLE)"),
        "KILL_ON_JOB_CLOSE remains scoped to the final JobHandle Arc"
    );
}

/// Fact 3: the daemon waiter owns the probe for the whole wait and receives
/// the lifecycle task's report before constructing its receipt.
#[test]
fn drive_to_exit_still_takes_the_probe_by_value() {
    let source = read_repo_file("src/command.rs");

    assert!(
        source.contains("async fn drive_to_exit(mut probe: ProcessProbe)"),
        "drive_to_exit must retain the probe by value while awaiting its lifecycle report"
    );
    assert!(
        source.contains("probe.wait().await"),
        "drive_to_exit must still await `probe.wait()` while holding the probe; \
         the ownership argument is about what is alive ACROSS that await."
    );
}

/// The complementary branch: an outcome with no reaped status must not become a
/// success. `Cancelled` carries no exit code.
#[test]
fn a_cancelled_outcome_still_maps_to_no_exit_code() {
    let source = read_repo_file("src/command.rs");

    assert!(
        source.contains("ProbeOutcome::Cancelled => None"),
        "A cancelled probe MUST map to `exit_code: None`. Mapping it to Some(0) \
         -- or letting it fall through to a default -- would let an explicit \
         kill surface as a clean exit, which is the same false-green this \
         tripwire exists to prevent."
    );
}
