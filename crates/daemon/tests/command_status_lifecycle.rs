// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

use std::path::PathBuf;
use std::time::Duration;

use terminal_commander_core::{BucketReadRequest, JobState};
use terminal_commanderd::{CommandStartRequest, DaemonConfig, DaemonState};

#[cfg(unix)]
use terminal_commander_core::{
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cmd-status-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

#[test]
fn command_status_counts_lifecycle_event_when_no_rules_match() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("lifecycle");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let exe = std::env::current_exe()
            .expect("current test binary path")
            .to_string_lossy()
            .into_owned();

        let resp = state
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
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(resp.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
            ) {
                break;
            }
        }

        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read ok");
        let kinds: Vec<&str> = bread.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, vec!["command_exited"]);

        let status = state.command.status(resp.job_id).expect("status ok");
        assert_eq!(status.events_emitted, 1);

        state
            .command
            .stop(resp.job_id, "test-peer")
            .expect("redundant stop is idempotent");
        let after_stop = state
            .command
            .status(resp.job_id)
            .expect("status after stop");
        assert_eq!(
            after_stop.events_emitted, 1,
            "redundant stop must not clobber the terminal lifecycle-event count"
        );

        cleanup(&data);
    });
}

// D7: a command lifecycle waiter must be AWAITED by the graceful-shutdown
// drain BEFORE the store closes, so a command exiting in the shutdown
// window still persists its command_exited event + exit audit row.
//
// `drain_lifecycle_tasks` is exactly what `run_ipc_server` calls between
// the IPC connection drain and `shutdown_store`. Here we call it directly
// WITHOUT first polling for terminal state: if the waiter were a detached
// `tokio::spawn` (the pre-fix behavior), the command_exited event would
// race the assertion and could be missing. Because the waiter is tracked
// in the lifecycle JoinSet and the drain joins it to completion, the
// event MUST already be appended the instant the drain returns. Cross-
// platform: the self-exec `--list` argv is the same quick-exit command
// the no-rules test above uses on all targets.
#[test]
fn lifecycle_waiter_is_drained_before_store_close() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("drain-before-store");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let exe = std::env::current_exe()
            .expect("current test binary path")
            .to_string_lossy()
            .into_owned();

        let resp = state
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
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        // Simulate the graceful-shutdown sequence: drain the lifecycle
        // waiters (this is the new D7 step). No prior poll-wait: the drain
        // is what guarantees the waiter ran to completion.
        state.command.drain_lifecycle_tasks().await;

        // The waiter has been awaited to completion, so the synthetic
        // command_exited event is already in the bucket and the exit was
        // recorded on the job. A store close happening now (the real
        // shutdown path) would therefore NOT lose the final event.
        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read ok");
        let kinds: Vec<&str> = bread.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec!["command_exited"],
            "drain must await the waiter so command_exited is persisted before store close"
        );
        assert!(
            matches!(
                state.command.job_record(resp.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed)
            ),
            "drain must leave the job in a terminal state"
        );

        cleanup(&data);
    });
}

