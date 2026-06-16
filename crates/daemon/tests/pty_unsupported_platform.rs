// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Platform availability of the PTY command lane.
//!
//! - On a platform with NO PTY backend (neither unix `pty-process` nor Windows
//!   ConPTY) the daemon MUST return `UnsupportedPlatform`, never a faked empty
//!   success (constitution VII: honest degradation). That is the
//!   `not(any(unix, windows))` test below.
//! - On Windows (US3a/TC53) the ConPTY backend is LIVE, so `pty_command_list`
//!   with no jobs returns a real, bounded, empty list -- NOT
//!   `UnsupportedPlatform`. That is the `windows` test below.

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

/// On a platform with no PTY backend at all, the daemon must honestly report
/// `UnsupportedPlatform` rather than fake an empty success.
#[cfg(not(any(unix, windows)))]
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

/// US3a/TC53: on Windows the ConPTY backend is LIVE, so `pty_command_list` with
/// no running jobs returns a real, bounded, empty list -- proving the PTY lane
/// is served (not `UnsupportedPlatform`) on a host where ConPTY initializes.
#[cfg(windows)]
#[tokio::test]
async fn pty_command_list_is_live_on_windows() {
    let (state, data) = make_state("list-win");
    let req = RequestEnvelope {
        correlation_id: 7,
        request: IpcRequest::PtyCommandList,
    };

    let resp = terminal_commanderd::ipc::dispatch_envelope(
        &state,
        Instant::now(),
        &req,
        &PeerIdentity::unknown(),
    )
    .await;

    assert_eq!(resp.correlation_id, 7);
    match resp.result {
        IpcResult::Ok { response } => {
            // A live empty list is the correct response with no jobs running.
            // It must NOT be an UnsupportedPlatform error.
            let rendered = format!("{response:?}");
            assert!(
                rendered.contains("PtyCommandList"),
                "expected a live PtyCommandList response on Windows, got: {rendered}"
            );
        }
        IpcResult::Err { error } => {
            assert_ne!(
                error.code,
                IpcErrorCode::UnsupportedPlatform,
                "pty_command_list must be SERVED on Windows (ConPTY), not UnsupportedPlatform"
            );
            panic!("unexpected error from pty_command_list on Windows: {error:?}");
        }
    }

    cleanup(&data);
}
