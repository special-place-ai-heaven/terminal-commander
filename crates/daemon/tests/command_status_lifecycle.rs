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
                receipt_shape: None,
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
                receipt_shape: None,
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
                receipt_shape: None,
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
        assert_eq!(receipt.lines_omitted, 0, "the tail shows every line");

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
                receipt_shape: None,
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
                receipt_shape: None,
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
                receipt_shape: None,
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
            receipt_shape: None,
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

/// Child helpers for the `lines_omitted` receipt tests below, NOT tests.
/// Each prints its lines and exits before libtest prints its result
/// lines, so the captured output is the harness banner plus these lines.
fn emit_lines_then_exit(n: usize) {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    for i in 0..n {
        let _ = writeln!(out, "RECEIPT_CHILD_LINE_{i}");
    }
    let _ = out.flush();
    std::process::exit(0);
}

#[test]
#[ignore = "child-process helper for the receipt tests, not a test"]
fn helper_child_emit_ten_lines() {
    emit_lines_then_exit(10);
}

#[test]
#[ignore = "child-process helper for the receipt tests, not a test"]
fn helper_child_emit_one_line() {
    emit_lines_then_exit(1);
}

/// Run a self-exec helper with no rules and return its no-silence receipt.
async fn no_rule_receipt(helper: &str) -> terminal_commander_ipc::CommandReceipt {
    let data = tmp_data_dir("receipt-omitted");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).unwrap();
    let exe = std::env::current_exe()
        .expect("current test binary path")
        .to_string_lossy()
        .into_owned();
    let resp = state
        .command
        .start_combed(CommandStartRequest {
            argv: vec![
                exe,
                helper.to_owned(),
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
            receipt_shape: None,
            strip_ansi: true,
            peer_discriminator: None,
        })
        .expect("start ok");
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if matches!(
            state.command.job_record(resp.job_id).map(|r| r.state),
            Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
        ) {
            break;
        }
    }
    let status = state.command.status(resp.job_id).expect("status ok");
    cleanup(&data);
    status
        .receipt
        .expect("zero-rule run must carry a no-silence receipt")
}

/// `lines_omitted` says how much output the 5-line tail does NOT show, so
/// a quiet receipt cannot be misread as the whole output. Distinct from
/// `tail_incomplete` (the ring itself lost frames), which stays false here.
#[test]
fn receipt_reports_lines_the_tail_omits() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let receipt = no_rule_receipt("helper_child_emit_ten_lines").await;
        // The self-exec child also prints libtest's banner, so the frame
        // count is 10 plus a few; the relation is what is pinned.
        assert!(receipt.lines_suppressed >= 10, "{receipt:?}");
        assert_eq!(receipt.tail.len(), 5, "{receipt:?}");
        assert_eq!(
            receipt.tail.last().map(String::as_str),
            Some("RECEIPT_CHILD_LINE_9"),
            "{receipt:?}"
        );
        assert_eq!(
            receipt.lines_omitted,
            receipt.lines_suppressed - 5,
            "{receipt:?}"
        );
        assert!(!receipt.tail_incomplete, "{receipt:?}");

        let short = no_rule_receipt("helper_child_emit_one_line").await;
        assert!(short.lines_suppressed <= 5, "{short:?}");
        assert_eq!(short.tail.len() as u64, short.lines_suppressed, "{short:?}");
        assert_eq!(short.lines_omitted, 0, "{short:?}");
    });
}

#[test]
#[ignore = "child-process helper for the receipt tests, not a test"]
fn helper_child_emit_hundred_lines() {
    emit_lines_then_exit(100);
}

/// Prints past the default 4096-frame ring so the earliest output is evicted.
#[test]
#[ignore = "child-process helper for the receipt tests, not a test"]
fn helper_child_emit_past_the_ring() {
    emit_lines_then_exit(5000);
}

fn self_exec_argv(helper: &str) -> Vec<String> {
    let exe = std::env::current_exe()
        .expect("current test binary path")
        .to_string_lossy()
        .into_owned();
    vec![
        exe,
        helper.to_owned(),
        "--ignored".to_owned(),
        "--exact".to_owned(),
        "--nocapture".to_owned(),
    ]
}

