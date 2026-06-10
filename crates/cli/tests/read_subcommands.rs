//! Live round-trip coverage for the P3 daemon-backed read subcommands.
//!
//! Spawns a real `terminal-commanderd` in `ipc-server` mode under an isolated
//! `TC_DATA` + `TC_SESSION`, seeds rule state via a DIRECT `DaemonClient`
//! (`registry_import_pack` with activate=true), then drives the CLI binary and
//! asserts it prints the REAL seeded data:
//!
//! 1. `rules list` prints a seeded active rule id (`cleanup.disk-usage`).
//! 2. `rules show cleanup.disk-usage` prints that rule's definition.
//! 3. `rules show missing.rule` exits non-zero with a typed `RuleNotFound`
//!    error (NEVER a fabricated "not found" success).
//! 4. `jobs` / `probes` / `policy` round-trip and exit 0 against a live daemon
//!    with no running jobs (honestly-empty tables).
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
    p.push(format!("tc-cli-read-{tag}-{pid}-{nanos}-{n}"));
    p
}

/// A live daemon under an isolated state dir. Owns cleanup on drop.
struct LiveDaemon {
    child: std::process::Child,
    base: PathBuf,
    token: String,
    state_dir: PathBuf,
    endpoint: String,
}

impl LiveDaemon {
    /// Spawn a daemon in ipc-server mode, wait for it to bind (pidfile appears),
    /// and read back the endpoint it recorded.
    fn spawn(tag: &str) -> Self {
        let base = tmp_data_dir(tag);
        // S7: the TC_SESSION token must be UNIQUE PER TEST PROCESS. On
        // Windows the IPC endpoint is `\\.\pipe\terminal-commander-<token>`
        // — derived from the token ALONE, not the (unique) TC_DATA dir —
        // so a constant token made every parallel test in this file bind
        // the SAME pipe: one test's daemon answered another test's probe,
        // and once the owner's Drop killed it the remaining CLIs failed
        // exit-69 EndpointBindFailed. (Unix never collided: the socket
        // path derives from the unique state dir, which is why CI-linux
        // stayed green.)
        //
        // The token must also stay SHORT: on unix the endpoint is
        // `<TC_DATA>/<token>/terminal-commanderd.sock`, and `sun_path`
        // caps the whole socket path at ~104 bytes. A first cut embedded
        // `{tag}{pid}{nanos}` here and pushed the path over that cap —
        // the daemon could not bind and every spawn panicked "daemon
        // never bound" on CI-linux. The pid alone is unique per nextest
        // test PROCESS (one process per test); the 4 nanos hex digits
        // defend against a stale orphan from a previously aborted run.
        // The human-readable `tag` already lives in the TC_DATA base dir.
        let token = format!(
            "t{:x}{:04x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos())
                & 0xffff
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
            state_dir,
            endpoint,
        }
    }

    /// Run the CLI binary with the daemon's TC_DATA + TC_SESSION so it resolves
    /// the SAME endpoint via the supervisor resolver.
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

/// Seed active rule state via a DIRECT DaemonClient: import the embedded
/// `cleanup` pack with activate=true under Global scope. Returns the activated
/// rule ids the daemon reports.
async fn seed_cleanup_pack(endpoint: &str) -> Vec<String> {
    let client = DaemonClient::new(endpoint);
    let req = IpcRequest::RegistryImportPack(RegistryImportPackParams {
        pack: "cleanup".to_string(),
        activate: true,
        scope: Some(ActivationScope::Global),
    });
    match client.call(1, req).await.expect("import_pack call") {
        IpcResponse::RegistryImportPack(r) => r.activated,
        other => panic!("unexpected import_pack response: {other:?}"),
    }
}

#[test]
fn rules_list_prints_seeded_active_rule() {
    let daemon = LiveDaemon::spawn("ruleslist");
    let endpoint = daemon.endpoint.clone();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let activated = rt.block_on(seed_cleanup_pack(&endpoint));
    assert!(
        activated.iter().any(|id| id == "cleanup.disk-usage"),
        "seed must activate cleanup.disk-usage; activated={activated:?}"
    );

    let out = daemon.run_cli(&["rules", "list"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "rules list must exit 0; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stdout.contains("cleanup.disk-usage"),
        "rules list must print the seeded rule; stdout={stdout}, stderr={stderr}"
    );
}

#[test]
fn rules_show_prints_seeded_rule_definition() {
    let daemon = LiveDaemon::spawn("rulesshow");
    let endpoint = daemon.endpoint.clone();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(seed_cleanup_pack(&endpoint));

    let out = daemon.run_cli(&["rules", "show", "cleanup.disk-usage"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "rules show must exit 0; stdout={stdout}, stderr={stderr}"
    );
    assert!(
        stdout.contains("cleanup.disk-usage") && stdout.contains("event_kind"),
        "rules show must render the rule definition; stdout={stdout}, stderr={stderr}"
    );
}

#[test]
fn rules_show_missing_rule_exits_typed_error() {
    let daemon = LiveDaemon::spawn("rulesmissing");
    // No seeding needed: a never-imported rule id must surface a typed
    // RuleNotFound from the daemon, NOT a fabricated "not found" success.
    let out = daemon.run_cli(&["rules", "show", "missing.rule"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code = out.status.code();
    assert!(
        code.is_some() && code != Some(0),
        "rules show <missing> must exit non-zero; code={code:?}, stdout={stdout}, stderr={stderr}"
    );
    // Require the TYPED daemon error specifically. The previous
    // `|| stderr.contains("failed")` arm let an exit-69
    // "unavailable: ... EndpointBindFailed" satisfy this test vacuously
    // — exactly the daemon-never-reached failure mode S7 produced.
    assert!(
        stderr.to_lowercase().contains("rulenotfound"),
        "missing rule must surface the typed RuleNotFound daemon error; stderr={stderr}"
    );
    // Honesty: nothing fabricated on stdout.
    assert!(
        !stdout.contains("missing.rule"),
        "missing rule must not print a fabricated definition; stdout={stdout}"
    );
}

#[test]
fn jobs_probes_policy_round_trip_against_live_daemon() {
    let daemon = LiveDaemon::spawn("jpp");
    // No jobs/probes running: these are honestly-empty real results, exit 0.
    for (args, marker) in [
        (["jobs"].as_slice(), "runtime jobs:"),
        (["probes"].as_slice(), "KIND"),
        (["policy"].as_slice(), "policy status:"),
    ] {
        let out = daemon.run_cli(args);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?} must exit 0; stdout={stdout}, stderr={stderr}"
        );
        assert!(
            stdout.contains(marker),
            "{args:?} must render its table ({marker:?}); stdout={stdout}, stderr={stderr}"
        );
    }
    // Keep state_dir referenced so the field is read on every platform.
    assert!(daemon.state_dir.exists());
}
