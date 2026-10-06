// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Windows PTY argv-lane spawn-safety tests (FCR2-002 / FCR2-003).
//!
//! Sibling to `pty_ipc.rs` (`#![cfg(unix)]`). Guards what the ConPTY backend
//! actually executes: `portable-pty` replaces a typed extension during its
//! PATH search, and it quotes arguments with the CRT rules that `cmd.exe`
//! ignores when the program is a `.bat`/`.cmd`.

#![cfg(windows)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use terminal_commander_core::{
    BucketId, ContextHint, JobId, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_supervisor::identity::PeerIdentity;
use terminal_commanderd::{
    BucketEventsSinceParams, DaemonConfig, DaemonState, IpcError, IpcErrorCode, IpcRequest,
    IpcResponse, IpcResult, PolicyCapsSection, PtyCommandStartParams, PtyCommandStopParams,
    RequestEnvelope,
};

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let p = std::env::temp_dir().join(format!(
        "tc-pty-win-{tag}-{}-{nanos}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&p).expect("create temp dir");
    p
}

fn make_state(tag: &str) -> (Arc<DaemonState>, PathBuf) {
    let data = temp_dir(tag);
    let mut cfg = DaemonConfig::defaults_in(data.join("data"));
    // The interpreter deny is the allow_shell=false gate; pin it so a
    // default flip cannot turn the deny tests into spawn attempts.
    cfg.policy.caps = Some(PolicyCapsSection {
        allow_shell: Some(false),
        ..Default::default()
    });
    let state = DaemonState::bootstrap(cfg).expect("bootstrap daemon state");
    (Arc::new(state), data)
}

async fn dispatch(state: &Arc<DaemonState>, id: u64, request: IpcRequest) -> IpcResult {
    let req = RequestEnvelope {
        correlation_id: id,
        request,
    };
    terminal_commanderd::ipc::dispatch_envelope(
        state,
        Instant::now(),
        &req,
        &PeerIdentity::unknown(),
    )
    .await
    .result
}

const fn start_params(argv: Vec<String>, env: Vec<(String, String)>) -> IpcRequest {
    IpcRequest::PtyCommandStart(PtyCommandStartParams {
        environment: None,
        argv,
        cwd: None,
        env,
        bucket_config: None,
        rules: vec![],
        rows: None,
        cols: None,
        tag: None,
        limits: None,
    })
}

fn rule(id: &str, kind: RuleType, text: &str) -> RuleDefinition {
    let (pattern, keywords) = match kind {
        RuleType::Regex => (Some(text.to_owned()), None),
        _ => (None, Some(vec![text.to_owned()])),
    };
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind,
        status: RuleStatus::Active,
        severity: Severity::High,
        event_kind: id.to_owned(),
        stream: None,
        description: None,
        pattern,
        keywords,
        captures: vec![],
        summary_template: format!("matched {id}"),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// `PATH` with `dir` first, so a bare name resolves to the temp fixture.
fn path_with(dir: &Path) -> (String, String) {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let joined: OsString = std::env::join_paths(paths).expect("join PATH");
    ("PATH".to_owned(), joined.to_string_lossy().into_owned())
}

/// Headless Windows CI cannot always host ConPTY; mirror the skip used by
/// `runtime_state_windows.rs` for spawn failures that are environmental.
fn conpty_environmental(error: &IpcError) -> bool {
    let m = &error.message;
    error.code == IpcErrorCode::UnsupportedPlatform
        || (error.code == IpcErrorCode::Internal
            && (m.contains("0xC0000142")
                || m.contains("STATUS_DLL_INIT_FAILED")
                || m.contains("-1073741502")
                || m.to_ascii_lowercase().contains("conpty")))
}

/// Start, wait for exit, return the event kinds the job's bucket recorded.
/// `None` = ConPTY is unavailable on this host (skip).
async fn run_to_exit(
    state: &Arc<DaemonState>,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    rules: Vec<RuleDefinition>,
) -> Option<Vec<String>> {
    let mut req = start_params(argv, env);
    if let IpcRequest::PtyCommandStart(p) = &mut req {
        p.rules = rules;
    }
    let (job_id, bucket_id): (JobId, BucketId) = match dispatch(state, 1, req).await {
        IpcResult::Ok {
            response: IpcResponse::PtyCommandStart(s),
        } => (s.job_id, s.bucket_id),
        IpcResult::Err { error } if conpty_environmental(&error) => {
            eprintln!("SKIP: ConPTY unavailable: {}", error.message);
            return None;
        }
        other => panic!("pty_command_start failed: {other:?}"),
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seq = 2;
    loop {
        seq += 1;
        let live = match dispatch(state, seq, IpcRequest::PtyCommandList).await {
            IpcResult::Ok {
                response: IpcResponse::PtyCommandList(r),
            } => r.entries.iter().any(|e| e.job_id == job_id),
            other => panic!("pty_command_list failed: {other:?}"),
        };
        if !live {
            break;
        }
        assert!(Instant::now() < deadline, "pty job never exited");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Let the reader thread drain the tail before reading the bucket.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let events = match dispatch(
        state,
        seq + 1,
        IpcRequest::BucketEventsSince(BucketEventsSinceParams {
            bucket_id,
            cursor: 0,
            severity_min: None,
            kind_filter: None,
            limit: None,
        }),
    )
    .await
    {
        IpcResult::Ok {
            response: IpcResponse::BucketEventsSince(r),
        } => r.events,
        other => panic!("bucket_events_since failed: {other:?}"),
    };
    let _ = dispatch(
        state,
        seq + 2,
        IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id }),
    )
    .await;
    Some(events.into_iter().map(|e| e.kind).collect())
}

/// Copy a harmless system binary under an interpreter's name, so a spawn of
/// the "interpreter" proves reachability without running a real shell.
fn fake_program(dir: &Path, name: &str) {
    let system =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    std::fs::copy(system.join(r"System32\hostname.exe"), dir.join(name))
        .expect("copy hostname.exe fixture");
}

/// FCR2-002: an interpreter name with a swapped extension must not reach the
/// interpreter. `portable-pty` rewrites `bash.txt` to `bash.exe` during its
/// PATH search, after the raw-argv deny already passed it.
#[tokio::test]
async fn pty_extension_swapped_interpreter_is_denied() {
    let (state, dir) = make_state("ext-swap");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_program(&bin, "bash.exe");
    fake_program(&bin, "wsl.exe");
    let explicit = bin.join("bash.txt").to_string_lossy().into_owned();
    for argv0 in ["bash.txt", "wsl.foo", explicit.as_str()] {
        match dispatch(
            &state,
            1,
            start_params(vec![argv0.to_owned()], vec![path_with(&bin)]),
        )
        .await
        {
            IpcResult::Err { error } => assert_eq!(
                error.code,
                IpcErrorCode::ShellInterpreterDenied,
                "{argv0}: expected a shell-interpreter deny, got {error:?}"
            ),
            IpcResult::Ok { response } => {
                if let IpcResponse::PtyCommandStart(s) = &response {
                    let _ = dispatch(
                        &state,
                        2,
                        IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id: s.job_id }),
                    )
                    .await;
                }
                panic!("{argv0}: extension-swapped interpreter was accepted: {response:?}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Precision guard: a `.cmd` shim started by bare name (npm/npx/yarn shape)
/// still runs on the PTY lane.
#[tokio::test]
async fn pty_cmd_shim_by_bare_name_still_starts() {
    let (state, dir) = make_state("cmd-shim");
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("tcshim.cmd"),
        "@echo off\r\necho TC_SHIM_OK %1\r\n",
    )
    .unwrap();
    let Some(kinds) = run_to_exit(
        &state,
        vec!["tcshim".to_owned(), "--version".to_owned()],
        vec![path_with(&bin)],
        vec![rule(
            "tc_shim_ok",
            RuleType::Keyword,
            "TC_SHIM_OK --version",
        )],
    )
    .await
    else {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    assert!(
        kinds.iter().any(|k| k == "tc_shim_ok"),
        "the .cmd shim must start and print its marker; events: {kinds:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// FCR2-003 (BatBadBut): `cmd.exe` re-parses a batch file's command line, so
/// `&` in an argument must stay literal instead of starting a second command.
#[tokio::test]
async fn pty_batch_argument_metacharacters_stay_literal() {
    let (state, dir) = make_state("batbadbut");
    let script = dir.join("echo arg.cmd");
    std::fs::write(&script, "@echo off\r\necho ARG=%1\r\n").unwrap();
    let Some(kinds) = run_to_exit(
        &state,
        vec![
            script.to_string_lossy().into_owned(),
            "a&echo TC_INJ_7F3".to_owned(),
        ],
        vec![],
        vec![
            rule("tc_literal", RuleType::Keyword, "ARG=\"a&echo TC_INJ_7F3\""),
            // The injected command prints the marker with no quote before it.
            rule("tc_injected", RuleType::Regex, "^[^\"]*TC_INJ_7F3"),
        ],
    )
    .await
    else {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    assert!(
        !kinds.iter().any(|k| k == "tc_injected"),
        "cmd.exe ran the injected command; events: {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "tc_literal"),
        "the batch file must receive the literal argument; events: {kinds:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// FCR2-002 / FCR2-003: NUL anywhere in argv, and CR/LF in a batch file's
/// arguments (cmd.exe would cut the command line there), are caller errors.
#[tokio::test]
async fn pty_rejects_nul_and_batch_line_breaks_as_argv_invalid() {
    let (state, dir) = make_state("argv-invalid");
    let script = dir.join("noop.cmd");
    std::fs::write(&script, "@echo off\r\n").unwrap();
    let script = script.to_string_lossy().into_owned();
    let cases = [
        vec!["hostname\0.txt".to_owned()],
        vec!["hostname".to_owned(), "a\0b".to_owned()],
        vec![script.clone(), "a\r\necho TC_INJ".to_owned()],
        vec![script, "a\nb".to_owned()],
    ];
    for argv in cases {
        match dispatch(&state, 1, start_params(argv.clone(), vec![])).await {
            IpcResult::Err { error } => assert_eq!(
                error.code,
                IpcErrorCode::ArgvInvalid,
                "{argv:?}: expected argv_invalid, got {error:?}"
            ),
            IpcResult::Ok { response } => {
                if let IpcResponse::PtyCommandStart(s) = &response {
                    let _ = dispatch(
                        &state,
                        2,
                        IpcRequest::PtyCommandStop(PtyCommandStopParams { job_id: s.job_id }),
                    )
                    .await;
                }
                panic!("{argv:?}: invalid argv was accepted: {response:?}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
