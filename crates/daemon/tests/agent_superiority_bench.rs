// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Agent-ergonomics superiority benchmark.
//!
//! Reproducible, committed proof that Terminal Commander delivers a
//! higher benefit than raw shell capture for the canonical agent task:
//! "run a noisy command and surface only the signal (an error)".
//!
//! Trust comes from proven, measurable benefit. This test runs ONE
//! noisy command through the real daemon + sifter pipeline and prints,
//! with `--nocapture`, the hard numbers an agent actually pays:
//!
//!   * RAW  cost  = every stdout/stderr byte an agent ingests when it
//!                  captures the command output itself (the shell norm).
//!   * SIGNAL cost = the bytes of the matched events the agent ingests
//!                  through Terminal Commander's bucket instead.
//!
//! The reduction ratio is the token-saving an agent gains. The test
//! also asserts hard floors so the benefit cannot silently regress.
//!
//! Run:
//!   cargo test -p terminal-commanderd --test agent_superiority_bench \
//!       -- --nocapture
//!
//! Unix-only: the IPC surface is UDS (`#![cfg(unix)]`).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{
    ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
};
use terminal_commanderd::{
    BucketEventsSinceParams, BucketWaitParams, CommandStartParams, DaemonClient, DaemonConfig,
    DaemonState, IpcRequest, IpcResponse, IpcServer, RegistryActivateParams, RegistryUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-bench-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn python3_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A regex rule matching the single error line. status=Active so it is
/// runtime-eligible (the draft-poison gate would otherwise reject it).
fn error_rule() -> RuleDefinition {
    RuleDefinition {
        id: "bench-error".to_owned(),
        version: 1,
        kind: RuleType::Regex,
        status: RuleStatus::Active,
        severity: Severity::High,
        event_kind: "error_seen".to_owned(),
        stream: Some(SourceStream::Stderr),
        description: Some("surface the buried error line".to_owned()),
        pattern: Some("(?i)error".to_owned()),
        keywords: None,
        captures: vec![],
        summary_template: "error detected".to_owned(),
        tags: vec!["bench".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// Canonical noisy task: `noise_lines` of ~100-byte stdout noise plus a
/// single error line on stderr. Mirrors the real-world "long, noisy job
/// with one thing you care about" an agent faces.
fn noisy_argv(noise_lines: u32) -> Vec<String> {
    let py = format!(
        r#"
import sys
N = {noise_lines}
for i in range(N):
    print(f"info: processing record {{i:08d}} of {{N}} " + "ok " * 24, flush=False)
print("error: checksum mismatch on record 347", file=sys.stderr, flush=True)
sys.stdout.flush()
"#
    );
    vec!["python3".to_owned(), "-u".to_owned(), "-c".to_owned(), py]
}

/// Cursor adversarial-review findings #1 (live-poison heal) and #6
/// (inline draft rule must not hard-fail the start). A non-eligible
/// rule reaching the command path must be SKIPPED by
/// `merge_active_and_inline`, letting the command run, instead of
/// failing the whole start with `SifterError::NotActive` -> `Internal`.
#[test]
fn draft_inline_rule_is_skipped_not_fatal_to_command_start() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("inlinedraft");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(20));

        // An Active rule that WILL fire + a Draft rule that must be
        // skipped rather than poisoning the start.
        let active = error_rule();
        let mut draft = error_rule();
        draft.id = "bench-draft-inline".to_owned();
        draft.status = RuleStatus::Draft;

        let resp = client
            .call(
                1,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(50),
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![active, draft],
                    grace_ms: Some(30_000),
                }),
            )
            .await
            .expect("command_start must SUCCEED despite a Draft inline rule");
        let started = match resp {
            IpcResponse::CommandStartCombed(r) => r,
            other => panic!("unexpected: {other:?}"),
        };

        // The Active rule still fires; the command runs to completion.
        let mut cursor: u64 = 0;
        let mut errors_seen = 0u32;
        let mut exited = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !exited && Instant::now() < deadline {
            let r = match client
                .call(
                    2,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(1_000),
                    }),
                )
                .await
                .expect("bucket_wait")
            {
                IpcResponse::BucketWait(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            for e in &r.events {
                match e.kind.as_str() {
                    "error_seen" => errors_seen += 1,
                    "command_exited" | "command_failed" => exited = true,
                    _ => {}
                }
            }
            cursor = r.next_cursor;
        }
        assert!(
            exited,
            "command must run to completion, not fail at sifter build"
        );
        assert_eq!(errors_seen, 1, "the Active rule must still fire");

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
#[allow(clippy::too_many_lines)]
fn tc_signal_cost_is_orders_of_magnitude_below_raw_shell_cost() {
    if !python3_available() {
        eprintln!("skipping: python3 not on PATH");
        return;
    }
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("superiority");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(20));

        // --- The RAW shell baseline an agent would otherwise pay. ---
        // ~100 bytes/line: 5000 lines ~= 500 KB the agent must ingest
        // and scan itself to find one error line.
        let noise_lines = 5_000u32;
        let raw_bytes_estimate: usize = {
            // Measure the real raw size by running the same program and
            // counting bytes, so the baseline is empirical, not assumed.
            let argv = noisy_argv(noise_lines);
            let out = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .output()
                .expect("run noisy program for raw baseline");
            out.stdout.len() + out.stderr.len()
        };

        // --- The Terminal Commander signal path. ---
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: error_rule(),
                }),
            )
            .await
            .expect("upsert");
        client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "bench-error".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");

        let started = match client
            .call(
                3,
                IpcRequest::CommandStartCombed(CommandStartParams {
                    environment: None,
                    argv: noisy_argv(noise_lines),
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(30_000),
                }),
            )
            .await
            .expect("command_start_combed")
        {
            IpcResponse::CommandStartCombed(r) => r,
            other => panic!("unexpected: {other:?}"),
        };

        // Drain until exit. Count signal bytes the agent actually reads.
        let mut cursor: u64 = 0;
        let mut signal_bytes: usize = 0;
        let mut signal_events: usize = 0;
        let mut errors_seen: u32 = 0;
        let mut command_exited = false;
        let deadline = Instant::now() + Duration::from_secs(20);
        while !command_exited && Instant::now() < deadline {
            let resp = client
                .call(
                    4,
                    IpcRequest::BucketWait(BucketWaitParams {
                        bucket_id: started.bucket_id,
                        cursor,
                        severity_min: None,
                        kind_filter: None,
                        limit: None,
                        timeout_ms: Some(1_000),
                    }),
                )
                .await
                .expect("bucket_wait");
            let r = match resp {
                IpcResponse::BucketWait(r) => r,
                other => panic!("unexpected: {other:?}"),
            };
            for e in &r.events {
                signal_events += 1;
                signal_bytes += serde_json::to_string(e).map_or(0, |s| s.len());
                match e.kind.as_str() {
                    "error_seen" => errors_seen += 1,
                    "command_exited" | "command_failed" => command_exited = true,
                    _ => {}
                }
            }
            cursor = r.next_cursor;
        }

        // Also measure the one-shot full drain an agent might do instead.
        let full = match client
            .call(
                5,
                IpcRequest::BucketEventsSince(BucketEventsSinceParams {
                    bucket_id: started.bucket_id,
                    cursor: 0,
                    severity_min: None,
                    kind_filter: None,
                    limit: None,
                }),
            )
            .await
            .expect("bucket_events_since")
        {
            IpcResponse::BucketEventsSince(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        let full_drain_bytes = serde_json::to_string(&full.events).map_or(0, |s| s.len());

        // ~4 chars per token is the standard rough heuristic.
        let raw_tokens = raw_bytes_estimate / 4;
        let signal_tokens = signal_bytes.max(full_drain_bytes) / 4;
        // Precision loss is irrelevant for a token-ratio readout.
        #[allow(clippy::cast_precision_loss)]
        let reduction = if signal_bytes == 0 {
            f64::INFINITY
        } else {
            raw_bytes_estimate as f64 / signal_bytes as f64
        };

        eprintln!("==== AGENT SUPERIORITY BENCHMARK (canonical noisy task) ====");
        eprintln!("noise lines emitted          : {noise_lines}");
        eprintln!("RAW shell bytes (agent reads): {raw_bytes_estimate}");
        eprintln!("RAW approx tokens            : {raw_tokens}");
        eprintln!("TC signal events             : {signal_events}");
        eprintln!("TC signal bytes (agent reads): {signal_bytes}");
        eprintln!("TC full-drain bytes          : {full_drain_bytes}");
        eprintln!("TC approx tokens             : {signal_tokens}");
        eprintln!("errors surfaced              : {errors_seen}");
        eprintln!("REDUCTION (raw / signal)     : {reduction:.1}x");
        eprintln!("steps to answer: shell=1 capture-all + manual scan; TC=bounded signal read");
        eprintln!("============================================================");

        // Hard floors so the proven benefit cannot silently regress.
        assert!(command_exited, "command did not exit within deadline");
        assert_eq!(errors_seen, 1, "must surface exactly the one error signal");
        assert!(
            raw_bytes_estimate > 400_000,
            "raw baseline should be ~0.5 MB; got {raw_bytes_estimate}"
        );
        assert!(
            signal_bytes < raw_bytes_estimate / 50,
            "TC signal cost ({signal_bytes}) must be < 1/50th of raw ({raw_bytes_estimate}); \
             trust requires a large, proven reduction"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
