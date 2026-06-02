// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC42b daemon-level tests for live rule rebind across running
//! command/probe streams.
//!
//! Exercises the in-place `SifterRuntime::rebuild` swap that the
//! TC42b registry IPC handlers trigger, plus the bounded
//! `CommandRuntime::rebind_all_jobs` report and audit row.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    BucketId, ContextHint, ProbeId, RuleDefinition, RuleStatus, RuleType, Severity, SourceFrame,
    SourceStream,
};
use terminal_commander_sifters::SifterRuntime;
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcRequest, IpcResponse, IpcServer, RegistryActivateParams,
    RegistryDeactivateParams, RegistryUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-rebind-{tag}-{pid}-{nanos}-{n}"));
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

fn kw_rule(id: &str, keyword: &str, event_kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity: Severity::Medium,
        event_kind: event_kind.to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec![keyword.to_owned()]),
        captures: vec![],
        summary_template: "matched".to_owned(),
        tags: vec!["tc42b".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

#[test]
fn sifter_runtime_rebuild_swaps_rule_set_in_place() {
    // Start with no rules.
    let sifter = SifterRuntime::build(&[]).unwrap();
    assert_eq!(sifter.rule_count(), 0);

    let probe = ProbeId::new();
    let bucket = BucketId::new();
    let frame_before = SourceFrame::new(probe, SourceStream::Stdout, "needle here".to_owned());
    assert!(
        sifter.evaluate(&frame_before, bucket).is_empty(),
        "no rules => no drafts"
    );

    // Swap in a keyword rule.
    let report = sifter
        .rebuild(&[kw_rule("kw-a", "needle", "needle_match")])
        .unwrap();
    assert_eq!(report.old_rule_count, 0);
    assert_eq!(report.new_rule_count, 1);
    assert_eq!(sifter.rule_count(), 1);

    // Same evaluator handle, different result. This is the TC42b
    // contract: callers holding `Arc<SifterRuntime>` see the new
    // rule set on future frames without a new probe API.
    let frame_after = SourceFrame::new(probe, SourceStream::Stdout, "needle here".to_owned());
    let drafts = sifter.evaluate(&frame_after, bucket);
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].kind, "needle_match");

    // Swap back to empty: future frames are silent again.
    let report = sifter.rebuild(&[]).unwrap();
    assert_eq!(report.old_rule_count, 1);
    assert_eq!(report.new_rule_count, 0);
    let frame_silent = SourceFrame::new(probe, SourceStream::Stdout, "needle here".to_owned());
    assert!(sifter.evaluate(&frame_silent, bucket).is_empty());
}

#[test]
fn sifter_rebuild_failure_preserves_prior_rule_set() {
    let sifter = SifterRuntime::build(&[kw_rule("kw-a", "needle", "k")]).unwrap();
    assert_eq!(sifter.rule_count(), 1);

    // Build a bogus rule (regex kind with an unclosed group). The
    // rebuild must fail and leave the prior compiled state intact.
    let mut bad = kw_rule("bad", "x", "k");
    bad.kind = RuleType::Regex;
    bad.keywords = None;
    bad.pattern = Some("(unclosed".to_owned());

    let err = sifter.rebuild(std::slice::from_ref(&bad)).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.to_ascii_lowercase().contains("regex") || msg.contains("RegexCompile"),
        "expected regex compile error, got: {msg}"
    );
    // Prior rule still fires.
    let probe = ProbeId::new();
    let bucket = BucketId::new();
    let frame = SourceFrame::new(probe, SourceStream::Stdout, "needle here".to_owned());
    assert_eq!(sifter.evaluate(&frame, bucket).len(), 1);
}

#[test]
#[allow(clippy::too_many_lines)]
fn rebind_all_jobs_after_activate_emits_audit_row_for_each_running_job() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("rebind-audit");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = terminal_commanderd::DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Upsert a keyword rule but do NOT activate it yet.
        let def = kw_rule("kw-rebind", "needle", "needle_match");
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams { definition: def }),
            )
            .await
            .expect("upsert");

        // Start two long-running commands so there are concrete
        // jobs to rebind. `sleep 1` is enough; both probes will be
        // alive when the activate call lands.
        let start = client
            .call(
                2,
                IpcRequest::CommandStartCombed(terminal_commanderd::CommandStartParams {
                    environment: None,
                    argv: vec!["sleep".to_owned(), "1".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(5_000),
                }),
            )
            .await
            .expect("start1");
        let _bucket_a = match start {
            IpcResponse::CommandStartCombed(s) => s.bucket_id,
            other => panic!("unexpected response: {other:?}"),
        };
        let start = client
            .call(
                3,
                IpcRequest::CommandStartCombed(terminal_commanderd::CommandStartParams {
                    environment: None,
                    argv: vec!["sleep".to_owned(), "1".to_owned()],
                    cwd: None,
                    env: vec![],
                    bucket_config: None,
                    rules: vec![],
                    grace_ms: Some(5_000),
                }),
            )
            .await
            .expect("start2");
        let _bucket_b = match start {
            IpcResponse::CommandStartCombed(s) => s.bucket_id,
            other => panic!("unexpected response: {other:?}"),
        };

        // Activate the rule while both commands are still running.
        client
            .call(
                4,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-rebind".to_owned(),
                    version: None,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("activate");

        // Two `command_sifter_rebind` audit rows must land — one
        // per running job.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let rebind_rows: Vec<_> = rows
            .iter()
            .filter(|r| r.action == "command_sifter_rebind")
            .collect();
        assert!(
            rebind_rows.len() >= 2,
            "expected >=2 sifter_rebind rows after activate; got {}: {rows:?}",
            rebind_rows.len()
        );

        // Deactivate -> another batch of rebind audit rows.
        client
            .call(
                5,
                IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                    rule_id: "kw-rebind".to_owned(),
                    version: 1,
                    scope: Some(terminal_commander_core::ActivationScope::Global),
                }),
            )
            .await
            .expect("deactivate");
        let rows2 = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let rebind_rows2: Vec<_> = rows2
            .iter()
            .filter(|r| r.action == "command_sifter_rebind")
            .collect();
        assert!(
            rebind_rows2.len() > rebind_rows.len(),
            "deactivate must produce additional rebind audit rows; before={}, after={}",
            rebind_rows.len(),
            rebind_rows2.len()
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
