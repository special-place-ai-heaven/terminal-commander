// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Verifies that the daemon writes its startup line to
// `<data_dir>/logs/terminal-commanderd.log` even when stdio is
// redirected.

use std::path::PathBuf;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_writes_startup_line_to_log() {
    let data_dir: PathBuf = {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "tc-daemon-log-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    };
    let log_dir = data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("terminal-commanderd.log");

    let daemon_bin = env!("CARGO_BIN_EXE_terminal-commanderd");
    let mut child = std::process::Command::new(daemon_bin)
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("start")
        .arg("--mode")
        .arg("ipc-server")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Give the daemon up to 5s to bind and log.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut got = false;
    while std::time::Instant::now() < deadline {
        if log_path.exists() {
            let contents = std::fs::read_to_string(&log_path).unwrap_or_default();
            if contents.contains("IPC server bound") {
                got = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    child.kill().ok();
    let _ = child.wait();
    // Clean up temp dir
    let _ = std::fs::remove_dir_all(&data_dir);
    assert!(
        got,
        "daemon did not write startup line to {}",
        log_path.display()
    );
}
