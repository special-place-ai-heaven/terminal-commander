// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors

//! Command runtime (TC38) integration tests.
//!
//! Unix only. On Windows the file compiles to an empty module so
//! the workspace still builds. The pipeline under test:
//!
//! ```text
//! CommandRuntime::start_combed
//!   -> shell-bridge guard (SHELL_INTERPRETERS_DENY)
//!   -> PolicyEngine::evaluate(CommandStart)
//!   -> Router::bucket_create
//!   -> ProcessProbe::spawn (tokio::process)
//!   -> DaemonEventSink -> Router::bucket_append
//!   -> waiter task: JobManager::finish + lifecycle event append
//!   -> PersistentAudit rows: command_start, command_exit (or
//!      command_rejected on deny path)
//! ```
//!
//! Non-shell helper: tests use `python3` directly because it is the
//! TC03 dev prerequisite and avoids invoking an interpreter that
//! would trip the shell-bridge guard. argv[0] is the python3
//! binary itself; argv[1..] are flags + a `-c` payload. The
//! python3 binary is NOT a shell interpreter for the purposes of
//! the deny list; it is the program being executed.

#![cfg(unix)]

use std::path::PathBuf;
use std::time::Duration;

use terminal_commander_core::{
    BucketReadRequest, ContextHint, JobState, RuleDefinition, RuleStatus, RuleType, Severity,
};
use terminal_commander_store::AuditReadRequest;
use terminal_commanderd::{CommandError, CommandStartRequest, DaemonConfig, DaemonState};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-cmd-{tag}-{pid}-{nanos}-{n}"));
    p
}

fn cleanup(p: &std::path::Path) {
    let _ = std::fs::remove_dir_all(p);
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn keyword_rule(id: &str, kw: &str, severity: Severity, kind: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Keyword,
        status: RuleStatus::Active,
        severity,
        event_kind: kind.to_owned(),
        stream: None,
        description: None,
        pattern: None,
        keywords: Some(vec![kw.to_owned()]),
        captures: vec![],
        summary_template: format!("matched {kw}"),
        tags: vec![],
        rate_limit_per_min: None,
        redact: vec![],
        context_hint: ContextHint::default(),
        examples: vec![],
    }
}

/// Build a python3 argv that writes to stdout AND stderr, then
/// exits with the requested code. python3 is a non-shell program.
fn py_argv(stdout: &str, stderr: &str, exit_code: i32) -> Vec<String> {
    let sout = stdout.replace('\'', "\\'");
    let serr = stderr.replace('\'', "\\'");
    let script = format!(
        "import sys; print('{sout}'); print('{serr}', file=sys.stderr); sys.exit({exit_code})"
    );
    vec!["python3".to_owned(), "-c".to_owned(), script]
}

#[test]
fn command_start_emits_matching_signal_into_bucket_no_raw_text() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("happy");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Bind a single keyword rule. Use a marker that does not
        // collide with the noisy stdout content.
        let rule = keyword_rule("test.fail", "FAILURE", Severity::High, "kw_fail");

        // Noisy stdout + one matching stderr line. Exit cleanly.
        let req = CommandStartRequest {
            argv: py_argv("step alpha and beta", "FAILURE: thing", 0),
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![rule],
            grace: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");

        // Wait for child + waiter task to complete.
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(resp.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
            ) {
                break;
            }
        }

        // Bucket read: structured SignalEvent vec, never raw text.
        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read ok");
        let kinds: Vec<String> = bread.events.iter().map(|e| e.kind.clone()).collect();
        assert!(
            kinds.iter().any(|k| k == "kw_fail"),
            "expected kw_fail in {kinds:?}"
        );
        assert!(
            kinds
                .iter()
                .any(|k| k == "command_exited" || k == "command_failed"),
            "expected lifecycle event in {kinds:?}"
        );

        // No raw stdout content leaks into NON-lifecycle event
        // summaries. The lifecycle event legitimately echoes argv
        // (operator-supplied text).
        for e in &bread.events {
            if e.kind == "command_exited" || e.kind == "command_failed" {
                continue;
            }
            assert!(
                !e.summary.contains("step alpha"),
                "raw stdout leaked into non-lifecycle event summary: {}",
                e.summary
            );
            assert!(
                !e.summary.contains("and beta"),
                "raw stdout leaked into non-lifecycle event summary: {}",
                e.summary
            );
        }

        let status = state.command.status(resp.job_id).expect("status ok");
        assert!(status.frames_total >= 2, "frames: {}", status.frames_total);
        assert!(status.events_emitted >= 1);

        // Audit: command_start (allow) and command_exit landed.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
        assert!(actions.contains(&"command_start"), "actions: {actions:?}");
        assert!(actions.contains(&"command_exit"), "actions: {actions:?}");
        for r in &rows {
            assert!(
                matches!(
                    r.decision.as_str(),
                    "allow" | "deny" | "allow_with_audit" | "error" | "info"
                ),
                "unexpected decision: {}",
                r.decision
            );
        }

        cleanup(&data);
    });
}

