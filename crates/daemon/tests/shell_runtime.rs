// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Shell-exec runtime (TC49) integration tests.
//!
//! Unix only. On Windows the file compiles to an empty module so the
//! workspace still builds. The pipeline under test:
//!
//! ```text
//! ShellRuntime::exec
//!   -> validate shell_line (non-empty, <= MAX_SHELL_LINE_BYTES)
//!   -> argv = [shell, "-lc", shell_line]
//!   -> CommandRuntime::start_combed_shell        // StartLane::Shell
//!        -> PolicyAction::CommandShellStart       // allow_shell cap gate
//!        -> command_shell_start audit + shared spawn core
//! ```
//!
//! Caps wiring note: `DaemonState::bootstrap` now threads
//! `config.resolved_caps()` into the engine via `with_config_caps` (TC49
//! Task 6, `state.rs`). To exercise the cap-ON path here deterministically
//! WITHOUT switching the whole bootstrapped profile, `caps_command_runtime`
//! reuses a bootstrapped state's wired Arcs (router / rings / jobs / audit /
//! activation / sources) but builds a SEPARATE `CommandRuntime` whose policy
//! engine has `allow_shell` flipped on via `PolicyEngine::with_config_caps`.
//! The default-deny test goes through the real bootstrapped `state.command`
//! (default config, caps off), proving the default surface stays safe.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::Arc;

use terminal_commanderd::audit::AuditSink;
use terminal_commanderd::command::CommandRuntime;
use terminal_commanderd::policy::{PolicyCaps, PolicyEngine};
use terminal_commanderd::{
    CommandError, DaemonConfig, DaemonState, MAX_SHELL_LINE_BYTES, PolicyProfile, ShellExecRequest,
    ShellRuntime,
};

fn tmp_data_dir(tag: &str) -> PathBuf {
    static TC_DD_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = TC_DD_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    p.push(format!("tc-shell-{tag}-{pid}-{nanos}-{n}"));
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

/// Build a `CommandRuntime` that shares `state`'s wired infrastructure but
/// carries a caps-enabled policy engine (TC49 Task 6 has not yet wired
/// `resolved_caps()` into `bootstrap`, so the engine inside `state.command`
/// still has all caps false). `profile` must be an exec-capable profile for
/// `CommandShellStart` to reach `AllowWithAudit`.
fn caps_command_runtime(
    state: &DaemonState,
    profile: PolicyProfile,
    caps: PolicyCaps,
) -> Arc<CommandRuntime> {
    let policy = PolicyEngine::with_config_caps(profile, None, None, caps);
    Arc::new(CommandRuntime::new(
        Arc::clone(&state.router),
        Arc::clone(&state.rings),
        Arc::clone(&state.jobs),
        Arc::clone(&state.audit) as Arc<dyn AuditSink>,
        policy,
        Arc::clone(&state.activation),
        Arc::clone(&state.sources),
    ))
}

/// Default profile (`developer_local`), caps OFF: the shell lane must be
/// denied by `CommandShellStart` -> `PolicyDenied`. This goes through the
/// REAL bootstrapped command runtime, proving the default surface is safe.
#[test]
fn shell_exec_denied_default_profile() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("deny-default");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let shell = ShellRuntime::new(Arc::clone(&state.command));
        let err = shell
            .exec(ShellExecRequest::line("echo a | wc -c"))
            .unwrap_err();
        assert!(
            matches!(err, CommandError::PolicyDenied(_)),
            "expected PolicyDenied, got: {err:?}"
        );

        cleanup(&data);
    });
}

/// Caps ON: a real pipeline (`echo a | wc -c`) spawns and returns a job id.
/// The pipe + word count is shell behavior the argv lane cannot express,
/// so this also proves the shell line reached an actual shell.
#[test]
fn shell_exec_runs_pipeline_when_cap_on() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("pipeline-cap-on");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let cmd = caps_command_runtime(
            &state,
            PolicyProfile::DeveloperLocal,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let shell = ShellRuntime::new(cmd);

        let resp = shell
            .exec(ShellExecRequest::line("echo a | wc -c"))
            .expect("pipeline must spawn when allow_shell is on");
        assert!(
            !resp.job_id.to_wire_string().is_empty(),
            "spawned job must carry a non-empty job id"
        );

        cleanup(&data);
    });
}

/// A `shell_line` over `MAX_SHELL_LINE_BYTES` is rejected up front with
/// `ArgvItemTooLong` BEFORE any spawn or policy evaluation. `index` is `0`
/// (the request's single user input), not the downstream `argv[2]` slot.
#[test]
fn shell_exec_rejects_oversize_line() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("oversize-line");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        // Cap ON so the rejection cannot be attributed to the policy gate:
        // the oversize guard must fire FIRST, regardless of caps.
        let cmd = caps_command_runtime(
            &state,
            PolicyProfile::DeveloperLocal,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let shell = ShellRuntime::new(cmd);

        let big = "x".repeat(MAX_SHELL_LINE_BYTES + 1);
        let err = shell.exec(ShellExecRequest::line(big)).unwrap_err();
        assert!(
            matches!(
                err,
                CommandError::ArgvItemTooLong { index: 0, len } if len == MAX_SHELL_LINE_BYTES + 1
            ),
            "expected ArgvItemTooLong {{ index: 0 }}, got: {err:?}"
        );

        cleanup(&data);
    });
}

/// Round-6 dedup invariant: the in-flight dedup key is
/// `(peer, argv, cwd, tag)` and `argv[2]` IS the shell line, so two DISTINCT
/// shell lines must NOT collapse to one job — each gets its own `job_id`.
/// (A regression that keyed dedup on a lane tag instead of the full argv
/// would wrongly merge these two starts.)
#[test]
fn shell_exec_distinct_lines_yield_distinct_jobs() {
    let runtime = rt();
    runtime.block_on(async {
        let data = tmp_data_dir("distinct-lines");
        let cfg = DaemonConfig::defaults_in(&data);
        let state = DaemonState::bootstrap(cfg).unwrap();

        let cmd = caps_command_runtime(
            &state,
            PolicyProfile::DeveloperLocal,
            PolicyCaps {
                allow_shell: true,
                ..Default::default()
            },
        );
        let shell = ShellRuntime::new(cmd);

        let a = shell
            .exec(ShellExecRequest::line("echo a | wc -c"))
            .expect("first line spawns");
        let b = shell
            .exec(ShellExecRequest::line("echo bb | wc -c"))
            .expect("second, distinct line spawns");

        assert_ne!(
            a.job_id, b.job_id,
            "distinct shell lines must not collapse to one job"
        );
        assert_ne!(
            a.bucket_id, b.bucket_id,
            "distinct shell lines must land in distinct buckets"
        );

        cleanup(&data);
    });
}
