#![cfg(unix)]
//! probe_endpoint must require a well-formed Health response, not just a
//! connectable socket — a non-daemon listener is NOT "our daemon".

use terminal_commander_supervisor::ensure::{probe_endpoint, Endpoint};

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
