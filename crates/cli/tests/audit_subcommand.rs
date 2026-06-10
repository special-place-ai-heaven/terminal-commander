//! Live round-trip coverage for the P4 `audit` subcommand.
//!
//! Spawns a real `terminal-commanderd` in `ipc-server` mode under an isolated
//! `TC_DATA` + `TC_SESSION`, performs an AUDITED op via a DIRECT `DaemonClient`
//! (`registry_import_pack` with activate=true, which lands one
//! `ipc_registry_import_pack` audit row through the normal audited dispatch
//! envelope), then drives the CLI binary and asserts:
//!
//! 1. `terminal-commander audit` exits 0 and prints the audited action row
//!    (REAL data read back from the persistent audit log, never fabricated).
//! 2. `system_discover` lists `audit_since` so discovery stays truthful.
//!
//! Cross-platform: the transport lives entirely inside `DaemonClient`
//! (UDS on Unix, named pipe on Windows), so this test names no transport
//! detail. On the Windows dev box the `#[cfg(windows)]` pipe client compiles
//! and runs; on CI-linux the `#[cfg(unix)]` socket client is exercised.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use terminal_commander_core::ActivationScope;
use terminal_commander_ipc::{DaemonClient, IpcRequest, IpcResponse, RegistryImportPackParams};
use terminal_commander_supervisor::pidfile::{pidfile_path, read_pidfile_raw};

/// Cross-package binary discovery for the sibling `terminal-commanderd` crate:
/// derive its path from the test binary's `target/<profile>/deps/<test>`
/// location -> sibling `target/<profile>/<name>[.exe]`.
fn target_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent() // deps/
        .and_then(|p| p.parent()) // <profile>/
        .expect("profile dir");
    let mut bin = profile_dir.join(name);
    if cfg!(windows) {
        bin.set_extension("exe");
    }
    bin
}

fn tmp_data_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cli-audit-{tag}-{pid}-{nanos}-{n}"));
    p
}

/// A live daemon under an isolated state dir. Owns cleanup on drop.
struct LiveDaemon {
    child: std::process::Child,
    base: PathBuf,
    token: String,
    endpoint: String,
}

impl LiveDaemon {
    fn spawn(tag: &str) -> Self {
        let base = tmp_data_dir(tag);
        // S7: unique per test process. On Windows the pipe endpoint is
        // derived from the token ALONE, so a constant token collides
        // across parallel tests and with stale orphans from aborted
        // runs. See read_subcommands.rs for the full rationale.
        let token = format!(
            "{tag}{}x{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos())
        );
        let token = token.as_str();
        let state_dir = base.join(token);

        let daemon_bin = target_bin("terminal-commanderd");
        assert!(
            daemon_bin.exists(),
            "daemon binary not found at {}; cargo dev-dep should have built it",
            daemon_bin.display()
        );
        let child = Command::new(&daemon_bin)
            .args(["start", "--mode", "ipc-server"])
            .env("TC_DATA", &base)
            .env("TC_SESSION", token)
            .env_remove("TC_SOCKET")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn daemon");

        let pid_path = pidfile_path(&state_dir);
        let bind_deadline = Instant::now() + Duration::from_secs(5);
        while !pid_path.exists() && Instant::now() < bind_deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        if !pid_path.exists() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&base);
            panic!(
                "daemon never bound: pidfile missing at {}",
                pid_path.display()
            );
        }
        let endpoint = read_pidfile_raw(&state_dir)
            .expect("pidfile present after bind")
            .endpoint;

        Self {
            child,
            base,
            token: token.to_string(),
            endpoint,
        }
    }

    fn run_cli(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_terminal-commander"))
            .args(args)
            .env("TC_DATA", &self.base)
            .env("TC_SESSION", &self.token)
            .env_remove("TC_SOCKET")
            .output()
            .expect("run cli")
    }
}

impl Drop for LiveDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Perform an AUDITED op via a DIRECT DaemonClient: import the embedded
/// `cleanup` pack with activate=true. The dispatch records one
/// `ipc_registry_import_pack` audit row through the normal audited envelope.
async fn perform_audited_op(endpoint: &str) {
    let client = DaemonClient::new(endpoint);
    let req = IpcRequest::RegistryImportPack(RegistryImportPackParams {
        pack: "cleanup".to_string(),
        activate: true,
        scope: Some(ActivationScope::Global),
    });
    match client.call(1, req).await.expect("import_pack call") {
        IpcResponse::RegistryImportPack(_) => {}
        other => panic!("unexpected import_pack response: {other:?}"),
    }
}

/// Query `system_discover` directly and return the advertised method list.
async fn discover_methods(endpoint: &str) -> Vec<String> {
    let client = DaemonClient::new(endpoint);
    match client
        .call(2, IpcRequest::SystemDiscover)
        .await
        .expect("system_discover call")
    {
        IpcResponse::SystemDiscover(r) => r.methods,
        other => panic!("unexpected system_discover response: {other:?}"),
    }
}

#[test]
fn audit_subcommand_prints_real_audited_row() {
    let daemon = LiveDaemon::spawn("auditrow");
    let endpoint = daemon.endpoint.clone();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(perform_audited_op(&endpoint));

    let out = daemon.run_cli(&["audit"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "audit must exit 0; stdout={stdout}, stderr={stderr}"
    );
    // The header is always printed (honestly-empty would still show it).
    assert!(
        stdout.contains("AUDIT_ID") && stdout.contains("ACTION"),
        "audit must render its table header; stdout={stdout}"
    );
    // The audited import op must appear as a REAL row read from the log.
    assert!(
        stdout.contains("registry_import_pack"),
        "audit must print the audited registry_import_pack row; stdout={stdout}"
    );
}

#[test]
fn system_discover_lists_audit_since() {
    let daemon = LiveDaemon::spawn("discover");
    let endpoint = daemon.endpoint.clone();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let methods = rt.block_on(discover_methods(&endpoint));
    assert!(
        methods.iter().any(|m| m == "audit_since"),
        "system_discover must advertise audit_since; methods={methods:?}"
    );
}