/// Run `argv` with no rules and the given receipt shape. Returns the
/// receipt plus every line still retained in the ring, so a test can
/// check positions without knowing the exact frame count.
async fn shaped_receipt(
    argv: Vec<String>,
    shape: terminal_commander_ipc::ReceiptShape,
) -> (terminal_commander_ipc::CommandReceipt, Vec<String>) {
    let data = tmp_data_dir("receipt-shape");
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).unwrap();
    let resp = state
        .command
        .start_combed(CommandStartRequest {
            argv,
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            receipt_shape: Some(shape),
            strip_ansi: true,
            peer_discriminator: None,
        })
        .expect("start ok");
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if matches!(
            state.command.job_record(resp.job_id).map(|r| r.state),
            Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
        ) {
            break;
        }
    }
    let status = state.command.status(resp.job_id).expect("status ok");
    let probe_id = state.jobs.get(resp.job_id).expect("job").config.probe_id;
    let retained = state
        .rings
        .tail_frames(probe_id, usize::MAX, usize::MAX)
        .expect("ring")
        .lines;
    cleanup(&data);
    let receipt = status
        .receipt
        .expect("zero-rule run must carry a no-silence receipt");
    (receipt, retained)
}

const fn shape(head_lines: u32, tail_lines: u32) -> terminal_commander_ipc::ReceiptShape {
    terminal_commander_ipc::ReceiptShape {
        head_lines,
        tail_lines,
    }
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// Head = first H lines, tail = last T lines, `lines_omitted` = the gap.
#[test]
fn receipt_head_and_tail_split_the_output() {
    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_ten_lines"),
        shape(3, 4),
    ));
    let n = all.len();
    assert_eq!(r.lines_suppressed, n as u64, "{r:?}");
    assert_eq!(r.head, all[..3], "{r:?}");
    assert_eq!(r.tail, all[n - 4..], "{r:?}");
    assert_eq!(
        r.tail.last().map(String::as_str),
        Some("RECEIPT_CHILD_LINE_9")
    );
    assert_eq!(r.lines_omitted, n as u64 - 7, "{r:?}");
    assert!(!r.tail_incomplete, "{r:?}");
}

/// A tail longer than what follows the head shrinks: no line is shown twice.
#[test]
fn receipt_tail_never_overlaps_the_head() {
    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_ten_lines"),
        shape(3, 50),
    ));
    assert_eq!(r.head, all[..3], "{r:?}");
    assert_eq!(r.tail, all[3..], "{r:?}");
    assert_eq!(r.lines_omitted, 0, "{r:?}");
}

/// Out-of-range counts clamp (head 20, tail 50); a zero tail is allowed.
#[test]
fn receipt_shape_clamps_and_allows_an_empty_tail() {
    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_hundred_lines"),
        shape(999, 999),
    ));
    let n = all.len();
    assert_eq!(r.head, all[..20], "{r:?}");
    assert_eq!(r.tail, all[n - 50..], "{r:?}");
    assert_eq!(r.lines_omitted, n as u64 - 70, "{r:?}");

    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_ten_lines"),
        shape(0, 0),
    ));
    assert!(r.head.is_empty() && r.tail.is_empty(), "{r:?}");
    assert_eq!(r.lines_omitted, all.len() as u64, "{r:?}");
}

/// Once the ring has evicted the earliest output the true head is gone:
/// no head is shown (later lines never stand in for it) and the receipt
/// says the window is lossy.
#[test]
fn receipt_head_is_empty_when_the_start_was_evicted() {
    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_past_the_ring"),
        shape(3, 5),
    ));
    let n = all.len();
    assert!(r.head.is_empty(), "{:?}", r.head);
    assert!(r.tail_incomplete);
    assert_eq!(r.tail, all[n - 5..]);
    assert_eq!(
        r.tail.last().map(String::as_str),
        Some("RECEIPT_CHILD_LINE_4999")
    );
    assert!(
        r.lines_suppressed > n as u64,
        "ring must have evicted frames"
    );
    assert_eq!(r.lines_omitted, r.lines_suppressed - 5);
}

#[test]
#[ignore = "child-process helper for the receipt tests, not a test"]
fn helper_child_emit_long_lines() {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    for i in 0..10 {
        let _ = writeln!(out, "LONG_{i}_{}", "x".repeat(3000));
    }
    let _ = out.flush();
    std::process::exit(0);
}

/// Head and tail share ONE 4096-byte raw-text budget (constitution III):
/// long lines are dropped rather than overrunning it, and the receipt says
/// the shown window is lossy.
#[test]
fn receipt_head_and_tail_share_one_byte_budget() {
    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_long_lines"),
        shape(3, 5),
    ));
    let bytes: usize = r.head.iter().chain(&r.tail).map(String::len).sum();
    assert!(bytes <= 4096, "{bytes} bytes shown: {r:?}");
    assert!(r.tail_incomplete, "{r:?}");
    assert!(!r.tail.is_empty(), "{r:?}");
    let n = all.len();
    assert_eq!(r.head, all[..r.head.len()]);
    assert_eq!(r.tail, all[n - r.tail.len()..]);
    assert_eq!(
        r.lines_omitted,
        (n - r.head.len() - r.tail.len()) as u64,
        "{r:?}"
    );
}

