// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Live coverage for the Phase 2 `subscription-stream` CLI bridge (AC9):
//! one NDJSON object per matched event to stdout, exit 0 on `--max` reached,
//! NON-ZERO on an unknown sub_id (the restart/closed terminate signal).
//!
//! Spawns a real `terminal-commanderd` in `ipc-server` mode under an isolated
//! `TC_DATA` + `TC_SESSION`, seeds a subscription + a noisy command via a
//! DIRECT `DaemonClient`, then drives the `terminal-commander` CLI binary and
//! asserts the REAL streamed events (never a fabricated row).
//!
//! Cross-platform: the unknown-sub_id terminate-signal case runs everywhere;
//! the NDJSON delivery case needs `printf` (a coreutil resolved via PATH) and
//! the UDS direct-seed client, so it is gated to unix (the authoritative gate
//! is the Linux/WSL one).

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

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
    p.push(format!("tc-cli-stream-{tag}-{pid}-{nanos}-{n}"));
    p
}

/// A live daemon under an isolated state dir. Owns cleanup on drop.
struct LiveDaemon {
    child: std::process::Child,
    base: PathBuf,
    token: String,
    // `endpoint` is read only by the `#[cfg(unix)]` direct-seed test below; on
    // Windows that module is gated out, so the field is genuinely unused there.
    #[cfg_attr(not(unix), allow(dead_code))]
    endpoint: String,
}

