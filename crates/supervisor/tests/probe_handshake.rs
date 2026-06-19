#![cfg(unix)]
//! probe_endpoint must require a well-formed Health response, not just a
//! connectable socket — a non-daemon listener is NOT "our daemon".

use terminal_commander_supervisor::ensure::{
    Endpoint, ProbeHealth, probe_endpoint, probe_endpoint_health,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Bind a Unix socket that, on connect, drains the request frame then writes
/// back ONE length-prefixed response frame carrying `resp_json`. Returns the
/// socket path; the listener task lives for the duration of the test process.
async fn spawn_health_responder(resp_json: &'static [u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tc-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("x.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            // Drain the request: 4-byte length prefix then that many bytes.
            let mut len_buf = [0_u8; 4];
            if s.read_exact(&mut len_buf).await.is_err() {
                continue;
            }
            let req_len = u32::from_be_bytes(len_buf) as usize;
            let mut req = vec![0_u8; req_len];
            if s.read_exact(&mut req).await.is_err() {
                continue;
            }
            let resp_len = u32::try_from(resp_json.len()).unwrap().to_be_bytes();
            let _ = s.write_all(&resp_len).await;
            let _ = s.write_all(resp_json).await;
            let _ = s.flush().await;
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    sock
}

#[tokio::test]
async fn connectable_non_daemon_socket_is_not_our_daemon() {
    let dir = std::env::temp_dir().join(format!(
        "tc-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("x.sock");
    let listener = tokio::net::UnixListener::bind(&sock).unwrap();
    tokio::spawn(async move {
        while let Ok((s, _)) = listener.accept().await {
            drop(s);
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let ep = Endpoint::UnixSocket { path: sock.clone() };
    assert!(
        !probe_endpoint(&ep).await,
        "a socket that connects but never answers Health must NOT count as our daemon"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn modern_health_with_idle_secs_is_surfaced() {
    // Modern daemon: Health response carries idle_secs as a sibling of
    // uptime_secs (verified against crates/daemon/src/ipc/protocol.rs).
    let sock = spawn_health_responder(
        br#"{"correlation_id":0,"result":{"kind":"ok","response":{"method":"health","uptime_secs":1,"idle_secs":42,"version":"0.1.51"}}}"#,
    )
    .await;
    let ep = Endpoint::UnixSocket { path: sock };
    assert_eq!(
        probe_endpoint_health(&ep).await,
        Some(ProbeHealth {
            idle_secs: Some(42),
            version: Some("0.1.51".to_string())
        }),
        "a modern Health reply must surface its idle_secs AND its version"
    );
    // The bool wrapper must still see this as our daemon.
    assert!(probe_endpoint(&ep).await);
}

#[tokio::test]
async fn legacy_health_without_idle_secs_is_alive_idle_unknown() {
    // Legacy daemon: Health omits idle_secs. This is ALIVE-but-idle-unknown,
    // NOT "not our daemon": idle_secs is None, the probe still returns Some.
    let sock = spawn_health_responder(
        br#"{"correlation_id":0,"result":{"kind":"ok","response":{"method":"health","uptime_secs":1}}}"#,
    )
    .await;
    let ep = Endpoint::UnixSocket { path: sock };
    assert_eq!(
        probe_endpoint_health(&ep).await,
        Some(ProbeHealth {
            idle_secs: None,
            // A legacy daemon omits `version`; the probe normalises the
            // empty-string wire value to `None` = "version unknown".
            version: None
        }),
        "a legacy Health reply (no idle_secs / no version) is alive with idle + version UNKNOWN, not absent"
    );
    assert!(probe_endpoint(&ep).await);
}
