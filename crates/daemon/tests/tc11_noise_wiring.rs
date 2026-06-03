// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC11 integration: progress pre-sift, dedupe collapse, suppression counters.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use terminal_commander_core::{BucketReadRequest, JobState, Severity, SourceStream};
use terminal_commanderd::{CommandStartRequest, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc11-noise-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn wait_terminal(state: &DaemonState, job_id: terminal_commander_core::JobId) {
    for _ in 0..80 {
        std::thread::sleep(Duration::from_millis(50));
        if matches!(
            state.command.job_record(job_id).map(|r| r.state),
            Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
        ) {
            return;
        }
    }
    panic!("job {job_id} did not reach terminal state");
}

fn repeat_rule() -> terminal_commander_core::RuleDefinition {
    use terminal_commander_core::{ContextHint, RuleStatus, RuleType};
    terminal_commander_core::RuleDefinition {
        id: "tc11.repeat".to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Low,
        event_kind: "repeat_hit".to_owned(),
        stream: Some(SourceStream::Stdout),
        description: None,
        pattern: None,
        keywords: Some(vec!["REPEAT_ME".to_owned()]),
        captures: vec![],
        summary_template: "repeat".to_owned(),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[test]
fn progress_only_stdout_suppresses_before_evaluate() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("progress");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let resp = state
            .command
            .start_combed(CommandStartRequest {
                argv: vec![
                    "python3".to_owned(),
                    "-c".to_owned(),
                    "import time\nfor _ in range(30):\n print('45%')\n time.sleep(0.05)\n"
                        .to_owned(),
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

        let status = state.command.status(resp.job_id).expect("status");
        assert!(
            status.frames_suppressed_progress > 0,
            "progress lines must be suppressed: {status:?}"
        );
        assert_eq!(
            status.frames_suppressed,
            status.frames_suppressed_progress + status.frames_suppressed_dedupe
        );
        // Only lifecycle event (no rule matches on progress-only output).
        assert_eq!(status.events_emitted, 1, "lifecycle only: {status:?}");

        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read");
        assert!(
            !bread.events.iter().any(|e| !e.kind.starts_with("command_")),
            "no rule-driven bucket events"
        );

        cleanup(&data);
    });
}

#[test]
fn dedupe_collapses_repeated_matches_within_window() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let data = tmp_data_dir("dedupe");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let resp = state
            .command
            .start_combed(CommandStartRequest {
                argv: vec![
                    "python3".to_owned(),
                    "-c".to_owned(),
                    "import time\nfor _ in range(20):\n print('REPEAT_ME')\n time.sleep(0.1)\n"
                        .to_owned(),
                ],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![repeat_rule()],
                grace: None,
                tag: None,
            })
            .expect("start ok");

        wait_terminal(&state, resp.job_id);

        let status = state.command.status(resp.job_id).expect("status");
        assert!(
            status.frames_suppressed_dedupe > 0,
            "dedupe must collapse repeats: {status:?}"
        );
        // One rule-driven emit + lifecycle `command_exited`.
        assert_eq!(
            status.events_emitted, 2,
            "deduped rule matches collapse to one emit: {status:?}"
        );

        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read");
        let hits: Vec<_> = bread
            .events
            .iter()
            .filter(|e| e.kind == "repeat_hit")
            .collect();
        assert_eq!(hits.len(), 1, "one representative repeat_hit event");
        let hit = &hits[0];
        assert!(
            hit.count > 1,
            "operator-visible count must reflect collapsed repeats: {hit:?}"
        );

        cleanup(&data);
    });
}

// PTY `password_prompt` dedupe bypass: `crates/probes/src/noise_pipeline.rs`
// (`password_prompt_bypasses_dedupe`) and `crates/daemon/tests/pty_ipc.rs`.
