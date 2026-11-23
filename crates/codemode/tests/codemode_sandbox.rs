// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Adversarial sandbox containment, dual call-budget, feed-vs-wall timer,
//! strict `serde_ignored` keyword-argument, and 60-op catalog contract tests
//! for `terminal-commander-codemode`.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use terminal_commander_codemode::backend::BoxFuture;
use terminal_commander_codemode::params::validate_kwargs_schema_only;
use terminal_commander_codemode::{
    AsyncBridgeBackend, AsyncIpcCaller, CodeBackendError, ExecuteRequest, ExecuteStatus, OpClass,
    TC_CODE_OPS, TcCodeLimits, TcCodeOp, TcCodeRunner, execute_script, execute_script_async,
    render_idioms, render_stubs,
};
use terminal_commander_core::{
    BucketId, EventId, JobId, JobState, ProbeId, RuleId, SignalEvent, SourceStream,
};
use terminal_commander_ipc::{
    BucketWaitResponse, CommandOutputTailResponse, CommandStartResponse, CommandStatusResponse,
    IpcContextFrame, IpcError, IpcErrorCode, IpcRequest, IpcResponse, OutcomeTrust,
    SubscriptionPullResponse,
};

const EXPECTED_60_TOOL_NAMES: &[&str] = &[
    "system_discover",
    "health",
    "policy_status",
    "self_check",
    "audit_since",
    "command_start_combed",
    "run_and_watch",
    "command_status",
    "command_stop",
    "shell_exec",
    "command_output_tail",
    "bucket_events_since",
    "bucket_wait",
    "bucket_summary",
    "event_context",
    "registry_search",
    "registry_get",
    "registry_upsert",
    "registry_test",
    "registry_activate",
    "registry_import_pack",
    "registry_deactivate",
    "registry_list_active",
    "registry_suggest_from_samples",
    "recipe_search",
    "recipe_get",
    "recipe_upsert",
    "recipe_test",
    "recipe_activate",
    "recipe_deactivate",
    "recipe_list_active",
    "recipe_run",
    "file_read_window",
    "file_search",
    "file_write",
    "file_watch_start",
    "file_watch_stop",
    "file_watch_list",
    "pty_command_start",
    "pty_command_write_stdin",
    "pty_command_stop",
    "pty_command_list",
    "credential_request",
    "shell_session_start",
    "shell_session_exec",
    "shell_session_status",
    "shell_session_stop",
    "shell_session_list",
    "workspace_snapshot_create",
    "workspace_snapshot_apply",
    "runtime_state",
    "probe_list",
    "probe_status",
    "subscription_open",
    "subscription_pull",
    "subscription_list",
    "subscription_close",
    "subscription_seek",
    "target_list",
    "target_probe",
];

#[allow(clippy::needless_pass_by_value)]
fn mock_triage_backend(
    op: TcCodeOp,
    kwargs: Map<String, Value>,
    _remaining_wall: Duration,
) -> Result<Value, CodeBackendError> {
    validate_kwargs_schema_only(op, kwargs.clone())?;
    match op {
        TcCodeOp::RunAndWatch => Ok(json!({
            "job_id": "job_00000000000000000000000000000001",
            "bucket_id": "bkt_00000000000000000000000000000001",
            "state": "failed",
            "exit_code": 101,
            "pipeline_exit_masked": false,
            "signals": [{"event_id": "evt_00000000000000000000000000000001", "summary": "error[E0308]: mismatched types"}],
            "signal_count": 1,
            "signals_capped": false,
            "compact": false,
            "receipt": Value::Null,
            "complete": true,
            "wait_exhausted": false,
            "wait_cap_ms": 60_000,
            "poll_hint_ms": Value::Null,
            "cursor": 1,
            "degraded": false,
            "recover_hint": Value::Null,
            "outcome_trust": "observed",
        })),
        TcCodeOp::BucketEventsSince => Ok(json!({
            "bucket_id": "bkt_00000000000000000000000000000001",
            "cursor_in": 0,
            "next_cursor": 2,
            "has_more": false,
            "dropped_count": 0,
            "events": [
                {
                    "event_id": "evt_00000000000000000000000000000001",
                    "bucket_id": "bkt_00000000000000000000000000000001",
                    "seq": 1,
                    "severity": "high",
                    "kind": "compile_error",
                    "summary": "error[E0308]: mismatched types"
                },
                {
                    "event_id": "evt_00000000000000000000000000000002",
                    "bucket_id": "bkt_00000000000000000000000000000001",
                    "seq": 2,
                    "severity": "high",
                    "kind": "test_failure",
                    "summary": "test runner::smoke ... FAILED"
                }
            ]
        })),
        TcCodeOp::EventContext => {
            let eid = kwargs.get("event_id").and_then(Value::as_str).unwrap_or("");
            Ok(json!({
                "bucket_id": "bkt_00000000000000000000000000000001",
                "event_id": eid,
                "anchor_missing": false,
                "unavailable_reason": Value::Null,
                "pointer_unavailable_reason": Value::Null,
                "frames": [
                    {
                        "probe_id": "prb_00000000000000000000000000000001",
                        "frame_id": "frm_00000000000000000000000000000001",
                        "stream": "stderr",
                        "line": 14,
                        "text": format!("context for {eid}")
                    }
                ],
                "total_bytes": 32,
                "truncated": false,
            }))
        }
        TcCodeOp::CommandOutputTail => Ok(json!({
            "job_id": "job_00000000000000000000000000000001",
            "lines": ["error: could not compile `demo`", "build failed"],
            "returned_lines": 2,
            "truncated_lines": false,
            "truncated_bytes": false,
            "evicted_frames": 0,
        })),
        TcCodeOp::Health => Ok(json!({
            "ok": true,
            "uptime_secs": 42,
            "version": "0.3.15",
            "identity": Value::Null,
        })),
        TcCodeOp::FileWrite => Ok(json!({
            "path": kwargs.get("path").cloned().unwrap_or(Value::Null),
            "bytes_written": kwargs.get("content").and_then(Value::as_str).map_or(0, str::len),
        })),
        _ => Ok(json!({ "op": op.as_str(), "kwargs": kwargs })),
    }
}

