// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Cross-transport parity and security-boundary integration tests for
//! `terminal-commander-codemode`: side-by-side verification of
//! `DaemonClientCaller` (real UDS / Windows named-pipe IPC) and
//! `EmbeddedCodeCaller` (in-process `EmbeddedEngine` with zero sockets).

#![cfg(any(unix, windows))]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use terminal_commander_codemode::backend::BoxFuture;
use terminal_commander_codemode::{
    AsyncBridgeBackend, AsyncIpcCaller, CodeBackendError, DaemonClientCaller, ExecuteOutcome,
    ExecuteRequest, ExecuteStatus, execute_script_async,
};
use terminal_commander_core::{JobId, RecipeDefinition, RecipeStatus};
use terminal_commander_ipc::protocol::CredentialOwnerAction;
use terminal_commander_ipc::{
    AuditSinceParams, CredentialStatus, PtyCommandStartParams, PtyCommandStopParams,
    RecipeUpsertParams,
};
use terminal_commanderd::embedded::{
    EmbeddedAuthority, EmbeddedEngine, EmbeddedOptions, EmbeddedOwnerInteraction,
};
use terminal_commanderd::{
    DaemonClient, DaemonConfig, DaemonState, IpcRequest, IpcResponse, PolicyCapsSection,
    PolicyPathsSection, PolicyProfile,
};

#[cfg(unix)]
type ServerHandle = terminal_commanderd::ServerHandle;
#[cfg(windows)]
type ServerHandle = terminal_commanderd::PipeServerHandle;

const FIXTURE_ENV: &str = "TC_CODEMODE_PARITY_FIXTURE";
const SECRET_NEVER_SURFACED: &str = "s3cret-codemode-parity-Z9";

/// Minimal ~30-line `AsyncIpcCaller` adapter routing every Code Mode host call
/// through `EmbeddedEngine::execute` in-process with zero sockets, preserving
/// `PeerIdentity` and `EmbeddedAuthority`.
#[derive(Clone)]
struct EmbeddedCodeCaller(Arc<EmbeddedEngine>);

impl AsyncIpcCaller for EmbeddedCodeCaller {
    fn call_ipc<'a>(
        &'a self,
        request: IpcRequest,
        target_id: Option<&'a str>,
        _timeout: Duration,
    ) -> BoxFuture<'a, Result<IpcResponse, CodeBackendError>> {
        Box::pin(async move {
            if let Some(id) = target_id {
                return Err(CodeBackendError::refusal(
                    "unknown_target",
                    format!("unknown target_id '{id}' (embedded engine is local-only)"),
                ));
            }
            self.0
                .execute(request)
                .await
                .map_err(|err| CodeBackendError::from_ipc_error(&err))
        })
    }
}

fn serve_ipc(state: &Arc<DaemonState>) -> (PathBuf, ServerHandle) {
    #[cfg(unix)]
    {
        let handle =
            terminal_commanderd::IpcServer::new(Arc::clone(state), state.config.socket_path())
                .spawn()
                .expect("spawn unix ipc server");
        (handle.socket_path().to_path_buf(), handle)
    }
    #[cfg(windows)]
    {
        let name = state.config.pipe_name();
        let handle = terminal_commanderd::PipeServer::new(Arc::clone(state), name.clone())
            .spawn()
            .expect("spawn windows pipe server");
        (PathBuf::from(name), handle)
    }
}

