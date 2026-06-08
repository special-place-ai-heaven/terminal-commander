// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! TC-5 live IPC tests for `handle_self_check`'s REAL spawn probe.
//!
//! The IPC `self_check` no longer returns a hardcoded false-green: it runs a
//! profile-gated `command_start_combed` round-trip into ONE cached immortal
//! self-check bucket, polled to terminal state. These tests stand up the real
//! UDS server in a temp dir (NEVER the live daemon) and exercise:
//!
//! 1. DeveloperLocal healthy: `failures == 0`, the report carries a
//!    `spawn probe: ok` line, and a `command_start`/`allow` audit row landed
//!    (the spawn really happened).
//! 2. bucket reuse: across two self_checks `bucket_count` grows by AT MOST 1
//!    (the cached bucket is reused).
//! 3. dedup distinct jobs: two back-to-back self_checks produce TWO distinct
//!    `command_start` rows (distinct job_id wire strings) -- the fresh nonce
//!    defeats the TC-2 in-flight dedup collapse.
//! 4. read_only_observer SKIP: `failures == 0` AND the report contains
//!    `spawn probe skipped:` -- a denied policy is never a false RED.
//! 5. negative: the extracted `selfcheck_spawn_probe` helper, called directly
//!    with a nonexistent-but-policy-allowed binary, reports `failed == true`.
//!
//! Linux/WSL only (UDS).

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse, IpcServer, PolicyProfile,
    ServerHandle,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-selfcheck-{tag}-{pid}-{nanos}-{n}"));
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

/// Build a TEST server on its own temp data dir + socket. The optional
/// `profile` overrides the default DeveloperLocal so the SKIP path can be
/// exercised.
fn build_server_with_profile(
    data: &std::path::Path,
    profile: PolicyProfile,
) -> (Arc<DaemonState>, ServerHandle) {
    let mut cfg = DaemonConfig::defaults_in(data);
    cfg.policy.profile = profile;
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let socket = state.config.socket_path();
    let handle = IpcServer::new(Arc::clone(&state), socket).spawn().unwrap();
    (state, handle)
}

fn build_server(data: &std::path::Path) -> (Arc<DaemonState>, ServerHandle) {
    build_server_with_profile(data, PolicyProfile::DeveloperLocal)
}

/// Issue a SelfCheck IPC call and return the typed response.
async fn call_self_check(client: &DaemonClient, id: u64) -> terminal_commanderd::SelfCheckResponse {
    let resp = client
        .call(id, IpcRequest::SelfCheck)
        .await
        .expect("self_check call");
    match resp {
        IpcResponse::SelfCheck(r) => r,
        other => panic!("unexpected response: {other:?}"),
    }
}