fn sample_valid_param_value(param_name: &str, py_type: &str) -> Value {
    match param_name {
        "argv" => json!(["cargo", "check"]),
        "env" => json!([{"key": "RUST_LOG", "value": "info"}]),
        "rules" => json!([{"pattern": "ERROR"}]),
        "rules_json" => Value::Null,
        "limits" => json!({"memory": "512MiB", "priority": "normal"}),
        "scope" => json!({"kind": "global"}),
        "samples" => json!([{"text": "error[E0308]: mismatched types", "stream": "stderr"}]),
        "sample_lines" => json!(["error[E0308]: mismatched types"]),
        "fills" => json!({"target": "debug"}),
        "sources" => json!({"kind": "all"}),
        "kind" if py_type.starts_with("list[") => json!(["error"]),
        "rule_ids" => json!(["cargo.compile-error"]),
        "detail" => json!("summary"),
        "wait_until" => json!("exit"),
        "severity_min" => json!("high"),
        "bucket_id" => json!("bkt_00000000000000000000000000000001"),
        "job_id" | "watch_id" => json!("job_00000000000000000000000000000001"),
        "probe_id" => json!("prb_00000000000000000000000000000001"),
        "event_id" => json!("evt_00000000000000000000000000000001"),
        "session_id" => json!("ses_00000000000000000000000000000001"),
        _ => {
            if py_type.starts_with("bool") {
                json!(true)
            } else if py_type.starts_with("int") {
                json!(1)
            } else if py_type.starts_with("list[str]") {
                json!(["item"])
            } else if py_type.starts_with("dict") {
                json!({})
            } else {
                json!("sample")
            }
        }
    }
}

fn minimal_required_kwargs(op: TcCodeOp) -> Map<String, Value> {
    let mut map = Map::new();
    for p in op.descriptor().params {
        if p.required {
            map.insert(
                p.name.to_owned(),
                sample_valid_param_value(p.name, p.py_type),
            );
        }
    }
    map
}

#[test]
fn multi_step_python_triage_chain_executes_and_tracks_calls_and_mutations() {
    let runner = TcCodeRunner::default();
    let code = r#"
run = run_and_watch(
    argv=["cargo", "test", "--workspace"],
    rules=[{"pattern": r"error(\[E\d+\])?:|FAILED", "severity": "high"}],
    wait_ms=15000,
)
ev = bucket_events_since(bucket_id=run["bucket_id"], cursor=0, severity_min="high")
contexts = [event_context(event_id=e["event_id"]) for e in ev["events"]]
tail = command_output_tail(job_id=run["job_id"], max_lines=10, strip_ansi=True)
print("triaged", len(ev["events"]), "events")
{
    "ok": run["exit_code"] == 0,
    "exit_code": run["exit_code"],
    "error_count": len(ev["events"]),
    "first_frame": contexts[0]["frames"][0]["text"],
    "tail_last": tail["lines"][-1],
}
"#;
    let out = runner.execute(code, &mock_triage_backend);
    assert_eq!(
        out.status,
        ExecuteStatus::Ok,
        "unexpected error: {:?}",
        out.error
    );
    // 1 run_and_watch (Mutate) + 1 bucket_events_since (Read) + 2 event_context (Read) + 1 command_output_tail (Read) = 5 calls, 1 mutation
    assert_eq!(out.calls, 5);
    assert_eq!(out.mutations, 1);
    assert_eq!(out.stdout, "triaged 2 events\n");
    assert_eq!(
        out.value,
        Some(json!({
            "ok": false,
            "exit_code": 101,
            "error_count": 2,
            "first_frame": "context for evt_00000000000000000000000000000001",
            "tail_last": "build failed",
        }))
    );
}

