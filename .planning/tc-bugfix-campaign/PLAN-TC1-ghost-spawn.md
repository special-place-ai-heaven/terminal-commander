# PLAN-TC1 -- Ghost spawn: stop blind-retrying mutating RPCs (Phase 1)

**Source:** TC trust-defects campaign (`plan-final.json` Phase 1 / fork F1=a) +
`review-verdict.json` required amendment #4 (Phase 1 is_idempotent) + adopted
optional improvements (split self-heal from re-send; operation-neutral mutating
remedy; correct the stale doc comment; lock in PTY-path coverage).
**Posture:** URGENT regression kill, PURE LOGIC, zero daemon state. The single
highest-harm item: a >5s client timeout makes `daemon_client.rs:191-198` blindly
re-send a cloned `CommandStartCombed` => DOUBLE SPAWN. Smallest correct change:
gate the retry on `request.is_idempotent()`. Stays inside the no-new-deps + the
crates/mcp/src spawn/fs/socket guards (the edit reads a bool and forwards an
error). All fixes touch cfg(unix)/cfg(windows) transport paths + tests, so BOTH
OS gates are mandatory.

Language: ASCII only.

---

## Summary table

| Symptom | Location (file:line) | Fix sketch | Effort | Test impact |
|---------|----------------------|------------|--------|-------------|
| Transport error on a MUTATING request triggers a blind re-send => double-spawn | `crates/mcp/src/daemon_client.rs:191-198` (`Err(e) if e.is_transport()` re-sends a cloned request) | Gate the re-send on `request.is_idempotent()`; on a mutating transport error return Err WITHOUT re-send | **S** | unit (mock inner.call invoked==1 for mutating, ==2 for read); live (one job, not two) |
| No idempotency classifier exists on IpcRequest | `crates/ipc/src/protocol.rs` (IpcRequest enum at :195) | Add `pub const fn is_idempotent(&self) -> bool`, EXHAUSTIVE match, governing RULE at top | **S** | unit (table-driven exhaustive: false for mutating, true for reads) |
| SubscriptionPull mis-classified as idempotent | `crates/daemon/src/subscriptions/pull.rs:541,544` (offset advance + commit server-side INSIDE the pull) | Classify SubscriptionPull = **false** (non-idempotent); add unit assertion citing the offset commit | **S** | unit (SubscriptionPull is_idempotent()==false) |
| Sub variants omitted from classification | `crates/ipc/src/protocol.rs:283-302` (SubscriptionOpen/Close, AuditSince, CommandOutputTail) | Open=false (mints sub_id+slot), Close=false (frees slot), AuditSince=true, CommandOutputTail=true | **S** | unit (incl. SubscriptionOpen-retry leaks a slot case) |
| transport_unavailable_error doc comment lies + remedy says "retry the tool" | `crates/mcp/src/tools.rs:1790-1796` (doc), :1804-1805 (remedy), :1808 (`McpError::internal_error`) | Split into mutating/non-mutating; correct the doc comment; operation-neutral mutating remedy | **S** | unit (mutating remedy has no "retry the tool"; says reconcile via command_status/runtime_state) |
| into_mcp_error has no IpcRequest in scope to know mutating-ness | `crates/mcp/src/tools.rs:1667` (into_mcp_error short-circuits transport BEFORE the code match) | Add sibling `into_mcp_error_mutating(&IpcError)` (or thread a bool) used by mutating tool arms | **S** | unit (mutating arms produce the mutating remedy) |
| try_self_heal bundled inside the gated retry branch | `crates/mcp/src/daemon_client.rs:196` | Move try_self_heal OUT of the gate: self-heal on transport error for BOTH; re-send ONLY if idempotent | **S** | unit (status self-heal still runs for a mutating RPC; no re-send) |

**Estimated files:** 3 (`crates/ipc/src/protocol.rs`, `crates/mcp/src/daemon_client.rs`,
`crates/mcp/src/tools.rs`).

---

## Per-item detail

### TC-1a -- blind retry of a mutating RPC double-spawns

