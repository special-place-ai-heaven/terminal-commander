// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Windows unified-runtime-view contract tests (T1).
//!
//! Sibling to `runtime_state.rs` (`#![cfg(unix)]`). Guards the PTY branch in
//! `collect_probes` on Windows: cfg reachability (merge-gated sentinel) and,
//! when ConPTY can spawn, live `runtime_state` enumeration (workspace test).

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::{
    DaemonConfig, DaemonState, IpcErrorCode, IpcRequest, IpcResponse, IpcResult, ListLimitParams,
    ProbeKind, PtyCommandStartParams, PtyCommandStopParams, RequestEnvelope,
};

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-runtime-state-win-{tag}-{pid}-{nanos}"));
    p
}

fn cleanup(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn make_state(tag: &str) -> (Arc<DaemonState>, PathBuf) {
    let data = temp_data_dir(tag);
    let cfg = DaemonConfig::defaults_in(&data);
    let state = DaemonState::bootstrap(cfg).expect("bootstrap daemon state");
    (Arc::new(state), data)
}

async fn dispatch(
    state: &Arc<DaemonState>,
    id: u64,
    request: IpcRequest,
) -> terminal_commanderd::ResponseEnvelope {
    let req = RequestEnvelope {
        correlation_id: id,
        request,
    };
    terminal_commanderd::ipc::dispatch_envelope(
        state,
        Instant::now(),
        &req,
        &PeerIdentity::unknown(),
    )
    .await
}

fn conpty_environmental_skip(reason: &str) {
    eprintln!("SKIP runtime_state_lists_live_pty_on_windows: {reason}");
}

fn spawn_error_is_conpty_environmental(message: &str) -> bool {
    message.contains("0xC0000142")
        || message.contains("STATUS_DLL_INIT_FAILED")
        || message.contains("-1073741502")
        || message.to_ascii_lowercase().contains("conpty")
}

/// ponytail: source-level sentinel for T1 (cfg re-narrowed on Windows). Headless
/// Windows CI cannot rely on live ConPTY spawn; this is merge-gated via
/// `scripts/windows-gate.ps1`. Upgrade path: structural AST check if we add one.
#[test]
fn collect_probes_pty_enumeration_cfg_admits_windows() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ipc/handlers/runtime.rs");
    let source = std::fs::read_to_string(&path).expect("read runtime.rs");
    let fn_start = source
        .find("fn collect_probes")
        .expect("collect_probes in runtime.rs");
    let fn_body = &source[fn_start..];
    let pty_marker = fn_body
        .find("// PtyRuntime: list returns")
        .expect("PTY enumeration section in collect_probes");
    let after_pty = &fn_body[pty_marker..];
    let cfg_line = after_pty
        .lines()
        .find(|line| line.trim_start().starts_with("#[cfg"))
        .expect("PTY loop cfg attribute in collect_probes");
    let normalized: String = cfg_line.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        normalized.contains("windows"),
        "PTY enumeration in collect_probes must admit Windows; got: {cfg_line}"
    );
    assert!(
        normalized != "#[cfg(unix)]",
        "PTY enumeration must not be unix-only; got: {cfg_line}"
    );
}

/// Unified-view contract: a live PTY started on an isolated daemon appears in
/// `runtime_state` (not view parity with `pty_command_list`).
#[tokio::test]
async fn runtime_state_lists_live_pty_on_windows() {
    let (state, data) = make_state("runtime-pty");
    let start_resp = dispatch(
        &state,
        1,
        IpcRequest::PtyCommandStart(PtyCommandStartParams {
            environment: None,
            argv: vec![
                "ping".to_owned(),
                "-n".to_owned(),
                "60".to_owned(),
                "127.0.0.1".to_owned(),
            ],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            rows: None,
            cols: None,
            tag: None,
        }),
    )
    .await;
    let (job_id, probe_id) = match start_resp.result {
        IpcResult::Ok {
            response: IpcResponse::PtyCommandStart(s),
        } => (s.job_id, s.probe_id),
        IpcResult::Err { error } if error.code == IpcErrorCode::UnsupportedPlatform => {
            conpty_environmental_skip("PTY lane UnsupportedPlatform on this host");
            cleanup(&data);
            return;
        }
        IpcResult::Err { error }
            if error.code == IpcErrorCode::Internal
                && spawn_error_is_conpty_environmental(&error.message) =>
        {
            conpty_environmental_skip(&error.message);
            cleanup(&data);
            return;
        }
        other => panic!("pty_command_start failed unexpectedly: {other:?}"),
    };

    let runtime_resp = dispatch(
        &state,
        2,
        IpcRequest::RuntimeState(ListLimitParams::default()),
    )
    .await;
    match runtime_resp.result {
        IpcResult::Ok {
            response: IpcResponse::RuntimeState(r),
        } => {
            assert_eq!(
                r.pty_jobs, 1,
                "fresh daemon with one live PTY must report pty_jobs == 1; probes: {:?}",
                r.probes
            );
            let pty_row = r
                .probes
                .iter()
                .find(|p| matches!(p.kind, ProbeKind::Pty))
                .unwrap_or_else(|| panic!("expected ProbeKind::Pty row; probes: {:?}", r.probes));
            assert_eq!(
                pty_row.probe_id, probe_id,
                "unified view must carry the started PTY probe_id"
            );
            assert_eq!(pty_row.job_id, job_id);
        }
        other => panic!("runtime_state failed: {other:?}"),
    }

    let _ = dispatch(
        &state,
        3,
        IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id }),
    )
    .await;
    cleanup(&data);
}
