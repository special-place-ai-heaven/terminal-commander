#![cfg(windows)]

//! Windows named-pipe probe regressions.

use std::time::Duration;

use terminal_commander_supervisor::ensure::{
    Endpoint, EnsureDaemonOptions, EnsureDaemonStatus, ensure_daemon,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ServerOptions;

#[tokio::test]
async fn no_spawn_probe_honors_its_caller_selected_deadline() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let pipe_name = format!(
        r"\\.\pipe\terminal-commander-delayed-probe-{}-{nonce}",
        std::process::id()
    );
    let server_name = pipe_name.clone();
    let (delay_started_tx, delay_started_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let _ = delay_started_tx.send(());
        tokio::time::sleep(Duration::from_millis(750)).await;
        let mut pipe = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&server_name)
            .expect("create delayed probe pipe");
        pipe.connect().await.expect("accept health probe");

        let mut len_buf = [0_u8; 4];
        pipe.read_exact(&mut len_buf)
            .await
            .expect("read request length");
        let mut request = vec![0_u8; u32::from_be_bytes(len_buf) as usize];
        pipe.read_exact(&mut request)
            .await
            .expect("read health request");

        let response = br#"{"correlation_id":0,"result":{"kind":"ok","response":{"method":"health","uptime_secs":1,"idle_secs":0,"version":"test"}}}"#;
        pipe.write_all(&u32::try_from(response.len()).unwrap().to_be_bytes())
            .await
            .expect("write response length");
        pipe.write_all(response)
            .await
            .expect("write health response");
        pipe.flush().await.expect("flush health response");
    });
    delay_started_rx
        .await
        .expect("delayed server task reached its timer");

    let base = std::env::temp_dir().join(format!("tc-supervisor-probe-{nonce}"));
    let expected_name = pipe_name.clone();
    let endpoint = Endpoint::WindowsPipe { name: pipe_name };
    let status = ensure_daemon(EnsureDaemonOptions {
        daemon_binary: "unused-terminal-commanderd".into(),
        state_dir: base.clone(),
        log_dir: base.join("logs"),
        endpoint: endpoint.clone(),
        startup_timeout: Duration::from_secs(5),
        allow_spawn: false,
    })
    .await;
    assert!(
        matches!(
            &status,
            EnsureDaemonStatus::AlreadyRunning {
                endpoint: Endpoint::WindowsPipe { name },
                ..
            } if name == &expected_name
        ),
        "a no-spawn probe must remain live for its caller-selected deadline; status: {status:?}"
    );
    server.await.expect("delayed probe server task");
}