#[test]
fn command_start_denied_for_sudo_argv() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny-sudo");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let req = CommandStartRequest {
            argv: vec![
                "sudo".to_owned(),
                "rm".to_owned(),
                "-rf".to_owned(),
                "/".to_owned(),
            ],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
        };
        let err = state.command.start_combed(req).unwrap_err();
        assert!(matches!(err, CommandError::PolicyDenied(_)));

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "command_rejected" && r.decision == "deny"),
            "rows: {rows:?}"
        );

        cleanup(&data);
    });
}

/// Shell-bridge guard: `sh` as a bare basename must be denied by
/// the command runtime BEFORE the policy engine sees it.
#[test]
fn command_start_denied_for_bare_sh_argv() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny-sh-bare");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let job_count_before = state.jobs.list().len();
        let req = CommandStartRequest {
            argv: vec!["sh".to_owned(), "-c".to_owned(), "echo hello".to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
        };
        let err = state.command.start_combed(req).unwrap_err();
        assert!(
            matches!(err, CommandError::ShellInterpreterDenied(ref s) if s == "sh"),
            "unexpected error: {err:?}"
        );
        // No process spawned: job manager unchanged.
        assert_eq!(state.jobs.list().len(), job_count_before);

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let reject = rows
            .iter()
            .find(|r| r.action == "command_rejected" && r.decision == "deny")
            .expect("expected command_rejected audit row");
        assert!(
            reject
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("shell interpreter"),
            "audit reason should name the shell-bridge guard: {:?}",
            reject.reason
        );

        cleanup(&data);
    });
}

/// Shell-bridge guard: `/bin/sh` as an absolute path must also be
/// denied. Catches the policy-bypass attempt of writing the full
/// path instead of the bare basename.
#[test]
fn command_start_denied_for_absolute_sh_argv() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny-sh-abs");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let job_count_before = state.jobs.list().len();
        let req = CommandStartRequest {
            argv: vec![
                "/bin/sh".to_owned(),
                "-c".to_owned(),
                "echo nope".to_owned(),
            ],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
        };
        let err = state.command.start_combed(req).unwrap_err();
        assert!(
            matches!(err, CommandError::ShellInterpreterDenied(ref s) if s == "sh"),
            "unexpected error: {err:?}"
        );
        assert_eq!(state.jobs.list().len(), job_count_before);

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "command_rejected" && r.decision == "deny"),
            "rows: {rows:?}"
        );
        cleanup(&data);
    });
}

