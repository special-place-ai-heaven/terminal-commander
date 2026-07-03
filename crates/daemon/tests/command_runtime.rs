// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
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

/// Build a deliberately INVALID inline rule: a regex rule whose
/// pattern does not compile. `SifterRuntime::build` (called from
/// `start_combed`) rejects it, exercising the caller-fixable error
/// path. The unbalanced `(` is a classic malformed regex.
fn invalid_regex_rule(id: &str) -> RuleDefinition {
    RuleDefinition {
        id: id.to_owned(),
        version: 1,
        kind: RuleType::Regex,
        status: RuleStatus::Active,
        severity: Severity::High,
        event_kind: "kw_bad".to_owned(),
        stream: None,
        description: None,
        pattern: Some("(".to_owned()),
        keywords: None,
        captures: vec![],
        summary_template: "bad rule".to_owned(),
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
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
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

/// US2 (T033 / FR-011): starting a recognized tool (`git`) with NO
/// active pack attaches a `pack_available` hint to the response. The
/// command itself need not exist on the box -- the hint is computed
/// from argv[0] BEFORE spawn -- but we use a real benign program name
/// argv to keep the spawn path live; `git` is the recognized tool.
#[test]
fn recognized_tool_without_pack_gets_pack_available_hint() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("hint-git");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let req = CommandStartRequest {
            // argv[0] basename `git` is recognized; `--version` is a
            // benign subcommand so the spawn succeeds where git exists
            // and is harmless where it does not (the hint is pre-spawn).
            argv: vec!["git".to_owned(), "--version".to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");
        let hint = resp.hint.expect("git start without pack must carry a hint");
        assert_eq!(hint.kind, "pack_available");
        assert_eq!(hint.pack, "git");
        assert_eq!(hint.action, "registry_import_pack");

        cleanup(&data);
    });
}

/// US2 (T033 / FR-011): an UNRECOGNIZED tool gets NO hint.
#[test]
fn unrecognized_tool_gets_no_pack_hint() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("hint-none");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let req = CommandStartRequest {
            argv: py_argv("hi", "", 0),
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");
        assert!(
            resp.hint.is_none(),
            "python3 is not a recognized pack tool; expected no hint, got {:?}",
            resp.hint
        );

        cleanup(&data);
    });
}

/// US2 (T031 / FR-009): with `universal_extractors` ON and NO
/// tool-specific rule active, a command's stderr error line still
/// produces a baseline LOW-severity signal.
#[test]
fn universal_extractors_emit_baseline_signal_when_enabled() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("universal-on");
        let mut cfg = DaemonConfig::defaults_in(&data);
        cfg.sifters.universal_extractors = true;
        let state = DaemonState::bootstrap(cfg).unwrap();

        // No inline rules, no active pack: the universal baseline is
        // the only thing that can emit. Stderr carries an error line.
        let req = CommandStartRequest {
            argv: py_argv("ordinary stdout", "error: something broke", 1),
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");

        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(resp.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
            ) {
                break;
            }
        }

        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read ok");
        let baseline = bread
            .events
            .iter()
            .find(|e| e.kind == "universal_error")
            .expect("universal extractor must emit a baseline error signal");
        assert_eq!(
            baseline.severity,
            Severity::Low,
            "universal extractor signal must be LOW severity"
        );

        cleanup(&data);
    });
}

/// US2 (T031 / FR-009): with `universal_extractors` OFF (the default),
/// the SAME command emits NO baseline signal -- only the lifecycle
/// event. Proves the flag actually gates the behavior.
#[test]
fn universal_extractors_silent_when_disabled() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("universal-off");
        let cfg = DaemonConfig::defaults_in(&data);
        // Default: universal_extractors is false.
        assert!(!cfg.sifters.universal_extractors);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let req = CommandStartRequest {
            argv: py_argv("ordinary stdout", "error: something broke", 1),
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");

        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(40)).await;
            if matches!(
                state.command.job_record(resp.job_id).map(|r| r.state),
                Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
            ) {
                break;
            }
        }

        let bread = state
            .router
            .bucket_events_since(resp.bucket_id, &BucketReadRequest::new(0))
            .expect("bucket read ok");
        assert!(
            !bread.events.iter().any(|e| e.kind == "universal_error"),
            "with the flag OFF, no universal baseline signal must appear"
        );

        cleanup(&data);
    });
}

