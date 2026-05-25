// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

use std::path::PathBuf;
use std::time::Duration;

use terminal_commander_core::{BucketReadRequest, JobState};
use terminal_commanderd::{CommandStartRequest, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-cmd-status-{tag}-{pid}-{nanos}"));
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
