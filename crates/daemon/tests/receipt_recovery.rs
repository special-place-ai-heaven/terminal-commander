// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Durable receipt acceptance checks use isolated stores and sacrificial owners.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use terminal_commander_core::{
    BucketId, BucketReadRequest, JobConfig, JobId, JobState, ProbeId, SourceType,
};
use terminal_commander_ipc::protocol::{CommandStatusParams, IpcErrorCode};
use terminal_commander_store::{AuditEntry, AuditReadRequest};
use terminal_commanderd::embedded::EmbeddedEngine;
use terminal_commanderd::{AuditSink, CommandStartRequest, DaemonConfig, DaemonState};

async fn await_file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("receipt fixture handshake timed out");
}

#[test]
fn receipt_child_fixture() {
    let Some(dir) = std::env::var_os("TC_RECEIPT_CHILD_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mut launches = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("launches"))
        .unwrap();
    launches.write_all(b"started\n").unwrap();
    drop(launches);
    std::fs::write(dir.join("child-ready"), b"").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dir.join("child-release").exists() {
        assert!(
            Instant::now() < deadline,
            "receipt child exceeded its fixture deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    std::fs::write(dir.join("child-exited"), b"").unwrap();
}

fn start_waiting_job(state: &DaemonState, dir: &Path) -> (JobId, BucketId) {
    let response = state
        .command
        .start_combed(CommandStartRequest {
            argv: vec![
                std::env::current_exe()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                "--exact".into(),
                "receipt_child_fixture".into(),
                "--nocapture".into(),
            ],
            cwd: None,
            env: vec![(
                "TC_RECEIPT_CHILD_DIR".into(),
                dir.to_string_lossy().into_owned(),
            )],
            bucket_config: None,
            rules: vec![],
            grace: Some(Duration::from_millis(50)),
            tag: None,
            dedup_nonce: None,
            receipt_shape: None,
            strip_ansi: true,
            peer_discriminator: None,
            limits: None,
        })
        .unwrap();
    (response.job_id, response.bucket_id)
}

fn reject_receipts(config: &DaemonConfig) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(config.db_path()).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON job_receipts BEGIN SELECT RAISE(FAIL, 'injected receipt write failure'); END;").unwrap();
    conn
}

fn seed_inflight(state: &DaemonState) -> JobId {
    let job_id = JobId::new();
    state.jobs.start(JobConfig {
        job_id,
        argv: vec!["receipt-fixture".into()],
        bucket_id: BucketId::new(),
        probe_id: ProbeId::new(),
        source_type: SourceType::Process,
        grace_secs: 0,
    });
    state.jobs.mark_running(job_id);
    state
        .audit
        .emit(&AuditEntry::new(
            "command_start",
            job_id.to_wire_string(),
            "allow",
        ))
        .unwrap();
    job_id
}

async fn assert_lost_without_replay(config: DaemonConfig, job_id: JobId) {
    let replacement = EmbeddedEngine::bootstrap(config).unwrap();
    for _ in 0..3 {
        let error = replacement
            .command_status(CommandStatusParams { job_id })
            .await
            .unwrap_err();
        assert_eq!(error.code, IpcErrorCode::JobLost);
        let encoded = serde_json::to_value(&error).unwrap();
        assert_eq!(encoded["details"]["job_id"], job_id.to_wire_string());
        assert_eq!(
            encoded["details"]["current_instance_id"],
            serde_json::to_value(replacement.identity()).unwrap()["instance_id"]
        );
    }
    replacement.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_terminal_write_recovers_as_lost_without_replaying_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let config = DaemonConfig::defaults_in(dir.path());
    let state = DaemonState::bootstrap(config.clone()).unwrap();
    let (job_id, bucket_id) = start_waiting_job(&state, dir.path());
    await_file(&dir.path().join("child-ready")).await;
    let conn = reject_receipts(&config);
    std::fs::write(dir.path().join("child-release"), b"").unwrap();
    state.command.drain_lifecycle_tasks().await;
    assert_eq!(state.command.status(job_id).unwrap().exit_code, Some(0));
    let events = state
        .router
        .bucket_events_since(bucket_id, &BucketReadRequest::new(0))
        .unwrap();
    assert_eq!(
        events
            .events
            .iter()
            .filter(|event| event.kind.as_str() == "command_exited")
            .count(),
        1
    );
    assert!(
        state
            .store
            .get_job_receipt(&job_id.to_wire_string())
            .unwrap()
            .is_none()
    );
    assert!(
        state
            .store
            .job_start_recorded(&job_id.to_wire_string())
            .unwrap()
    );
    conn.execute_batch("DROP TRIGGER reject_receipt;").unwrap();
    drop(conn);
    state.store.shutdown().unwrap();
    drop(state);
    assert_lost_without_replay(config, job_id).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("launches"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[tokio::test]
async fn repeated_cancel_persists_one_terminal_transition_and_reconstructs() {
    let dir = tempfile::tempdir().unwrap();
    let config = DaemonConfig::defaults_in(dir.path());
    let state = DaemonState::bootstrap(config.clone()).unwrap();
    let (job_id, _) = start_waiting_job(&state, dir.path());
    await_file(&dir.path().join("child-ready")).await;
    state.command.stop(job_id, "receipt-test").unwrap();
    state.command.stop(job_id, "receipt-test-repeat").unwrap();
    state.command.drain_lifecycle_tasks().await;
    let live = state.command.status(job_id).unwrap();
    assert_eq!(live.state, JobState::Cancelled);
    let row = state
        .store
        .get_job_receipt(&job_id.to_wire_string())
        .unwrap()
        .unwrap();
    assert_eq!(row.terminal_state, "cancelled");
    let audit = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|entry| entry.action == "command_stop"
                && entry.decision == "allow"
                && entry.subject == job_id.to_wire_string())
            .count(),
        1
    );
    let conn = rusqlite::Connection::open(config.db_path()).unwrap();
    let receipt_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM job_receipts WHERE job_id = ?1",
            [job_id.to_wire_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipt_count, 1);
    drop(conn);
    state.store.shutdown().unwrap();
    drop(state);
    let replacement = EmbeddedEngine::bootstrap(config).unwrap();
    let recovered = replacement
        .command_status(CommandStatusParams { job_id })
        .await
        .unwrap();
    assert_eq!(recovered.state, JobState::Cancelled);
    assert_eq!(recovered.exit_code, live.exit_code);
    assert!(recovered.restarted);
    assert_eq!(recovered.bytes_total, live.bytes_total);
    assert_eq!(
        serde_json::to_value(&recovered).unwrap()["outcome_trust"],
        "reconstructed"
    );
    replacement.shutdown().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("launches"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[tokio::test]
async fn abandonment_is_durable_without_an_invented_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let config = DaemonConfig::defaults_in(dir.path());
    let state = DaemonState::bootstrap(config.clone()).unwrap();
    let job_id = seed_inflight(&state);
    assert_eq!(state.record_abandoned_jobs(), 1);
    assert_eq!(state.record_abandoned_jobs(), 1);
    state.store.shutdown().unwrap();
    drop(state);
    let replacement = EmbeddedEngine::bootstrap(config).unwrap();
    let recovered = replacement
        .command_status(CommandStatusParams { job_id })
        .await
        .unwrap();
    assert_eq!(recovered.state, JobState::Cancelled);
    assert_eq!(recovered.exit_code, None);
    assert_eq!(
        serde_json::to_value(&recovered).unwrap()["outcome_trust"],
        "abandoned"
    );
    replacement.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_abandonment_write_reports_zero_recorded_and_recovers_as_lost() {
    let dir = tempfile::tempdir().unwrap();
    let config = DaemonConfig::defaults_in(dir.path());
    let state = DaemonState::bootstrap(config.clone()).unwrap();
    let job_id = seed_inflight(&state);
    let conn = reject_receipts(&config);
    let recorded = state.record_abandoned_jobs();
    assert!(
        state
            .store
            .get_job_receipt(&job_id.to_wire_string())
            .unwrap()
            .is_none()
    );
    conn.execute_batch("DROP TRIGGER reject_receipt;").unwrap();
    drop(conn);
    state.store.shutdown().unwrap();
    drop(state);
    assert_lost_without_replay(config, job_id).await;
    assert_eq!(
        recorded, 0,
        "failed receipt writes are not recorded abandonments"
    );
}

#[test]
fn receipt_owner_fixture() {
    let Some(dir) = std::env::var_os("TC_RECEIPT_OWNER_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let state = DaemonState::bootstrap(DaemonConfig::defaults_in(&dir)).unwrap();
        let (job_id, _) = start_waiting_job(&state, &dir);
        await_file(&dir.join("child-ready")).await;
        std::fs::write(dir.join("owner-id"), job_id.to_wire_string()).unwrap();
        std::fs::write(dir.join("owner-ready"), b"").unwrap();
        await_file(&dir.join("crash-now")).await;
        // Deliberately bypass destructors, runtime shutdown, and receipt writes.
        // This is a separate fixture process, never the installed daemon.
        std::process::exit(86);
    });
}

#[tokio::test]
async fn abrupt_owner_loss_never_becomes_success_or_an_automatic_rerun() {
    let dir = tempfile::tempdir().unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "receipt_owner_fixture", "--nocapture"])
        .env("TC_RECEIPT_OWNER_DIR", dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    terminal_commander_core::windows_silent(&mut command);
    let mut owner = command.spawn().unwrap();
    await_file(&dir.path().join("owner-ready")).await;
    let job_id =
        JobId::parse_wire(&std::fs::read_to_string(dir.path().join("owner-id")).unwrap()).unwrap();
    std::fs::write(dir.path().join("crash-now"), b"").unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = owner.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(status.code(), Some(86));
    // A Unix child can survive abrupt owner death. Release this fixture before
    // reconstruction; Windows may already have killed it on JobObject close.
    std::fs::write(dir.path().join("child-release"), b"").unwrap();
    #[cfg(unix)]
    await_file(&dir.path().join("child-exited")).await;
    assert_lost_without_replay(DaemonConfig::defaults_in(dir.path()), job_id).await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("launches"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}