/// US2 (T033 / FR-011): a recognized tool whose pack IS already active
/// gets NO hint (the agent already imported it).
#[test]
fn recognized_tool_with_active_pack_gets_no_hint() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("hint-active");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Import + activate the git pack so a `git.*` rule is live in
        // the global scope, then start git: the hint must be suppressed.
        let import = state
            .store
            .import_rule_pack_by_name("git", true)
            .expect("import git pack active");
        assert!(!import.imported.is_empty());
        for rule_id in &import.imported {
            // Activate every imported git rule under the global scope.
            if let Ok(Some(def)) = state.store.get_latest_rule(rule_id) {
                state
                    .activation
                    .activate(def, terminal_commander_core::ActivationScope::Global);
            }
        }

        let req = CommandStartRequest {
            argv: vec!["git".to_owned(), "--version".to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };
        let resp = state.command.start_combed(req).expect("start ok");
        assert!(
            resp.hint.is_none(),
            "git pack is active; hint must be suppressed, got {:?}",
            resp.hint
        );

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
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
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

/// FIX 1 regression: an invalid INLINE rule must fail BEFORE the
/// bucket is allocated, so no bucket is orphaned, and the error is
/// the caller-fixable `Sifter` variant (mapped to `RuleInvalid` at
/// the IPC boundary), never a server-fault.
///
/// Asserts:
///  1. `start_combed` returns `CommandError::Sifter(_)`.
///  2. NO bucket was created: `Router::bucket_create` audits a
///     `bucket_create` row on every allocation, so zero such rows
///     proves nothing was leaked. (Before the fix the bucket was
///     created first, leaving exactly one orphaned `bucket_create`
///     row and an unreachable bucket.)
#[test]
fn command_start_with_invalid_inline_rule_fails_fast_without_leaking_bucket() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("bad-rule");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let req = CommandStartRequest {
            argv: py_argv("noise", "noise", 0),
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![invalid_regex_rule("test.bad")],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };

        let err = state.command.start_combed(req).unwrap_err();
        assert!(
            matches!(err, CommandError::Sifter(_)),
            "expected Sifter (rule-compile) error, got: {err:?}"
        );

        // No bucket leaked: the bucket is allocated only AFTER the
        // rule set compiles, so a failed compile must leave zero
        // `bucket_create` audit rows behind.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        let bucket_creates = rows.iter().filter(|r| r.action == "bucket_create").count();
        assert_eq!(
            bucket_creates, 0,
            "invalid inline rule leaked a bucket: {bucket_creates} bucket_create rows in {rows:?}"
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
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
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
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
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
                tag: None,
                dedup_nonce: None,
                strip_ansi: true,
                peer_discriminator: None,
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

/// TC49 Task-4 regression lock: threading `StartLane` through
/// `start_combed_inner` MUST NOT weaken the argv lane. The default
/// `start_combed` path (`StartLane::Argv`) still hard-denies a shell
/// interpreter as `argv[0]` with `ShellInterpreterDenied` and still
/// writes a `command_rejected` deny audit row — byte-for-byte the
/// pre-TC49 behavior, BEFORE the policy engine. The shell lane's
/// `allow_shell` gate is a separate door (`start_combed_shell`); it
/// never relaxes this guard.
#[test]
fn argv_shell_interpreter_still_denied_unchanged() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny-argv-unchanged");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let job_count_before = state.jobs.list().len();
        let req = CommandStartRequest {
            argv: vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };
        let err = state.command.start_combed(req).unwrap_err();
        assert!(
            matches!(err, CommandError::ShellInterpreterDenied(ref s) if s == "sh"),
            "argv lane must still hard-deny the shell interpreter: {err:?}"
        );
        // The guard runs BEFORE policy/spawn: no process started.
        assert_eq!(state.jobs.list().len(), job_count_before);

        // The deny audit row is the argv label, NOT command_shell_rejected.
        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "command_rejected" && r.decision == "deny"),
            "expected a command_rejected deny row: {rows:?}"
        );
        assert!(
            !rows.iter().any(|r| r.action == "command_shell_rejected"),
            "argv lane must not emit a shell-lane audit label: {rows:?}"
        );

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
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
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
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
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
        hint: None,
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
        restarted: false,
    };
    assert_small_response(&s);
}