**Symptom:** When the client request times out (>5s) on a `CommandStartCombed`,
the shared retry path treats the transport failure as recoverable and re-sends a
clone of the same mutating request. The first spawn may already be running on the
daemon; the re-send spawns a SECOND identical job. This is a regression at HEAD.

**Citations:**

```191:198:crates/mcp/src/daemon_client.rs
// (retry guard) Err(e) if e.is_transport() => { try_self_heal(...); re-send cloned request }
```

```195:195:crates/ipc/src/protocol.rs
// IpcRequest enum -- the variant set classified by is_idempotent()
```

**Fix:**

1. **Add `is_idempotent()` to IpcRequest** (`crates/ipc/src/protocol.rs`,
   additive `pub const fn`). State the governing RULE at the top of the helper:
   "return false for any RPC whose retry could create/duplicate a server-side
   resource or mint a fresh id; return true only for pure bounded reads and
   idempotent-effect repositioning." EXHAUSTIVE match so a future variant forces
   a deliberate classification.
   - **false (non-idempotent):** CommandStartCombed, CommandStop (Phase 6a),
     PtyCommandStart, PtyCommandWriteStdin, PtyCommandStop, RegistryUpsert,
     RegistryActivate, RegistryDeactivate, RegistryImportPack, FileWatchStart,
     FileWatchStop, Shutdown, **SubscriptionPull** (server-side offset commit --
     amendment #4), **SubscriptionOpen** (mints fresh sub_id + slot; a blind
     retry LEAKS a slot and can trip SubscriptionLimitExceeded -- amendment #4),
     **SubscriptionClose** (frees a slot; conservative non-retry -- amendment #4).
   - **true (idempotent):** Health, CommandStatus, BucketWait, RuntimeState,
     ProbeList, ProbeStatus, BucketEventsSince, BucketSummary, EventContext,
     RegistrySearch/Get/Test/ListActive, FileReadWindow/Search/WatchList,
     PtyCommandList, SubscriptionList/Seek, SystemDiscover, PolicyStatus,
     SelfCheck, **AuditSince** (pure read -- amendment #4), **CommandOutputTail**
     (pure read -- amendment #4).

2. **Gate the retry** (`crates/mcp/src/daemon_client.rs:191`): change the guard
   from `Err(e) if e.is_transport()` to
   `Err(e) if e.is_transport() && request.is_idempotent()`. A transport failure
   on a NON-idempotent request returns `Err(e)` immediately (no re-send). Update
   the doc comment (:155-186) to state mutating RPCs are NOT auto-retried.

3. **Split self-heal from re-send (adopted optional):** `try_self_heal()` (the
   Health re-probe / cached-status flip) is currently bundled inside the gated
   branch at `daemon_client.rs:196`. Move it OUT: call `try_self_heal()` on a
   transport error for BOTH idempotent and mutating RPCs, but only RE-SEND when
   `is_idempotent()`. This restores transparent reconnect-status recovery for a
   mutating call issued immediately after a legitimate daemon replace, without
   re-sending it.

**Effort:** S. **Test:**
- unit (crates/ipc, table-driven, exhaustive): `is_idempotent()` returns false
  for every mutating variant and true for every read variant; explicit case
  asserting `SubscriptionPull.is_idempotent()==false` with a comment citing the
  server-side offset commit (pull.rs:541,544); explicit case noting a
  SubscriptionOpen retry would leak a slot. source-status: test-only.
- unit (crates/mcp daemon_client, mock inner.call): a mutating request hitting a
  transport error is NOT retried (inner.call invoked == 1); a read request IS
  retried (== 2); a mutating request still runs try_self_heal once. source-status:
  test-only/mock.
- integration through daemon IPC (TESTING.md 3.4, own TEST socket + data dir):
  start a real >5s-spawning command via run_and_watch and assert exactly ONE job
  in runtime_state. source-status: live.
- integration (adopted optional, lock in PTY coverage): pty_command_start under an
  induced transport timeout produces exactly ONE PTY job. source-status: live.

---

### sub -- transport_unavailable_error doc comment lie + "retry the tool" remedy

**Symptom:** The envelope built for a mid-call transport failure routes through
`McpError::internal_error` (numeric -32603) while its doc comment claims it is
"never a raw `internal_error` (-32603)", and its remedy literally says "retry the
tool" -- fatal for a mutating op (it instructs the exact re-send that
double-spawns).

**Citations:**

```1790:1796:crates/mcp/src/tools.rs
/// ... a clean, recoverable signal, never a raw `internal_error` (-32603). ...
```

```1804:1808:crates/mcp/src/tools.rs
"remedy": "the daemon was unavailable; retry the tool -- the adapter \
           re-establishes the daemon on the next call",
// ...
McpError::internal_error("daemon_unavailable", Some(payload))
```

**Fix:**

1. **Split transport_unavailable_error** (`crates/mcp/src/tools.rs:1797`) into
   mutating/non-mutating variants (or add a bool param). The mutating message
   DROPS "retry the tool" and is **operation-neutral** (adopted optional --
   honest for start, stop, and shutdown alike): "this mutating operation may or
   may not have taken effect; call command_status/runtime_state to confirm the
   actual state before re-issuing." Both keep `code: "daemon_unavailable"`; the
   numeric -32603 is UNCHANGED this phase (the wire-code change is DEFERRED to
   RISK_REGISTER R-07).

2. **Correct the stale doc comment (adopted optional, scope-guardian):** rewrite
   tools.rs:1790-1796 to stop claiming "never a raw internal_error (-32603)" --
   that is the very falsehood the TC-1a sub-defect is about. Name the comment fix
   explicitly so a documented lie is not left in place. The corrected comment
   states the envelope DOES currently use the internal_error numeric code
   (-32603) by design, with `code: "daemon_unavailable"` as the stable
   discriminator, and that changing the numeric code is tracked separately
   (R-07).

3. **Thread the mutating flag from the call site**
   (`crates/mcp/src/tools.rs:1667`, into_mcp_error): into_mcp_error short-circuits
   transport BEFORE the code match and has NO IpcRequest in scope (VERIFIED). Add
   a sibling `into_mcp_error_mutating(&IpcError)` (or thread a bool) used by the
   mutating tool arms (run_and_watch start at tools.rs:643 and
   command_start_combed) so the mutating remedy is produced; read-only arms keep
   `into_mcp_error`.

**Effort:** S. **Test:**
- unit (crates/mcp): the mutating remedy text contains no "retry the tool" and
  instructs command_status/runtime_state reconciliation; the non-mutating remedy
  is unchanged. source-status: test-only.
- unit (crates/mcp): assert the corrected doc-comment intent via a behavioral
  test where possible (the envelope code field is "daemon_unavailable"); the
  numeric-code value is documented, not asserted to change. source-status:
  test-only.

---

## Invariants (Phase 1)

- crates/mcp/src stays free of Command::new/Command::spawn/TcpListener/UdpSocket
  (CI guard 1) and tokio::fs/std::fs/File::open/read_to_string/read_to_end (CI
  guard 2) -- the edit only reads a bool and forwards an error. CAUTION (adopted
  optional): any new comment/string literal added to crates/mcp/src must avoid
  the exact guard-2 literals (write "file system", not "std::fs").
- `is_idempotent()` is EXHAUSTIVE: a future variant must be classified
  deliberately (compile-forced).
- FileWatchStart/RegistryActivate are state-creating -> classified
  non-idempotent (conservative; worst case is a non-retry the agent can
  re-issue, never a silent double-effect).
- No fake success: the mutating-failure envelope returns an honest "reconcile"
  remedy, never a false "retry".

## Verification (Phase 1)

- `wsl bash scripts/linux-gate.sh` (fmt --check + clippy --workspace
  --all-targets -D warnings + nextest --workspace + TC47 load gate + MCP guards
  1 & 2 + rustc==1.95.0).
- `pwsh -File scripts/windows-gate.ps1` (windows_no_console_spawn +
  windows_spawn_site_coverage; the IPC transport path used by the cfg(windows)
  named-pipe client is touched).
- `cargo nextest run -p terminal-commander-ipc -p terminal-commander-mcp`.
- manual: drive a >5s-spawning command via run_and_watch on a TEST socket;
  confirm ONE job, not two; confirm the mutating-failure envelope no longer says
  "retry the tool".
