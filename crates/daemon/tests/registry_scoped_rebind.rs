// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! TC42c daemon-level tests for scoped registry bindings.
//!
//! Verifies that `registry_activate` / `registry_deactivate` honor
//! the supplied `ActivationScope`:
//! - bucket / job-scoped activations only reach the matching live
//!   job's sifter;
//! - global activation still reaches every live job (TC42b preserved);
//! - a scope referring to an unknown bucket/job/probe id is rejected
//!   with a typed `ScopeInvalid` error and does NOT silently widen.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_core::{
    ActivationScope, BucketId, ContextHint, JobId, ProbeId, RuleDefinition, RuleStatus, RuleType,
    Severity,
};
use terminal_commanderd::{
    CommandStartParams, DaemonClient, DaemonConfig, DaemonState, IpcErrorCode, IpcRequest,
    IpcResponse, IpcServer, RegistryActivateParams, RegistryDeactivateParams, RegistryUpsertParams,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-scoped-{tag}-{pid}-{nanos}"));
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
        tags: vec!["tc42c".to_owned()],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

fn sleeper(
    client: &DaemonClient,
    correlation: u64,
) -> impl std::future::Future<Output = IpcResponse> + '_ {
    let req = IpcRequest::CommandStartCombed(CommandStartParams {
        argv: vec!["sleep".to_owned(), "1".to_owned()],
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace_ms: Some(5_000),
    });
    async move { client.call(correlation, req).await.expect("start") }
}

#[test]
fn bucket_scoped_activation_only_rebinds_matching_job() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("bucket-only");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        // Upsert the rule but leave it inactive.
        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-scope", "needle", "needle_match"),
                }),
            )
            .await
            .expect("upsert");

        // Start two sleeping commands. Each gets its own bucket.
        let start_a = sleeper(&client, 2).await;
        let bucket_a = match start_a {
            IpcResponse::CommandStartCombed(s) => s.bucket_id,
            other => panic!("unexpected: {other:?}"),
        };
        let start_b = sleeper(&client, 3).await;
        let bucket_b = match start_b {
            IpcResponse::CommandStartCombed(s) => s.bucket_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Activate the rule with bucket scope = bucket A only.
        let resp = client
            .call(
                4,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-scope".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Bucket {
                        bucket_id: bucket_a,
                    }),
                }),
            )
            .await
            .expect("activate scoped");
        let act = match resp {
            IpcResponse::RegistryActivate(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(matches!(act.scope, ActivationScope::Bucket { .. }));
        // Exactly one live job matches the bucket scope.
        assert_eq!(act.jobs_rebound, 1, "only bucket A's job should be rebound");

        // Resolver: scoped snapshot for the matching job MUST include
        // the rule, the other bucket's job MUST NOT.
        let live = state.command.live_jobs();
        let job_a = live
            .iter()
            .find(|j| j.bucket_id == bucket_a)
            .expect("job A");
        let job_b = live
            .iter()
            .find(|j| j.bucket_id == bucket_b)
            .expect("job B");

        let rules_a =
            state
                .activation
                .snapshot_for_job(job_a.bucket_id, job_a.job_id, job_a.probe_id);
        let rules_b =
            state
                .activation
                .snapshot_for_job(job_b.bucket_id, job_b.job_id, job_b.probe_id);
        assert!(rules_a.iter().any(|d| d.id == "kw-scope"));
        assert!(
            rules_b.iter().all(|d| d.id != "kw-scope"),
            "bucket B must NOT see the bucket-A-scoped rule"
        );

        // Global snapshot also dedupes; it should contain the rule
        // once since it is active under one scope.
        let global = state.activation.snapshot();
        assert!(global.iter().any(|d| d.id == "kw-scope"));

        // Deactivate the same scope. Job A's resolved set loses the
        // rule; job B was never affected.
        let resp = client
            .call(
                5,
                IpcRequest::RegistryDeactivate(RegistryDeactivateParams {
                    rule_id: "kw-scope".to_owned(),
                    version: 1,
                    scope: Some(ActivationScope::Bucket {
                        bucket_id: bucket_a,
                    }),
                }),
            )
            .await
            .expect("deactivate scoped");
        let dr = match resp {
            IpcResponse::RegistryDeactivate(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(dr.was_deactivated);
        let rules_a_after =
            state
                .activation
                .snapshot_for_job(job_a.bucket_id, job_a.job_id, job_a.probe_id);
        assert!(rules_a_after.iter().all(|d| d.id != "kw-scope"));

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn job_scoped_activation_only_rebinds_matching_job() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("job-only");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-job", "needle", "needle_match"),
                }),
            )
            .await
            .expect("upsert");

        // Start two sleeping commands.
        let start_a = sleeper(&client, 2).await;
        let job_a_id = match start_a {
            IpcResponse::CommandStartCombed(s) => s.job_id,
            other => panic!("unexpected: {other:?}"),
        };
        let start_b = sleeper(&client, 3).await;
        let _job_b_id = match start_b {
            IpcResponse::CommandStartCombed(s) => s.job_id,
            other => panic!("unexpected: {other:?}"),
        };

        // Scope by job_id = A.
        let resp = client
            .call(
                4,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-job".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Job { job_id: job_a_id }),
                }),
            )
            .await
            .expect("activate job-scoped");
        let act = match resp {
            IpcResponse::RegistryActivate(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert_eq!(act.jobs_rebound, 1);

        let live = state.command.live_jobs();
        let job_a = live.iter().find(|j| j.job_id == job_a_id).unwrap();
        let job_b = live.iter().find(|j| j.job_id != job_a_id).unwrap();

        let rules_a =
            state
                .activation
                .snapshot_for_job(job_a.bucket_id, job_a.job_id, job_a.probe_id);
        let rules_b =
            state
                .activation
                .snapshot_for_job(job_b.bucket_id, job_b.job_id, job_b.probe_id);
        assert!(rules_a.iter().any(|d| d.id == "kw-job"));
        assert!(rules_b.iter().all(|d| d.id != "kw-job"));

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn global_activation_still_reaches_every_live_job() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("global");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-global", "needle", "needle_match"),
                }),
            )
            .await
            .expect("upsert");

        let _ = sleeper(&client, 2).await;
        let _ = sleeper(&client, 3).await;

        let resp = client
            .call(
                4,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-global".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Global),
                }),
            )
            .await
            .expect("activate global");
        let act = match resp {
            IpcResponse::RegistryActivate(r) => r,
            other => panic!("unexpected: {other:?}"),
        };
        assert!(matches!(act.scope, ActivationScope::Global));
        assert_eq!(act.jobs_rebound, 2, "global must reach both live jobs");

        for j in state.command.live_jobs() {
            let rules = state
                .activation
                .snapshot_for_job(j.bucket_id, j.job_id, j.probe_id);
            assert!(rules.iter().any(|d| d.id == "kw-global"));
        }

        handle.shutdown().await;
        cleanup(&data);
    });
}