// ---------------------------------------------------------------------
// TC-2: narrow in-flight dedup guard in start_combed.
// Source-status: live. Each test stands up its OWN temp data dir and an
// in-process CommandRuntime via DaemonState::bootstrap -- NEVER the
// default/live daemon socket.
// ---------------------------------------------------------------------

/// python3 argv that sleeps for `secs` then exits 0. A non-shell program
/// kept alive long enough that a duplicate start lands while it is still
/// in-flight (the dedup entry is present from before-spawn until exit).
fn sleep_argv(secs: f64) -> Vec<String> {
    vec![
        "python3".to_owned(),
        "-c".to_owned(),
        format!("import time; time.sleep({secs})"),
    ]
}

/// Build a start request with an explicit nonce / peer discriminator so
/// the tests drive the dedup key deterministically. argv-only, no rules.
fn dedup_req(
    argv: Vec<String>,
    nonce: Option<&str>,
    peer: Option<u64>,
) -> terminal_commanderd::CommandStartRequest {
    terminal_commanderd::CommandStartRequest {
        argv,
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace: None,
        tag: None,
        dedup_nonce: nonce.map(str::to_owned),
        peer_discriminator: peer,
        strip_ansi: true,
    }
}

/// Count `bucket_create` audit rows. start_combed allocates exactly one
/// bucket per REAL spawn (and audits it), so this is a precise proxy for
/// "how many processes were actually spawned" -- a deduped duplicate
/// never reaches `bucket_create`.
fn bucket_create_count(state: &DaemonState) -> usize {
    let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
    rows.iter().filter(|r| r.action == "bucket_create").count()
}

fn wait_terminal(state: &DaemonState, job_id: terminal_commander_core::JobId) {
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(40));
        if matches!(
            state.command.job_record(job_id).map(|r| r.state),
            Some(JobState::Exited | JobState::Failed | JobState::Cancelled)
        ) {
            return;
        }
    }
}

// (a) Two CommandStartCombed with the SAME nonce while the first is still
// in flight collapse to the SAME (job_id, bucket_id) and spawn ONE
// process.
#[test]
fn dedup_same_nonce_in_window_collapses_to_one_job() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("dedup-same-nonce");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Long-running so the entry is live when the duplicate arrives.
        let first = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), Some("nonce-A"), Some(7)))
            .expect("first start ok");
        let second = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), Some("nonce-A"), Some(7)))
            .expect("second start (deduped) ok");

        assert_eq!(
            first.job_id, second.job_id,
            "same nonce in-window must return the SAME job_id"
        );
        assert_eq!(first.bucket_id, second.bucket_id);
        assert_eq!(first.probe_id, second.probe_id);
        assert_eq!(
            bucket_create_count(&state),
            1,
            "exactly ONE process/bucket must be spawned for a deduped duplicate"
        );
        assert_eq!(
            state.command.live_jobs().len(),
            1,
            "exactly one live job after a deduped duplicate"
        );

        wait_terminal(&state, first.job_id);
        cleanup(&data);
    });
}

// (b) Never-collapse: two starts with DISTINCT nonces but identical
// argv/cwd/tag spawn TWO distinct jobs.
#[test]
fn dedup_distinct_nonces_never_collapse() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("dedup-distinct-nonce");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let a = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), Some("nonce-1"), Some(7)))
            .expect("start a");
        let b = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), Some("nonce-2"), Some(7)))
            .expect("start b");

        assert_ne!(
            a.job_id, b.job_id,
            "distinct nonces must NEVER collapse, even with identical argv"
        );
        assert_eq!(
            bucket_create_count(&state),
            2,
            "two distinct nonces must spawn two processes"
        );
        assert_eq!(state.command.live_jobs().len(), 2);

        wait_terminal(&state, a.job_id);
        wait_terminal(&state, b.job_id);
        cleanup(&data);
    });
}