/// A stdout rule matching "hello"; status=Active so it is
/// runtime-eligible (the draft-poison gate would otherwise reject it).
#[cfg(unix)]
fn hello_rule() -> RuleDefinition {
    RuleDefinition {
        id: "lifecycle-hello".to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: "hello_seen".to_owned(),
        stream: Some(SourceStream::Stdout),
        description: Some("match the hello line".to_owned()),
        pattern: None,
        keywords: Some(vec!["hello".to_owned()]),
        captures: vec![],
        summary_template: "hello detected".to_owned(),
        tags: vec!["lifecycle".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[cfg(unix)]
fn wait_terminal(state: &DaemonState, job_id: terminal_commander_core::JobId) {
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(40));
        if matches!(
            state.command.job_record(job_id).map(|r| r.state),
            Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
        ) {
            return;
        }
    }
}

// TCE-ERG-1: a command that finishes with ZERO rule-driven events must
// return a non-empty, truthful exit receipt instead of silence.
#[cfg(unix)]
#[test]
fn no_rule_command_returns_exit_receipt() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("receipt-norule");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let resp = state
            .command
            .start_combed(CommandStartRequest {
                // argv-only, no shell: printf is not in
                // SHELL_INTERPRETERS_DENY. Emits two stdout lines.
                argv: vec!["/usr/bin/printf".to_owned(), "hello\nworld\n".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace: None,
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        wait_terminal(&state, resp.job_id);

        let status = state.command.status(resp.job_id).expect("status ok");
        let receipt = status
            .receipt
            .expect("zero-rule run must carry a no-silence receipt");
        assert_eq!(receipt.exit_code, Some(0));
        assert_eq!(receipt.lines_suppressed, 2);
        assert_eq!(receipt.tail, vec!["hello".to_owned(), "world".to_owned()]);
        assert!(!receipt.tail_incomplete);

        cleanup(&data);
    });
}

// F1: command_output_tail returns bounded lines without requiring a rule.
#[cfg(unix)]
#[test]
fn command_output_tail_returns_bounded_lines_without_a_rule() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("tail-norule");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // printf emits 3 stdout lines; tail max_lines=2 must truncate.
        let resp = state
            .command
            .start_combed(CommandStartRequest {
                argv: vec![
                    "/usr/bin/printf".to_owned(),
                    "line1\nline2\nline3\n".to_owned(),
                ],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace: None,
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        wait_terminal(&state, resp.job_id);

        let rec = state.jobs.get(resp.job_id).expect("job record present");
        let probe_id = rec.config.probe_id;
        let tail = state
            .rings
            .tail_frames(probe_id, 2, 65_536)
            .expect("tail ok");
        assert_eq!(tail.lines.len(), 2, "max_lines=2 cap enforced");
        let frame_count = state.rings.frame_count(probe_id);
        let truncated_lines = frame_count > tail.lines.len();
        assert!(
            truncated_lines,
            "3 frames but only 2 returned -> truncated_lines"
        );

        cleanup(&data);
    });
}

// F1: command_output_tail clamps max_lines to 200 server-side.
#[cfg(unix)]
#[test]
fn command_output_tail_clamps_to_200_lines() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("tail-clamp");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // seq produces 250 lines (one number per line).
        let resp = state
            .command
            .start_combed(CommandStartRequest {
                argv: vec!["/usr/bin/seq".to_owned(), "250".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace: None,
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        wait_terminal(&state, resp.job_id);

        let rec = state.jobs.get(resp.job_id).expect("job record present");
        let probe_id = rec.config.probe_id;
        // The handler clamps a caller's max_lines to MAX_TAIL_LINES=200
        // (see handle_command_output_tail). This test exercises the ring
        // at that already-clamped value to prove that asking for 200 of
        // 250 frames returns at most 200 and flags truncation. The
        // end-to-end clamp of an over-cap request is covered by the MCP
        // e2e test command_output_tail_clamps_to_200_lines.
        let tail = state
            .rings
            .tail_frames(probe_id, 200, 65_536)
            .expect("tail ok");
        assert!(
            tail.lines.len() <= 200,
            "returned_lines {} must not exceed hard cap 200",
            tail.lines.len()
        );
        let frame_count = state.rings.frame_count(probe_id);
        let truncated_lines = frame_count > tail.lines.len();
        assert!(
            truncated_lines,
            "250 frames but at most 200 returned -> truncated_lines"
        );

        cleanup(&data);
    });
}

// TCE-ERG-1 carve-out (A1): when a rule matches, the "never raw output"
// contract still holds -- no receipt tail is produced.
#[cfg(unix)]
#[test]
fn rule_match_command_has_no_receipt() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("receipt-rule");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let resp = state
            .command
            .start_combed(CommandStartRequest {
                // argv-only, no shell: printf is not in
                // SHELL_INTERPRETERS_DENY. Emits two stdout lines.
                argv: vec!["/usr/bin/printf".to_owned(), "hello\nworld\n".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![hello_rule()],
                grace: None,
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
                peer_discriminator: None,
            })
            .expect("start ok");

        wait_terminal(&state, resp.job_id);

        let status = state.command.status(resp.job_id).expect("status ok");
        assert!(
            status.receipt.is_none(),
            "a rule match must suppress the receipt tail"
        );

        cleanup(&data);
    });
}

// =====================================================================
// S1 + S4 regressions (2026-06-10 field review).
// =====================================================================

/// Child helper for the mid-run tests below, NOT a test. Spawned as
/// `current_exe helper_child_emit_then_linger --ignored --exact
/// --nocapture` so the child prints a few lines immediately and then
/// lingers long enough for the parent to observe a RUNNING job with
/// captured output. The `#[ignore]` keeps it out of normal runs.
#[test]
#[ignore = "child-process helper for the mid-run status tests, not a test"]
fn helper_child_emit_then_linger() {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    for i in 0..3 {
        let _ = writeln!(out, "LINGER_CHILD_LINE_{i}");
    }
    let _ = out.flush();
    std::thread::sleep(Duration::from_secs(10));
}