/// Spot-check the rest of the deny list across both bare and
/// absolute-path forms.
#[test]
fn command_start_denies_all_known_shell_interpreters() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny-shells");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Subset that covers POSIX shells + Windows shells +
        // .exe-cased variants.
        let cases: &[(&str, &str)] = &[
            ("bash", "bash"),
            ("dash", "dash"),
            ("zsh", "zsh"),
            ("fish", "fish"),
            ("ksh", "ksh"),
            ("csh", "csh"),
            ("tcsh", "tcsh"),
            ("ash", "ash"),
            ("busybox", "busybox"),
            ("powershell", "powershell"),
            ("powershell.exe", "powershell.exe"),
            ("pwsh", "pwsh"),
            ("pwsh.exe", "pwsh.exe"),
            ("cmd", "cmd"),
            ("cmd.exe", "cmd.exe"),
            ("/usr/bin/bash", "bash"),
            ("/usr/local/bin/zsh", "zsh"),
        ];

        for (argv0, expected_match) in cases {
            let req = CommandStartRequest {
                argv: vec![(*argv0).to_owned(), "-c".to_owned(), "true".to_owned()],
                cwd: None,
                env: vec![],
                bucket_config: None,
                rules: vec![],
                grace: None,
            };
            let err = state.command.start_combed(req).unwrap_err();
            match err {
                CommandError::ShellInterpreterDenied(ref s) => {
                    assert_eq!(
                        s, expected_match,
                        "argv0={argv0} expected match {expected_match} got {s}"
                    );
                }
                other => panic!("expected ShellInterpreterDenied for argv0={argv0}, got {other:?}"),
            }
        }

        cleanup(&data);
    });
}

#[test]
fn nonzero_exit_produces_command_failed_event_in_bucket() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("fail");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Non-shell exit-7: python3 -c "import sys; sys.exit(7)"
        let req = CommandStartRequest {
            argv: vec![
                "python3".to_owned(),
                "-c".to_owned(),
                "import sys; sys.exit(7)".to_owned(),
            ],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");

        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(resp.job_id).map(|r| r.state),
                Some(JobState::Failed | JobState::Exited | JobState::Cancelled)
            ) {
                break;
            }
        }

        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read ok");
        let has_failed = bread.events.iter().any(|e| e.kind == "command_failed");
        assert!(has_failed, "events: {:?}", bread.events);
        let failed = bread
            .events
            .iter()
            .find(|e| e.kind == "command_failed")
            .unwrap();
        // TC02 invariant: severity Medium+ needs pointer OR reason.
        assert!(
            failed.pointer.is_some() || failed.pointer_unavailable_reason.is_some(),
            "command_failed missing pointer-or-reason"
        );

        let status = state.command.status(resp.job_id).expect("status ok");
        assert_eq!(status.exit_code, Some(7));
        assert!(matches!(status.state, JobState::Failed));

        cleanup(&data);
    });
}

#[test]
fn empty_argv_is_rejected_before_spawn() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("empty");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();
        let req = CommandStartRequest {
            argv: vec![],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
        };
        let err = state.command.start_combed(req).unwrap_err();
        assert!(matches!(err, CommandError::EmptyArgv));
        cleanup(&data);
    });
}

/// Compile-time guard: CommandStartResponse and CommandStatusResponse
/// do NOT carry any raw stdout/stderr String/Vec<u8> lane.
#[test]
fn response_types_have_no_raw_stream_lane() {
    use terminal_commanderd::{CommandStartResponse, CommandStatusResponse};
    fn assert_small_response<T: serde::Serialize>(_v: &T) {}
    let r = CommandStartResponse {
        job_id: terminal_commander_core::JobId::new(),
        bucket_id: terminal_commander_core::BucketId::new(),
        probe_id: terminal_commander_core::ProbeId::new(),
        cursor: 0,
    };
    assert_small_response(&r);
    let s = CommandStatusResponse {
        job_id: terminal_commander_core::JobId::new(),
        bucket_id: terminal_commander_core::BucketId::new(),
        probe_id: terminal_commander_core::ProbeId::new(),
        state: JobState::Exited,
        frames_total: 0,
        frames_stdout: 0,
        frames_stderr: 0,
        bytes_total: 0,
        events_emitted: 0,
        frames_suppressed: 0,
        frames_suppressed_progress: 0,
        frames_suppressed_dedupe: 0,
        exit_code: Some(0),
        signal: None,
        duration_ms: Some(0),
        receipt: None,
    };
    assert_small_response(&s);
}