// (c) Never-block: a third identical start AFTER the first completes
// (entry evicted on completion) gets a FRESH job.
#[test]
fn dedup_entry_evicted_on_completion_does_not_block_rerun() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("dedup-rerun");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Quick exit so the entry is evicted on completion.
        let first = state
            .command
            .start_combed(dedup_req(py_argv("done", "", 0), Some("nonce-R"), Some(7)))
            .expect("first start ok");
        wait_terminal(&state, first.job_id);

        // Same nonce, but the first job is terminal and its entry evicted:
        // this must NOT collapse to the dead job -- it must spawn fresh.
        let second = state
            .command
            .start_combed(dedup_req(py_argv("done", "", 0), Some("nonce-R"), Some(7)))
            .expect("rerun start ok");
        assert_ne!(
            first.job_id, second.job_id,
            "a re-run AFTER completion must get a FRESH job (never blocked)"
        );
        assert_eq!(
            bucket_create_count(&state),
            2,
            "the rerun must spawn its own process"
        );

        wait_terminal(&state, second.job_id);
        cleanup(&data);
    });
}

// (d) Eviction-on-spawn-failure: a start that fails to spawn (bogus
// binary) evicts its fingerprint so an immediate identical retry is NOT
// blocked by a stale entry (it reaches the spawn again and fails again,
// rather than wrongly collapsing to a job that never existed).
#[test]
fn dedup_evicts_on_spawn_failure_so_retry_is_not_blocked() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("dedup-spawn-fail");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let bogus = vec![
            "/nonexistent/tc-dedup-bogus-binary".to_owned(),
            "--noop".to_owned(),
        ];
        let first = state
            .command
            .start_combed(dedup_req(bogus.clone(), Some("nonce-F"), Some(7)))
            .expect_err("bogus binary must fail to spawn");
        // F7: a missing binary (OS `ErrorKind::NotFound`) is now classified
        // as the caller-fixable `ProgramNotFound`, carved out of the generic
        // `Spawn`. The dedup-eviction contract is independent of the variant;
        // accept either spawn-failure shape.
        assert!(
            matches!(
                first,
                CommandError::ProgramNotFound { .. } | CommandError::Spawn(_)
            ),
            "first failure must be a spawn failure, got {first:?}"
        );

        // The fingerprint was evicted on the spawn-failure path, so the
        // identical retry must reach the spawn again (and fail again) --
        // a stale block would instead return Ok with phantom ids.
        let second = state
            .command
            .start_combed(dedup_req(bogus, Some("nonce-F"), Some(7)))
            .expect_err("retry must also reach spawn and fail, not be blocked");
        assert!(
            matches!(
                second,
                CommandError::ProgramNotFound { .. } | CommandError::Spawn(_)
            ),
            "a leaked entry must never block a legitimate retry; got {second:?}"
        );
    });
}

// F7: a command whose program does not exist must fail to start with the
// structured, caller-fixable `CommandError::ProgramNotFound { argv0 }`
// (carved out of the generic `Spawn`), carrying the offending `argv0`.
// This is the runtime-layer half of the fix; the IPC mapping
// (`map_command_error` -> `IpcErrorCode::ProgramNotFound`) is asserted in
// the daemon `ipc::handlers::common` unit tests. A deterministically-absent
// program name is used so the OS spawn returns `ErrorKind::NotFound`.
#[test]
fn missing_program_yields_structured_program_not_found_receipt() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("program-not-found");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let missing = "tc_nonexistent_program_f7_xyz";
        let req = CommandStartRequest {
            argv: vec![missing.to_owned(), "--noop".to_owned()],
            cwd: None,
            env: vec![],
            bucket_config: None,
            rules: vec![],
            grace: None,
            tag: None,
            dedup_nonce: None,
            strip_ansi: true,
            peer_discriminator: None,
        };

        let err = state
            .command
            .start_combed(req)
            .expect_err("a non-existent program must fail to start");

        match err {
            CommandError::ProgramNotFound { argv0 } => {
                assert_eq!(argv0, missing, "the receipt must name the offending argv0");
            }
            other => panic!(
                "expected CommandError::ProgramNotFound, got {other:?} \
                 (a missing program is a caller-fixable command attempt, \
                 not a generic Spawn/Internal fault)"
            ),
        }

        cleanup(&data);
    });
}

