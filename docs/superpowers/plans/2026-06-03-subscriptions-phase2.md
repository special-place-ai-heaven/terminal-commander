# Subscriptions Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Coding agent: **rust-pro** (T1/T2), **documentation-writer** + a shell author (T3). Each task ends gated: code -> code-reviewer -> test-runner -> dual-OS gate (T3 is scripts/docs: shellcheck/lint + a manual recipe, no Rust gate).

**Goal:** Ship the Phase 2 ergonomic accelerators on top of the merged Phase 1 core: (1) a CLI `subscription-stream <sub_id>` bridge that turns a `subscription_pull` loop into one NDJSON object per matched event for a harness `Monitor`; (2) a guarded, best-effort MCP `notifications/message` nudge on non-empty pulls (default OFF, never load-bearing); (3) a default-OFF Stop-hook keep-alive reference script + the documented Real-Time-Active harness patterns. None of these is required for correctness; the pull-loop core behaves identically without them (spec §"Portability", §"Phasing" Phase 2).

**Architecture:**
- **Stream bridge = OPTION (a) reconnect-per-pull on BOTH OSes** (spec §5). A new `subscription-stream` clap subcommand drives a loop of one-shot `subscription_pull` calls, each a fresh connect through the existing `connect_or_unavailable` helper (`crates/cli/src/ipc.rs:108`), with a per-call `with_timeout(12s)` (`crates/ipc/src/client.rs:50`, `crates/ipc/src/pipe_client.rs:45`). The daemon holds offsets per `sub_id`, so reconnect-per-pull is lossless across reconnects with NO new `DaemonClient` API. Windows `ERROR_PIPE_BUSY` retry is already handled inside `pipe_client.rs` (`PIPE_BUSY_RETRIES`/`PIPE_BUSY_DELAY_MS`, `crates/ipc/src/pipe_client.rs:23,26`). The bridge emits one newline-delimited JSON object per matched event to stdout, flushes per event, exits 0 on close/`--max`, and exits NON-ZERO on `UnknownSubscription` or daemon shutdown so a `Monitor` terminates rather than silently idling.
- **MCP notification nudge** = advertise `enable_logging()` in `get_info` (`crates/mcp/src/tools.rs:1516`, currently only `enable_tools()`), and in the `subscription_pull` TOOL ONLY (`crates/mcp/src/tools.rs:1434`), when the pull returns a NON-EMPTY batch AND `TC_MCP_NOTIFY=1`, call `ctx.peer.notify_logging_message(...)` as a best-effort side-channel (errors ignored). The `#[tool]` fn gains a `RequestContext<RoleServer>` parameter (rmcp injects it; `RoleServer`/`RequestContext` are already imported and used by `initialize`, `crates/mcp/src/tools.rs:1529`). NEVER fired on the idle/empty path. Honest doc comment + a docs note: Claude Code DROPS notifications to idle sessions (#36665 "not planned", #61797), so delivery is always the pull.
- **Stop-hook keep-alive** = a `settings.json` `Stop` hook + reference scripts (`.sh` + `.ps1`) under `packages/terminal-commander/hooks/`. Default OFF (guarded by an env opt-in). Maintains its OWN per-session counter in a temp file keyed by `session_id` (because `stop_hook_active` is a BOOLEAN "already-continuing", not a count); at max N (default 3) it emits `{continue:false, stopReason}` + a loud message. No-ops in headless runs.

**Tech Stack:** Rust (tokio current-thread runtime for the CLI loop, mirroring `run_daemon_command` at `crates/cli/src/main.rs:188`; `serde_json` for NDJSON), rmcp MCP SDK (`Peer<RoleServer>::notify_logging_message`, `LoggingMessageNotificationParam`, `ServerCapabilities::builder().enable_logging()`), POSIX shell + PowerShell for the Stop hook, `cargo nextest`, dual gate (`scripts/linux-gate.sh` via WSL + `scripts/windows-gate.ps1`).

**Spec:** `docs/superpowers/specs/2026-06-02-subscriptions-design.md` — read §5 (stream bridge, option a chosen), §7 (notification nudge, best-effort, Peer prereq), §"Real-Time-Active" (Monitor / one-shot pull / `/loop` / Stop-hook), §"Phasing" (Phase 2). AC9 (stream NDJSON + non-zero on unknown) is the Phase 2 gate.

**Constraints (do not violate):**
- MCP crate (`crates/mcp/src`) forbids `Command::new`/`spawn`, `TcpListener`/`UdpSocket`, `std::fs`/`tokio::fs`/`File::open`/`read_to_string` (the linux gate greps for it). `notify_logging_message` is FACADE-LEGAL: it is an in-process send over the already-open stdio pipe (no spawn/fs/socket). The grep guards MUST still pass after T2.
- `IpcErrorCode` is a CLOSED set. Phase 2 adds NO new error codes; the `into_mcp_error` exhaustive match (`crates/mcp/src/tools.rs:1550`) is UNCHANGED. `UnknownSubscription` (already present) is the stream-loop's terminate signal.
- Every new Rust source file starts with the PolyForm SPDX header:
  ```
  // SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
  // Copyright 2026 The Terminal Commander Authors
  ```
- The notification is BEST-EFFORT only, NEVER load-bearing — delivery of events is ALWAYS the pull. The Stop hook is the only mechanism that can wedge a session; it is bounded HARD and default OFF.
- No push/merge without the operator's approval (commit to a review branch and stop).
- `env` is OVERLAY (untouched by this work).

---

## Task 0: Integration branch + baseline gate

**Files:** none (git only).

- [ ] **Step 1: Branch off the current `main` (Phase 1 merged)**

```bash
git switch main
git pull --ff-only
git switch -c feat/subscriptions-phase2
```

- [ ] **Step 2: Baseline gate (must be green BEFORE feature work)**

Run (WSL): `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && CARGO_TARGET_DIR=\$HOME/tc-linux-target bash scripts/linux-gate.sh"`
Expected: PASS (Phase 1 is merged + verified). If red, STOP — the base is broken, not your change.

---

## Task 1: CLI `subscription-stream <sub_id> [--max N]` bridge (AC9)

**Files:**
- Modify: `crates/cli/src/main.rs` — add `Command::SubscriptionStream { sub_id, max }` variant to `enum Command` (`crates/cli/src/main.rs:49`); add a `run()` dispatch arm (`crates/cli/src/main.rs:137`); add `fn run_subscription_stream(sub_id: &str, max: Option<usize>) -> std::process::ExitCode`.
- Reuse: `connect_or_unavailable` (`crates/cli/src/ipc.rs:108`), `CliIpcError` (`crates/cli/src/ipc.rs:50`), `IpcRequest::SubscriptionPull` + `SubscriptionPullParams` + `IpcResponse::SubscriptionPull` + `SubscriptionPullResponse`.
- Test: `crates/cli/tests/subscription_stream.rs` (new; mirror the live-daemon harness in `crates/cli/tests/read_subcommands.rs`).

> **Why a new fn and not `run_daemon_command`:** `run_daemon_command` (`crates/cli/src/main.rs:188`) is SINGLE-SHOT — it issues exactly one `connect_or_unavailable(1, req)` and renders once. The stream is a LOOP of pulls. We reuse the SAME building blocks (current-thread runtime, `connect_or_unavailable`) but own the loop + NDJSON emit + exit-code mapping.

- [ ] **Step 1: Write the failing integration test FIRST**

Mirror `crates/cli/tests/read_subcommands.rs`'s `LiveDaemon` harness (spawn a real `terminal-commanderd` in `ipc-server` mode under an isolated `TC_DATA`/`TC_SESSION`; seed via a direct `DaemonClient`). New file `crates/cli/tests/subscription_stream.rs`:

```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Live coverage for the Phase 2 `subscription-stream` CLI bridge (AC9):
//! NDJSON one-object-per-event to stdout, exit 0 on close/--max, non-zero on
//! an unknown sub_id. Spawns a real daemon; drives the CLI binary.

// ... reuse target_bin / tmp_data_dir / LiveDaemon from read_subcommands.rs ...

#[test]
fn stream_unknown_sub_id_exits_nonzero() {
    let daemon = LiveDaemon::spawn("stream-unknown");
    let out = std::process::Command::new(target_bin("terminal-commander"))
        .args(["subscription-stream", "00000000-0000-0000-0000-000000000000", "--max", "1"])
        .env("TC_SOCKET", &daemon.endpoint)
        // ... TC_DATA / TC_SESSION wiring as read_subcommands does ...
        .output()
        .expect("run cli");
    assert!(!out.status.success(), "unknown sub_id must exit non-zero");
    // stderr names the typed UnknownSubscription error, never a fabricated row.
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("subscription") || err.contains("unknown"), "got: {err}");
}

#[test]
fn stream_emits_one_ndjson_object_per_event_then_exits_on_max() {
    let daemon = LiveDaemon::spawn("stream-ndjson");
    // 1. Open a subscription via a DIRECT DaemonClient ({severity_min: high,
    //    sources: all}). 2. Start a noisy command that emits >=2 high-sev
    //    events. 3. Run `subscription-stream <sub_id> --max 2`. 4. Assert
    //    stdout is EXACTLY 2 newline-delimited JSON objects, each parses, each
    //    carries an event + its source origin; exit 0.
    // (full body drives the live daemon end to end)
}
```

- [ ] **Step 2: Run it — expect FAIL** (subcommand does not exist; binary rejects `subscription-stream`).

Run: `cargo test -p terminal-commander-cli --test subscription_stream`
Expected: compile or runtime FAIL.

- [ ] **Step 3: Add the clap variant**

In `enum Command` (`crates/cli/src/main.rs:49`), after `Probes`/before `Policy` (order is not load-bearing):

```rust
    /// Stream matched events for an open subscription as NDJSON (one JSON
    /// object per event) to stdout, for a harness `Monitor`. Loops
    /// `subscription_pull` (reconnect-per-pull); exits 0 on close or when
    /// `--max` events are emitted, NON-ZERO on an unknown sub_id or daemon
    /// shutdown so the `Monitor` terminates instead of silently idling.
    SubscriptionStream {
        /// Opaque sub_id from `subscription_open`.
        sub_id: String,
        /// Stop after emitting this many events (default: stream until close).
        #[arg(long)]
        max: Option<usize>,
    },
```

- [ ] **Step 4: Add the dispatch arm** in `fn run` (`crates/cli/src/main.rs:137`):

```rust
        Command::SubscriptionStream { sub_id, max } => run_subscription_stream(&sub_id, max),
```

- [ ] **Step 5: Implement `run_subscription_stream` (FULL code)**

Insert after `run_probes` (`crates/cli/src/main.rs:269`). Uses a current-thread runtime (mirrors `run_daemon_command`); a fresh `connect_or_unavailable` per pull (reconnect-per-pull, option a). The 12s per-pull timeout is enforced INSIDE `connect_or_unavailable`'s `DaemonClient` — but that helper builds a `DaemonClient::new(...)` with the 5s default. So this task ALSO threads an override: add a sibling `connect_or_unavailable_with_timeout(correlation_id, request, timeout)` in `crates/cli/src/ipc.rs` (a thin copy that calls `.with_timeout(timeout)` on the constructed client), and `run_subscription_stream` calls it with `Duration::from_secs(12)`.

```rust
/// `subscription-stream <sub_id> [--max N]` -> a reconnect-per-pull loop that
/// emits one NDJSON object per matched event to stdout (flushed per event).
///
/// Exit codes: 0 on `--max` reached or a clean close; non-zero on
/// `UnknownSubscription` (sub gone / daemon restarted -> the Monitor must
/// terminate) and on an unavailable daemon. The daemon holds this sub's
/// offsets, so reconnect-per-pull is lossless across reconnects.
fn run_subscription_stream(sub_id: &str, max: Option<usize>) -> std::process::ExitCode {
    use std::io::Write;
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let pull_timeout = std::time::Duration::from_secs(12);
    let mut emitted: usize = 0;
    let mut corr: u64 = 1;
    rt.block_on(async {
        let mut stdout = std::io::stdout();
        loop {
            let req = IpcRequest::SubscriptionPull(
                terminal_commander_ipc::SubscriptionPullParams {
                    sub_id: sub_id.to_owned(),
                    // Bound each pull to <= the server cap so the loop returns
                    // promptly; the server clamps to MAX_PULL_TIMEOUT_MS (8s).
                    max,
                    timeout_ms: None,
                },
            );
            let resp = ipc::connect_or_unavailable_with_timeout(corr, req, pull_timeout).await;
            corr = corr.wrapping_add(1);
            let pull = match resp {
                Ok(IpcResponse::SubscriptionPull(p)) => p,
                Ok(other) => {
                    unexpected_variant("subscription_pull").report("subscription-stream");
                    let _ = other;
                    return std::process::ExitCode::from(1);
                }
                // UnknownSubscription (sub gone / restart) and Unavailable both
                // terminate the stream non-zero so a Monitor stops.
                Err(err) => {
                    err.report("subscription-stream");
                    return std::process::ExitCode::from(err.exit_code());
                }
            };
            for ev in &pull.events {
                // One NDJSON object per matched event; flush per event so a
                // Monitor sees each line immediately.
                match serde_json::to_string(ev) {
                    Ok(line) => {
                        if writeln!(stdout, "{line}").is_err() || stdout.flush().is_err() {
                            return std::process::ExitCode::from(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("terminal-commander: subscription-stream: serialize failed: {e}");
                        return std::process::ExitCode::from(1);
                    }
                }
                emitted += 1;
                if let Some(limit) = max {
                    if emitted >= limit {
                        return std::process::ExitCode::SUCCESS;
                    }
                }
            }
            // Empty pull (idle/liveness) -> loop again; the next pull re-arms
            // the daemon's Notify. `lagged`/`truncated` are surfaced once
            // per pull on stderr so a Monitor's operator can see loss.
            if pull.lagged || pull.truncated {
                eprintln!(
                    "terminal-commander: subscription-stream: lagged={} truncated={}",
                    pull.lagged, pull.truncated
                );
            }
        }
    })
}
```

(`SubscriptionEvent`/`SubEvent` wire type already derives `Serialize` — it is returned over IPC today; confirm the exact wire struct name from `SubscriptionPullResponse.events` and serialize that element directly.)

- [ ] **Step 6: Add `connect_or_unavailable_with_timeout` in `crates/cli/src/ipc.rs`**

A thin sibling of `connect_or_unavailable` (`crates/cli/src/ipc.rs:108`) — identical probe-before-IPC, but the final `DaemonClient::new(...)` becomes `DaemonClient::new(...).with_timeout(timeout)`:

```rust
/// Like [`connect_or_unavailable`] but with a caller-chosen per-call timeout.
/// Used by `subscription-stream`, whose blocking pulls need > the 5 s default
/// so an idle ~8 s server pull returns SUCCESS, not a client timeout.
pub(crate) async fn connect_or_unavailable_with_timeout(
    correlation_id: u64,
    request: IpcRequest,
    timeout: std::time::Duration,
) -> Result<IpcResponse, CliIpcError> {
    // ... identical to connect_or_unavailable up to the AlreadyRunning/Started
    // arm, then:
    let client = DaemonClient::new(endpoint_string(&endpoint)).with_timeout(timeout);
    client.call(correlation_id, request).await.map_err(CliIpcError::Ipc)
}
```

(Refactor option for the reviewer: extract the shared probe body so `connect_or_unavailable` delegates with the 5s default — keeps one probe path. Do this only if it stays surgical.)

- [ ] **Step 7: Run the tests — PASS.** Then clippy:

Run: `cargo test -p terminal-commander-cli --test subscription_stream` -> PASS
Run: `cargo clippy -p terminal-commander-cli --all-targets -- -D warnings` -> clean

- [ ] **Step 8: GATE — code -> code-reviewer -> test-runner.** CODE-REVIEW FOCUS: confirm reconnect-per-pull is lossless (daemon holds offsets; no client cursor); confirm `UnknownSubscription` AND unavailable both map to NON-ZERO exit; confirm `--max` stops at exactly N; confirm per-event flush; confirm no panic on a torn connection (maps to exit 1). Then commit:

```bash
git add crates/cli/src/main.rs crates/cli/src/ipc.rs crates/cli/tests/subscription_stream.rs
git commit -m "feat(cli): subscription-stream NDJSON bridge (reconnect-per-pull, AC9)"
```

---

## Task 2: Guarded best-effort MCP notification nudge (spec §7)

**Files:**
- Modify: `crates/mcp/src/tools.rs` — `get_info` (`crates/mcp/src/tools.rs:1516`): add `.enable_logging()` to the `ServerCapabilities::builder()` chain. `subscription_pull` tool (`crates/mcp/src/tools.rs:1434`): add a `RequestContext<RoleServer>` param + the guarded `notify_logging_message` send on a non-empty batch when `TC_MCP_NOTIFY=1`.
- Modify: `crates/mcp/src/tools.rs` imports — add `rmcp::model::LoggingMessageNotificationParam` (and `LoggingLevel` if the param needs it); `RoleServer`/`RequestContext` are already imported (`crates/mcp/src/tools.rs:33-39`).
- Docs: `docs/integrations/claude-code.md` — a short "subscription notifications are best-effort" note citing #36665/#61797.
- Test: `crates/mcp/tests/mcp_subscription_notify.rs` (new) — flag OFF: no notification attempted; flag ON + non-empty pull: the peer send is attempted.

> **rmcp facts (locked, do not re-litigate):** `RequestContext<RoleServer>.peer: Peer<RoleServer>` (rmcp `service.rs:660`) exposes `peer.notify_logging_message(LoggingMessageNotificationParam)` (rmcp `service/server.rs:439`, wire `"notifications/message"`); the capability is advertised via `ServerCapabilities::builder().enable_logging()`. This is facade-legal (in-process send over the open stdio pipe; no spawn/fs/socket). Claude Code DROPS notifications to idle sessions (#36665 "not planned", #61797) — BEST-EFFORT only.

- [ ] **Step 1: Write the failing test FIRST**

`crates/mcp/tests/mcp_subscription_notify.rs` (mirror an existing `crates/mcp/tests/*` harness that pairs against a live daemon):

```rust
// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! Phase 2: the subscription_pull notification nudge is GUARDED (TC_MCP_NOTIFY)
//! and BEST-EFFORT. With the flag off, no notification is attempted on a
//! non-empty pull; with it on + a non-empty pull, a notifications/message send
//! is attempted (and a send error is ignored, never failing the tool result).

#[tokio::test]
async fn notify_off_by_default_no_message_on_nonempty_pull() {
    // TC_MCP_NOTIFY unset: open + start a noisy probe + pull (non-empty) ->
    // the CallToolResult is SUCCESS and the captured peer notifications are 0.
}

#[tokio::test]
async fn notify_on_attempts_message_on_nonempty_pull_only() {
    // TC_MCP_NOTIFY=1: a non-empty pull attempts exactly one notification; an
    // idle/empty pull attempts NONE; a send error does not fail the tool.
}
```

(If the rmcp test harness cannot capture peer notifications directly, assert the SUCCESS path is unchanged with the flag both off and on, and unit-test the small `should_notify(flag, batch_len) -> bool` gate extracted in Step 3. State which approach you used.)

- [ ] **Step 2: Run it — expect FAIL.** `cargo test -p terminal-commander-mcp --test mcp_subscription_notify`

- [ ] **Step 3: Advertise logging in `get_info`**

`replace_symbol_body` on `get_info` (`crates/mcp/src/tools.rs:1516`) — change ONLY the builder chain:

```rust
        ServerInfo::new(ServerCapabilities::builder().enable_tools().enable_logging().build())
```

(Keep server info, protocol version, and instructions identical.)

- [ ] **Step 4: Add the guarded send to the `subscription_pull` tool**

`replace_symbol_body` on `subscription_pull` (`crates/mcp/src/tools.rs:1434`). The signature gains `ctx: RequestContext<RoleServer>` (rmcp's `#[tool]` macro injects it — same type `initialize` already takes, `crates/mcp/src/tools.rs:1529`):

```rust
    async fn subscription_pull(
        &self,
        Parameters(params): Parameters<McpSubscriptionPullParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let ipc = SubscriptionPullParams {
            sub_id: params.sub_id,
            max: params.max,
            timeout_ms: params.timeout_ms,
        };
        match self.pull_daemon.call(IpcRequest::SubscriptionPull(ipc)).await {
            Ok(IpcResponse::SubscriptionPull(SubscriptionPullResponse {
                events,
                liveness,
                lagged,
                truncated,
            })) => {
                // BEST-EFFORT nudge: ONLY on a non-empty batch AND only when
                // explicitly opted in (TC_MCP_NOTIFY=1). NEVER on the idle
                // path. A send error is ignored -- delivery of events is
                // ALWAYS the pull, never this notification. Claude Code drops
                // notifications to idle sessions (#36665 "not planned",
                // #61797), so this is a hint for harnesses that surface
                // notifications and a no-op for those that do not.
                if !events.is_empty() && std::env::var("TC_MCP_NOTIFY").as_deref() == Ok("1") {
                    let max_sev = liveness // or derive from events' severities
                        .iter()
                        .map(|_l| ())
                        .count();
                    let _ = ctx
                        .peer
                        .notify_logging_message(rmcp::model::LoggingMessageNotificationParam {
                            level: rmcp::model::LoggingLevel::Info,
                            logger: Some("terminal-commander".to_owned()),
                            data: serde_json::json!({
                                "subscription": "new_events",
                                "count": events.len(),
                                "lagged": lagged,
                                "max_sev_buckets": max_sev,
                            }),
                        })
                        .await; // ignore send errors -- best-effort only
                }
                json_tool_result(&serde_json::json!({
                    "events": events,
                    "liveness": liveness,
                    "lagged": lagged,
                    "truncated": truncated,
                }))
            }
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }
```

(Verify the exact `LoggingMessageNotificationParam` field set against the pinned rmcp 1.7.0 in `Cargo.lock` before writing — `level`/`logger`/`data` are the documented fields; adjust if the struct differs. The "max severity X" summary in spec §7 can be computed from `events`' severities; the placeholder above is illustrative.)

- [ ] **Step 5: MCP facade guard MUST still pass**

The send is in-process over the open stdio pipe — no `Command::new`, no `fs`, no socket. Run the two grep guards exactly as `scripts/linux-gate.sh` runs them and confirm `crates/mcp/src` is still clean.

- [ ] **Step 6: Docs note**

In `docs/integrations/claude-code.md`, add a short subsection: subscription event notifications are advertised (`logging`) but BEST-EFFORT — opt in with `TC_MCP_NOTIFY=1`, and note Claude Code drops notifications to idle sessions (#36665 "not planned", #61797). The authoritative delivery is always `subscription_pull` (or `subscription-stream` under `Monitor`).

- [ ] **Step 7: Run tests + check + clippy**

Run: `cargo test -p terminal-commander-mcp --test mcp_subscription_notify` -> PASS
Run: `cargo check -p terminal-commander-mcp` -> PASS (the contract tests `catalogue_lists_thirty_six_live_tools` / `tool_router_exposes_all_live_tools` are UNCHANGED — no new tool, no count change)
Run: `cargo clippy -p terminal-commander-mcp --all-targets -- -D warnings` -> clean

- [ ] **Step 8: GATE — code -> code-reviewer -> test-runner.** CODE-REVIEW FOCUS: confirm NO send on the idle/empty path; confirm the flag default-OFF; confirm send errors are ignored (the `let _ =`); confirm the doc comment is honest about #36665/#61797; confirm facade grep guards pass; confirm `get_info` still advertises tools. Commit:

```bash
git add crates/mcp/src/tools.rs crates/mcp/tests/mcp_subscription_notify.rs docs/integrations/claude-code.md
git commit -m "feat(mcp): guarded best-effort subscription_pull notification nudge (TC_MCP_NOTIFY, off by default)"
```

---

## Task 3: Stop-hook keep-alive reference scripts + Real-Time-Active docs

**Files (scripts + docs, no Rust):**
- Create: `packages/terminal-commander/hooks/tc-subscription-keepalive.sh`
- Create: `packages/terminal-commander/hooks/tc-subscription-keepalive.ps1`
- Create: `packages/terminal-commander/hooks/settings.snippet.json` (the `Stop` hook wiring example)
- Create: `packages/terminal-commander/hooks/README.md` (install + manual verification recipe + the contract)
- Modify: `docs/integrations/claude-code.md` and/or `docs/integrations/README.md` — a "Real-Time-Active patterns" section (Monitor over `subscription-stream`; one-shot backgrounded `subscription_pull`; `/loop`/`ScheduleWakeup` cadence; the Stop-hook keep-alive, default OFF).

> **Stop-hook contract (locked):** exit 0 + JSON `{decision:"block", reason, hookSpecificOutput:{hookEventName:"Stop", additionalContext:"<events>"}}` blocks the stop + injects events; `{continue:false, stopReason}` force-stops. `stop_hook_active` is a BOOLEAN ("already continuing"), NOT a count — so the script maintains its OWN per-session counter in a temp file keyed by `session_id`. At max N (default 3) it emits `{continue:false, stopReason}` + a loud message. Default OFF (an env opt-in guard). No-ops headless (no interactive session -> no events / immediate allow-stop). ANTI-GOAL: high-rate streams (use Monitor over `subscription-stream` for those).

- [ ] **Step 1: Write `tc-subscription-keepalive.sh` (FULL reference)**

```bash
#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Terminal Commander subscription keep-alive Stop hook (REFERENCE, default OFF).
#
# Reads the Stop-hook JSON from stdin. If TC_KEEPALIVE_SUB is set to an open
# sub_id, it pulls pending events once; if any, it BLOCKS the stop and injects
# them so the model reacts in-session. Bounded HARD: at most TC_KEEPALIVE_MAX
# (default 3) CONSECUTIVE keep-alives per session_id (its OWN counter in a temp
# file -- stop_hook_active is a boolean, not a count). On budget exhaustion it
# force-allows stop with a loud message. No-ops when the guard env is unset.
set -euo pipefail

# Default OFF: do nothing unless explicitly enabled.
if [ "${TC_KEEPALIVE:-0}" != "1" ] || [ -z "${TC_KEEPALIVE_SUB:-}" ]; then
  exit 0
fi

input="$(cat)"
session_id="$(printf '%s' "$input" | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
session_id="${session_id:-unknown}"
max="${TC_KEEPALIVE_MAX:-3}"

counter_file="${TMPDIR:-/tmp}/tc-keepalive-${session_id}.count"
count="$(cat "$counter_file" 2>/dev/null || echo 0)"

# Pull pending events ONCE (one-shot; bounded by the daemon's pull cap).
events="$(terminal-commander subscription-stream "$TC_KEEPALIVE_SUB" --max 20 2>/dev/null || true)"

if [ -z "$events" ]; then
  # Nothing pending: reset the budget and allow the stop.
  rm -f "$counter_file"
  exit 0
fi

if [ "$count" -ge "$max" ]; then
  # Budget exhausted: force-allow stop with a loud message; reset the budget.
  rm -f "$counter_file"
  printf '%s' '{"continue":false,"stopReason":"terminal-commander keep-alive budget exhausted; events pending -- resume via subscription_pull"}'
  exit 0
fi

# Under budget: block the stop and inject the events.
echo $((count + 1)) > "$counter_file"
# Emit a single JSON object; embed events as additionalContext (JSON-escaped).
esc="$(printf '%s' "$events" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))' 2>/dev/null || printf '"%s"' "events pending")"
printf '{"decision":"block","reason":"terminal-commander: new subscription events","hookSpecificOutput":{"hookEventName":"Stop","additionalContext":%s}}' "$esc"
exit 0
```

- [ ] **Step 2: Write `tc-subscription-keepalive.ps1` (FULL reference, same contract)**

```powershell
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 The Terminal Commander Authors
#
# Terminal Commander subscription keep-alive Stop hook (REFERENCE, default OFF).
# Mirrors the .sh contract on Windows PowerShell.
$ErrorActionPreference = 'Stop'

if ($env:TC_KEEPALIVE -ne '1' -or [string]::IsNullOrEmpty($env:TC_KEEPALIVE_SUB)) { exit 0 }

$raw = [Console]::In.ReadToEnd()
$session = 'unknown'
try { $session = ($raw | ConvertFrom-Json).session_id } catch {}
if ([string]::IsNullOrEmpty($session)) { $session = 'unknown' }
$max = if ($env:TC_KEEPALIVE_MAX) { [int]$env:TC_KEEPALIVE_MAX } else { 3 }

$counterFile = Join-Path $env:TEMP "tc-keepalive-$session.count"
$count = 0
if (Test-Path $counterFile) { $count = [int](Get-Content $counterFile -Raw) }

$events = (& terminal-commander subscription-stream $env:TC_KEEPALIVE_SUB --max 20 2>$null) -join "`n"

if ([string]::IsNullOrWhiteSpace($events)) {
  Remove-Item $counterFile -ErrorAction SilentlyContinue
  exit 0
}

if ($count -ge $max) {
  Remove-Item $counterFile -ErrorAction SilentlyContinue
  @{ continue = $false; stopReason = 'terminal-commander keep-alive budget exhausted; events pending -- resume via subscription_pull' } | ConvertTo-Json -Compress
  exit 0
}

($count + 1) | Set-Content $counterFile
$obj = @{ decision = 'block'; reason = 'terminal-commander: new subscription events';
          hookSpecificOutput = @{ hookEventName = 'Stop'; additionalContext = $events } }
$obj | ConvertTo-Json -Compress -Depth 6
exit 0
```

- [ ] **Step 3: `settings.snippet.json` + `hooks/README.md`**

`settings.snippet.json` (a copy-paste example for `.claude/settings.json`):

```json
{
  "hooks": {
    "Stop": [
      {
        "matcher": "",
        "hooks": [
          { "type": "command", "command": "$HOME/.../hooks/tc-subscription-keepalive.sh" }
        ]
      }
    ]
  }
}
```

`hooks/README.md` documents: the default-OFF guard (`TC_KEEPALIVE=1` + `TC_KEEPALIVE_SUB=<sub_id>`), the budget (`TC_KEEPALIVE_MAX`, default 3), the temp-file-per-session counter rationale (`stop_hook_active` is a boolean), the headless no-op, the ANTI-GOAL (low-rate completion watches only; use Monitor for high-rate), and a MANUAL VERIFICATION RECIPE:
1. Open a sub (`subscription_open {sources:all, severity_min: high}`), note `sub_id`.
2. Export `TC_KEEPALIVE=1 TC_KEEPALIVE_SUB=<sub_id>`; wire the Stop hook.
3. Start a command that emits a high-sev event; ask the model to stop.
4. Observe the stop is BLOCKED and the event is injected, up to 3 times, then a loud force-stop.
5. With `TC_KEEPALIVE` unset, the model stops normally (no-op).

- [ ] **Step 4: Real-Time-Active docs section**

In `docs/integrations/claude-code.md` (and a brief cross-link from `docs/integrations/README.md`), add a "Real-Time-Active patterns" section per spec §"Real-Time-Active":
- **Primary — Monitor**: `Monitor("terminal-commander subscription-stream <sub_id>")` -> one model turn per matched event line. Persistent session-length watches.
- **One-shot — backgrounded pull**: a blocking `subscription_pull` that returns on the awaited event ("tell me when X completes/activates").
- **Cadence — `/loop` / `ScheduleWakeup` / `CronCreate`**: interval re-invocation.
- **Optional hack — Stop-hook keep-alive** (default OFF; the only mechanism that can wedge a session; bounded to N=3). Point to `packages/terminal-commander/hooks/`.
- **Cross-harness**: Codex/Cursor use their own background loop over `subscription_pull` (see `docs/integrations/codex-cli.md`, `cursor.md`).

- [ ] **Step 5: Lint + manual verification (no Rust gate)**

Run `shellcheck packages/terminal-commander/hooks/tc-subscription-keepalive.sh` (note in the PR if shellcheck is unavailable; the script targets bash). Validate the `.ps1` parses (`pwsh -NoProfile -Command "Get-Command -Syntax ..."` or a parse check). Validate `settings.snippet.json` with `validate_file_syntax`. Run the manual recipe from `hooks/README.md` once and record the observed block/inject/force-stop behavior in the PR (verify-as-user, since hooks only fire in a live interactive session).

- [ ] **Step 6: GATE — code-reviewer (scripts) -> manual verify.** REVIEW FOCUS: default-OFF guard present in both scripts; per-session counter file keyed by `session_id`; budget exhaustion emits `continue:false` + loud message; headless no-op; the `block` JSON shape matches the contract exactly; no secrets/paths leaked. Commit:

```bash
git add packages/terminal-commander/hooks/ docs/integrations/claude-code.md docs/integrations/README.md
git commit -m "docs+hooks: Real-Time-Active patterns + default-OFF Stop-hook keep-alive reference (.sh/.ps1)"
```

---

## Task 4: Dual-OS gate + AC9 sweep

**Files:** none (verification).

- [ ] **Step 1: Linux gate (WSL)**

Run: `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && CARGO_TARGET_DIR=\$HOME/tc-linux-target bash scripts/linux-gate.sh"`
Expected: fmt clean, clippy -D warnings clean, `cargo nextest run --workspace` green (incl. the new `subscription_stream` + `mcp_subscription_notify` tests), MCP grep guards PASS (notification did not add fs/spawn/socket).

- [ ] **Step 2: Windows gate**

Run (pwsh): `pwsh -File scripts/windows-gate.ps1`
Expected: windows_no_console + windows_spawn_site_coverage green; the `subscription-stream` CLI test compiles+runs on the named-pipe path (reconnect-per-pull exercises `pipe_client` `ERROR_PIPE_BUSY` retry).

- [ ] **Step 3: AC9 + Phase 2 checklist** — verify against a real run:
  - AC9: `subscription-stream` emits NDJSON, one line per event, flushed per event; exits non-zero on `UnknownSubscription`; a `Monitor` over it wakes one turn per line (drive it live and record the evidence).
  - Notification: flag OFF -> no send on non-empty pull; flag ON -> attempted on non-empty only, never idle; tool result unchanged either way; facade clean.
  - Stop-hook: manual recipe block/inject/force-stop observed; default-OFF no-op confirmed.

- [ ] **Step 4: Commit** any gate-driven fixups. Final: `chore(subscriptions): Phase 2 dual-OS green (stream + notify + keep-alive)`. STOP for operator approval before push/merge (commit to the review branch).

---

## Self-Review (run before execution)

- **Spec coverage:** §5 stream bridge, option (a) reconnect-per-pull on both OSes (Task 1, AC9); §7 notification nudge, best-effort, `enable_logging` + `notify_logging_message`, `Peer<RoleServer>` prereq satisfied via the `RequestContext` tool param (Task 2); §"Real-Time-Active" Monitor/one-shot/cadence/Stop-hook + §"Phasing" Phase 2 (Task 3). All three Phase 2 deliverables mapped.
- **Out of scope (Phase 1 done; Phase 3 NOT here):** the pull engine, registry, predicate, liveness, boot_id, the 4 tools, the 2 IpcErrorCode variants (all Phase 1, merged). Tags, `subscription_seek`, proportional fairness are Phase 3 (separate plan).
- **Type consistency:** the stream serializes the SAME wire event element returned in `SubscriptionPullResponse.events` (no new wire type). The notification uses `LoggingMessageNotificationParam` (rmcp 1.7.0; verify field set against `Cargo.lock`). No `IpcErrorCode` added — `UnknownSubscription` (Phase 1) is the stream terminate signal; `into_mcp_error` is UNCHANGED, so the exhaustive match still compiles. The two MCP contract tests (`catalogue_lists_thirty_six_live_tools`, `tool_router_exposes_all_live_tools`) are UNCHANGED — Phase 2 adds NO MCP tool.
- **Facade discipline:** the notification send is in-process over the open stdio pipe — no spawn/fs/socket; the linux-gate grep guards must still pass (Task 2 Step 5 + Task 4 Step 1).
- **Gate discipline:** Task 1/2 end code -> code-reviewer -> test-runner; dual-OS at Task 4. Task 3 is scripts/docs: shellcheck/parse-check/`validate_file_syntax` + a live manual recipe (hooks only fire in a real interactive session, so verify-as-user is mandatory).
- **Honesty:** the notification is BEST-EFFORT, never load-bearing (delivery is always the pull). The Stop-hook is the only wedging mechanism — default OFF, bounded N=3, headless no-op.

---

## Canonical checklist (Phase 2 surfaces)

**CLI subcommand (Task 1):**
1. `crates/cli/src/main.rs`: `Command::SubscriptionStream` variant; `run` dispatch arm; `run_subscription_stream` loop.
2. `crates/cli/src/ipc.rs`: `connect_or_unavailable_with_timeout` (12s) sibling.
3. Test: `crates/cli/tests/subscription_stream.rs` live-daemon NDJSON + non-zero-on-unknown (mirror `read_subcommands.rs`).
4. Exit codes: 0 on close/--max; non-zero on `UnknownSubscription`/unavailable/torn connection.

**MCP notification (Task 2):**
1. `crates/mcp/src/tools.rs`: `get_info` += `.enable_logging()`; `subscription_pull` += `RequestContext<RoleServer>` param + guarded `notify_logging_message`.
2. Imports: `LoggingMessageNotificationParam` (+`LoggingLevel`); `RoleServer`/`RequestContext` already present.
3. Guards: NON-EMPTY batch AND `TC_MCP_NOTIFY=1`; never idle; ignore send errors; facade grep clean.
4. NO new tool, NO new `IpcErrorCode`, NO contract-test change.

**Stop-hook + docs (Task 3):**
1. `packages/terminal-commander/hooks/`: `.sh` + `.ps1` + `settings.snippet.json` + `README.md`.
2. Default OFF; per-session temp-file counter; block+inject; `continue:false` at budget; headless no-op.
3. `docs/integrations/`: Real-Time-Active patterns section.
4. Verify: shellcheck/parse-check/`validate_file_syntax` + live manual recipe.

**Verify (Task 4):** `scripts/linux-gate.sh` (WSL) + `scripts/windows-gate.ps1`; AC9 live; notification on/off; Stop-hook manual.
