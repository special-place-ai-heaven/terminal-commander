// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Verifies that the Windows named-pipe IPC server mirrors the UDS
// server on a malformed frame: instead of silently dropping the
// connection (EOF), it writes a typed `IpcError` envelope back to the
// client (correlation_id 0) before closing. A clean disconnect between
// frames still closes silently.
//
// Parity target: `ipc_roundtrip.rs::malformed_json_returns_typed_error_and_closes`
// and `oversized_frame_rejected` (the UDS equivalents).

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use terminal_commanderd::ipc::pipe_server::PipeServer;
use terminal_commanderd::{DaemonConfig, DaemonState, MAX_FRAME_BYTES};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::sleep;

fn temp_data_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    p.push(format!("tc-pipe-malformed-{tag}-{pid}-{nanos}"));
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

fn unique_pipe_name(tag: &str) -> String {
    format!(
        r"\\.\pipe\tc-test-malformed-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    )
}

/// Read one length-prefixed response envelope from the pipe client and
/// return the parsed JSON value.
async fn read_response_json<R>(stream: &mut R) -> serde_json::Value
where
    R: AsyncReadExt + Unpin,
{
    let mut len_buf = [0_u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .expect("server must write a length-prefixed response, not EOF");
    let resp_len = u32::from_be_bytes(len_buf) as usize;
    let mut resp = vec![0_u8; resp_len];
    stream
        .read_exact(&mut resp)
        .await
        .expect("server must write the full response payload");
    serde_json::from_slice(&resp).expect("response must be valid JSON")
}

/// Malformed JSON in the payload must yield a typed error envelope
/// (correlation_id 0) on the named pipe, mirroring the UDS server,
/// instead of a silent disconnect.
#[tokio::test]
async fn malformed_json_returns_typed_error_envelope() {
    let pipe_name = unique_pipe_name("badjson");
    let (state, data) = make_state("badjson");
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    // Let the accept loop start and enter connect().
    sleep(Duration::from_millis(50)).await;

    let mut client = ClientOptions::new()
        .open(&pipe_name)
        .expect("client open named pipe");

    let payload = b"{not valid json at all";
    let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
    client.write_all(&len).await.expect("write length prefix");
    client.write_all(payload).await.expect("write payload");

    let env = read_response_json(&mut client).await;
    assert_eq!(
        env["correlation_id"].as_u64(),
        Some(0),
        "framing-error envelope must use correlation_id 0 (UDS parity); got {env}"
    );
    let code = env["result"]["error"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a typed error code; got {env}"));
    assert!(
        code == "malformed_json" || code == "schema_mismatch",
        "expected malformed_json/schema_mismatch, got {code}"
    );

    handle.shutdown().await;
    cleanup(&data);
}

/// A length prefix above MAX_FRAME_BYTES must be rejected with a typed
/// `frame_too_large` envelope on the named pipe (UDS parity).
#[tokio::test]
async fn oversized_frame_returns_typed_error_envelope() {
    let pipe_name = unique_pipe_name("toobig");
    let (state, data) = make_state("toobig");
    let server = PipeServer::new(Arc::clone(&state), pipe_name.clone());
    let handle = server.spawn().expect("spawn pipe server");

    sleep(Duration::from_millis(50)).await;

    let mut client = ClientOptions::new()
        .open(&pipe_name)
        .expect("client open named pipe");

    let bogus_len = u32::try_from(MAX_FRAME_BYTES + 1024).unwrap().to_be_bytes();
    client
        .write_all(&bogus_len)
        .await
        .expect("write oversized length prefix");

    let env = read_response_json(&mut client).await;
    assert_eq!(
        env["correlation_id"].as_u64(),
        Some(0),
        "framing-error envelope must use correlation_id 0 (UDS parity); got {env}"
    );
    assert_eq!(
        env["result"]["error"]["code"].as_str(),
        Some("frame_too_large"),
        "expected frame_too_large; got {env}"
    );

    handle.shutdown().await;
    cleanup(&data);
}
