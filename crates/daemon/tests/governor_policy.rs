// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Resource governor policy through the daemon IPC: the `[governor]` block in
//! `policy_status`, per-start limit resolution and clamping, byte-identical
//! responses when the governor is disabled, and one end-to-end over-limit
//! allocation stopped by the kernel.
//!
//! The allocating child is this test binary itself, re-invoked with
//! `--exact alloc_helper` and `TC_TEST_ALLOC_MIB`, so no extra program is
//! needed. Run normally, `alloc_helper` sees no variable and does nothing.

#![cfg(any(unix, windows))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::governor::Governor;
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, GovernorSection, IpcRequest, IpcResponse,
    PolicyProfile,
};

#[cfg(unix)]
type ServerHandle = terminal_commanderd::ServerHandle;
#[cfg(windows)]
type ServerHandle = terminal_commanderd::PipeServerHandle;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// The allocation helper. Allocates and touches `TC_TEST_ALLOC_MIB` MiB in
/// 1 MiB steps; under a lower ceiling the allocator fails and the process
/// aborts (or the kernel kills it).
#[test]
fn alloc_helper() {
    let Some(mib) = std::env::var("TC_TEST_ALLOC_MIB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    else {
        return;
    };
    let mut keep: Vec<Vec<u8>> = Vec::with_capacity(mib);
    for _ in 0..mib {
        keep.push(vec![1u8; 1 << 20]);
    }
    std::hint::black_box(&keep);
    println!("alloc_helper: allocated {mib} MiB without being stopped");
}

struct Daemon {
    state: Arc<DaemonState>,
    client: DaemonClient,
    _handle: ServerHandle,
    _data: tempfile::TempDir,
}