#[test]
fn positional_arguments_raise_type_error_with_signature_hint_and_zero_calls() {
    let runner = TcCodeRunner::default();
    let out = runner.execute(r#"run_and_watch(["git", "status"])"#, &mock_triage_backend);
    assert_eq!(out.status, ExecuteStatus::Error);
    assert_eq!(out.calls, 0);
    assert_eq!(out.mutations, 0);
    let err = out.error.expect("error payload");
    assert_eq!(err.kind, "type_error");
    assert_eq!(err.exception_type, "TypeError");
    assert!(
        err.message.contains("keyword arguments only")
            && err.message.contains("def run_and_watch(*, argv: list[str]"),
        "got message: {}",
        err.message
    );
}

#[test]
fn all_60_catalogue_ops_exist_unique_and_reject_unknown_kwargs_with_signature() {
    assert_eq!(TC_CODE_OPS.len(), 60);
    assert_eq!(TcCodeOp::ALL.len(), 60);
    assert_eq!(EXPECTED_60_TOOL_NAMES.len(), 60);

    let catalog_names: BTreeSet<&str> = TC_CODE_OPS.iter().map(|d| d.name).collect();
    let expected_names: BTreeSet<&str> = EXPECTED_60_TOOL_NAMES.iter().copied().collect();
    assert_eq!(
        catalog_names, expected_names,
        "TC_CODE_OPS must match the exact 60 full-surface tools"
    );

    let runner = TcCodeRunner::default();
    for desc in TC_CODE_OPS {
        assert_eq!(TcCodeOp::from_name(desc.name), Some(desc.op));
        assert_eq!(desc.op.as_str(), desc.name);
        assert_eq!(desc.op.op_class(), desc.op_class);

        // Passing `unknown_kwarg_xyz=123` alongside minimal required kwargs must raise TypeError
        // with `unexpected keyword argument` and `def <op_name>` for EVERY one of the 60 ops.
        let mut bad_kwargs = minimal_required_kwargs(desc.op);
        bad_kwargs.insert("unknown_kwarg_xyz".to_owned(), json!(123));
        let err = validate_kwargs_schema_only(desc.op, bad_kwargs)
            .expect_err("unknown_kwarg_xyz must be rejected");
        assert_eq!(err.kind, "type_error", "op {} kind mismatch", desc.name);
        assert!(
            err.message
                .contains("unexpected keyword argument 'unknown_kwarg_xyz'")
                && err.message.contains(&format!("def {}", desc.name)),
            "op {} bad message: {}",
            desc.name,
            err.message
        );

        // Also prove end-to-end inside Monty VM that TypeError is raised!
        let snippet = format!("{}(unknown_kwarg_xyz=123)", desc.name);
        let out = runner.execute(&snippet, &mock_triage_backend);
        assert_eq!(
            out.status,
            ExecuteStatus::Error,
            "VM should fail on unknown kwarg for {}",
            desc.name
        );
        let vm_err = out.error.expect("vm error");
        assert_eq!(vm_err.kind, "type_error");
        assert_eq!(vm_err.exception_type, "TypeError");
        assert!(
            vm_err.message.contains(&format!("def {}", desc.name)),
            "VM TypeError for {} missing signature: {}",
            desc.name,
            vm_err.message
        );
    }

    // Specific common LLM mistakes called out in handover section 4.5:
    for (snippet, offending, sig_prefix) in [
        (
            r#"run_and_watch(argv=["echo", "hi"], timeout_ms=1000)"#,
            "timeout_ms",
            "def run_and_watch(*,",
        ),
        (
            r#"bucket_wait(job_id="job_00000000000000000000000000000001", cursor=0)"#,
            "job_id",
            "def bucket_wait(*,",
        ),
        ("health(foo=1)", "foo", "def health()"),
    ] {
        let out = runner.execute(snippet, &mock_triage_backend);
        assert_eq!(out.status, ExecuteStatus::Error);
        let err = out.error.expect("error");
        assert_eq!(err.exception_type, "TypeError");
        assert!(
            err.message.contains(offending) && err.message.contains(sig_prefix),
            "expected '{offending}' and '{sig_prefix}' in '{}'",
            err.message
        );
    }
}

#[test]
fn zero_drift_contract_every_catalog_paramspec_is_accepted_by_serde_deserializer() {
    for desc in TC_CODE_OPS {
        // 1. Minimal required kwargs must decode cleanly.
        let required_only = minimal_required_kwargs(desc.op);
        validate_kwargs_schema_only(desc.op, required_only.clone()).unwrap_or_else(|e| {
            panic!(
                "op `{}` rejected minimal required kwargs {:?}: {e}",
                desc.name, required_only
            )
        });

        // 2. Every individual optional ParamSpec in catalog.rs must be consumed by serde_ignored
        //    (never flagged as an unknown keyword argument).
        for param in desc.params {
            let mut single_opt = required_only.clone();
            if !(desc.op == TcCodeOp::CommandStartCombed
                || desc.op == TcCodeOp::RunAndWatch
                || desc.op == TcCodeOp::ShellExec
                || desc.op == TcCodeOp::FileWatchStart
                || desc.op == TcCodeOp::PtyCommandStart
                || desc.op == TcCodeOp::ShellSessionStart)
                || param.name != "rules_json"
            {
                single_opt.insert(
                    param.name.to_owned(),
                    sample_valid_param_value(param.name, param.py_type),
                );
            } else {
                single_opt.insert("rules_json".to_owned(), json!("[]"));
            }
            validate_kwargs_schema_only(desc.op, single_opt).unwrap_or_else(|e| {
                panic!(
                    "op `{}` ParamSpec `{}` ({}) drifted from params.rs serde struct: {e}",
                    desc.name, param.name, param.py_type
                )
            });
        }
    }

    // Also verify `samples` alias on `registry_suggest_from_samples` is accepted!
    let mut alias_map = Map::new();
    alias_map.insert("samples".to_owned(), json!(["ERROR: failed"]));
    validate_kwargs_schema_only(TcCodeOp::RegistrySuggestFromSamples, alias_map)
        .expect("registry_suggest_from_samples must accept legacy 'samples' alias");
}

#[test]
fn zero_egress_containment_blocks_filesystem_os_subprocess_and_network() {
    let runner = TcCodeRunner::default();

    for (label, snippet, expected_exc) in [
        ("open", r#"open(".env").read()"#, "PermissionError"),
        (
            "os_listdir",
            r#"import os; os.listdir(".")"#,
            "PermissionError",
        ),
        (
            "pathlib_read",
            r#"import pathlib; pathlib.Path("Cargo.toml").read_text()"#,
            "PermissionError",
        ),
        ("subprocess", "import subprocess", "ModuleNotFoundError"),
        ("socket", "import socket", "ModuleNotFoundError"),
        ("urllib", "import urllib.request", "ModuleNotFoundError"),
    ] {
        let out = runner.execute(snippet, &mock_triage_backend);
        assert_eq!(out.status, ExecuteStatus::Error, "case {label} must fail");
        let err = out.error.expect("error info");
        assert_eq!(
            err.exception_type, expected_exc,
            "case {label} expected {expected_exc}, got {err:?}"
        );
    }
}

#[test]
fn sleep_resolves_immediately_under_zero_sleep_policy() {
    let runner = TcCodeRunner::default();
    let t0 = Instant::now();
    let out = runner.execute("import time\ntime.sleep(5)\n42", &mock_triage_backend);
    let elapsed = t0.elapsed();
    assert_eq!(
        out.status,
        ExecuteStatus::Ok,
        "sleep should succeed immediately: {:?}",
        out.error
    );
    assert_eq!(out.value, Some(json!(42)));
    assert!(
        elapsed < Duration::from_millis(500),
        "SleepMode::Zero should not block wall clock, took {elapsed:?}"
    );
}

#[test]
fn infinite_cpu_loop_is_preempted_by_feed_timeout() {
    let limits = TcCodeLimits {
        max_feed_duration: Duration::from_millis(50),
        max_wall_duration: Duration::from_secs(5),
        ..TcCodeLimits::default()
    };
    let runner = TcCodeRunner::new(limits);
    let t0 = Instant::now();
    let out = runner.execute("while True:\n    pass\n", &mock_triage_backend);
    let elapsed = t0.elapsed();
    assert_eq!(out.status, ExecuteStatus::Error);
    let err = out.error.expect("timeout error");
    assert_eq!(err.kind, "timeout");
    assert_eq!(err.exception_type, "TimeoutError");
    assert!(
        elapsed < Duration::from_secs(2),
        "CPU feed timeout took too long: {elapsed:?}"
    );
}

#[test]
fn host_wait_exceeding_feed_budget_succeeds_while_wall_budget_enforces_hard_cap() {
    // Part A: `max_feed_duration = 80ms`, `max_wall_duration = 2s`.
    // Mock `bucket_wait` sleeps 220ms on the OS thread while Monty is suspended.
    // Because Monty's CPU feed clock is paused during host suspension, this MUST succeed!
    let observed_remaining = AtomicU64::new(0);
    let sleeping_backend = |op: TcCodeOp, _kw: Map<String, Value>, rem: Duration| {
        observed_remaining.store(
            u64::try_from(rem.as_millis()).unwrap_or(0),
            Ordering::Relaxed,
        );
        if op == TcCodeOp::BucketWait {
            std::thread::sleep(Duration::from_millis(220));
            Ok(json!({
                "bucket_id": "bkt_00000000000000000000000000000001",
                "cursor_in": 0,
                "next_cursor": 1,
                "heartbeat": true,
                "dropped_count": 0,
                "events": [],
            }))
        } else {
            Ok(json!({}))
        }
    };

    let ok_limits = TcCodeLimits {
        max_feed_duration: Duration::from_millis(80),
        max_wall_duration: Duration::from_secs(2),
        ..TcCodeLimits::default()
    };
    let ok_runner = TcCodeRunner::new(ok_limits);
    let out = ok_runner.execute(
        r#"
w = bucket_wait(bucket_id="bkt_00000000000000000000000000000001", cursor=0, timeout_ms=500)
w["next_cursor"]
"#,
        &sleeping_backend,
    );
    assert_eq!(
        out.status,
        ExecuteStatus::Ok,
        "host wait > max_feed_duration must succeed when within max_wall_duration: {:?}",
        out.error
    );
    assert_eq!(out.value, Some(json!(1)));
    assert!(observed_remaining.load(Ordering::Relaxed) > 1000);

    // Part B: Exceeding `max_wall_duration` (100ms) during the 220ms host wait aborts with TimeoutError.
    let tight_wall_limits = TcCodeLimits {
        max_feed_duration: Duration::from_secs(5),
        max_wall_duration: Duration::from_millis(100),
        ..TcCodeLimits::default()
    };
    let wall_runner = TcCodeRunner::new(tight_wall_limits);
    let wall_out = wall_runner.execute(
        r#"
try:
    bucket_wait(bucket_id="bkt_00000000000000000000000000000001", cursor=0, timeout_ms=500)
except Exception:
    pass
"#,
        &sleeping_backend,
    );
    assert_eq!(wall_out.status, ExecuteStatus::Error);
    let err = wall_out.error.expect("wall timeout error");
    assert_eq!(err.kind, "timeout");
    assert_eq!(err.exception_type, "TimeoutError");
}

#[test]
fn memory_bomb_is_rejected_with_memory_limit_error() {
    let limits = TcCodeLimits {
        max_memory_bytes: 4 * 1024 * 1024,
        ..TcCodeLimits::default()
    };
    let runner = TcCodeRunner::new(limits);
    let out = runner.execute("'x' * 100_000_000", &mock_triage_backend);
    assert_eq!(out.status, ExecuteStatus::Error);
    let err = out.error.expect("memory error");
    assert_eq!(err.kind, "memory_limit");
    assert_eq!(err.exception_type, "MemoryError");
}

#[test]
fn deep_recursion_is_capped_with_recursion_limit_error() {
    let limits = TcCodeLimits {
        max_recursion_depth: 40,
        ..TcCodeLimits::default()
    };
    let runner = TcCodeRunner::new(limits);
    let out = runner.execute(
        "def f(n):\n    return f(n + 1)\nf(0)\n",
        &mock_triage_backend,
    );
    assert_eq!(out.status, ExecuteStatus::Error);
    let err = out.error.expect("recursion error");
    assert_eq!(err.kind, "recursion_limit");
    assert_eq!(err.exception_type, "RecursionError");
}

#[test]
fn dual_call_budgets_enforce_host_call_limit_and_mutation_call_limit_uncatchably() {
    // 1. max_host_calls = 5 aborts runaway Read loop even inside try/except.
    let read_hits = AtomicUsize::new(0);
    let counting_read_backend = |_op: TcCodeOp,
                                 _kw: Map<String, Value>,
                                 _rem: Duration|
     -> Result<Value, CodeBackendError> {
        read_hits.fetch_add(1, Ordering::Relaxed);
        Ok(json!({ "ok": true }))
    };
    let host_runner = TcCodeRunner::new(TcCodeLimits {
        max_host_calls: 5,
        ..TcCodeLimits::default()
    });
    let host_out = host_runner.execute(
        r"
for i in range(40):
    try:
        health()
    except Exception:
        pass
",
        &counting_read_backend,
    );
    assert_eq!(host_out.status, ExecuteStatus::Error);
    assert_eq!(host_out.calls, 5);
    assert_eq!(host_out.mutations, 0);
    assert_eq!(read_hits.load(Ordering::Relaxed), 5);
    assert_eq!(host_out.error.unwrap().kind, "host_call_limit");

    // 2. max_mutation_calls = 3 (with max_host_calls = 64) allows 10 Read calls (`health()`)
    //    + 3 Mutate calls (`file_write(...)`), then aborts uncatchably on the 4th Mutate call!
    let mutate_hits = AtomicUsize::new(0);
    let mixed_backend =
        |op: TcCodeOp, kw: Map<String, Value>, rem: Duration| -> Result<Value, CodeBackendError> {
            if op.op_class() == OpClass::Mutate {
                mutate_hits.fetch_add(1, Ordering::Relaxed);
            }
            mock_triage_backend(op, kw, rem)
        };
    let mut_runner = TcCodeRunner::new(TcCodeLimits {
        max_host_calls: 64,
        max_mutation_calls: 3,
        ..TcCodeLimits::default()
    });
    let mut_out = mut_runner.execute(
        r#"
for _ in range(10):
    health()
for idx in range(6):
    try:
        file_write(path="/tmp/out.txt", content="hello")
    except Exception:
        pass
"#,
        &mixed_backend,
    );
    assert_eq!(mut_out.status, ExecuteStatus::Error);
    assert_eq!(mut_out.calls, 13, "10 reads + 3 mutations before abort");
    assert_eq!(mut_out.mutations, 3);
    assert_eq!(mutate_hits.load(Ordering::Relaxed), 3);
    let mut_err = mut_out.error.expect("mutation limit error");
    assert_eq!(mut_err.kind, "mutation_call_limit");
    assert!(
        mut_err
            .message
            .contains("mutation call limit exceeded (3): split batch mutations"),
        "unexpected message: {}",
        mut_err.message
    );
}

#[test]
fn oversized_output_spills_losslessly_to_tc_code_spill_and_honors_allow_spill_false() {
    let temp = tempfile::tempdir().expect("tempdir");

    // 1. allow_spill_files = false writes zero files.
    let no_spill_req = ExecuteRequest {
        code: "{'big': 'z' * 2000}".to_owned(),
        limits: TcCodeLimits {
            max_output_chars: 256,
            allow_spill_files: false,
            ..TcCodeLimits::default()
        },
        spill_dir: Some(temp.path().to_path_buf()),
    };
    let no_spill_out = execute_script(&no_spill_req, &mock_triage_backend);
    assert_eq!(no_spill_out.status, ExecuteStatus::Ok);
    assert_eq!(no_spill_out.truncated, Some(true));
    assert!(no_spill_out.spill_path.is_none());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);

    // 2. allow_spill_files = true writes `tc-code-spill-*.json` with lossless payload and evicts > 32.
    let mut recorded_paths = Vec::new();
    for idx in 0..35 {
        let req = ExecuteRequest {
            code: format!("{{'idx': {idx}, 'pad': '\\u00e9' * 1500}}"),
            limits: TcCodeLimits {
                max_output_chars: 256,
                allow_spill_files: true,
                ..TcCodeLimits::default()
            },
            spill_dir: Some(temp.path().to_path_buf()),
        };
        let out = execute_script(&req, &mock_triage_backend);
        assert_eq!(out.status, ExecuteStatus::Ok);
        assert_eq!(out.truncated, Some(true));
        let p = out.spill_path.expect("spill_path present");
        assert!(
            p.contains("tc-code-spill-")
                && std::path::Path::new(&p)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json")),
            "unexpected spill filename: {p}"
        );
        if idx == 34 {
            let saved: Value =
                serde_json::from_str(&std::fs::read_to_string(&p).expect("read spill"))
                    .expect("parse spill");
            assert_eq!(saved["value"]["idx"], 34);
            assert_eq!(
                saved["value"]["pad"].as_str().map(|s| s.chars().count()),
                Some(1500)
            );
            assert_eq!(saved["mutations"], 0);
        }
        recorded_paths.push(p);
    }

    let remaining = std::fs::read_dir(temp.path()).unwrap().count();
    assert!(
        remaining <= 32,
        "spill ring must cap retained files at <= 32, found {remaining}"
    );
    assert!(!std::path::Path::new(&recorded_paths[0]).exists());
    assert!(std::path::Path::new(recorded_paths.last().unwrap()).exists());
}

#[test]
fn render_stubs_and_idioms_cover_all_60_ops_and_domain_groups() {
    let index = render_stubs(None, None);
    let idioms = render_idioms();

    for heading in [
        "## Commands & Shell (command)",
        "## Buckets & Event Context (bucket_events)",
        "## Files & File Watches (files)",
        "## Comb Rule Registry (registry)",
        "## Argv Recipes (recipe)",
        "## PTY, Credentials & Persistent Sessions (pty_session)",
        "## Multiplexed Subscriptions (subscriptions)",
        "## Status, Discovery, Audit, Runtime & Federation (runtime_audit_federation)",
    ] {
        assert!(
            index.contains(heading),
            "domain heading `{heading}` missing from index"
        );
    }

    for desc in TC_CODE_OPS {
        let sig = desc.op.signature_one_liner();
        assert!(
            index.contains(&sig),
            "empty-query stub index missing `{sig}`"
        );
        assert!(idioms.contains(&sig), "idioms manual missing `{sig}`");
    }

    let detailed = render_stubs(Some("bucket_wait event_context"), Some(3));
    assert!(detailed.contains("from typing import TypedDict"));
    assert!(detailed.contains("class BucketWaitOut(TypedDict):"));
    assert!(detailed.contains("class EventContextOut(TypedDict):"));
    assert!(detailed.contains("def bucket_wait(*,"));
    assert!(detailed.contains("def event_context(*,"));

    let suggest_stub = render_stubs(Some("registry_suggest_from_samples"), Some(1));
    assert!(suggest_stub.contains("proposed_rules: list[RuleDef]"));
}

struct FakeAsyncIpc {
    job_id: JobId,
    bucket_id: BucketId,
    probe_id: ProbeId,
    event_id: EventId,
    last_wait_timeout_ms: Arc<AtomicU64>,
    last_pull_timeout_ms: Arc<AtomicU64>,
}

impl AsyncIpcCaller for FakeAsyncIpc {
    #[allow(clippy::too_many_lines)]
    fn call_ipc<'a>(
        &'a self,
        request: IpcRequest,
        _target_id: Option<&'a str>,
        _timeout: Duration,
    ) -> BoxFuture<'a, Result<IpcResponse, CodeBackendError>> {
        Box::pin(async move {
            match request {
                IpcRequest::PolicyStatus => Err(CodeBackendError::refusal("mock", "unused")),
                IpcRequest::CommandStartCombed(_) => {
                    Ok(IpcResponse::CommandStartCombed(CommandStartResponse {
                        job_id: self.job_id,
                        bucket_id: self.bucket_id,
                        probe_id: self.probe_id,
                        cursor: 0,
                        hint: None,
                        wslenv_dropped: Vec::new(),
                        limits_applied: None,
                        limits_clamped: Vec::new(),
                        governor: None,
                    }))
                }
                IpcRequest::CommandStatus(_) => {
                    Ok(IpcResponse::CommandStatus(CommandStatusResponse {
                        job_id: self.job_id,
                        bucket_id: self.bucket_id,
                        probe_id: self.probe_id,
                        state: JobState::Exited,
                        frames_total: 2,
                        frames_stdout: 0,
                        frames_stderr: 2,
                        bytes_total: 48,
                        events_emitted: 1,
                        frames_suppressed: 0,
                        frames_suppressed_progress: 0,
                        frames_suppressed_dedupe: 0,
                        exit_code: Some(1),
                        signal: None,
                        duration_ms: Some(12),
                        receipt: None,
                        restarted: false,
                        outcome_trust: OutcomeTrust::Observed,
                        pipeline_exit_masked: false,
                        awaiting_credential: None,
                        elapsed_ms: None,
                        last_output_age_ms: None,
                        governor: None,
                        limits_applied: None,
                        peak_memory_bytes: None,
                        exit_reason: None,
                        host_ceiling_joined: None,
                        process_observation: None,
                        process_cleanup: None,
                        process_identity: None,
                        cpu: None,
                    }))
                }
                IpcRequest::BucketWait(p) => {
                    self.last_wait_timeout_ms
                        .store(p.timeout_ms.unwrap_or(0), Ordering::Relaxed);
                    let ev: SignalEvent = serde_json::from_value(json!({
                        "event_id": self.event_id,
                        "bucket_id": self.bucket_id,
                        "seq": 1,
                        "timestamp": "1970-01-01T00:00:00Z",
                        "severity": "high",
                        "kind": "compile_error",
                        "summary": "error[E0425]: cannot find value",
                        "rule": {
                            "id": RuleId::new(),
                            "version": 1,
                            "registry_id": "inline-0"
                        },
                        "source": {
                            "probe_id": self.probe_id,
                            "source_type": "process",
                            "stream": "stderr",
                            "job_id": self.job_id
                        },
                        "pointer": {
                            "frame_id": terminal_commander_core::FrameId::new(),
                            "line": 7,
                            "stream": "stderr",
                            "context_available": true
                        }
                    }))
                    .expect("valid SignalEvent JSON");
                    Ok(IpcResponse::BucketWait(BucketWaitResponse {
                        bucket_id: self.bucket_id,
                        cursor_in: p.cursor,
                        next_cursor: 1,
                        heartbeat: false,
                        wake_reason: Some(terminal_commander_core::BucketWakeReason::Append),
                        timeout_clamped_to_ms: None,
                        job_exit: None,
                        dropped_count: 0,
                        events: vec![ev],
                    }))
                }
                IpcRequest::EventContext(p) => Ok(IpcResponse::EventContext(
                    terminal_commander_ipc::EventContextResponse {
                        bucket_id: self.bucket_id,
                        event_id: p.event_id,
                        anchor_missing: false,
                        unavailable_reason: None,
                        pointer_unavailable_reason: None,
                        frames: vec![IpcContextFrame {
                            probe_id: self.probe_id,
                            frame_id: terminal_commander_core::FrameId::new(),
                            stream: SourceStream::Stderr,
                            line: Some(7),
                            text: "let x = missing_ident;".to_owned(),
                        }],
                        total_bytes: 22,
                        truncated: false,
                    },
                )),
                IpcRequest::CommandOutputTail(p) => {
                    Ok(IpcResponse::CommandOutputTail(CommandOutputTailResponse {
                        job_id: p.job_id,
                        lines: vec!["error[E0425]: cannot find value".to_owned()],
                        returned_lines: 1,
                        truncated_lines: false,
                        truncated_bytes: false,
                        evicted_frames: 0,
                    }))
                }
                IpcRequest::SubscriptionPull(p) => {
                    self.last_pull_timeout_ms
                        .store(p.timeout_ms.unwrap_or(0), Ordering::Relaxed);
                    Ok(IpcResponse::SubscriptionPull(SubscriptionPullResponse {
                        events: Vec::new(),
                        liveness: Vec::new(),
                        lagged: false,
                        truncated: false,
                    }))
                }
                IpcRequest::ShellExec(_) => Err(CodeBackendError::from_ipc_error(&IpcError::new(
                    IpcErrorCode::PolicyDenied,
                    "allow_shell capability is disabled",
                ))),
                _ => Err(CodeBackendError::refusal("Unsupported", "unmocked in fake")),
            }
        })
    }
}