impl LiveDaemon {
    /// Spawn a daemon in ipc-server mode, wait for it to bind (pidfile appears),
    /// and read back the endpoint it recorded.
    fn spawn(tag: &str) -> Self {
        let base = tmp_data_dir(tag);
        let token = "streamtest";
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

#[test]
fn stream_unknown_sub_id_exits_nonzero() {
    // An unknown sub_id is the restart/closed terminate signal: the bridge must
    // exit NON-ZERO so a `Monitor` over it stops instead of silently idling.
    let daemon = LiveDaemon::spawn("stream-unknown");
    let out = daemon.run_cli(&[
        "subscription-stream",
        "00000000-0000-0000-0000-000000000000",
        "--max",
        "1",
    ]);
    assert!(
        !out.status.success(),
        "unknown sub_id must exit non-zero; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // stderr names the typed error, never a fabricated event row.
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        err.contains("subscription") || err.contains("unknown"),
        "stderr must name the typed unknown-subscription error; got: {err}"
    );
    // No fabricated NDJSON on stdout for the error path.
    assert!(
        out.stdout.is_empty(),
        "no event rows on the unknown-sub_id path; got stdout={:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn pull_unknown_sub_id_exits_nonzero() {
    // The one-shot `subscription-pull` must exit NON-ZERO on an unknown sub_id
    // (sub gone / daemon restarted) so a hook treats it as "no events / stop"
    // rather than a fabricated empty success.
    let daemon = LiveDaemon::spawn("pull-unknown");
    let out = daemon.run_cli(&[
        "subscription-pull",
        "00000000-0000-0000-0000-000000000000",
        "--max",
        "20",
    ]);
    assert!(
        !out.status.success(),
        "unknown sub_id must exit non-zero; stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        err.contains("subscription") || err.contains("unknown"),
        "stderr must name the typed unknown-subscription error; got: {err}"
    );
    assert!(
        out.stdout.is_empty(),
        "no event rows on the unknown-sub_id path; got stdout={:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[cfg(unix)]
mod unix_live {
    use super::{Duration, LiveDaemon};
    use std::time::Instant;

    use terminal_commander_core::{
        ContextHint, RuleDefinition, RuleStatus, RuleType, Severity, SourceStream,
    };
    use terminal_commander_ipc::{
        CommandStartParams, DaemonClient, IpcRequest, IpcResponse, SubscriptionOpenParams,
        SubscriptionPredicate, SubscriptionSourceSel,
    };

    /// A HIGH-severity keyword rule so its events pass `severity_min: high`.
    fn high_sev_keyword_rule() -> RuleDefinition {
        RuleDefinition {
            id: "stream.needle".to_owned(),
            version: 1,
            kind: RuleType::Keyword,
            status: RuleStatus::Active,
            severity: Severity::High,
            event_kind: "needle_hit".to_owned(),
            stream: Some(SourceStream::Stdout),
            description: None,
            pattern: None,
            keywords: Some(vec!["NEEDLE".to_owned()]),
            captures: vec![],
            summary_template: "needle".to_owned(),
            tags: vec![],
            rate_limit_per_min: None,
            redact: vec![],
            context_hint: ContextHint::default(),
            examples: vec![],
        }
    }

    /// `printf` (NOT a shell) emits several distinct NEEDLE lines so the keyword
    /// rule produces multiple high-sev events without dedupe collapsing them.
    fn noisy_start_params() -> CommandStartParams {
        CommandStartParams {
            environment: None,
            argv: vec![
                "printf".to_owned(),
                "NEEDLE a\nNEEDLE b\nNEEDLE c\nNEEDLE d\n".to_owned(),
            ],
            cwd: None,
            env: Vec::new(),
            bucket_config: None,
            rules: vec![high_sev_keyword_rule()],
            grace_ms: Some(2_000),
            tag: None,
            dedup_nonce: None,
        }
    }

    #[test]
    fn stream_emits_one_ndjson_object_per_event_then_exits_on_max() {
        let daemon = LiveDaemon::spawn("stream-ndjson");
        let endpoint = daemon.endpoint.clone();

        // Seed a subscription + a noisy command via a DIRECT DaemonClient on a
        // dedicated multi-thread runtime (the daemon must keep serving the CLI
        // concurrently).
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("seed runtime");
        let sub_id = rt.block_on(async {
            let client = DaemonClient::new(endpoint.clone()).with_timeout(Duration::from_secs(10));
            let open = client
                .call(
                    1,
                    IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                        predicate: SubscriptionPredicate {
                            severity_min: Some(Severity::High),
                            kind: None,
                            sources: SubscriptionSourceSel::All,
                            tag: None,
                        },
                    }),
                )
                .await
                .expect("subscription_open");
            let sub_id = match open {
                IpcResponse::SubscriptionOpen(r) => r.sub_id,
                other => panic!("unexpected open response: {other:?}"),
            };
            let started = client
                .call(2, IpcRequest::CommandStartCombed(noisy_start_params()))
                .await
                .expect("command_start_combed");
            assert!(matches!(started, IpcResponse::CommandStartCombed(_)));
            sub_id
        });

        // Drive the CLI bridge: stream exactly 2 events then exit 0 on --max.
        let out = daemon.run_cli(&["subscription-stream", &sub_id, "--max", "2"]);
        assert!(
            out.status.success(),
            "stream --max 2 must exit 0; stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );

        let stdout = String::from_utf8(out.stdout).expect("stdout utf8");
        let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            lines.len(),
            2,
            "exactly 2 NDJSON objects (one per event), got {}: {stdout:?}",
            lines.len()
        );
        for line in &lines {
            let v: serde_json::Value =
                serde_json::from_str(line).expect("each line is a JSON object");
            // The serialized wire event carries its matched signal event + the
            // bucket origin tag (SubscriptionEvent.{bucket_id,event}).
            assert!(
                v.get("event").is_some(),
                "each NDJSON object carries its matched event: {line}"
            );
            assert!(
                v.get("bucket_id").is_some(),
                "each NDJSON object carries its bucket origin: {line}"
            );
        }
    }

    #[test]
    fn pull_empty_open_sub_returns_promptly_and_exits_zero() {
        // The CRITICAL property of the one-shot verb: against an OPEN sub with NO
        // pending events it must return PROMPTLY and exit 0 -- it must NOT loop
        // (that is `subscription-stream`'s job and would wedge a Stop hook).
        use terminal_commander_core::Severity;
        use terminal_commander_ipc::{
            DaemonClient, IpcRequest, IpcResponse, SubscriptionOpenParams, SubscriptionPredicate,
            SubscriptionSourceSel,
        };

        let daemon = LiveDaemon::spawn("pull-empty");
        let endpoint = daemon.endpoint.clone();

        // Open a subscription (no command started -> no events will be pending).
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("seed runtime");
        let sub_id = rt.block_on(async {
            let client = DaemonClient::new(endpoint.clone()).with_timeout(Duration::from_secs(10));
            let open = client
                .call(
                    1,
                    IpcRequest::SubscriptionOpen(SubscriptionOpenParams {
                        predicate: SubscriptionPredicate {
                            severity_min: Some(Severity::High),
                            kind: None,
                            sources: SubscriptionSourceSel::All,
                            tag: None,
                        },
                    }),
                )
                .await
                .expect("subscription_open");
            match open {
                IpcResponse::SubscriptionOpen(r) => r.sub_id,
                other => panic!("unexpected open response: {other:?}"),
            }
        });

        // One-shot pull with the default small timeout. Time it: a single ~1 s
        // server wait + client overhead is well under the ceiling, whereas a
        // looping stream would never return.
        let start = Instant::now();
        let out = daemon.run_cli(&["subscription-pull", &sub_id, "--max", "20"]);
        let elapsed = start.elapsed();

        assert!(
            out.status.success(),
            "empty one-shot pull must exit 0; stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stdout.is_empty(),
            "empty pull emits no event rows; got stdout={:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        // The non-loop guarantee: a looping stream would block ~8 s/pull forever.
        // The one-shot returns in ~1 s; allow generous slack for CI scheduling.
        assert!(
            elapsed < Duration::from_secs(6),
            "one-shot pull must NOT loop/block; returned in {elapsed:?}"
        );
    }
}