// (e) Nonce-less fallback: two identical nonce-less starts from the same
// peer within the window collapse (old-adapter protection); after the
// window they spawn distinct jobs.
#[test]
fn dedup_nonceless_fallback_collapses_in_window_then_distinct_after() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("dedup-fallback");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // In-window: identical signature (argv/cwd/tag) + same peer, no
        // nonce. The fallback window collapses the duplicate.
        let a = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), None, Some(42)))
            .expect("start a");
        let b = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), None, Some(42)))
            .expect("start b (deduped)");
        assert_eq!(
            a.job_id, b.job_id,
            "nonce-less identical same-peer start in-window must collapse (old-adapter protection)"
        );
        assert_eq!(
            bucket_create_count(&state),
            1,
            "the in-window fallback duplicate must not spawn a second process"
        );

        // After the TTL (3s) the fallback entry is the backstop-dropped:
        // an identical nonce-less start now spawns a distinct job. (The
        // first job is still sleeping, so this proves the WINDOW, not
        // completion-eviction, is what reopens the key.)
        std::thread::sleep(Duration::from_millis(3_200));
        let c = state
            .command
            .start_combed(dedup_req(sleep_argv(5.0), None, Some(42)))
            .expect("start c after window");
        assert_ne!(
            a.job_id, c.job_id,
            "after the fallback window an identical nonce-less start must spawn a DISTINCT job"
        );
        assert_eq!(
            bucket_create_count(&state),
            2,
            "the post-window start must spawn its own process"
        );

        wait_terminal(&state, a.job_id);
        wait_terminal(&state, c.job_id);
        cleanup(&data);
    });
}

// ---------------------------------------------------------------------------
// TC-5: bucket-reuse seam (`start_combed_reusing`). These prove the self-check
// can spawn distinct jobs into ONE reused bucket without churning a fresh bucket
// per call, and that the normal `start_combed` path is unchanged.
// ---------------------------------------------------------------------------

/// TC-5 (a): `start_combed_reusing(req, None)` behaves exactly like
/// `start_combed` -- it ALLOCATES a fresh bucket (one `bucket_create` audit
/// row) and returns bounded ids. The `None` reuse argument is the public
/// path; this guards that the refactor did not change fresh-spawn behavior.
#[test]
fn start_combed_reusing_none_allocates_a_fresh_bucket_like_start_combed() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("reuse-none");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let resp = state
            .command
            .start_combed_reusing(dedup_req(sleep_argv(1.0), Some("reuse-none-1"), None), None)
            .expect("start ok");

        // A fresh bucket was created (exactly one create audit row).
        assert_eq!(
            bucket_create_count(&state),
            1,
            "reuse=None must allocate exactly one fresh bucket"
        );
        // Bounded wire ids only.
        assert!(resp.job_id.to_wire_string().starts_with("job_"));
        assert!(resp.bucket_id.to_wire_string().starts_with("bkt_"));
        assert!(resp.probe_id.to_wire_string().starts_with("prb_"));

        wait_terminal(&state, resp.job_id);
        cleanup(&data);
    });
}

/// TC-5 (b): two `start_combed_reusing` calls with the SAME `Some(bid)` (the
/// bucket id returned by the first call) both succeed, yield DISTINCT
/// `job_id`s into the SAME `bucket_id`, and create EXACTLY ONE bucket -- the
/// reuse path must NOT re-run `bucket_create` (no `AlreadyExists` error).
#[test]
fn start_combed_reusing_same_bucket_yields_distinct_jobs_one_bucket() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("reuse-same");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // First call: None -> fresh bucket; capture its id to reuse.
        let first = state
            .command
            .start_combed_reusing(dedup_req(sleep_argv(1.0), Some("reuse-same-1"), None), None)
            .expect("first start ok");
        let reused = first.bucket_id;

        // Second call: Some(reused) -> distinct job into the SAME bucket.
        let second = state
            .command
            .start_combed_reusing(
                dedup_req(sleep_argv(1.0), Some("reuse-same-2"), None),
                Some(reused),
            )
            .expect("second start must NOT error AlreadyExists");

        assert_ne!(
            first.job_id, second.job_id,
            "reused-bucket spawns must get distinct job ids"
        );
        assert_eq!(
            first.bucket_id, second.bucket_id,
            "both jobs must land in the SAME reused bucket"
        );
        assert_ne!(
            first.probe_id, second.probe_id,
            "each spawn gets its own probe id"
        );
        // The crux: the reuse path created NO second bucket.
        assert_eq!(
            bucket_create_count(&state),
            1,
            "reuse must create exactly one bucket across two spawns"
        );

        wait_terminal(&state, first.job_id);
        wait_terminal(&state, second.job_id);
        cleanup(&data);
    });
}

