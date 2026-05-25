// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

#![cfg(not(unix))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResult, RequestEnvelope,
};

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-pty-unsupported-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn make_state(tag: &str) -> (Arc<DaemonState>, PathBuf) {
    let data = temp_data_dir(tag);
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).expect("bootstrap daemon state");
    (Arc::new(state), data)
}

#[tokio::test]
async fn pty_command_list_reports_unsupported_platform_instead_of_empty_success() {
    let (state, data) = make_state("list");
    let req = RequestEnvelope {
        correlation_id: 1,
        request: IpcRequest::PtyCommandList,
    };

    let resp = terminal_commanderd::ipc::dispatch_envelope(
        &state,
        Instant::now(),
        &req,
        &PeerIdentity::unknown(),
    )
    .await;

    assert_eq!(resp.correlation_id, 1);
    match resp.result {
        IpcResult::Err { error } => {
            assert_eq!(error.code, IpcErrorCode::UnsupportedPlatform);
            assert!(
                error
                    .message
                    .contains("PTY command runtime is not available"),
                "unexpected error message: {}",
                error.message
            );
        }
        IpcResult::Ok { response } => {
            panic!("pty_command_list must not fake empty success on this platform: {response:?}");
        }
    }

    cleanup(&data);
}
