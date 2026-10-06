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
use terminal_commander_ipc::{CommandStartResponse, CommandStatusResponse};
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

/// The allocation helper, driven by env (run normally it does nothing):
/// - `TC_TEST_OOM_FIRST=1` (Linux): raise this process's OOM score so a
///   cgroup OOM kill picks it over a sibling job.
/// - `TC_TEST_ALLOC_MIB=n`: allocate and touch n MiB in 1 MiB steps; under a
///   lower ceiling the allocator fails and the process aborts (or the kernel
///   kills it).
/// - `TC_TEST_FILL=1`: reserve 1 MiB steps until the allocator refuses, then
///   keep going, so the job sits at its ceiling without failing.
/// - `TC_TEST_SLEEP_MS=n`: print `alloc_helper: ready`, then sleep.
#[test]
fn alloc_helper() {
    let var = |k: &str| std::env::var(k).ok();
    if var("TC_TEST_OOM_FIRST").is_some() && cfg!(target_os = "linux") {
        let _ = std::fs::write("/proc/self/oom_score_adj", "1000");
    }
    let mut keep: Vec<Vec<u8>> = Vec::new();
    if let Some(mib) = var("TC_TEST_ALLOC_MIB").and_then(|v| v.parse::<usize>().ok()) {
        for _ in 0..mib {
            keep.push(vec![1u8; 1 << 20]);
        }
        println!("alloc_helper: allocated {mib} MiB without being stopped");
    }
    if var("TC_TEST_FILL").is_some() {
        loop {
            let mut chunk: Vec<u8> = Vec::new();
            if chunk.try_reserve_exact(1 << 20).is_err() {
                break;
            }
            chunk.resize(1 << 20, 1);
            keep.push(chunk);
        }
        println!("alloc_helper: filled {} MiB", keep.len());
    }
    std::hint::black_box(&keep);
    if let Some(ms) = var("TC_TEST_SLEEP_MS").and_then(|v| v.parse::<u64>().ok()) {
        println!("alloc_helper: ready");
        std::thread::sleep(Duration::from_millis(ms));
    }
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

/// `command_start_combed` with optional `limits` and env; the typed reply.
async fn start_typed(
    d: &Daemon,
    limits: Option<Value>,
    env: &[(&str, &str)],
    nonce: Option<&str>,
) -> CommandStartResponse {
    let mut params = json!({ "argv": helper_argv(), "grace_ms": 2000 });
    if let Some(l) = limits {
        params["limits"] = l;
    }
    if !env.is_empty() {
        params["env"] = json!(env);
    }
    if let Some(n) = nonce {
        params["dedup_nonce"] = json!(n);
    }
    match d
        .client
        .call(1, request("command_start_combed", &params))
        .await
    {
        Ok(IpcResponse::CommandStartCombed(r)) => r,
        other => panic!("unexpected start reply: {other:?}"),
    }
}

/// [`start_typed`] as JSON.
async fn start(d: &Daemon, limits: Option<Value>, env: &[(&str, &str)]) -> Value {
    serde_json::to_value(start_typed(d, limits, env, None).await).unwrap()
}

async fn status_typed(d: &Daemon, job_id: &Value) -> CommandStatusResponse {
    let reply = d
        .client
        .call(2, request("command_status", &json!({ "job_id": job_id })))
        .await;
    let IpcResponse::CommandStatus(s) = reply.unwrap() else {
        panic!("unexpected status reply");
    };
    s
}

async fn stop(d: &Daemon, method: &str, job_id: &Value) {
    d.client
        .call(4, request(method, &json!({ "job_id": job_id })))
        .await
        .unwrap_or_else(|e| panic!("{method} failed: {e:?}"));
}

/// Poll the output tail until the helper prints `alloc_helper: ready`.
async fn wait_ready(d: &Daemon, job_id: &Value) {
    let deadline = Instant::now() + Duration::from_mins(1);
    loop {
        let reply = d
            .client
            .call(
                5,
                request(
                    "command_output_tail",
                    &json!({ "job_id": job_id, "max_lines": 50, "max_bytes": 8192 }),
                ),
            )
            .await;
        if let Ok(IpcResponse::CommandOutputTail(t)) = reply
            && t.lines.iter().any(|l| l.contains("alloc_helper: ready"))
        {
            return;
        }
        assert!(Instant::now() < deadline, "helper never became ready");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn policy_governor(d: &Daemon) -> terminal_commander_ipc::GovernorStatus {
    let IpcResponse::PolicyStatus(p) = d.client.call(9, IpcRequest::PolicyStatus).await.unwrap()
    else {
        panic!("unexpected reply");
    };
    p.governor.expect("governor block present")
}

fn audit_actions(d: &Daemon, subject: &str) -> Vec<(String, Option<String>, Option<String>)> {
    d.state
        .store
        .audit_since(&AuditReadRequest::new(0))
        .unwrap()
        .into_iter()
        .filter(|row| row.subject == subject)
        .map(|row| (row.action, row.reason, row.metadata_json))
        .collect()
}

/// Wire bytes with the volatile parts (ids, every number) blanked, so two
/// runs of the same job compare byte for byte, field order included.
fn normalize(wire: &str) -> String {
    let mut s = wire.to_owned();
    for key in ["\"job_id\":\"", "\"bucket_id\":\"", "\"probe_id\":\""] {
        let mut from = 0;
        while let Some(i) = s[from..].find(key) {
            let start = from + i + key.len();
            let end = start + s[start..].find('"').unwrap();
            s.replace_range(start..end, "ID");
            from = start + 2;
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('0');
            }
            in_digits = true;
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
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

const GOVERNOR_STATUS_KEYS: [&str; 4] = [
    "governor",
    "limits_applied",
    "peak_memory_bytes",
    "exit_reason",
];

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
             [governor]\ndefault_job_memory = \"{raw}\"\nhost_ceiling = \"none\"\n"
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
    let percent_note = g
        .status()
        .note
        .is_some_and(|n| n.contains("needs host memory"));
    assert_eq!(percent_note, !host_known);

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
        let clamp = rows
            .iter()
            .find(|row| row.action == "governor_clamp" && row.subject == job)
            .expect("no governor_clamp audit row");
        let meta: Value = serde_json::from_str(clamp.metadata_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            meta["requested"],
            json!({ "memory": "2GiB", "priority": "normal" })
        );
        assert_eq!(
            meta["applied"],
            json!({ "memory_bytes": GIB, "priority": "below_normal" })
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
/// `limits`. Compared as wire BYTES: the disabled run equals an enabled run
/// of the same job with its governor fields cleared (what an ungoverned
/// daemon emits), field order included, ids and numbers blanked.
#[test]
fn disabled_governor_is_byte_identical_to_ungoverned() {
    rt().block_on(async {
        let off = daemon(|c| c.governor.enabled = false);
        let on = daemon(|c| c.governor.default_job_memory = "1GiB".to_owned());
        let limits = Some(json!({ "memory": "512MiB" }));

        let start_off = start_typed(&off, limits.clone(), &[], None).await;
        let job_off = serde_json::to_value(start_off.job_id).unwrap();
        wait_terminal(&off, &job_off).await;
        let status_off = status_typed(&off, &job_off).await;
        let mut start_on = start_typed(&on, limits, &[], None).await;
        let job_on = serde_json::to_value(start_on.job_id).unwrap();
        wait_terminal(&on, &job_on).await;
        let mut status_on = status_typed(&on, &job_on).await;

        // The enabled run really was governed, so the comparison is not vacuous.
        assert!(start_on.limits_applied.is_some(), "{start_on:?}");
        assert!(start_on.governor.is_some(), "{start_on:?}");
        assert!(status_on.governor.is_some(), "{status_on:?}");
        assert!(status_on.limits_applied.is_some(), "{status_on:?}");

        start_on.limits_applied = None;
        start_on.limits_clamped.clear();
        start_on.governor = None;
        status_on.governor = None;
        status_on.limits_applied = None;
        status_on.peak_memory_bytes = None;
        status_on.exit_reason = None;
        let off_bytes = serde_json::to_string(&start_off).unwrap();
        println!("disabled start: {off_bytes}");
        assert_eq!(
            normalize(&off_bytes),
            normalize(&serde_json::to_string(&start_on).unwrap())
        );
        let off_bytes = serde_json::to_string(&status_off).unwrap();
        println!("disabled status: {off_bytes}");
        assert_eq!(
            normalize(&off_bytes),
            normalize(&serde_json::to_string(&status_on).unwrap())
        );

        let g = policy_governor(&off).await;
        assert!(!g.enabled);
        assert_eq!(g.host_ceiling_bytes, None);
        assert_eq!(g.host_ceiling_mode, None);
    });
}

/// Dedup: the limits are part of a start's identity. Same argv inside the
/// window with different limits starts a second job; the same nonce collapses
/// to the first job and the reply still carries its governor fields.
#[test]
fn dedup_keys_on_limits_and_a_duplicate_reports_them() {
    rt().block_on(async {
        let d = daemon(|_| {});
        let sleep = [("TC_TEST_SLEEP_MS", "20000")];
        let small = Some(json!({ "memory": "256MiB" }));

        let first = start_typed(&d, small.clone(), &sleep, Some("gov-nonce")).await;
        let dup = start_typed(&d, small.clone(), &sleep, Some("gov-nonce")).await;
        assert_eq!(first.job_id, dup.job_id, "same nonce collapses");
        assert!(
            first.limits_applied.is_some() && first.governor.is_some(),
            "{first:?}"
        );
        assert_eq!(dup.limits_applied, first.limits_applied, "{dup:?}");
        assert_eq!(dup.governor, first.governor, "{dup:?}");

        let fallback = start_typed(&d, small.clone(), &sleep, None).await;
        let fallback_dup = start_typed(&d, small, &sleep, None).await;
        assert_eq!(
            fallback.job_id, fallback_dup.job_id,
            "control: same limits collapse"
        );
        assert_eq!(fallback_dup.limits_applied, fallback.limits_applied);
        let other = start_typed(&d, Some(json!({ "memory": "128MiB" })), &sleep, None).await;
        assert_ne!(
            other.job_id, fallback.job_id,
            "different limits are a different job"
        );
        assert_eq!(
            other.limits_applied.and_then(|l| l.memory_bytes),
            Some(128 * MIB)
        );

        for job in [first.job_id, fallback.job_id, other.job_id] {
            let job = serde_json::to_value(job).unwrap();
            stop(&d, "command_stop", &job).await;
            wait_terminal(&d, &job).await;
        }
    });
}

/// A running governed job shows its governor and limits, never a peak or an
/// exit reason; a stopped job never carries `exit_reason` (both lanes). On
/// Windows the job is first filled to its ceiling, so a stop there is the
/// case the old code reported as a ceiling hit.
#[test]
fn running_status_shows_governor_and_a_stop_never_reports_the_ceiling() {
    rt().block_on(async {
        let d = daemon(|_| {});
        let mut env = vec![("TC_TEST_SLEEP_MS", "60000")];
        if cfg!(windows) {
            env.push(("TC_TEST_FILL", "1"));
        }
        let limits = Some(json!({ "memory": "64MiB" }));
        let r = start(&d, limits.clone(), &env).await;
        assert!(r.get("governor").is_some(), "start carries governor: {r}");
        wait_ready(&d, &r["job_id"]).await;
        let running = serde_json::to_value(status_typed(&d, &r["job_id"]).await).unwrap();
        println!("running status: {running}");
        assert_eq!(running["state"], json!("running"), "{running}");
        assert_eq!(running["governor"], r["governor"], "{running}");
        assert_eq!(running["limits_applied"], r["limits_applied"], "{running}");
        assert!(running.get("peak_memory_bytes").is_none(), "{running}");
        assert!(running.get("exit_reason").is_none(), "{running}");

        stop(&d, "command_stop", &r["job_id"]).await;
        let done = wait_terminal(&d, &r["job_id"]).await;
        // Let the waiter publish the final report, then re-read.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let done2 = serde_json::to_value(status_typed(&d, &r["job_id"]).await).unwrap();
        println!("stopped status: {done2}");
        for v in [&done, &done2] {
            assert!(
                v.get("exit_reason").is_none(),
                "stop is not the ceiling: {v}"
            );
            assert!(v.get("governor").is_some(), "{v}");
            assert_eq!(v["limits_applied"], r["limits_applied"], "{v}");
        }

        // PTY lane.
        let params = json!({
            "argv": helper_argv(),
            "env": env,
            "limits": limits,
        });
        let IpcResponse::PtyCommandStart(p) = d
            .client
            .call(7, request("pty_command_start", &params))
            .await
            .unwrap()
        else {
            panic!("unexpected pty start reply");
        };
        let p = serde_json::to_value(p).unwrap();
        assert!(
            p.get("governor").is_some(),
            "pty start carries governor: {p}"
        );
        let pty_running = loop {
            let v = serde_json::to_value(status_typed(&d, &p["job_id"]).await).unwrap();
            if v["state"] == json!("running") {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(pty_running["governor"], p["governor"], "{pty_running}");
        assert_eq!(
            pty_running["limits_applied"], p["limits_applied"],
            "{pty_running}"
        );
        assert!(
            pty_running.get("peak_memory_bytes").is_none(),
            "{pty_running}"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        stop(&d, "pty_command_stop", &p["job_id"]).await;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let reply = d
                .client
                .call(
                    2,
                    request("command_status", &json!({ "job_id": p["job_id"] })),
                )
                .await;
            if let Ok(IpcResponse::CommandStatus(s)) = reply {
                let v = serde_json::to_value(s).unwrap();
                assert!(v.get("exit_reason").is_none(), "pty stop: {v}");
                assert_eq!(v["limits_applied"], p["limits_applied"], "pty stop: {v}");
                if v.get("peak_memory_bytes").is_some() || Instant::now() > deadline {
                    println!("pty stopped status: {v}");
                    break;
                }
            }
            assert!(Instant::now() < deadline + Duration::from_secs(1));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

/// The host ceiling bounds governed jobs in aggregate: two jobs summing past
/// a 150 MiB ceiling, each under its own limit, and the second's allocation
/// fails while the first keeps running. Where the host has no aggregate
/// primitive (rlimit), `policy_status` says so and nothing is faked.
///
/// The host ceiling is installed once per process, so whichever daemon boots
/// first in a shared test process (plain `cargo test`) fixes it. The scenario
/// therefore runs in a re-exec of this binary that owns a fresh process.
#[test]
fn host_ceiling_bounds_jobs_in_aggregate() {
    const CHILD: &str = "TC_TEST_HOST_CEILING_CHILD";
    if std::env::var_os(CHILD).is_some() {
        host_ceiling_scenario();
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "host_ceiling_bounds_jobs_in_aggregate",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stdout}{stderr}");
    assert!(
        out.status.success(),
        "host ceiling child failed: {}",
        out.status
    );
    assert!(
        stdout.contains("1 passed"),
        "the child must run the scenario: {stdout}"
    );
}

fn host_ceiling_scenario() {
    rt().block_on(async {
        let d = daemon(|c| {
            "150MiB".clone_into(&mut c.governor.host_ceiling);
            "none".clone_into(&mut c.governor.default_job_memory);
        });
        let g = policy_governor(&d).await;
        println!(
            "host ceiling: bytes={:?} mode={:?} available={:?}",
            g.host_ceiling_bytes, g.host_ceiling_mode, g.mode_available
        );
        let mode = g
            .host_ceiling_mode
            .clone()
            .expect("a ceiling was configured");
        if let terminal_commander_ipc::GovernorModeWire::Unavailable(why) = &mode {
            assert_ne!(
                g.mode_available,
                terminal_commander_ipc::GovernorModeWire::JobObject,
                "Windows must install the host job: {why}"
            );
            println!("host_ceiling_mode unavailable ({why}); aggregate not asserted");
            return;
        }
        assert_eq!(g.host_ceiling_bytes, Some(150 * MIB));

        let first = start(
            &d,
            Some(json!({ "memory": "140MiB" })),
            &[("TC_TEST_ALLOC_MIB", "100"), ("TC_TEST_SLEEP_MS", "60000")],
        )
        .await;
        wait_ready(&d, &first["job_id"]).await;
        let second = start(
            &d,
            // Different limits, so the fallback dedup (env is not part of the
            // fingerprint) cannot collapse this start into the first.
            Some(json!({ "memory": "130MiB" })),
            &[("TC_TEST_ALLOC_MIB", "100"), ("TC_TEST_OOM_FIRST", "1")],
        )
        .await;
        let s2 = wait_terminal(&d, &second["job_id"]).await;
        println!("second job: {s2}");
        assert_ne!(
            s2["exit_code"],
            json!(0),
            "second allocation must fail: {s2}"
        );
        // Its own 130 MiB limit was not reached, so the host ceiling is the
        // reported reason, and it is audited with the ceiling and the peak.
        assert_eq!(s2["exit_reason"], json!("host_ceiling"), "{s2}");
        let rows = audit_actions(&d, second["job_id"].as_str().unwrap());
        let row = rows
            .iter()
            .find(|(a, ..)| a == "governor_host_ceiling")
            .unwrap_or_else(|| panic!("no governor_host_ceiling audit row: {rows:?}"));
        let meta: Value = serde_json::from_str(row.2.as_deref().unwrap()).unwrap();
        assert_eq!(meta["memory_limit_bytes"], json!(150 * MIB), "{meta}");
        let s1 = serde_json::to_value(status_typed(&d, &first["job_id"]).await).unwrap();
        assert_eq!(s1["state"], json!("running"), "first job survives: {s1}");
        stop(&d, "command_stop", &first["job_id"]).await;
        wait_terminal(&d, &first["job_id"]).await;
    });
}

/// Acceptance 6: a governed job always runs and its status says how it was
/// governed. Under rlimit (a plain WSL session) the profile default memory
/// is not applied and policy_status explains why; an unavailable enforcer is
/// audited as `governor_unavailable`.
#[test]
fn a_job_runs_whatever_the_enforcer_and_status_says_how() {
    rt().block_on(async {
        let d = daemon(|c| c.governor.default_job_memory = "1GiB".to_owned());
        let g = policy_governor(&d).await;
        let r = start(&d, None, &[]).await;
        let status = wait_terminal(&d, &r["job_id"]).await;
        println!("mode_available={:?} status={status}", g.mode_available);
        assert_eq!(status["exit_code"], json!(0), "the job ran: {status}");
        let mode = status["governor"].clone();
        assert!(!mode.is_null(), "{status}");
        let rows = audit_actions(&d, r["job_id"].as_str().unwrap());
        let unavailable = rows.iter().any(|(a, ..)| a == "governor_unavailable");
        assert_eq!(
            unavailable,
            mode.get("unavailable").is_some() || status["host_ceiling_joined"] == json!(false),
            "governor_unavailable row iff the job ran ungoverned or outside the              host ceiling: {rows:?}"
        );
        if mode == json!("rlimit") {
            assert!(
                r["limits_applied"].get("memory_bytes").is_none(),
                "rlimit: default memory not applied: {r}"
            );
            let note = g.note.clone().unwrap_or_default();
            assert!(
                note.contains(terminal_commanderd::governor::RLIMIT_DEFAULT_NOTE),
                "{note}"
            );
            assert!(matches!(
                g.host_ceiling_mode,
                Some(terminal_commander_ipc::GovernorModeWire::Unavailable(_))
            ));
            // An explicit request is applied.
            let r = start(&d, Some(json!({ "memory": "512MiB" })), &[]).await;
            assert_eq!(r["limits_applied"]["memory_bytes"], json!(512 * MIB), "{r}");
            wait_terminal(&d, &r["job_id"]).await;
        }
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

/// Unix PTY lane: a governed PTY job over its ceiling reports its mode and
/// fails; in cgroup mode the kernel's OOM kill is the reported exit reason.
#[cfg(unix)]
#[test]
fn governed_unix_pty_over_the_ceiling_reports() {
    rt().block_on(async {
        let d = daemon(|_| {});
        let params = json!({
            "argv": helper_argv(),
            "env": [["TC_TEST_ALLOC_MIB", "300"]],
            "limits": { "memory": "100MiB" },
        });
        let IpcResponse::PtyCommandStart(r) = d
            .client
            .call(1, request("pty_command_start", &params))
            .await
            .unwrap()
        else {
            panic!("unexpected pty start reply");
        };
        let r = serde_json::to_value(r).unwrap();
        assert_eq!(r["limits_applied"]["memory_bytes"], json!(100 * MIB), "{r}");
        let status = wait_terminal(&d, &r["job_id"]).await;
        println!("unix pty governed status: {status}");
        let mode = status["governor"].clone();
        assert_eq!(mode, r["governor"], "{status}");
        assert_ne!(status["exit_code"], json!(0), "{status}");
        if mode == json!("cgroup") {
            assert_eq!(status["exit_reason"], json!("memory_ceiling"), "{status}");
        } else {
            println!("unix pty governor mode: {mode} (failure asserted)");
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