#[tokio::test]
async fn async_bridge_backend_dispatches_compound_run_and_watch_and_clamps_wait_to_wall_budget() {
    let last_wait = Arc::new(AtomicU64::new(0));
    let last_pull = Arc::new(AtomicU64::new(0));
    let fake = FakeAsyncIpc {
        job_id: JobId::new(),
        bucket_id: BucketId::new(),
        probe_id: ProbeId::new(),
        event_id: EventId::new(),
        last_wait_timeout_ms: Arc::clone(&last_wait),
        last_pull_timeout_ms: Arc::clone(&last_pull),
    };
    let bucket_wire = fake.bucket_id.to_wire_string();

    let handle = tokio::runtime::Handle::current();
    let backend = AsyncBridgeBackend::new(handle, fake);

    let req = ExecuteRequest {
        code: format!(
            r#"
run = run_and_watch(argv=["cargo", "check"], rules=[{{"pattern": "error"}}], wait_ms=60000)
ctx = event_context(event_id=run["signals"][0]["event_id"])
tail = command_output_tail(job_id=run["job_id"], strip_ansi=True)
w = bucket_wait(bucket_id="{bucket_wire}", cursor=1, timeout_ms=50000)
p = subscription_pull(sub_id="sub-1", timeout_ms=8000)
denied = None
try:
    shell_exec(shell_line="echo hi | cat")
except RuntimeError as exc:
    denied = str(exc)
{{
    "exit_code": run["exit_code"],
    "signal_summary": run["signals"][0]["summary"],
    "ctx_line": ctx["frames"][0]["text"],
    "tail": tail["lines"],
    "pull_empty": len(p["events"]) == 0,
    "denied": denied,
}}
"#
        ),
        limits: TcCodeLimits {
            // 600ms total wall budget: both bucket_wait(50000) and subscription_pull(8000)
            // MUST be clamped to <= 600ms!
            max_wall_duration: Duration::from_millis(600),
            ..TcCodeLimits::default()
        },
        spill_dir: None,
    };

    let out = execute_script_async(req, backend)
        .await
        .expect("spawn_blocking join");
    assert_eq!(
        out.status,
        ExecuteStatus::Ok,
        "async bridge script failed: {:?}",
        out.error
    );
    // 1 run_and_watch (Mutate) + 1 event_context + 1 command_output_tail + 1 bucket_wait + 1 subscription_pull + 1 shell_exec (Mutate) = 6 calls, 2 mutations
    assert_eq!(out.calls, 6);
    assert_eq!(out.mutations, 2);
    assert!(
        last_wait.load(Ordering::Relaxed) <= 600,
        "bucket_wait timeout_ms was not clamped to remaining_wall: {}",
        last_wait.load(Ordering::Relaxed)
    );
    assert!(
        last_pull.load(Ordering::Relaxed) <= 600,
        "subscription_pull timeout_ms was not clamped to remaining_wall: {}",
        last_pull.load(Ordering::Relaxed)
    );
    let val = out.value.expect("value");
    assert_eq!(val["exit_code"], 1);
    assert_eq!(val["signal_summary"], "error[E0425]: cannot find value");
    assert_eq!(val["ctx_line"], "let x = missing_ident;");
    assert_eq!(val["pull_empty"], true);
    assert!(
        val["denied"]
            .as_str()
            .unwrap_or("")
            .contains("PolicyDenied"),
        "expected PolicyDenied in {:?}",
        val["denied"]
    );
}