#[allow(clippy::missing_const_for_fn)] // Non-const on Windows; no-op on Unix.
fn configure_unique_ipc_endpoint(cfg: &mut DaemonConfig, tag: &str) {
    #[cfg(windows)]
    {
        let pipe = format!(
            r"\\.\pipe\tc-codemode-parity-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        );
        cfg.daemon.socket_path = Some(PathBuf::from(pipe));
    }
    #[cfg(unix)]
    {
        let _ = (cfg, tag);
    }
}

struct DaemonIpcRig {
    state: Arc<DaemonState>,
    endpoint: PathBuf,
    handle: Option<ServerHandle>,
    client: DaemonClient,
}

impl DaemonIpcRig {
    fn start(mut cfg: DaemonConfig, tag: &str) -> Self {
        configure_unique_ipc_endpoint(&mut cfg, tag);
        let state = Arc::new(DaemonState::bootstrap(cfg).expect("bootstrap DaemonState"));
        let (endpoint, handle) = serve_ipc(&state);
        let client = DaemonClient::new(endpoint.clone()).with_timeout(Duration::from_secs(30));
        Self {
            state,
            endpoint,
            handle: Some(handle),
            client,
        }
    }

    async fn run_code(&self, code: &str) -> ExecuteOutcome {
        let caller = DaemonClientCaller::new(self.endpoint.clone())
            .with_default_timeout(Duration::from_secs(30));
        let backend = AsyncBridgeBackend::new(tokio::runtime::Handle::current(), caller);
        execute_script_async(ExecuteRequest::new(code), backend)
            .await
            .expect("execute_script_async over DaemonClientCaller")
    }

    async fn shutdown(mut self) {
        self.state.trigger_shutdown();
        if let Some(handle) = self.handle.take() {
            handle.shutdown().await;
        }
        let _ = self.state.watch.drain_watches().await;
        let _ = self.state.command.drain_lifecycle_tasks_report().await;
        let _ = self.state.pty.drain_lifecycle_tasks().await;
        let store = self.state.store.clone();
        let _ = tokio::task::spawn_blocking(move || store.shutdown()).await;
    }
}

async fn run_embedded_code(engine: &Arc<EmbeddedEngine>, code: &str) -> ExecuteOutcome {
    let backend = AsyncBridgeBackend::new(
        tokio::runtime::Handle::current(),
        EmbeddedCodeCaller(Arc::clone(engine)),
    );
    execute_script_async(ExecuteRequest::new(code), backend)
        .await
        .expect("execute_script_async over EmbeddedCodeCaller")
}

fn find_python() -> Option<String> {
    let candidates: &[&str] = if cfg!(windows) {
        &["python", "py"]
    } else {
        &["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"]
    };
    candidates
        .iter()
        .find(|candidate| {
            std::process::Command::new(candidate)
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
        })
        .map(|s| (*s).to_owned())
}

fn py_str(raw: &str) -> String {
    serde_json::to_string(raw).expect("json string literal is valid python string literal")
}

fn assert_no_file_contains(dir: &Path, needle: &[u8]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            assert_no_file_contains(&path, needle);
        } else if let Ok(bytes) = std::fs::read(&path) {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "secret leaked to disk at {}",
                path.display()
            );
        }
    }
}

/// Child fixture invoked by `codemode_end_to_end_workflow_parity` so the test
/// relies only on the compiled test binary itself (no shell or external tools).
#[test]
fn codemode_parity_triage_child() {
    if std::env::var_os(FIXTURE_ENV).is_none() {
        return;
    }
    let mut out = std::io::stdout();
    writeln!(out, "booting fixture worker").unwrap();
    writeln!(out, "ERROR[E0425]: unresolved symbol `alpha`").unwrap();
    writeln!(out, "context detail after alpha").unwrap();
    writeln!(out, "ERROR[E0308]: mismatched types in `beta`").unwrap();
    writeln!(out, "build summary: 2 errors emitted").unwrap();
    out.flush().unwrap();
}