#[test]
fn unknown_scope_id_is_rejected_with_typed_error() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("bad-scope");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
        let socket = state.config.socket_path();
        let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(2));

        client
            .call(
                1,
                IpcRequest::RegistryUpsert(RegistryUpsertParams {
                    definition: kw_rule("kw-bad", "needle", "needle_match"),
                }),
            )
            .await
            .expect("upsert");

        // Bucket scope referencing a freshly-generated id that does
        // NOT belong to any running command. Must be rejected with a
        // typed ScopeInvalid; the in-memory registry MUST remain
        // empty for this rule.
        let bogus_bucket = BucketId::new();
        let err = client
            .call(
                2,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-bad".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Bucket {
                        bucket_id: bogus_bucket,
                    }),
                }),
            )
            .await
            .expect_err("activate must reject unknown bucket");
        assert_eq!(err.code, IpcErrorCode::ScopeInvalid);

        // Same check for an unknown job id.
        let bogus_job = JobId::new();
        let err = client
            .call(
                3,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-bad".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Job { job_id: bogus_job }),
                }),
            )
            .await
            .expect_err("activate must reject unknown job");
        assert_eq!(err.code, IpcErrorCode::ScopeInvalid);

        // Same check for an unknown probe id.
        let bogus_probe = ProbeId::new();
        let err = client
            .call(
                4,
                IpcRequest::RegistryActivate(RegistryActivateParams {
                    rule_id: "kw-bad".to_owned(),
                    version: None,
                    scope: Some(ActivationScope::Probe {
                        probe_id: bogus_probe,
                    }),
                }),
            )
            .await
            .expect_err("activate must reject unknown probe");
        assert_eq!(err.code, IpcErrorCode::ScopeInvalid);

        // In-memory registry MUST NOT carry the rule under any scope.
        assert!(state.activation.is_empty());

        handle.shutdown().await;
        cleanup(&data);
    });
}