// ------------------------------------------------------------------
// US8 (FR-060): WSL nested-shell gate. The argv lane must classify a
// `wsl`/`wsl.exe` carrier the same way it classifies a bare interpreter:
// a shell smuggled through the WSL boundary is denied under
// allow_shell=false. Classification is pure argv logic (no WSL needed to
// run these); the spellings matrix is normative in policy-wsl.md.
// ------------------------------------------------------------------

/// Default (allow_shell=false) argv request builder for the WSL cases.
fn wsl_req(argv: &[&str]) -> CommandStartRequest {
    CommandStartRequest {
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace: None,
        tag: None,
        dedup_nonce: None,
        strip_ansi: true,
        peer_discriminator: None,
    }
}

/// The canonical live-repro: `wsl.exe -e bash -lc "..."` slipped past the
/// argv[0] denylist before US8. Now it is denied with the WSL-carrier
/// teaching error and a `command_rejected` deny row, and NO job spawns.
#[test]
fn wsl_nested_shell_denied_under_allow_shell_false() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-deny");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let job_count_before = state.jobs.list().len();
        let err = state
            .command
            .start_combed(wsl_req(&["wsl.exe", "-e", "bash", "-lc", "echo hi"]))
            .unwrap_err();
        match err {
            CommandError::WslNestedShellDenied {
                ref interpreter,
                ref carrier,
            } => {
                assert_eq!(interpreter, "bash", "must name the nested interpreter");
                assert_eq!(carrier, "wsl.exe", "must name the wsl carrier");
            }
            other => panic!("expected WslNestedShellDenied, got {other:?}"),
        }
        // Guard runs BEFORE policy/spawn: no process started.
        assert_eq!(state.jobs.list().len(), job_count_before);

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "command_rejected" && r.decision == "deny"),
            "expected a command_rejected deny row: {rows:?}"
        );
        cleanup(&data);
    });
}

/// Every deny spelling in policy-wsl.md classifies identically to a nested
/// shell (or fails closed), regardless of `-e`/`--`/bare, distro selectors,
/// or an absolute Windows path to wsl.exe.
#[test]
fn wsl_nested_shell_all_spellings_classified_identically() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-spellings");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // (argv, expected interpreter). Matrix from policy-wsl.md deny table.
        let cases: &[(&[&str], &str)] = &[
            (&["wsl.exe", "-e", "bash", "-lc", "echo hi"], "bash"),
            (&["wsl", "bash"], "bash"),
            (&["wsl.exe", "--", "sh", "-c", "echo hi"], "sh"),
            (&["wsl.exe", "-d", "Ubuntu", "-e", "zsh"], "zsh"),
            (&[r"C:\Windows\System32\wsl.exe", "-e", "bash"], "bash"),
            (&["wsl.exe", "--exec", "busybox", "sh"], "busybox"),
            (
                &["wsl.exe", "echo", "$(id)"],
                "default shell interpretation",
            ),
            (&["wsl.exe", "~"], "default shell"),
        ];

        for (argv, expected) in cases {
            let err = state.command.start_combed(wsl_req(argv)).unwrap_err();
            match err {
                CommandError::WslNestedShellDenied {
                    ref interpreter, ..
                } => {
                    assert_eq!(
                        interpreter, expected,
                        "argv={argv:?} expected interpreter {expected}, got {interpreter}"
                    );
                }
                other => panic!("argv={argv:?} expected WslNestedShellDenied, got {other:?}"),
            }
        }
        cleanup(&data);
    });
}