/// Spawn the linger helper through the command runtime and return the
/// start response. Cross-platform: the child is this very test binary.
fn start_linger_child(state: &DaemonState) -> terminal_commander_ipc::CommandStartResponse {
    let exe = std::env::current_exe()
        .expect("current test binary path")
        .to_string_lossy()
        .into_owned();
    state
        .command
        .start_combed(CommandStartRequest {
            argv: vec![
                exe,
                "helper_child_emit_then_linger".to_owned(),
                "--ignored".to_owned(),
                "--exact".to_owned(),
                "--nocapture".to_owned(),
            ],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        })
        .expect("start linger child")
}

/// S1: `command_status` counters must be NEAR-REAL-TIME, not exit-final.
/// The pinned failure mode: a job that had already produced output
/// reported `bytes_total: 0, frames_total: 0` mid-run (the binding's
/// `metrics` field is only populated at exit), so a polling agent
/// concluded "no output yet". The fix reads the probe's shared
/// `metrics_live` for non-terminal jobs.
#[test]
fn command_status_counters_are_live_mid_run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("live-counters");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let resp = start_linger_child(&state);

        // Poll: we must observe nonzero counters WHILE the job is still
        // running (no exit_code yet). The child lingers ~10 s after
        // printing, so a healthy capture path has ample window.
        let mut observed_live = false;
        for _ in 0..160 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = state.command.status(resp.job_id).expect("status ok");
            if status.exit_code.is_some()
                || matches!(
                    status.state,
                    JobState::Exited | JobState::Failed | JobState::Cancelled
                )
            {
                break;
            }
            if status.frames_total > 0 && status.bytes_total > 0 {
                observed_live = true;
                break;
            }
        }
        // Reap the child promptly; the assertion below is the verdict.
        let _ = state.command.stop(resp.job_id, "test-cleanup");
        assert!(
            observed_live,
            "mid-run command_status must report captured frames/bytes \
             (exit-final-only counters are the pinned S1 failure mode)"
        );

        cleanup(&data);
    });
}

#[test]
fn command_status_counters_survive_operator_cancel() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("cancel-counters");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let resp = start_linger_child(&state);

        let live = loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = state.command.status(resp.job_id).expect("status ok");
            if status.frames_total > 0 && status.bytes_total > 0 {
                break status;
            }
            assert!(
                !matches!(
                    status.state,
                    JobState::Exited | JobState::Failed | JobState::Cancelled
                ),
                "linger child terminated before emitting output: {status:?}"
            );
        };

        state
            .command
            .stop(resp.job_id, "test-cancel")
            .expect("cancel running command");
        let cancelled = state
            .command
            .status(resp.job_id)
            .expect("cancelled status remains available");

        assert_eq!(cancelled.state, JobState::Cancelled);
        assert!(
            cancelled.frames_total >= live.frames_total,
            "terminal status lost captured frames: live={live:?}, cancelled={cancelled:?}"
        );
        assert!(
            cancelled.bytes_total >= live.bytes_total,
            "terminal status lost captured bytes: live={live:?}, cancelled={cancelled:?}"
        );

        cleanup(&data);
    });
}

/// S4: live work must veto the idle self-reaper. The predicate the
/// reaper consults (`DaemonState::has_live_work`) must be true while a
/// command is still running and false once nothing is live — reaping
/// mid-job orphans the child and loses its receipt/exit event.
#[test]
fn has_live_work_tracks_running_commands() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("live-work");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        assert!(
            !state.has_live_work(),
            "fresh daemon state must report no live work"
        );

        let resp = start_linger_child(&state);
        assert!(
            state.has_live_work(),
            "a just-started (running) command is live work"
        );

        let _ = state.command.stop(resp.job_id, "test-cleanup");
        // The stop is synchronous in the job table (Cancelled is set under
        // the live lock), so the predicate must flip without polling the
        // child's actual teardown.
        let mut cleared = false;
        for _ in 0..100 {
            if !state.has_live_work() {
                cleared = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            cleared,
            "has_live_work must clear after the only job is stopped"
        );

        cleanup(&data);
    });
}