#[test]
#[ignore = "child-process helper for the receipt tests, not a test"]
fn helper_child_emit_thirty_400_byte_lines() {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    for i in 0..30 {
        let _ = writeln!(out, "L{i:02}_{}", "y".repeat(396));
    }
    let _ = out.flush();
    std::process::exit(0);
}

/// Max line counts on ~400-byte lines: the shared 4096-byte budget, not the
/// line caps, is what bounds the receipt.
#[test]
fn receipt_max_lines_stay_within_the_shared_byte_budget() {
    let (r, all) = block_on(shaped_receipt(
        self_exec_argv("helper_child_emit_thirty_400_byte_lines"),
        shape(20, 50),
    ));
    let head_bytes: usize = r.head.iter().map(String::len).sum();
    let tail_bytes: usize = r.tail.iter().map(String::len).sum();
    assert!(head_bytes <= 2048, "head {head_bytes} bytes: {r:?}");
    assert!(
        head_bytes + tail_bytes <= 4096,
        "{head_bytes}+{tail_bytes} bytes"
    );
    let n = all.len();
    assert_eq!(r.head, all[..r.head.len()]);
    assert_eq!(r.tail, all[n - r.tail.len()..]);
    assert!(
        r.head.len() + r.tail.len() < n,
        "budget must have cut lines"
    );
    // Lines the line caps asked for were dropped by the byte budget.
    assert!(r.tail_incomplete);
    assert_eq!(
        r.lines_omitted,
        (n - r.head.len() - r.tail.len()) as u64,
        "{r:?}"
    );
}

/// Exact counts with a plain argv producer (no libtest banner).
#[cfg(unix)]
#[test]
fn receipt_head_and_tail_exact_counts() {
    let lines = |n: usize| -> Vec<String> {
        let body: String = (1..=n).map(|i| i.to_string() + "\n").collect();
        vec!["/usr/bin/printf".to_owned(), body]
    };
    let (r, _) = block_on(shaped_receipt(lines(10), shape(3, 4)));
    assert_eq!(r.head, ["1", "2", "3"]);
    assert_eq!(r.tail, ["7", "8", "9", "10"]);
    assert_eq!(r.lines_omitted, 3);

    let (r, _) = block_on(shaped_receipt(lines(6), shape(3, 5)));
    assert_eq!(r.head, ["1", "2", "3"]);
    assert_eq!(r.tail, ["4", "5", "6"]);
    assert_eq!(r.lines_omitted, 0);
}

/// Liveness: a RUNNING job reports how long it has run and how long ago it
/// last produced output, so a clockless poller can tell quiet-but-alive
/// from stalled. Both drop out once terminal (`duration_ms` takes over).
#[test]
fn command_status_reports_liveness_while_running() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("liveness");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let resp = start_linger_child(&state);

        let mut live = None;
        for _ in 0..160 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = state.command.status(resp.job_id).expect("status ok");
            if !matches!(status.state, JobState::Starting | JobState::Running) {
                break;
            }
            if status.frames_total > 0 {
                live = Some(status);
                break;
            }
        }
        let _ = state.command.stop(resp.job_id, "test-cleanup");
        let done = state.command.status(resp.job_id).expect("status ok");
        let live = live.expect("linger child must be observed running with output");

        assert!(live.elapsed_ms.is_some(), "{live:?}");
        assert!(live.last_output_age_ms.is_some(), "{live:?}");
        assert!(done.elapsed_ms.is_none(), "{done:?}");
        assert!(done.last_output_age_ms.is_none(), "{done:?}");
        assert!(done.duration_ms.is_some(), "{done:?}");

        cleanup(&data);
    });
}

/// A running job that has printed nothing reports `elapsed_ms` but no
/// `last_output_age_ms` -- absent means "no output captured yet".
#[test]
fn command_status_silent_running_job_has_no_output_age() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("liveness-silent");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        // Silent while waiting; both are stopped long before they time out.
        #[cfg(unix)]
        let argv = vec!["sleep".to_owned(), "30".to_owned()];
        #[cfg(windows)]
        let argv = vec![
            "waitfor.exe".to_owned(),
            "/t".to_owned(),
            "30".to_owned(),
            format!("TcSilentLinger{}", std::process::id()),
        ];
        let resp = state
            .command
            .start_combed(CommandStartRequest {
                argv,
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

        let mut running = None;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let status = state.command.status(resp.job_id).expect("status ok");
            if status.state == JobState::Running {
                running = Some(status);
                break;
            }
        }
        let _ = state.command.stop(resp.job_id, "test-cleanup");
        let running = running.expect("silent child must be observed running");

        assert!(running.elapsed_ms.is_some(), "{running:?}");
        assert_eq!(running.frames_total, 0, "{running:?}");
        assert!(running.last_output_age_ms.is_none(), "{running:?}");

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
