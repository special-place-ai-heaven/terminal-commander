# Terminal Commander: MCP diagnostics, embedded parity, and AAP handoff

As of 2026-10-09. Uncommitted report requested for the Terminal Commander repair/integration agent.
AAP worktree: C:\AI_STUFF\PROGRAMMING\aap-kernel-24.
Branch: feat/tool-kernel-remainder. Verified hardening HEAD/remote: 6a138ee3cb89de85ffff0c42c101af8b555a841e.
Related evidence: CODEX-REPORT-24.md. SymForge has its own separate report.

## Conclusion and ownership

AAP embeds TC 0.1.86; this session's installed MCP adapter/daemon reports 0.3.14. Embedded parity is not achieved. Comparing official 0.3.14 source shows that simply upgrading the dependency would leave the output-liveness, Unix ownership, and descendant-CPU gaps described below. The TC agent should repair and publish the reusable embedded contracts; the AAP agent should migrate and consume those contracts, complete room-runtime integration, and demonstrate live parity.

The original instruction pins TC to =0.1.86 on this port. That pin remains intact. The latest request establishes embedded parity as the desired outcome; this document supplies the concrete cross-repository repair/migration work without silently substituting a newer dependency or mounting the deferred execution surface.

No TC repository, daemon, optional-server configuration, or dependency version was changed for this investigation. The backend invoke route and aap_supervised_shell remain deferred for the explicitly requested security review.

## Exact identities