/// Test 1 -- DeveloperLocal healthy. A real spawn happens: `failures == 0`,
/// the report carries an `ok` probe line, and a `command_start`/`allow` audit
/// row landed.
#[test]
fn self_check_developer_local_spawns_and_reports_healthy() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("healthy");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let r = call_self_check(&client, 1).await;
        assert_eq!(
            r.failures, 0,
            "healthy self_check must report 0 failures: {}",
            r.report
        );
        assert!(
            r.report.contains("spawn probe: ok"),
            "report must carry an ok probe line: {}",
            r.report
        );

        // The spawn really happened: a command_start/allow row for the probe.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut saw_allow = false;
        while std::time::Instant::now() < deadline {
            let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
            if rows
                .iter()
                .any(|r| r.action == "command_start" && r.decision == "allow")
            {
                saw_allow = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            saw_allow,
            "self_check spawn must land a command_start/allow audit row"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Test 2 -- bucket reuse. Two self_checks must grow `bucket_count` by AT MOST
/// one: the first probe creates + caches the immortal bucket, the second
/// reuses it.
#[test]
fn self_check_reuses_one_bucket_across_calls() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("reuse");
        let (_state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let before = {
            let resp = client
                .call(
                    1,
                    IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
                )
                .await
                .expect("runtime_state");
            match resp {
                IpcResponse::RuntimeState(r) => r.bucket_count,
                other => panic!("unexpected: {other:?}"),
            }
        };

        let r1 = call_self_check(&client, 2).await;
        let r2 = call_self_check(&client, 3).await;
        assert_eq!(r1.failures, 0, "first self_check failed: {}", r1.report);
        assert_eq!(r2.failures, 0, "second self_check failed: {}", r2.report);

        let after = {
            let resp = client
                .call(
                    4,
                    IpcRequest::RuntimeState(terminal_commanderd::ListLimitParams::default()),
                )
                .await
                .expect("runtime_state");
            match resp {
                IpcResponse::RuntimeState(r) => r.bucket_count,
                other => panic!("unexpected: {other:?}"),
            }
        };

        assert!(
            after <= before + 1,
            "two self_checks must add at most ONE bucket (cached reuse): before={before} after={after}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Test 3 -- dedup distinct jobs. Two back-to-back self_checks within the
/// dedup window must spawn TWO distinct jobs: a fresh per-call nonce defeats
/// the TC-2 in-flight collapse. We read the `command_start`/`allow` rows and
/// assert two DISTINCT job_id wire strings (the row subject).
#[test]
fn self_check_back_to_back_spawns_distinct_jobs() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("dedup");
        let (state, handle) = build_server(&data);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        // Two self_checks back-to-back (well within any dedup TTL window).
        let r1 = call_self_check(&client, 1).await;
        let r2 = call_self_check(&client, 2).await;
        assert_eq!(r1.failures, 0, "{}", r1.report);
        assert_eq!(r2.failures, 0, "{}", r2.report);

        // Poll the audit log until at least two distinct command_start/allow
        // subjects (job_id wire strings) appear.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut distinct_jobs: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::default();
        while std::time::Instant::now() < deadline {
            let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
            distinct_jobs = rows
                .iter()
                .filter(|r| r.action == "command_start" && r.decision == "allow")
                .map(|r| r.subject.clone())
                .collect();
            if distinct_jobs.len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(
            distinct_jobs.len() >= 2,
            "two self_checks must spawn two DISTINCT jobs (fresh nonce); saw: {distinct_jobs:?}"
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Test 4 -- read_only_observer SKIP. The probe argv is policy-denied under
/// ReadOnlyObserver, so the probe SKIPS: `failures == 0` AND the report
/// contains `spawn probe skipped:`. A correct deny must never be a false RED.
#[test]
fn self_check_read_only_observer_skips_without_failure() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("ro-skip");
        let (_state, handle) = build_server_with_profile(&data, PolicyProfile::ReadOnlyObserver);
        let client = DaemonClient::new(handle.socket_path().to_path_buf())
            .with_timeout(Duration::from_secs(5));

        let r = call_self_check(&client, 1).await;
        assert_eq!(
            r.failures, 0,
            "a policy-denied probe is a SKIP, never a failure: {}",
            r.report
        );
        assert!(
            r.report.contains("spawn probe skipped:"),
            "read_only_observer must SKIP the spawn probe: {}",
            r.report
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}

/// Test 5 -- negative (forced breakage). Call the extracted probe helper
/// DIRECTLY with a nonexistent binary that is policy-allowed under
/// DeveloperLocal (an absolute path under the data dir that does not exist).
/// The spawn fails to launch, so the helper reports `failed == true` -- proving
/// `failures > 0` on real breakage without making the inert noop exit nonzero.
#[test]
fn selfcheck_spawn_probe_reports_failure_on_nonexistent_binary() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("negative");
        let (state, handle) = build_server(&data);

        // A path that does not exist but is NOT a denied shell interpreter and
        // is allowed under DeveloperLocal (any non-shell argv[0] passes).
        let missing = data.join("definitely-not-a-real-binary-xyz");
        let argv = vec![missing.to_string_lossy().into_owned()];
        let cwd = data.clone();

        let outcome =
            terminal_commanderd::ipc::server::selfcheck_spawn_probe(&state, argv, cwd).await;
        assert!(
            outcome.failed,
            "a nonexistent binary must make the probe FAIL: {}",
            outcome.line
        );
        assert!(
            outcome.line.contains("FAILED"),
            "failure line must say FAILED: {}",
            outcome.line
        );

        handle.shutdown().await;
        cleanup(&data);
    });
}