// ---------------------------------------------------------------------------
// 1. End-to-end 1-turn workflow parity
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn end_to_end_one_turn_workflow_matches_between_daemon_ipc_and_embedded() {
    let temp = tempfile::tempdir().unwrap();
    let ipc_data = temp.path().join("ipc-data");
    let emb_data = temp.path().join("emb-data");
    let ipc_work = temp.path().join("ipc-work");
    let emb_work = temp.path().join("emb-work");
    std::fs::create_dir_all(&ipc_work).unwrap();
    std::fs::create_dir_all(&emb_work).unwrap();

    let ipc = DaemonIpcRig::start(DaemonConfig::defaults_in(&ipc_data), "e2e");
    let embedded = Arc::new(
        EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(&emb_data)).expect("bootstrap embed"),
    );

    let exe = py_str(&std::env::current_exe().unwrap().to_string_lossy());
    let build_script = |out_file: &Path| -> String {
        let out_path = py_str(&out_file.to_string_lossy());
        format!(
            r#"
exe = {exe}
report_path = {out_path}
run = run_and_watch(
    argv=[exe, "--exact", "codemode_parity_triage_child", "--nocapture"],
    env=[{{"key": "{FIXTURE_ENV}", "value": "1"}}],
    rules=[{{"pattern": r"ERROR\[E\d+\]:", "severity": "high", "event_kind": "compile_error"}}],
    wait_ms=15000,
    wait_until="exit",
)
events = bucket_events_since(bucket_id=run["bucket_id"], cursor=0, severity_min="high")
contexts = [
    event_context(event_id=ev["event_id"], bucket_id=run["bucket_id"], before=1, after=1)
    for ev in events["events"]
]
tail = command_output_tail(job_id=run["job_id"], max_lines=20, strip_ansi=True)
non_empty_tail = [ln for ln in tail["lines"] if "build summary:" in ln]
summary_text = f"errors={{len(events['events'])}};last={{non_empty_tail[-1]}}\n"
written = file_write(path=report_path, content=summary_text, create_dirs=True)
window = file_read_window(path=report_path, start_line=1, max_lines=5)
print("distilled", len(events["events"]), written["bytes_written"])
{{
    "state": run["state"],
    "exit_code": run["exit_code"],
    "complete": run["complete"],
    "degraded": run["degraded"],
    "outcome_trust": run["outcome_trust"],
    "signal_summaries": [s["summary"] for s in run["signals"]],
    "bucket_summaries": [e["summary"] for e in events["events"]],
    "context_anchor_hits": [
        any("ERROR[E" in fr["text"] for fr in ctx["frames"])
        for ctx in contexts
    ],
    "tail_last": non_empty_tail[-1],
    "bytes_written": written["bytes_written"],
    "read_back": [ln["text"] for ln in window["lines"]],
}}
"#
        )
    };

    let ipc_report = ipc_work.join("triage-report.txt");
    let emb_report = emb_work.join("triage-report.txt");

    let ipc_out = ipc.run_code(&build_script(&ipc_report)).await;
    let emb_out = run_embedded_code(&embedded, &build_script(&emb_report)).await;

    assert_eq!(
        ipc_out.status,
        ExecuteStatus::Ok,
        "ipc error: {:?}",
        ipc_out.error
    );
    assert_eq!(
        emb_out.status,
        ExecuteStatus::Ok,
        "embedded error: {:?}",
        emb_out.error
    );

    // 1 run_and_watch (Mutate) + 1 bucket_events_since (Read) + 2 event_context (Read)
    // + 1 command_output_tail (Read) + 1 file_write (Mutate) + 1 file_read_window (Read)
    // = 7 host calls, 2 mutations.
    assert_eq!(ipc_out.calls, 7);
    assert_eq!(ipc_out.mutations, 2);
    assert_eq!(ipc_out.calls, emb_out.calls);
    assert_eq!(ipc_out.mutations, emb_out.mutations);
    assert_eq!(ipc_out.stdout, emb_out.stdout);
    assert_eq!(ipc_out.value, emb_out.value);

    let expected = json!({
        "state": "exited",
        "exit_code": 0,
        "complete": true,
        "degraded": false,
        "outcome_trust": "observed",
        "signal_summaries": [
            "ERROR[E0425]: unresolved symbol `alpha`",
            "ERROR[E0308]: mismatched types in `beta`",
        ],
        "bucket_summaries": [
            "ERROR[E0425]: unresolved symbol `alpha`",
            "ERROR[E0308]: mismatched types in `beta`",
        ],
        "context_anchor_hits": [true, true],
        "tail_last": "build summary: 2 errors emitted",
        "bytes_written": 46,
        "read_back": ["errors=2;last=build summary: 2 errors emitted"],
    });
    assert_eq!(ipc_out.value, Some(expected));

    ipc.shutdown().await;
    let report = embedded.shutdown().await.unwrap();
    assert!(report.store_closed);
}

// ---------------------------------------------------------------------------
// 2. Hardened policy profile enforcement parity
// ---------------------------------------------------------------------------

fn hardened_policy_config(data_dir: &Path, allowed_repo: &Path) -> DaemonConfig {
    let mut cfg = DaemonConfig::defaults_in(data_dir);
    cfg.policy.profile = PolicyProfile::RepoOnly;
    cfg.policy.repo_root = Some(allowed_repo.to_path_buf());
    cfg.policy.caps = Some(PolicyCapsSection {
        allow_shell: Some(false),
        allow_session: Some(false),
        allow_privileged: Some(false),
        allow_remote: Some(false),
    });
    cfg.policy.llm_can_activate_recipes = Some(false);

    // Also configure `paths.write_allow` rooted at the canonical allowed repo so
    // both `RepoOnly` containment and `paths.write_allow` are exercised together.
    let canonical_repo =
        std::fs::canonicalize(allowed_repo).unwrap_or_else(|_| allowed_repo.to_path_buf());
    let mut glob_root = canonical_repo.to_string_lossy().replace('\\', "/");
    if let Some(stripped) = glob_root.strip_prefix("//?/") {
        glob_root = stripped.to_owned();
    }
    cfg.policy.paths = Some(PolicyPathsSection {
        read_allow: Vec::new(),
        write_allow: vec![format!("{glob_root}/**")],
        watch_allow: Vec::new(),
        deny_extra: Vec::new(),
    });
    cfg
}

