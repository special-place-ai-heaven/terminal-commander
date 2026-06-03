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