| Component | Observed identity | Evidence / limit |
|---|---|---|
| AAP embedded core, probes, sifters | Exact crates.io =0.1.86 for all three | Cargo.toml:40-42; Cargo.lock:6657-6705 |
| core checksum | 562597467435006ba911d7383f0729cf80944c4b511a882ff6c75067b8ca0490 | Cargo.lock |
| probes checksum | 8e54a135aee287a7058c73268fac485a91eaaad0f4729d6780e4580f17300873 | Cargo.lock |
| sifters checksum | 5e266c21b43ec4afce11686a79d23403ab0d6d731238ce96265ddd4cc6c6ee0f | Cargo.lock |
| Installed MCP adapter and daemon | 0.3.14 | Live health and system_discover(summary); version string does not identify a binary build hash |
| Official v0.3.14 release source | 7e4e2c08ede01c257601ab99e32f128dffb249c0 | [Official release](https://github.com/special-place-ai-heaven/terminal-commander/releases/tag/v0.3.14), [commit](https://github.com/special-place-ai-heaven/terminal-commander/commit/7e4e2c08ede01c257601ab99e32f128dffb249c0) |
| Local TC checkout, inspected read-only | e13972272309e7eef9e000c54c9655d021a7b797; feat/resource-governor; describe v0.3.12-33-ge139722 | Unpublished checkout is distinct from installed binary and v0.3.14 tag |

Pinned Windows source: C:/Users/poslj/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/terminal-commander-probes-0.1.86/src/process.rs.
Official comparison: [0.1.86 ProcessProbe](https://raw.githubusercontent.com/special-place-ai-heaven/terminal-commander/v0.1.86/crates/probes/src/process.rs), [0.3.14 ProcessProbe](https://raw.githubusercontent.com/special-place-ai-heaven/terminal-commander/v0.3.14/crates/probes/src/process.rs).

## MCP health and observed fault

Live checks in this session:

- health: ok=true, version=0.3.14, uptime_secs=1025 at the diagnostic check.
- self_check: failures=0; spawn probe completed, terminal Exited, exit Some(0); persistent audit enabled.
- runtime_state(limit=0): command_jobs=0, pty_jobs=0, file_watches=0, bucket_count=0, active_rules_count=0; no truncation.
- Tests/builds ran through Terminal Commander and produced observed terminal receipts. The final verification batch, job_01a121fb313276e5a98f5765276b3ed3, completed exit 0.

These are live smoke checks and supervised-command evidence. They do not prove every MCP feature or crash-recovery path healthy.

Observed missing-receipt case:

```text
command_status(job_id="job_01a121f4f7a977eb9ca129d1d014a877")
Mcp error -32602: daemon ipc error [JobLost]: job job_01a121f4f7a977eb9ca129d1d014a877 started but never recorded a terminal transition ({"ipc_code":"JobLost"})
```

The job launched:
```text
wsl -d Ubuntu-24.04 -- python3 /mnt/c/Users/poslj/AppData/Local/Temp/aap-pr24-run.py hardening-final-agents-verified cargo test -p aap-agents --lib
```

Last progress was compiling aap-browser. No exit file or test result was recorded for that label. A subsequent process check found no remaining Cargo/rustc process for the worktree. The replacement hardening-final-agents-complete run passed all 1,788 agent library tests. The lost run is excluded from success counts.

JobLost is the documented fail-closed result for durable start with no live owner/terminal receipt. It does not establish successful execution, command failure, or the cause of receipt loss. See [tagged status handler](https://raw.githubusercontent.com/special-place-ai-heaven/terminal-commander/v0.3.14/crates/daemon/src/ipc/handlers/command.rs), especially lines 167-205 and tests at 275-304. Graceful shutdown attempts cancellation/abandonment receipts in [state.rs](https://raw.githubusercontent.com/special-place-ai-heaven/terminal-commander/v0.3.14/crates/daemon/src/state.rs), lines 163-240; abrupt termination or failed durable writes can leave uncertainty. Existing status_lost_detection tests cover start/no-receipt behavior. The observed interruption's exact cause remains unproven.

TC agent diagnostic request: correlate this exact job ID with daemon start/stop instance IDs, command-start audit metadata, terminal receipt persistence, shutdown errors, and relevant OS termination events. Report timestamps/typed statuses/paths; omit command stdout and credential/environment values. Determine whether the daemon restarted, a worker panicked, or receipt persistence failed. Do not infer the cause merely from current uptime.

Add deterministic tests for abrupt daemon loss while a child runs, failed terminal-receipt writes, cancelled/abandoned receipt persistence, reconnect/status reconstruction, and exactly one terminal transition. Explicitly surface unknown outcome and diagnostic correlation IDs. Never reconstruct success from a start record; AAP must not automatically rerun an unsafe command after JobLost.

## Embedded gaps that remain in the newer release

### TC-EMBED-01: output activity depends on newline/EOF

Severity: blocks reliable silence-based supervision.

Both inspected versions read a bounded 64 KiB line, then update bytes_total after a line or EOF. For an open stream that has already written bytes without LF, live metrics remain unchanged. Overflow beyond the cap is discarded until LF/EOF; frame truncated_bytes records overflow, but bytes_total reflects retained raw length rather than all observed pipe bytes. Source comments referring to force-splitting do not match that discard-until-boundary implementation. Tagged locations: process.rs read_stream/read_line_bounded, approximately lines 714-765 and 804-870.

AAP impact: crates/aap-terminal/src/probe_session.rs samples metrics every 50 ms and mirrors deltas into TerminalJobLedger. It cannot repair byte information the dependency does not publish. A legitimate newline-free job can appear silent. Counting sifter events is also insufficient: this adapter deliberately has empty sifters.

Deterministic upstream repro/test recipe, proposed rather than claimed executed:

1. Spawn a Python fixture through ProcessProbe that writes one byte to stdout, flushes, writes a ready marker to a temporary control file, and waits for a release file without closing stdout.
2. Await the ready marker with a bounded condition wait. Assert live raw bytes >=1 and last-output monotonic timestamp advanced before releasing the child.
3. Repeat with stderr only, >64 KiB without LF, mixed stdout/stderr, and invalid UTF-8.
4. Release the fixture; assert final exact raw byte count and bounded retained frame size/dropped-byte count.

Required contract: update raw byte count and activity timestamp on every successful pipe read, before decoding/framing/filtering/truncation. Count dropped bytes as observed bytes. Keep frame/event counts separate. Expose metrics independently of callbacks and sifter configuration. AAP needs content-free liveness telemetry, not unrestricted raw output retention. Test metric publication before terminal completion and late-frame suppression.

### TC-EMBED-02: pipe drain failures collapse into EOF

Both inspected versions treat read_line_bounded errors as stream end at the read_stream match, e.g. tagged lines 717-728. A child can exit successfully while drain errors are not represented in the execution result.

Required contract: preserve bounded, typed stdout/stderr read failure metadata alongside exit status. AAP should distinguish child exit from complete observation. Test a fault-injected reader error after partial bytes; retain observed byte count, explicit observation failure, and exactly one terminal receipt. Do not blindly retry a command because observation failed after a possible side effect.

### TC-EMBED-03: Unix process identity and cleanup ownership

Severity: blocks robust process-tree supervision under runtime loss.

Public child_pid() is Windows-only in 0.1.86 (process.rs:361-366) and tagged 0.3.14 (approximately 425-430). Unix leader PID/process group exists privately, and spawning uses process_group(0). Neither inspected release implements Drop for ProcessProbe. AAP's wrapper Drop sends a single cancellation notification; verified descendant cleanup relies on the Tokio lifecycle task remaining alive.

Runtime abort/shutdown before that task processes cancellation can leave a Unix process/group alive. This is an API/ownership limit identified from source; a runtime-abort live repro has not been executed here and must be added upstream. Windows JobObject handle cleanup has different semantics.

The unpublished e139722 checkout exposes unconditional child_pid() around process.rs:451. That fixes access on that checkout only; it is absent from official v0.3.14, does not itself guarantee teardown, and must not be treated as release parity.

Required contract: public portable identity (Unix PID/PGID; Windows PID/JobObject identity where meaningful), stable ownership, and explicit cancel-and-reap guarantees. Ensure group termination does not depend solely on a runtime task that can vanish. Return a typed unsupported/cleanup-uncertain result when guarantees cannot be met. Detect missing runtime before spawning. Unix cancellation currently uses external kill(1); expose failure/degradation instead of silently claiming tree teardown.

Tests: handshake-driven leader + grandchild; explicit cancel; repeated cancel; owner drop with runtime alive; owner/runtime abort; runtime shutdown; cancellation during spawn; natural signal exit; descendant keeping a pipe open; exactly-once receipt. Observe both leader and descendant gone, with bounded cleanup and zombie reaping. Avoid fixed sleeps as the synchronization mechanism.

### TC-EMBED-04: leader CPU does not prove group idleness

AAP's ChildLivenessSampler retains sysinfo counters and samples one PID. A shell leader can be idle while a descendant performs a silent build. Zero leader CPU is therefore insufficient evidence for killing the job. The missing Unix PID also prevents wiring even the current leader sampler into this adapter on Linux.

The v0.3.14 [governor](https://raw.githubusercontent.com/special-place-ai-heaven/terminal-commander/v0.3.14/crates/probes/src/governor.rs), approximately lines 60-116, exposes memory/priority controls, not process-group CPU usage. CPU priority is not a CPU utilization measurement.

Required contract: aggregate process group/JobObject CPU usage with retained interval, instance/start identity, explicit unavailable/unknown state, and documented normalization. First/too-early/invalid/PID-reused samples remain unknown. AAP must warn on unknown; only verified whole-job inactivity can support an idle-kill decision. Test idle shell + busy grandchild, descendant churn, process exit, reused identity, first sample, and invalid values.

### TC-EMBED-05: MCP capabilities lack an established embedded parity facade

The installed MCP exposes discovery, health/self-check, supervised commands and typed receipts, combed buckets/events/tails, PTY/session operations, file watches, resource limits, subscriptions, registry/recipe operations, policy and audit. AAP's new embedded adapter consumes only ProcessProbe + metrics, empty SifterRuntime, EventSink, and a local JobLedger. Existing TerminalSession is a separate portable-pty implementation; it is not proof of identical TC MCP behavior.

TC agent deliver a feature matrix and supported public embed facade: which MCP operations delegate to shared reusable library methods, which require an optional daemon/IPC host, and which are intentionally unavailable to a room. Include capability discovery, API/schema version, engine/build identity, lifetime/cleanup ownership, and bounded typed errors. Do not make AAP invoke MCP tool-name strings to recreate library capabilities. Do not expand room authority to host registries, credentials, or unrelated environments.

Minimum AAP parity slice: room discovery/health/build/instance identity, advertised beachhead execution, explicit cwd and cleared child environment, bounded command execution/cancellation/receipts, raw-byte liveness, whole-job identity/teardown, resource limits, combed output, and recovery without silently changing authority. Registry/recipes/subscriptions should have explicit supported/deferred decisions; a daemon version match alone does not establish feature parity.

## AAP integration work and existing failures

Relevant AAP files:

- Cargo.toml:40 and Cargo.lock:6657: aligned exact TC dependency versions.
- deny.toml:65-67: scoped PolyForm exceptions.
- crates/aap-terminal/src/probe_session.rs:52: ProcessProbe adapter and pre-mount TODO.
- crates/aap-terminal/src/job_ledger.rs:20: monotonic output age; separates byte/event counts; ignores late data after completion.
- crates/aap-mcp/src/liveness.rs:30: retained one-PID sampler.
- crates/aap-mcp/src/watchdog.rs:38: unknown/invalid CPU warns; currently unsuitable for group-idle decisions without correct whole-job evidence.
- crates/aap-terminal/src/room_runtime.rs:636: RoomTerminalCommanderPort contract.
- Same file:680/689: probe/recover_after_crash currently return NotImplemented("T097").
- crates/aap-terminal/tests/room_runtime.rs: existing RED acceptance tests.

AAP must implement T097 against an in-room TC runtime, with /workspace RepoOnly policy, explicit environment allow-list, separate cleared control/child environments, actual discovered version/build/instance identity, exact advertised argv/environment receipt validation, health consistency, cancellation/deadline checks at every stage, and replacement instance validation after crash. Default host ProcessProbe environment/cwd inheritance is not an approved room boundary. Do not attach the host MCP service as a fallback.

Final terminal suite: 20 passed / 18 failed, exit 101. All 18 are existing T097 failures reproduced on untouched main, not a new TC dependency regression. Exact failed tests:

```text
an_unavailable_room_port_is_returned_without_any_host_fallback_attempt
cancelled_operation_is_denied_before_touching_the_room_port
cancellation_after_launch_prevents_discovery
deadline_expiry_after_launch_prevents_discovery
disabled_component_is_denied_before_the_room_port_is_touched
crash_restarts_with_the_control_environment_then_rediscovers_and_reprobes
cleared_control_and_child_environments_remain_exact_distinct_and_value_redacted
discovery_rejects_any_environment_outside_the_repo_only_policy
execution_receipt_argv_mismatch_is_rejected
discovery_with_no_advertised_environment_is_rejected_before_execution
health_build_mismatch_is_rejected_independently
execution_receipt_environment_mismatch_is_rejected
health_instance_mismatch_is_rejected_independently
health_version_mismatch_is_rejected_independently
nonzero_beachhead_cannot_report_ready_and_debug_is_redacted
probe_filters_discovery_executes_the_advertised_beachhead_and_reports_actual_identity
recovery_rejects_a_restart_that_returns_the_previous_instance_id
unhealthy_health_status_is_rejected_independently
```

Ownership: these NotImplemented failures belong to AAP room integration. The TC upstream API deficiencies above belong to TC's reusable engine. Repairing either side alone will not make the other complete.

## Migration and release requirements

1. TC publish a coherent embedded release with the required contracts and regression tests; document which defects remain. Align core/probes/sifters versions. Supply immutable release identity and crate checksums, dependency/license changes, public examples, and a compile-tested embed contract.
2. AAP migrate ProcessProbeConfig/identity/metrics/errors to that release. Version 0.3.14 adds limits/governor fields; review complete config defaults rather than letting struct evolution silently widen authority.
3. TC's tagged workspace has Rust 1.92 floor; AAP declares Rust 1.93, so that declaration is not itself a floor conflict. Verify the actual WSL/CI compiler and transitive requirements for the chosen release. Do not infer a crates.io publication or compatibility solely from a GitHub tag.
4. Keep explicit environment clearing, cwd confinement, argument bounds, process-tree ownership, resource ceilings, output/secret redaction, and audit/receipt bounds in the supported embedded path. Test environment-value redaction without printing the values.
5. License remains PolyForm-Noncommercial-1.0.0. AAP's MIT manifest does not relicense TC or grant commercial use. Update scoped deny.toml entries only for the selected dependency graph and rerun the license check.
6. Complete AAP T097 and room-scoped recovery acceptance. Separately obtain the already-requested security review before backend invoke mounting or aap_supervised_shell dispatch.

## Exact verified commands and acceptance gates

Commands ran in WSL Ubuntu-24.04 with Linux target and empty wrapper. Direct reproducible form:

```text
wsl -d Ubuntu-24.04 -- bash -lc "cd /mnt/c/AI_STUFF/PROGRAMMING/aap-kernel-24 && CARGO_TARGET_DIR=~/aap-kernel-target RUSTC_WRAPPER= CARGO_BUILD_JOBS=2 cargo test -p aap-terminal"
```

Actual logged run used aap-pr24-run.py label hardening-green-terminal with Cargo argv cargo test -p aap-terminal: exit 101, 10 unit + 7 PTY + 3 room passed, 18 T097 room failures. Log: /home/robert/aap-kernel-logs/hardening-green-terminal.log; exit file has 101.

Ten repeats of the final unit executable
/home/robert/aap-kernel-target/debug/deps/aap_terminal-2aee1a265b6fd8ef --test-threads=16:
all exit 0, 10 passed per run. This verifies live ProcessProbe stdout, final bytes, cancellation, Drop/group cleanup while Tokio stays alive, spawn-without-runtime rejection, and ledger behavior; it does not verify runtime-abort cleanup or newer-release parity.

cargo clippy -p aap-agents -p aap-mcp -p aap-terminal -p aap-core --lib: exit 0, existing warnings.
Main report records the other affected suites and stress runs.

Future parity acceptance must additionally run:

- TC upstream deterministic tests for TC-EMBED-01 through 04 and JobLost durability/recovery.
- AAP terminal full suite with T097 now green, plus no host fallback/ambient env/path escape.
- AAP MCP fault suite, whole-job liveness/watchdog cases, and affected agent/core suites.
- Compile-tested chosen release config and embed examples; lockfile, license and MSRV checks.
- Live room command discovery -> execute -> combed output -> cancellation -> crash/replacement -> status reconciliation, with exact version/build/instance/capabilities recorded.
- Backend/supervised-shell E2E only after the separate security gate.

Definition of done: required embedded capabilities demonstrably match their MCP semantics, every unsupported feature is explicit, the identified AAP integration failures pass, crash/receipt uncertainty never becomes fabricated success or unsafe automatic replay, and neither process cleanup nor byte liveness relies on missing telemetry.