/// Non-shell payloads via `-e`/`--exec` and WSL management flags are NOT
/// touched by the gate: they reach spawn exactly as before US8 (on a host
/// without wsl.exe they surface ProgramNotFound, never a policy denial).
#[test]
fn wsl_exec_introduced_non_shell_payload_and_management_flags_run_unchanged() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-passthrough");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let cases: &[&[&str]] = &[
            &["wsl.exe", "-e", "cargo", "build"],
            &["wsl.exe", "-d", "Ubuntu", "-e", "uname", "-a"],
            &["wsl.exe", "--list", "--verbose"],
            &["wsl.exe", "--status"],
        ];
        for argv in cases {
            let res = state.command.start_combed(wsl_req(argv));
            assert!(
                !matches!(
                    res,
                    Err(CommandError::WslNestedShellDenied { .. }
                        | CommandError::ShellInterpreterDenied(_)
                        | CommandError::PolicyDenied(_))
                ),
                "argv={argv:?} must NOT be denied by the WSL gate: {res:?}"
            );
        }
        cleanup(&data);
    });
}

/// A payload NOT introduced by `-e`/`--exec` is handed to the distro's
/// default shell by WSL itself (shell-interpreted), so it is a nested shell
/// even when the named program is not a shell (`echo`).
#[test]
fn wsl_bare_payload_without_exec_is_shell_interpreted_and_denied() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-bare-payload");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let err = state
            .command
            .start_combed(wsl_req(&["wsl.exe", "echo", "hello"]))
            .unwrap_err();
        match err {
            CommandError::WslNestedShellDenied {
                ref interpreter, ..
            } => {
                assert_eq!(
                    interpreter, "default shell interpretation",
                    "a bare (no -e) payload is shell-interpreted by WSL"
                );
            }
            other => panic!("expected WslNestedShellDenied, got {other:?}"),
        }
        cleanup(&data);
    });
}

/// `~` is the start-in-home selector, never a payload: it is skipped and the
/// token after it is the payload. `wsl.exe ~ bash` therefore classifies as a
/// bash nested shell, not a "default shell interpretation" of `~`.
#[test]
fn wsl_tilde_shorthand_is_selector_not_payload() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-tilde");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let err = state
            .command
            .start_combed(wsl_req(&["wsl.exe", "~", "bash"]))
            .unwrap_err();
        match err {
            CommandError::WslNestedShellDenied {
                ref interpreter, ..
            } => {
                assert_eq!(
                    interpreter, "bash",
                    "~ must be skipped as a selector so bash is the payload"
                );
            }
            other => panic!("expected WslNestedShellDenied, got {other:?}"),
        }
        cleanup(&data);
    });
}

/// An unrecognized flag in payload position fails closed: it is treated as a
/// potential shell payload and denied under allow_shell=false.
#[test]
fn wsl_unknown_construction_fails_closed() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-unknown");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let err = state
            .command
            .start_combed(wsl_req(&["wsl.exe", "--some-future-flag", "x"]))
            .unwrap_err();
        assert!(
            matches!(err, CommandError::WslNestedShellDenied { .. }),
            "unknown WSL construction must fail closed: {err:?}"
        );

        let rows = state.store.audit_since(&AuditReadRequest::new(0)).unwrap();
        assert!(
            rows.iter()
                .any(|r| r.action == "command_rejected" && r.decision == "deny"),
            "expected a command_rejected deny row: {rows:?}"
        );
        cleanup(&data);
    });
}

/// A bare `wsl.exe` with no command launches the distro's default
/// interactive shell, which is a nested shell -> denied.
#[test]
fn wsl_bare_invocation_is_default_shell_and_denied() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("wsl-bare");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let err = state
            .command
            .start_combed(wsl_req(&["wsl.exe"]))
            .unwrap_err();
        match err {
            CommandError::WslNestedShellDenied {
                ref interpreter, ..
            } => {
                assert_eq!(interpreter, "default shell");
            }
            other => panic!("expected WslNestedShellDenied, got {other:?}"),
        }
        cleanup(&data);
    });
}