fn serve(state: &Arc<DaemonState>) -> (PathBuf, ServerHandle) {
    #[cfg(unix)]
    {
        let handle =
            terminal_commanderd::IpcServer::new(Arc::clone(state), state.config.socket_path())
                .spawn()
                .unwrap();
        (handle.socket_path().to_path_buf(), handle)
    }
    #[cfg(windows)]
    {
        let name = format!(
            r"\\.\pipe\tc-test-governor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let handle = terminal_commanderd::PipeServer::new(Arc::clone(state), name.clone())
            .spawn()
            .unwrap();
        (PathBuf::from(name), handle)
    }
}

/// Must run inside a tokio runtime (the server spawns onto it).
fn daemon(configure: impl FnOnce(&mut DaemonConfig)) -> Daemon {
    let data = tempfile::tempdir().unwrap();
    let mut cfg = DaemonConfig::defaults_in(data.path());
    configure(&mut cfg);
    let state = Arc::new(DaemonState::bootstrap(cfg).unwrap());
    let (endpoint, handle) = serve(&state);
    Daemon {
        state,
        client: DaemonClient::new(endpoint).with_timeout(Duration::from_mins(1)),
        _handle: handle,
        _data: data,
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn request(method: &str, params: &Value) -> IpcRequest {
    serde_json::from_value(json!({"method": method, "params": params})).unwrap()
}

fn helper_argv() -> Vec<String> {
    vec![
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        "--exact".to_owned(),
        "alloc_helper".to_owned(),
        "--nocapture".to_owned(),
    ]
}

/// `command_start_combed` with optional `limits` and env; the start response
/// as JSON.
async fn start(d: &Daemon, limits: Option<Value>, env: &[(&str, &str)]) -> Value {
    let mut params = json!({ "argv": helper_argv(), "grace_ms": 2000 });
    if let Some(l) = limits {
        params["limits"] = l;
    }
    if !env.is_empty() {
        params["env"] = json!(env);
    }
    match d
        .client
        .call(1, request("command_start_combed", &params))
        .await
    {
        Ok(IpcResponse::CommandStartCombed(r)) => serde_json::to_value(r).unwrap(),
        other => panic!("unexpected start reply: {other:?}"),
    }
}

/// Poll `command_status` until the job is terminal; the status as JSON.
async fn wait_terminal(d: &Daemon, job_id: &Value) -> Value {
    let deadline = Instant::now() + Duration::from_mins(2);
    loop {
        let reply = d
            .client
            .call(2, request("command_status", &json!({ "job_id": job_id })))
            .await;
        let IpcResponse::CommandStatus(s) = reply.unwrap() else {
            panic!("unexpected status reply");
        };
        let v = serde_json::to_value(s).unwrap();
        if matches!(v["state"].as_str(), Some("exited" | "failed" | "cancelled")) {
            return v;
        }
        assert!(Instant::now() < deadline, "job never finished: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

const GOVERNOR_START_KEYS: [&str; 2] = ["limits_applied", "limits_clamped"];
const GOVERNOR_STATUS_KEYS: [&str; 3] = ["governor", "peak_memory_bytes", "exit_reason"];

#[test]
fn policy_status_reports_the_governor_block() {
    rt().block_on(async {
        let d = daemon(|_| {});
        let IpcResponse::PolicyStatus(p) =
            d.client.call(1, IpcRequest::PolicyStatus).await.unwrap()
        else {
            panic!("unexpected reply");
        };
        let g = p.governor.expect("governor block present");
        assert!(g.enabled);
        assert_eq!(
            g.default_priority,
            terminal_commander_ipc::JobPriority::BelowNormal
        );
        assert!(g.llm_can_raise_limits, "full_access may raise");
        let v = serde_json::to_value(&g).unwrap();
        println!("policy_status governor: {v}");
        // 60% of host memory when the host reports it; otherwise no limit and
        // a note saying so.
        assert!(
            g.default_job_memory_bytes.is_some() || g.note.is_some(),
            "{v}"
        );

        let hardened = daemon(|c| c.policy.profile = PolicyProfile::DeveloperLocal);
        let IpcResponse::PolicyStatus(p) = hardened
            .client
            .call(1, IpcRequest::PolicyStatus)
            .await
            .unwrap()
        else {
            panic!("unexpected reply");
        };
        assert!(!p.governor.unwrap().llm_can_raise_limits);
    });
}

/// `default_job_memory` strings: what each resolves to and which ones warn.
#[test]
fn default_job_memory_parsing_table() {
    let host_known = terminal_commander_probes::governor::host_memory().is_some();
    let ok: [(&str, Option<u64>); 6] = [
        ("24GiB", Some(24 * GIB)),
        ("512MiB", Some(512 * MIB)),
        ("123456", Some(123_456)),
        ("1TiB", Some(1 << 40)),
        ("none", None),
        ("NONE", None),
    ];
    for (raw, want) in ok {
        let cfg = DaemonConfig::from_toml(&format!(
            "[daemon]\ndata_dir = \"/tmp/tc-gov\"\n[policy]\nprofile = \"full_access\"\n\
             [governor]\ndefault_job_memory = \"{raw}\"\n"
        ))
        .unwrap();
        assert!(
            !cfg.warnings.iter().any(|w| w.contains("governor")),
            "{raw}: {:?}",
            cfg.warnings
        );
        let g = Governor::from_section(&cfg.governor, cfg.policy.profile);
        assert_eq!(g.default_memory_bytes, want, "{raw}");
    }
    // A percent needs host memory; when the host reports none the default is
    // no limit and the status says why.
    let g = Governor::from_section(
        &GovernorSection {
            default_job_memory: "40%".to_owned(),
            ..GovernorSection::default()
        },
        PolicyProfile::FullAccess,
    );
    assert_eq!(g.default_memory_bytes.is_some(), host_known);
    assert_eq!(g.status().note.is_some(), !host_known);

    // Malformed values warn and fall back to the built-in 60% / below_normal;
    // the daemon still boots.
    for bad in ["0", "0%", "101%", "lots", "1.5GiB", "24GB", ""] {
        let cfg = DaemonConfig::from_toml(&format!(
            "[daemon]\ndata_dir = \"/tmp/tc-gov\"\n[policy]\nprofile = \"full_access\"\n\
             [governor]\ndefault_job_memory = \"{bad}\"\ndefault_priority = \"urgent\"\n"
        ))
        .unwrap();
        let gov = cfg
            .warnings
            .iter()
            .filter(|w| w.contains("governor."))
            .count();
        assert_eq!(gov, 2, "{bad:?}: {:?}", cfg.warnings);
        let g = Governor::from_section(&cfg.governor, cfg.policy.profile);
        let builtin = Governor::from_section(&GovernorSection::default(), cfg.policy.profile);
        assert_eq!(
            g.default_memory_bytes, builtin.default_memory_bytes,
            "{bad:?}"
        );
        assert_eq!(
            g.default_priority,
            terminal_commander_ipc::JobPriority::BelowNormal
        );
    }
}

/// Acceptance 5: with `llm_can_raise_limits` false (a hardened profile's
/// default) a request above the defaults is clamped and reported; under
/// `full_access` the same request is honoured.
#[test]
fn hardened_profile_clamps_a_raise_and_full_access_does_not() {
    rt().block_on(async {
        let raise = json!({ "memory": "2GiB", "priority": "normal" });
        let set_default = |c: &mut DaemonConfig| c.governor.default_job_memory = "1GiB".to_owned();

        let hardened = daemon(|c| {
            set_default(c);
            c.policy.profile = PolicyProfile::DeveloperLocal;
        });
        let r = start(&hardened, Some(raise.clone()), &[]).await;
        assert_eq!(r["limits_clamped"], json!(["memory", "priority"]), "{r}");
        assert_eq!(
            r["limits_applied"],
            json!({ "memory_bytes": GIB, "priority": "below_normal" }),
            "{r}"
        );
        let job = r["job_id"].as_str().unwrap().to_owned();
        let status = wait_terminal(&hardened, &r["job_id"]).await;
        assert_eq!(status["exit_code"], json!(0), "{status}");
        let rows = hardened
            .state
            .store
            .audit_since(&AuditReadRequest::new(0))
            .unwrap();
        assert!(
            rows.iter()
                .any(|row| row.action == "governor_clamp" && row.subject == job),
            "no governor_clamp audit row"
        );

        let open = daemon(set_default);
        let r = start(&open, Some(raise), &[]).await;
        assert!(r.get("limits_clamped").is_none(), "{r}");
        assert_eq!(
            r["limits_applied"],
            json!({ "memory_bytes": 2 * GIB, "priority": "normal" }),
            "{r}"
        );
        wait_terminal(&open, &r["job_id"]).await;

        // Malformed request: a typed caller error, nothing spawned.
        let err = open
            .client
            .call(
                3,
                request(
                    "command_start_combed",
                    &json!({ "argv": helper_argv(), "limits": { "memory": "lots" } }),
                ),
            )
            .await
            .expect_err("bad limits must be refused");
        assert_eq!(err.code, terminal_commanderd::IpcErrorCode::ArgvInvalid);
    });
}

/// Acceptance 8: `governor.enabled = false` serializes no governor field on
/// the start response or the terminal status, even when the request carries
/// `limits`; the key sets equal an enabled run's minus the governor keys.
#[test]
fn disabled_governor_is_byte_identical_to_ungoverned() {
    rt().block_on(async {
        let off = daemon(|c| c.governor.enabled = false);
        let on = daemon(|c| c.governor.default_job_memory = "1GiB".to_owned());
        let limits = Some(json!({ "memory": "512MiB" }));

        let start_off = start(&off, limits.clone(), &[]).await;
        let status_off = wait_terminal(&off, &start_off["job_id"]).await;
        let start_on = start(&on, limits, &[]).await;
        let status_on = wait_terminal(&on, &start_on["job_id"]).await;
        println!("disabled start: {start_off}\ndisabled status: {status_off}");

        let strip = |v: &Value, gov: &[&str]| -> Vec<String> {
            keys(v)
                .into_iter()
                .filter(|k| !gov.contains(&k.as_str()))
                .collect()
        };
        for k in GOVERNOR_START_KEYS {
            assert!(start_off.get(k).is_none(), "{k} in {start_off}");
        }
        for k in GOVERNOR_STATUS_KEYS {
            assert!(status_off.get(k).is_none(), "{k} in {status_off}");
        }
        assert_eq!(keys(&start_off), strip(&start_on, &GOVERNOR_START_KEYS));
        assert_eq!(keys(&status_off), strip(&status_on, &GOVERNOR_STATUS_KEYS));
        // The enabled run really was governed, so the comparison is not vacuous.
        assert!(start_on.get("limits_applied").is_some(), "{start_on}");
        assert!(status_on.get("governor").is_some(), "{status_on}");

        let IpcResponse::PolicyStatus(p) =
            off.client.call(9, IpcRequest::PolicyStatus).await.unwrap()
        else {
            panic!("unexpected reply");
        };
        assert!(!p.governor.unwrap().enabled);
    });
}

/// Acceptance 1 end to end: a 100 MiB ceiling stops a 300 MiB allocation and
/// the receipt says so.
#[test]
fn governed_allocation_over_the_ceiling_is_stopped() {
    rt().block_on(async {
        let d = daemon(|_| {});
        let r = start(
            &d,
            Some(json!({ "memory": "100MiB" })),
            &[("TC_TEST_ALLOC_MIB", "300")],
        )
        .await;
        assert_eq!(r["limits_applied"]["memory_bytes"], json!(100 * MIB), "{r}");
        let status = wait_terminal(&d, &r["job_id"]).await;
        println!("governed status: {status}");
        let mode = status["governor"].clone();
        assert!(
            !mode.is_null(),
            "governed job must report its mode: {status}"
        );
        assert_ne!(
            status["exit_code"],
            json!(0),
            "the allocation must fail: {status}"
        );
        if cfg!(windows) {
            assert_eq!(mode, json!("job_object"), "{status}");
            assert_eq!(status["exit_reason"], json!("memory_ceiling"), "{status}");
            assert!(
                status["peak_memory_bytes"].as_u64().unwrap() >= 100 * MIB,
                "{status}"
            );
            let rows = d
                .state
                .store
                .audit_since(&AuditReadRequest::new(0))
                .unwrap();
            assert!(
                rows.iter()
                    .any(|row| row.action == "governor_memory_ceiling"),
                "no governor_memory_ceiling audit row"
            );
        } else if mode == json!("cgroup") {
            // The cgroup is the whole tree's ceiling: the kernel OOM-kills
            // the job and memory.events records it.
            println!("unix governor mode: cgroup (ceiling asserted)");
            assert_eq!(status["exit_reason"], json!("memory_ceiling"), "{status}");
            if let Some(peak) = status["peak_memory_bytes"].as_u64() {
                assert!(peak >= 100 * MIB, "{status}");
            } else {
                // memory.peak needs kernel 5.19+.
                println!("memory.peak not available on this kernel");
            }
        } else {
            // rlimit: per-process RLIMIT_DATA; a hit is not observable.
            println!("unix governor mode: {mode} (failure asserted, ceiling not observable)");
        }

        // The durable receipt keeps the governor fields, so a status
        // reconstructed after a restart reports them too.
        let job: terminal_commander_core::JobId =
            serde_json::from_value(r["job_id"].clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let rebuilt = loop {
            if let Some(s) = d.state.command.reconstructed_status(job) {
                break serde_json::to_value(s).unwrap();
            }
            assert!(Instant::now() < deadline, "receipt never persisted");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        for k in GOVERNOR_STATUS_KEYS {
            assert_eq!(rebuilt.get(k), status.get(k), "{k}: {rebuilt}");
        }
    });
}

/// The PTY lane reads the governor report deterministically: a governed
/// ConPTY job over its ceiling carries `governor` and `exit_reason` on every
/// run, never a silently missing report.
#[cfg(windows)]
#[test]
fn governed_conpty_over_the_ceiling_always_reports() {
    rt().block_on(async {
        let d = daemon(|_| {});
        for run in 1..=5 {
            let params = json!({
                "argv": helper_argv(),
                "env": [["TC_TEST_ALLOC_MIB", "300"]],
                "limits": { "memory": "100MiB" },
            });
            let IpcResponse::PtyCommandStart(r) = d
                .client
                .call(run, request("pty_command_start", &params))
                .await
                .unwrap()
            else {
                panic!("unexpected pty start reply");
            };
            let r = serde_json::to_value(r).unwrap();
            assert_eq!(r["limits_applied"]["memory_bytes"], json!(100 * MIB), "{r}");
            let status = wait_terminal(&d, &r["job_id"]).await;
            println!("run {run}: {status}");
            assert_eq!(
                status["governor"],
                json!("job_object"),
                "run {run}: {status}"
            );
            assert_eq!(
                status["exit_reason"],
                json!("memory_ceiling"),
                "run {run}: {status}"
            );
            assert!(
                status["peak_memory_bytes"].as_u64().unwrap() >= 100 * MIB,
                "run {run}: {status}"
            );
        }
    });
}