fn seed_recipe_definition() -> RecipeDefinition {
    let exe = std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    RecipeDefinition {
        recipe_id: "parity.demo".to_owned(),
        version: 1,
        title: "Parity demo recipe".to_owned(),
        summary: "Test recipe for llm_can_activate_recipes gate".to_owned(),
        argv: vec![exe, "--version".to_owned()],
        status: RecipeStatus::Active,
        tags: vec!["test".to_owned()],
        cwd: None,
        env_allowlist: Vec::new(),
        timeout_ms: None,
        rule_pack_ids: Vec::new(),
        placeholders: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // Complete side-by-side policy matrix across both engines.
async fn hardened_policy_enforced_identically_across_daemon_ipc_and_embedded() {
    let temp = tempfile::tempdir().unwrap();
    let ipc_data = temp.path().join("ipc-data");
    let emb_data = temp.path().join("emb-data");
    let ipc_allowed = temp.path().join("ipc-allowed");
    let emb_allowed = temp.path().join("emb-allowed");
    let outside_dir = temp.path().join("outside");
    std::fs::create_dir_all(&ipc_allowed).unwrap();
    std::fs::create_dir_all(&emb_allowed).unwrap();
    std::fs::create_dir_all(&outside_dir).unwrap();

    let ipc = DaemonIpcRig::start(hardened_policy_config(&ipc_data, &ipc_allowed), "policy");
    // EmbeddedEngine::bootstrap uses EmbeddedAuthority::PolicyOnly by default.
    let embedded = Arc::new(
        EmbeddedEngine::bootstrap(hardened_policy_config(&emb_data, &emb_allowed))
            .expect("bootstrap hardened embedded engine"),
    );

    // Upsert an active recipe so `recipe_activate` hits the admin gate rather than RecipeNotFound.
    let recipe = seed_recipe_definition();
    ipc.client
        .call(
            1,
            IpcRequest::RecipeUpsert(RecipeUpsertParams {
                definition: recipe.clone(),
            }),
        )
        .await
        .expect("ipc recipe_upsert");
    embedded
        .recipe_upsert(RecipeUpsertParams { definition: recipe })
        .await
        .expect("embedded recipe_upsert");

    // 2a. `shell_exec` fails with `PolicyDenied` (`RuntimeError`).
    let shell_code = r#"shell_exec(shell_line="echo hi")"#;
    let ipc_shell = ipc.run_code(shell_code).await;
    let emb_shell = run_embedded_code(&embedded, shell_code).await;
    for (label, out) in [("ipc", &ipc_shell), ("embedded", &emb_shell)] {
        assert_eq!(out.status, ExecuteStatus::Error, "{label} must fail");
        let err = out.error.as_ref().expect("error info");
        assert_eq!(err.exception_type, "RuntimeError", "{label}");
        assert!(
            err.message.starts_with("PolicyDenied:"),
            "{label} expected PolicyDenied, got: {}",
            err.message
        );
    }

    // 2b. `run_and_watch` with a shell interpreter fails with `ShellInterpreterDenied`.
    let interp_code = if cfg!(windows) {
        r#"run_and_watch(argv=["cmd", "/c", "echo hi"])"#
    } else {
        r#"run_and_watch(argv=["sh", "-c", "echo hi"])"#
    };
    let ipc_interp = ipc.run_code(interp_code).await;
    let emb_interp = run_embedded_code(&embedded, interp_code).await;
    for (label, out) in [("ipc", &ipc_interp), ("embedded", &emb_interp)] {
        assert_eq!(out.status, ExecuteStatus::Error, "{label} must fail");
        let err = out.error.as_ref().expect("error info");
        assert_eq!(err.exception_type, "RuntimeError", "{label}");
        assert!(
            err.message.starts_with("ShellInterpreterDenied:"),
            "{label} expected ShellInterpreterDenied, got: {}",
            err.message
        );
    }

    // 2c. `file_write` inside `repo_root` (`write_allow`) succeeds; outside fails.
    let ipc_ok_path = py_str(&ipc_allowed.join("allowed.txt").to_string_lossy());
    let emb_ok_path = py_str(&emb_allowed.join("allowed.txt").to_string_lossy());
    let ipc_write_ok = ipc
        .run_code(&format!(r#"file_write(path={ipc_ok_path}, content="ok")"#))
        .await;
    let emb_write_ok = run_embedded_code(
        &embedded,
        &format!(r#"file_write(path={emb_ok_path}, content="ok")"#),
    )
    .await;
    assert_eq!(
        ipc_write_ok.status,
        ExecuteStatus::Ok,
        "ipc allowed write failed: {:?}",
        ipc_write_ok.error
    );
    assert_eq!(
        emb_write_ok.status,
        ExecuteStatus::Ok,
        "embedded allowed write failed: {:?}",
        emb_write_ok.error
    );

    let ipc_bad_path = py_str(&outside_dir.join("denied-ipc.txt").to_string_lossy());
    let emb_bad_path = py_str(&outside_dir.join("denied-emb.txt").to_string_lossy());
    let ipc_write_bad = ipc
        .run_code(&format!(
            r#"file_write(path={ipc_bad_path}, content="nope")"#
        ))
        .await;
    let emb_write_bad = run_embedded_code(
        &embedded,
        &format!(r#"file_write(path={emb_bad_path}, content="nope")"#),
    )
    .await;
    for (label, out) in [("ipc", &ipc_write_bad), ("embedded", &emb_write_bad)] {
        assert_eq!(
            out.status,
            ExecuteStatus::Error,
            "{label} outside write must fail"
        );
        let err = out.error.as_ref().expect("error info");
        assert_eq!(err.exception_type, "RuntimeError", "{label}");
        assert!(
            (err.message.starts_with("PathDenied:") || err.message.starts_with("PolicyDenied:"))
                && (err.message.contains("no_allow_rule")
                    || err.message.contains("outside repo_root")
                    || err.message.contains("policy denied")),
            "{label} expected policy/path denial, got: {}",
            err.message
        );
    }
    assert!(!outside_dir.join("denied-ipc.txt").exists());
    assert!(!outside_dir.join("denied-emb.txt").exists());

    // 2d. `recipe_activate` from Code Mode (`from_mcp: true`) under `llm_can_activate_recipes = false`
    // fails with `PolicyDenied` (`recipe_activate_requires_admin`) on BOTH DaemonClient and
    // `EmbeddedEngine::bootstrap` (`EmbeddedAuthority::PolicyOnly`).
    let activate_code = r#"recipe_activate(recipe_id="parity.demo", scope={"kind": "global"})"#;
    let ipc_act = ipc.run_code(activate_code).await;
    let emb_act = run_embedded_code(&embedded, activate_code).await;
    for (label, out) in [("ipc", &ipc_act), ("embedded", &emb_act)] {
        assert_eq!(
            out.status,
            ExecuteStatus::Error,
            "{label} recipe_activate must fail"
        );
        let err = out.error.as_ref().expect("error info");
        assert_eq!(err.exception_type, "RuntimeError", "{label}");
        assert!(
            err.message.starts_with("PolicyDenied:")
                && err.message.contains("recipe_activate_requires_admin"),
            "{label} expected PolicyDenied recipe_activate_requires_admin, got: {}",
            err.message
        );
    }

    ipc.shutdown().await;
    embedded.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// 3. OS-critical infrastructure failsafe (`os_critical_path_protected`)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn os_critical_path_failsafe_blocks_argv_and_shell_before_spawn_in_full_access() {
    let temp = tempfile::tempdir().unwrap();
    let ipc_data = temp.path().join("ipc-data");
    let emb_data = temp.path().join("emb-data");

    let mut ipc_cfg = DaemonConfig::defaults_in(&ipc_data);
    ipc_cfg.policy.profile = PolicyProfile::FullAccess;
    let mut emb_cfg = DaemonConfig::defaults_in(&emb_data);
    emb_cfg.policy.profile = PolicyProfile::FullAccess;

    let ipc = DaemonIpcRig::start(ipc_cfg, "osguard");
    let embedded = Arc::new(EmbeddedEngine::bootstrap(emb_cfg).expect("bootstrap full_access"));

    // On Windows native argv, bare `/usr/lib/...` is intercepted first by the
    // POSIX-path-on-Windows typo guard unless carried by `wsl` or using a native
    // Windows protected subtree (`C:\Windows\System32\...`). Test `wsl -e rm -rf /usr/lib/...`
    // (and native `C:\Windows\System32\...` on Windows; direct `/usr/lib/...` on Unix)
    // so `argv_deletion_hit` fires before spawn on every host OS.
    let argv_guard_scripts: Vec<&str> = if cfg!(windows) {
        vec![
            r#"run_and_watch(argv=["wsl", "-e", "rm", "-rf", "/usr/lib/tc-codemode-guard-nonexistent"])"#,
            r#"run_and_watch(argv=["rm", "-rf", r"C:\Windows\System32\tc-codemode-guard-nonexistent"])"#,
        ]
    } else {
        vec![r#"run_and_watch(argv=["rm", "-rf", "/usr/lib/tc-codemode-guard-nonexistent"])"#]
    };

    for script in argv_guard_scripts {
        let ipc_out = ipc.run_code(script).await;
        let emb_out = run_embedded_code(&embedded, script).await;
        for (label, out) in [("ipc", &ipc_out), ("embedded", &emb_out)] {
            assert_eq!(
                out.status,
                ExecuteStatus::Error,
                "{label} must refuse {script}"
            );
            let err = out.error.as_ref().expect("error info");
            assert_eq!(err.exception_type, "RuntimeError", "{label}");
            assert!(
                err.message.starts_with("OsCriticalPathProtected:")
                    && err.message.contains("OS-critical infrastructure"),
                "{label} expected OsCriticalPathProtected, got: {}",
                err.message
            );
        }
    }

    // Shell lane: `cd /tmp && sudo rm -rf /etc/pam.d/tc-codemode-guard-nonexistent`
    let shell_guard_script = r#"shell_exec(shell_line="cd /tmp && sudo rm -rf /etc/pam.d/tc-codemode-guard-nonexistent")"#;
    let ipc_shell = ipc.run_code(shell_guard_script).await;
    let emb_shell = run_embedded_code(&embedded, shell_guard_script).await;
    for (label, out) in [("ipc", &ipc_shell), ("embedded", &emb_shell)] {
        assert_eq!(out.status, ExecuteStatus::Error, "{label} must refuse");
        let err = out.error.as_ref().expect("error info");
        assert_eq!(err.exception_type, "RuntimeError", "{label}");
        assert!(
            err.message.starts_with("OsCriticalPathProtected:")
                && err.message.contains("OS-critical infrastructure"),
            "{label} expected OsCriticalPathProtected on shell_exec, got: {}",
            err.message
        );
    }

    // Verify zero live jobs were spawned on either engine.
    assert_eq!(ipc.state.command.live_jobs().len(), 0);
    let emb_probes = embedded
        .probe_list(terminal_commanderd::ListLimitParams { limit: None })
        .await
        .unwrap();
    assert_eq!(emb_probes.probes.len(), 0);

    ipc.shutdown().await;
    embedded.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// 4. PTY `SecretInputDenied` & headless `credential_request` gate
// ---------------------------------------------------------------------------

const SUDO_PROMPT_CHILD: &str = "import sys\n\
sys.stdout.write('[sudo] password for dev: ')\n\
sys.stdout.flush()\n\
line = sys.stdin.readline()\n\
sys.exit(0)\n";

async fn wait_for_pty_credential_prompt_ipc(client: &DaemonClient, job_id: JobId) {
    for _ in 0..400 {
        if let Ok(IpcResponse::PtyCommandList(list)) =
            client.call(99, IpcRequest::PtyCommandList).await
            && list
                .entries
                .iter()
                .any(|e| e.job_id == job_id && e.awaiting_credential.is_some())
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("IPC PTY job {job_id} never entered awaiting_credential");
}

async fn wait_for_pty_credential_prompt_embedded(engine: &EmbeddedEngine, job_id: JobId) {
    for _ in 0..400 {
        let list = engine.pty_command_list().await.unwrap();
        if list
            .entries
            .iter()
            .any(|e| e.job_id == job_id && e.awaiting_credential.is_some())
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("Embedded PTY job {job_id} never entered awaiting_credential");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // Complete headless PTY credential gate scenario across both engines.
async fn pty_secret_input_denied_and_headless_credential_request_gate_parity() {
    let Some(python) = find_python() else {
        eprintln!("skipping PTY credential parity test: python not found on PATH");
        return;
    };

    let temp = tempfile::tempdir().unwrap();
    let ipc_data = temp.path().join("ipc-cred");
    let emb_data = temp.path().join("emb-cred");

    // Headless prompter (`"none"`) on DaemonState: never opens OS CredUI / askpass.
    let mut ipc_cfg = DaemonConfig::defaults_in(&ipc_data);
    ipc_cfg.credential_prompter_test_seam = Some("none".to_owned());
    let ipc = DaemonIpcRig::start(ipc_cfg, "cred");

    // Default `EmbeddedOwnerInteraction::HostManaged` on EmbeddedEngine: headless challenge.
    let emb_cfg = DaemonConfig::defaults_in(&emb_data);
    let embedded = Arc::new(
        EmbeddedEngine::bootstrap_with_options(
            emb_cfg,
            EmbeddedOptions {
                authority: EmbeddedAuthority::PolicyOnly,
                owner_interaction: EmbeddedOwnerInteraction::HostManaged,
            },
        )
        .expect("bootstrap headless embedded engine"),
    );

    let start_params = PtyCommandStartParams {
        environment: None,
        argv: vec![
            python,
            "-u".to_owned(),
            "-c".to_owned(),
            SUDO_PROMPT_CHILD.to_owned(),
        ],
        cwd: None,
        env: Vec::new(),
        bucket_config: None,
        rules: Vec::new(),
        rows: None,
        cols: None,
        tag: None,
        limits: None,
    };

    let IpcResponse::PtyCommandStart(ipc_started) = ipc
        .client
        .call(1, IpcRequest::PtyCommandStart(start_params.clone()))
        .await
        .expect("start ipc pty")
    else {
        panic!("expected PtyCommandStart response");
    };
    let emb_started = embedded
        .pty_command_start(start_params)
        .await
        .expect("start embedded pty");

    wait_for_pty_credential_prompt_ipc(&ipc.client, ipc_started.job_id).await;
    wait_for_pty_credential_prompt_embedded(&embedded, emb_started.job_id).await;

    let gate_script = |job_id: JobId| -> String {
        let jid = py_str(&job_id.to_wire_string());
        format!(
            r#"
jid = {jid}
listing = pty_command_list()
awaiting = [e for e in listing["entries"] if e["job_id"] == jid][0]["awaiting_credential"]
denied_msg = None
try:
    pty_command_write_stdin(job_id=jid, bytes="{SECRET_NEVER_SURFACED}\r")
except RuntimeError as exc:
    denied_msg = str(exc)

cred = credential_request(job_id=jid)
{{
    "awaiting_kind": awaiting["kind"],
    "denied_msg": denied_msg,
    "cred": cred,
}}
"#
        )
    };

    let ipc_out = ipc.run_code(&gate_script(ipc_started.job_id)).await;
    let emb_out = run_embedded_code(&embedded, &gate_script(emb_started.job_id)).await;

    assert_eq!(
        ipc_out.status,
        ExecuteStatus::Ok,
        "ipc script failed: {:?}",
        ipc_out.error
    );
    assert_eq!(
        emb_out.status,
        ExecuteStatus::Ok,
        "embedded script failed: {:?}",
        emb_out.error
    );

    let ipc_val = ipc_out.value.expect("ipc value");
    let emb_val = emb_out.value.expect("embedded value");

    for (label, val) in [("ipc", &ipc_val), ("embedded", &emb_val)] {
        assert_eq!(val["awaiting_kind"], json!("sudo"), "{label}");
        let denied = val["denied_msg"]
            .as_str()
            .expect("pty_command_write_stdin must have raised RuntimeError");
        assert!(
            denied.starts_with("SecretInputDenied:") && denied.contains("credential_request"),
            "{label} unexpected SecretInputDenied message: {denied}"
        );
        let serialized = serde_json::to_string(val).unwrap();
        assert!(
            !serialized.contains(SECRET_NEVER_SURFACED),
            "{label} leaked secret into outcome"
        );
    }

    // DaemonState (`prompter = "none"`) returns `owner_action_required` with CLI command hint.
    assert_eq!(
        ipc_val["cred"]["status"],
        serde_json::to_value(CredentialStatus::OwnerActionRequired).unwrap()
    );
    assert!(
        ipc_val["cred"]["command"]
            .as_str()
            .is_some_and(|cmd| cmd.contains("credential provide")),
        "expected CLI provide command hint on DaemonState: {:?}",
        ipc_val["cred"]
    );
    assert!(ipc_val["cred"]["owner_action"].is_null());

    // EmbeddedEngine (`HostManaged`) returns `owner_action_required` with typed challenge and no CLI command.
    assert_eq!(
        emb_val["cred"]["status"],
        serde_json::to_value(CredentialStatus::OwnerActionRequired).unwrap()
    );
    assert!(
        emb_val["cred"]["command"].is_null(),
        "embedded engine must never advertise an unbound CLI socket command"
    );
    let owner_action: CredentialOwnerAction =
        serde_json::from_value(emb_val["cred"]["owner_action"].clone())
            .expect("typed CredentialOwnerAction challenge");
    let CredentialOwnerAction::Provide { challenge } = owner_action;
    assert_eq!(challenge.job_id, emb_started.job_id);
    assert_eq!(challenge.instance_id, embedded.identity().instance_id);

    // Clean up live PTY children and verify the attempted model password never touched disk.
    let _ = ipc
        .client
        .call(
            2,
            IpcRequest::PtyCommandStop(PtyCommandStopParams {
                job_id: ipc_started.job_id,
            }),
        )
        .await;
    let _ = embedded
        .pty_command_stop(PtyCommandStopParams {
            job_id: emb_started.job_id,
        })
        .await;

    ipc.shutdown().await;
    embedded.shutdown().await.unwrap();

    assert_no_file_contains(&ipc_data, SECRET_NEVER_SURFACED.as_bytes());
    assert_no_file_contains(&emb_data, SECRET_NEVER_SURFACED.as_bytes());
}

// ---------------------------------------------------------------------------
// 5. Persistent `audit_since` parity
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // Complete persistent audit parity scenario across both engines.
async fn persistent_audit_since_records_matching_actions_and_decisions() {
    let temp = tempfile::tempdir().unwrap();
    let ipc_data = temp.path().join("ipc-audit");
    let emb_data = temp.path().join("emb-audit");
    let ipc_repo = temp.path().join("ipc-repo");
    let emb_repo = temp.path().join("emb-repo");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&ipc_repo).unwrap();
    std::fs::create_dir_all(&emb_repo).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let ipc = DaemonIpcRig::start(hardened_policy_config(&ipc_data, &ipc_repo), "audit");
    let embedded = Arc::new(
        EmbeddedEngine::bootstrap(hardened_policy_config(&emb_data, &emb_repo))
            .expect("bootstrap embedded for audit"),
    );

    let exe = py_str(&std::env::current_exe().unwrap().to_string_lossy());
    let build_audit_script = |repo_dir: &Path, bad_file: &Path| -> String {
        let cwd = py_str(&repo_dir.to_string_lossy());
        let good_file = py_str(&repo_dir.join("audited.txt").to_string_lossy());
        let denied_file = py_str(&bad_file.to_string_lossy());
        format!(
            r#"
exe = {exe}
run = run_and_watch(
    argv=[exe, "--exact", "codemode_parity_triage_child", "--nocapture"],
    cwd={cwd},
    env=[{{"key": "{FIXTURE_ENV}", "value": "1"}}],
    wait_ms=10000,
    wait_until="exit",
)
file_write(path={good_file}, content="audit-ok")

try:
    file_write(path={denied_file}, content="audit-deny")
except RuntimeError:
    pass

try:
    shell_exec(shell_line="echo denied-shell", cwd={cwd})
except RuntimeError:
    pass

audit = audit_since(cursor=0, limit=200)
[(row["action"], row["decision"]) for row in audit["rows"]]
"#
        )
    };

    let ipc_out = ipc
        .run_code(&build_audit_script(
            &ipc_repo,
            &outside.join("ipc-deny.txt"),
        ))
        .await;
    let emb_out = run_embedded_code(
        &embedded,
        &build_audit_script(&emb_repo, &outside.join("emb-deny.txt")),
    )
    .await;

    assert_eq!(
        ipc_out.status,
        ExecuteStatus::Ok,
        "ipc audit script failed: {:?}",
        ipc_out.error
    );
    assert_eq!(
        emb_out.status,
        ExecuteStatus::Ok,
        "embedded audit script failed: {:?}",
        emb_out.error
    );

    let to_pairs = |val: &Value| -> Vec<(String, String)> {
        val.as_array()
            .expect("rows array")
            .iter()
            .map(|pair| {
                let arr = pair.as_array().expect("2-tuple");
                (
                    arr[0].as_str().unwrap().to_owned(),
                    arr[1].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };

    let ipc_pairs = to_pairs(ipc_out.value.as_ref().unwrap());
    let emb_pairs = to_pairs(emb_out.value.as_ref().unwrap());

    // Required domain & IPC audit milestones across both runtimes.
    let required = [
        ("command_start", "allow"),
        ("ipc_command_start_combed", "info"),
        ("file_write", "allow"),
        ("ipc_file_write", "info"),
        ("file_write", "deny"),
        ("ipc_file_write", "error"),
        ("command_shell_rejected", "deny"),
        ("ipc_shell_exec", "error"),
    ];
    for (action, decision) in required {
        let target = (action.to_owned(), decision.to_owned());
        assert!(
            ipc_pairs.contains(&target),
            "IPC audit rows missing {target:?}; got: {ipc_pairs:?}"
        );
        assert!(
            emb_pairs.contains(&target),
            "Embedded audit rows missing {target:?}; got: {emb_pairs:?}"
        );
    }

    // Filter out UDS socket-accept rows (`ipc_ipc_connect`, which only `IpcServer` emits
    // on Unix socket accept), readiness `ipc_health`, and async child/waiter interleaving
    // (`command_exit`, `command_exited`, `bucket_wait`, `bucket_append`, `ipc_command_status`,
    // `ipc_bucket_wait`), then assert exact deterministic parity on synchronous policy/mutation rows.
    let sync_only = |pairs: Vec<(String, String)>| -> Vec<(String, String)> {
        pairs
            .into_iter()
            .filter(|(action, _)| {
                !matches!(
                    action.as_str(),
                    "ipc_ipc_connect"
                        | "ipc_health"
                        | "command_exit"
                        | "command_exited"
                        | "bucket_wait"
                        | "bucket_append"
                        | "ipc_command_status"
                        | "ipc_bucket_wait"
                )
            })
            .collect()
    };
    assert_eq!(sync_only(ipc_pairs), sync_only(emb_pairs));

    // Also confirm direct host `audit_since` sees the `ipc_audit_since` row persisted by the script's own read.
    let IpcResponse::AuditSince(ipc_direct) = ipc
        .client
        .call(
            10,
            IpcRequest::AuditSince(AuditSinceParams {
                cursor: 0,
                action_filter: Some("ipc_audit_since".to_owned()),
                decision_filter: None,
                limit: Some(10),
            }),
        )
        .await
        .unwrap()
    else {
        panic!("expected AuditSince");
    };
    let emb_direct = embedded
        .audit_since(AuditSinceParams {
            cursor: 0,
            action_filter: Some("ipc_audit_since".to_owned()),
            decision_filter: None,
            limit: Some(10),
        })
        .await
        .unwrap();
    assert_eq!(ipc_direct.rows.len(), 1);
    assert_eq!(emb_direct.rows.len(), 1);
    assert_eq!(ipc_direct.rows[0].decision, "info");
    assert_eq!(emb_direct.rows[0].decision, "info");

    ipc.shutdown().await;
    embedded.shutdown().await.unwrap();
}
