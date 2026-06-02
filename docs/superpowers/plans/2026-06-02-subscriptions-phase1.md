# Subscriptions Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Coding agent: **rust-pro**. Each task ends gated: code -> code-reviewer -> test-runner -> dual-OS gate.

**Goal:** Ship predicate-routed Subscriptions Phase 1 for Terminal Commander — one multiplexed, lossless, bounded consumer over many buckets (`subscription_open/pull/list/close`), built on the round-2 fix base, dual-OS verified.

**Architecture:** New in-memory `SubscriptionRegistry` in the daemon keyed by an OPAQUE per-open `sub_id` (independent offsets per open — no cross-consumer sharing). A multiplexed `pull` arms each in-scope bucket's `Notify` via `Notified::enable()` BEFORE rechecking offsets (lossless against the permit-less `notify_waiters()`), drains fair (capped round-robin), and returns bounded events + per-source liveness. Routing is a per-pull lazy rebuild over a new bucket->source side-table (no event bus). Thin MCP facade forwards 1:1 to daemon IPC.

**Tech Stack:** Rust (tokio 1.52 `sync::Notify`/`Notified::enable`), rmcp MCP SDK, SQLite store (unchanged), `cargo nextest`, dual gate (`scripts/linux-gate.sh` via WSL + `scripts/windows-gate.ps1`).

**Spec:** `docs/superpowers/specs/2026-06-02-subscriptions-design.md` @ `870350e` (signed off). Read it before each task. The end-to-end "add an IPC-method-backed MCP tool" anatomy (file:line for every layer) is in this session's grounding; the canonical checklist is reproduced at the end of this plan.

**Constraints (do not violate):**
- MCP crate (`crates/mcp/src`) forbids `Command::new`/`spawn`, `TcpListener`/`UdpSocket`, `std::fs`/`tokio::fs`/`File::open`/`read_to_string` (CI greps for it). Tools are pure IPC forwards.
- `IpcErrorCode` is a closed set; the 2 new variants are APPROVED (this plan), and the `into_mcp_error` match in `crates/mcp/src/tools.rs` is exhaustive (compiler forces classification).
- `env` is OVERLAY (untouched by this work).
- No push/merge without the operator's approval (the operator has granted it for this campaign).
- Every response bounded; no fake success; lossless = cursor/seq is truth, `Notify` is a latency hint.

---

## Task 0: Integration branch + bring spec/plan

**Files:** none (git only).

- [ ] **Step 1: Create the integration branch off the round-2 base**

```bash
git switch fix/cursor-review-round2
git switch -c feat/subscriptions
# bring the signed-off spec + this plan from feature/subscriptions
git checkout feature/subscriptions -- docs/superpowers/specs/2026-06-02-subscriptions-design.md docs/superpowers/plans/2026-06-02-subscriptions-phase1.md
git add docs/superpowers
git commit -m "docs: bring subscriptions spec + Phase 1 plan onto the round-2 base"
```

- [ ] **Step 2: Baseline gate (must be green BEFORE feature work)**

Run (WSL): `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && CARGO_TARGET_DIR=\$HOME/tc-linux-target bash scripts/linux-gate.sh"`
Expected: PASS (the 8 round-2 fixes already verified). If red, STOP — the base is broken, not your change.

---

## Task 1: IpcErrorCode variants (`UnknownSubscription`, `SubscriptionLimitExceeded`)

**Files:**
- Modify: `crates/ipc/src/protocol.rs` (enum `IpcErrorCode` ~L363-445)
- Modify: `crates/mcp/src/tools.rs` (exhaustive `into_mcp_error` ~L1372-1417)
- Test: `crates/ipc/src/protocol.rs` (tests mod) + compile of mcp

- [ ] **Step 1: Write the failing serde round-trip test**

In the protocol tests module:

```rust
#[test]
fn subscription_error_codes_roundtrip_snake_case() {
    for (code, wire) in [
        (IpcErrorCode::UnknownSubscription, "\"unknown_subscription\""),
        (IpcErrorCode::SubscriptionLimitExceeded, "\"subscription_limit_exceeded\""),
    ] {
        let s = serde_json::to_string(&code).unwrap();
        assert_eq!(s, wire);
        let back: IpcErrorCode = serde_json::from_str(&s).unwrap();
        assert_eq!(back, code);
    }
}
```

- [ ] **Step 2: Run it — expect FAIL** (`UnknownSubscription` not a variant).

Run: `cargo test -p terminal-commander-ipc subscription_error_codes_roundtrip_snake_case`
Expected: compile error / FAIL.

- [ ] **Step 3: Add the variants with doc comments**

In `enum IpcErrorCode`, after `RuleNotActive` (keep the closed-set doc above the enum):

```rust
    /// `subscription_pull`/`subscription_close` referenced a `sub_id` the
    /// daemon does not know (unknown or reset by a daemon restart). Caller
    /// re-opens. Approved goal-file amendment 2026-06-02.
    UnknownSubscription,
    /// `subscription_open` exceeded the max-subscriptions cap. Caller frees
    /// a slot (subscription_close) and retries. Approved 2026-06-02.
    SubscriptionLimitExceeded,
```

- [ ] **Step 4: Classify both in the exhaustive `into_mcp_error` match (mcp/tools.rs)**

Add both to the `invalid_params` arm (caller-fixable):

```rust
        | IpcErrorCode::RuleNotActive
        | IpcErrorCode::UnknownSubscription
        | IpcErrorCode::SubscriptionLimitExceeded => McpError::invalid_params(message, Some(data)),
```

- [ ] **Step 5: Run test + workspace check**

Run: `cargo test -p terminal-commander-ipc subscription_error_codes_roundtrip_snake_case` -> PASS
Run: `cargo check --workspace` -> PASS (exhaustive match satisfied).

- [ ] **Step 6: Commit**

```bash
git add crates/ipc/src/protocol.rs crates/mcp/src/tools.rs
git commit -m "feat(ipc): add UnknownSubscription + SubscriptionLimitExceeded error codes (approved amendment)"
```

---

## Task 2: `BucketManager::bucket_notify` accessor

**Files:**
- Modify: `crates/core/src/bucket.rs` (impl `BucketManager`, near `bucket_wait` ~L494)
- Test: `crates/core/src/bucket.rs` (tests mod)

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn bucket_notify_returns_handle_and_wakes() {
    let mgr = BucketManager::new();
    let id = BucketId::new();
    mgr.create_bucket(id, BucketConfig::default()).unwrap();
    let notify = mgr.bucket_notify(id).expect("handle");
    // unknown bucket -> NotFound
    assert!(matches!(mgr.bucket_notify(BucketId::new()), Err(BucketError::NotFound(_))));
    // an enrolled waiter wakes on append
    let mut fut = Box::pin(notify.notified());
    futures::future::poll_fn(|cx| { let _ = fut.as_mut().poll(cx); std::task::Poll::Ready(()) }).await; // enroll
    mgr.append(id, sample_draft()).unwrap();
    tokio::time::timeout(std::time::Duration::from_millis(200), fut).await.expect("woken");
}
```

(Reuse the file's existing `sample_draft()`/draft helper; if none, build a minimal `EventDraft` as other tests in the file do.)

- [ ] **Step 2: Run — expect FAIL** (`bucket_notify` undefined).

Run: `cargo test -p terminal-commander-core bucket_notify_returns_handle_and_wakes`

- [ ] **Step 3: Implement (mirror the private `bucket()` lock pattern)**

```rust
    /// Clone a bucket's wakeup [`Notify`] so a multiplexed consumer can arm it
    /// (see `subscription_pull`). Short outer read-lock to clone the cell Arc,
    /// then a short inner read-lock to clone the Notify — no lock held across await.
    pub fn bucket_notify(&self, bucket_id: BucketId) -> Result<Arc<Notify>, BucketError> {
        let cell = self.bucket(bucket_id)?;
        let inner = cell.read();
        Ok(Arc::clone(&inner.notify))
    }
```

- [ ] **Step 4: Run — PASS.** `cargo test -p terminal-commander-core bucket_notify_returns_handle_and_wakes`

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/bucket.rs
git commit -m "feat(core): BucketManager::bucket_notify accessor for multiplexed wait"
```

---

## Task 3: `boot_id` on `DaemonState`

**Files:**
- Modify: `crates/daemon/src/state.rs` (struct `DaemonState` ~L54, `bootstrap` ~L141, struct literal ~L241)
- Modify: `crates/daemon/Cargo.toml` (ensure `uuid` dep with `v4`) — check workspace deps first
- Test: `crates/daemon/tests/*` (assert boot_id stable within a session, changes across bootstrap)

- [ ] **Step 1: Ensure `uuid` is available**

Check: `grep -n "uuid" Cargo.toml crates/*/Cargo.toml`. If absent in workspace, add to `[workspace.dependencies]`: `uuid = { version = "1", features = ["v4"] }`, then `uuid = { workspace = true }` in `crates/daemon/Cargo.toml`. (Prefer an existing id type if the repo already has one — search `search_symbols uuid`.)

- [ ] **Step 2: Write the failing test**

```rust
#[test]
fn boot_id_is_stable_within_session() {
    let tmp = tempdir().unwrap();
    let s = DaemonState::bootstrap(DaemonConfig::defaults_in(tmp.path())).unwrap();
    let a = s.boot_id; let b = s.boot_id;
    assert_eq!(a, b);
    assert!(!a.is_nil());
}
```

- [ ] **Step 3: Add the field + mint at bootstrap**

Struct: `pub boot_id: uuid::Uuid,`. In `bootstrap`, before the struct literal: `let boot_id = uuid::Uuid::new_v4();`. Add `boot_id,` to the literal.

- [ ] **Step 4: Run — PASS.** **Step 5: Commit** `feat(daemon): mint a per-boot boot_id on DaemonState`.

---

## Task 4: Bucket source side-table (`BucketSource` + map + 3 writes + accessor)

**Files:**
- Create: `crates/daemon/src/subscriptions/source.rs` (new `subscriptions` module dir) — `BucketSource`, `BucketSourceTable`
- Modify: `crates/daemon/src/lib.rs` or `mod.rs` to declare `mod subscriptions;`
- Modify: `crates/daemon/src/state.rs` (add `pub sources: Arc<BucketSourceTable>` field + construct + a `dirty: AtomicU64` epoch)
- Modify: `crates/daemon/src/command.rs:444`, `file_watch.rs:274`, `pty_command.rs:280` (record source at bucket_create)
- Test: `crates/daemon/tests/subscription_source_table.rs`

- [ ] **Step 1: Define the types (failing test first)**

`BucketSource { kind: ProbeKind, job_id: Option<JobId>, probe_id: Option<ProbeId>, path: Option<PathBuf> }`. `BucketSourceTable { map: RwLock<HashMap<BucketId, BucketSource>>, dirty: AtomicU64 }` with `record(id, BucketSource)` (bumps `dirty`), `get(id) -> Option<BucketSource>`, `remove`? (NOT needed — buckets immortal), `snapshot() -> Vec<(BucketId, BucketSource)>`, `dirty_epoch() -> u64`.

Test asserts: record then get; dirty epoch increments on record.

- [ ] **Step 2: Wire into the 3 bucket_create sites** — immediately after each `self.router.bucket_create(bucket_id, cfg)?;`, call `state.sources.record(bucket_id, BucketSource{..})` with the in-scope identity (command: kind=Command, job_id, probe_id; file_watch: kind=FileWatch, watch_id as job_id, probe_id, path; pty: kind=Pty, job_id, probe_id). NOTE: these runtimes hold the table via the same `Arc::clone` threading as `activation` — add `sources: Arc<BucketSourceTable>` to `CommandRuntime::new`/`WatchRuntime::new`/`PtyRuntime::new` (mirror the `activation` Arc).

- [ ] **Step 3: Test + commit** `feat(daemon): bucket source side-table recorded at probe start`. dual-OS note: file_watch/pty cfg differs — ensure the pty write is under `#[cfg(unix)]` only.

---

## Task 5: `ProbeListEntry` liveness + per-kind derivation

**Files:**
- Modify: `crates/ipc/src/protocol.rs` (`ProbeListEntry` ~L1238 — add `#[serde(default)] pub liveness: Liveness`; define `enum Liveness`)
- Modify: `crates/daemon/src/ipc/handlers/runtime.rs` (`collect_probes` — set liveness in all 3 push sites)
- Test: `crates/daemon/tests/*` (exited command reports `exited{code}`; cancelled reports `cancelled`)

- [ ] **Step 1: Define `Liveness` (single authoritative union)**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Liveness {
    Starting,
    Running,
    Exited { code: i32 },
    Failed { code: Option<i32>, signal: Option<String> },
    Cancelled,
    Stopped,
    Dropped { count: u64 },
}
```

- [ ] **Step 2: Failing test** — start a command, kill it, assert `collect_probes` (or `probe_list`) reports `Cancelled`, NOT `Failed`; start+let-exit-0 reports `Exited{code:0}`.

- [ ] **Step 3: Derive per kind in `collect_probes`** — command: map `state.command.status(job).state` (`JobState`) -> Liveness per the mapping table (Starting->Starting, Running->Running, Exited->Exited{code}, Failed->Failed{code,signal}, Cancelled->Cancelled), folding `dropped_count>0` into a `Dropped` only when reporting bucket-level lag (keep probe liveness = process state; surface dropped separately on the bucket). file_watch: present -> Running. pty: present -> Running (+ exit if job-ledger wired, else Running). Set the field in ALL three push sites.

- [ ] **Step 4: Test + commit** `feat: per-source liveness on ProbeListEntry derived from JobState (not live-map presence)`.

---

## Task 6: Subscription + predicate types

**Files:**
- Create: `crates/daemon/src/subscriptions/model.rs` — `Predicate`, `SourceSel`, `Subscription`
- Test: `crates/daemon/src/subscriptions/model.rs` (predicate_hash stable + normalized; matches())

- [ ] **Step 1: Define (failing test first)**

```rust
pub enum SourceSel { All, Jobs(Vec<JobId>), Buckets(Vec<BucketId>), Probes(Vec<ProbeId>) }
pub struct Predicate {
    pub severity_min: Option<Severity>,   // per-EVENT (SignalEvent)
    pub kind: Option<Vec<String>>,        // per-EVENT
    pub sources: SourceSel,               // per-BUCKET (side-table)
}
pub struct Subscription {
    pub sub_id: uuid::Uuid,               // OPAQUE per-open
    pub predicate: Predicate,
    pub predicate_hash: u64,              // routing-EVALUATION sharing only
    pub offsets: HashMap<BucketId, u64>,
    pub created_at: std::time::Instant,
    pub last_pull_at: Option<std::time::Instant>,
    pub rr_start: usize,                  // round-robin rotation cursor
}
```

`Predicate::normalized_hash(&self) -> u64` (sort vecs, stable hash). `Predicate::bucket_in_scope(&self, id, &BucketSource) -> bool` (per-bucket). Per-event filtering reuses `BucketReadRequest{severity_min, kind_filter}` against `events_since` (do NOT re-filter events by hand).

Test: same predicate (fields reordered in vecs) -> same hash; different -> different; bucket_in_scope for All/Buckets/Jobs/Probes.

- [ ] **Step 2: Test + commit** `feat(subscriptions): predicate + subscription model with opaque sub_id`.

---

## Task 7: `SubscriptionRegistry` (in-memory, bounded, opaque, dirty-aware)

**Files:**
- Create: `crates/daemon/src/subscriptions/registry.rs` — `SubscriptionRegistry`
- Modify: `crates/daemon/src/state.rs` (add `pub subscriptions: Arc<SubscriptionRegistry>` + construct)
- Modify: `crates/ipc/src/protocol.rs` (cap const `MAX_SUBSCRIPTIONS = 64`, `MAX_BUCKETS_PER_SUBSCRIPTION = 200`)
- Test: `crates/daemon/src/subscriptions/registry.rs`

- [ ] **Step 1: Define + failing test**

`SubscriptionRegistry { subs: RwLock<HashMap<Uuid, Subscription>> }`. Methods: `open(predicate) -> Result<Uuid, IpcError>` (mint fresh uuid; enforce `MAX_SUBSCRIPTIONS` else `SubscriptionLimitExceeded`; init offsets for already-in-scope buckets to current tail — late-open from-now); `get_mut`-style `with_sub(id, f)` (returns `UnknownSubscription` on miss); `list() -> Vec<SubscriptionSummary>` (bounded); `close(id) -> bool`.

Test: open twice with identical predicate -> DISTINCT uuids + independent offsets (C1 isolation); cap enforced -> `SubscriptionLimitExceeded`; close removes; get-unknown -> `UnknownSubscription`.

- [ ] **Step 2: Test + commit** `feat(subscriptions): bounded in-memory registry, opaque sub_id, consumer isolation`.

---

## Task 8: Multiplexed pull engine (LOSSLESS — the load-bearing task)

**Files:**
- Create: `crates/daemon/src/subscriptions/pull.rs` — `pull(state, sub_id, max, timeout) -> Result<PullOutcome, IpcError>`
- Test: `crates/daemon/tests/subscription_pull_lossless.rs` (the AC3/AC4/AC12 adversarial tests)

- [ ] **Step 1: Write the adversarial failing tests FIRST**

```rust
// AC3: append in the gap between enroll and await is delivered on the SAME pull (no lost wakeup).
#[tokio::test(flavor = "multi_thread")] async fn pull_no_lost_wakeup_enroll_before_recheck() { /* drive the exact ordering */ }
// AC4: flooding bucket capped to ceil(max/N); quiet bucket's event appears same pull; N>max returns <=max; N==0 no panic.
#[tokio::test(flavor = "multi_thread")] async fn pull_fairness_capped_and_no_starvation() { /* */ }
// AC12: after eviction, clamp to head_seq-1, deliver survivor once, dropped delta = head_seq-1 - offset.
#[tokio::test(flavor = "multi_thread")] async fn pull_eviction_clamp_off_by_one() { /* */ }
// AC7: unknown sub_id -> UnknownSubscription (never empty).
#[tokio::test(flavor = "multi_thread")] async fn pull_unknown_sub_is_typed_error() { /* */ }
```

- [ ] **Step 2: Implement the enroll-before-recheck loop (FULL code — the correctness core)**

```rust
pub async fn pull(state: &Arc<DaemonState>, sub_id: Uuid, max: usize, timeout: Duration)
    -> Result<PullOutcome, IpcError>
{
    let cap = max.clamp(1, MAX_PULL_EVENTS);
    // ceil(DRAIN-safety) enforced by caller: timeout already capped < DRAIN_CEILING.
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        // (1) resolve + snapshot scope (rebuild via side-table; UnknownSubscription if gone)
        let (mut offsets, buckets, rr_start) = state.subscriptions
            .scope_snapshot(state, sub_id)?;           // Err(UnknownSubscription) if missing
        let n = buckets.len();
        if n == 0 {
            // no in-scope buckets: wait out the remaining time, return liveness-only
            tokio::time::sleep_until(deadline).await;
            return Ok(PullOutcome::idle(liveness(state, &buckets)));
        }
        // (2) clone Notify handles; (3) ENROLL each waiter BEFORE any read
        let notifies: Vec<Arc<Notify>> = buckets.iter()
            .filter_map(|b| state.buckets.bucket_notify(*b).ok()).collect();
        let mut futs: Vec<_> = notifies.iter().map(|n| Box::pin(n.notified())).collect();
        for f in &mut futs { f.as_mut().enable(); }    // <-- tokio enrollment; NOT create+pin
        // (4) fast-path recheck ALL offsets (after enroll)
        let drained = drain_fair(state, &buckets, &mut offsets, cap, rr_start)?;
        if !drained.events.is_empty() {
            state.subscriptions.commit_offsets(sub_id, &offsets, drained.next_rr)?;
            return Ok(PullOutcome::events(drained, liveness(state, &buckets)));
        }
        // (5) slow path: race enrolled futures vs remaining time
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(PullOutcome::idle(liveness(state, &buckets)));
        }
        let any = futures::future::select_all(futs.iter_mut().map(|f| f.as_mut()));
        match tokio::time::timeout(remaining, any).await {
            Err(_) => return Ok(PullOutcome::idle(liveness(state, &buckets))),  // timeout -> empty+liveness
            Ok(_) => { /* (6) woken (or spurious): loop re-enrolls + re-scans before await */ continue }
        }
    }
}
```

`drain_fair` (capped round-robin, AC4 + AC12 clamp):

```rust
fn drain_fair(state, buckets, offsets, cap, rr_start) -> Result<Drained, IpcError> {
    let n = buckets.len();
    let per = (cap / n).max(1);          // ceil handled by the running-total stop below
    let mut events = Vec::new();
    let mut lagged = false;
    let order: Vec<usize> = (0..n).map(|i| (rr_start + i) % n).collect();
    'outer: for pass in 0.. {
        let mut progressed = false;
        for &i in &order {
            if events.len() >= cap { break 'outer; }
            let bid = buckets[i];
            let st = state.buckets.state(bid).map_err(map_bucket_err)?;
            let off = offsets.entry(bid).or_insert(0);
            if *off < st.head_seq.saturating_sub(1) { *off = st.head_seq.saturating_sub(1); lagged = true; } // AC12 clamp
            let want = (per).min(cap - events.len());
            let resp = state.buckets.events_since(bid, &BucketReadRequest{
                cursor: *off, severity_min: /*predicate*/, kind_filter: /*predicate*/, limit: want as u32,
            }).map_err(map_bucket_err)?;
            if !resp.events.is_empty() {
                progressed = true;
                if let Some(last) = resp.events.last() { *off = last.seq; }
                events.extend(resp.events.into_iter().map(tag_with_source));
            }
            let _ = pass;
        }
        if !progressed { break; }
    }
    Ok(Drained{ events, lagged, next_rr: (rr_start + 1) % n })
}
```

- [ ] **Step 3: Run the 4 tests — PASS.** Then `cargo clippy -p terminal-commanderd --all-targets -- -D warnings`.

- [ ] **Step 4: Commit** `feat(subscriptions): lossless multiplexed pull (enroll-before-recheck) + capped round-robin`.

> CODE-REVIEW FOCUS for this task: confirm `f.as_mut().enable()` precedes the first `events_since`; confirm no lock held across `.await`; confirm spurious wake re-enrolls (the `continue` rebuilds `futs`); confirm `cap` honored for N>max; confirm clamp is `head_seq-1` and dropped delta is `head_seq-1 - off`.

---

## Task 9: IPC protocol types for the 4 methods

**Files:**
- Modify: `crates/ipc/src/protocol.rs` — `IpcRequest::{SubscriptionOpen,SubscriptionPull,SubscriptionList,SubscriptionClose}` + `IpcResponse::{...}` + the 4 `*Params`/`*Response` structs + caps (`MAX_PULL_EVENTS=50`, `DEFAULT_PULL_TIMEOUT_MS=5000`, `MAX_PULL_TIMEOUT_MS=8000` (< DRAIN_CEILING 10s)).
- Test: serde round-trip for each pair.

Response shapes (per spec §4):
- `SubscriptionOpenResponse { sub_id: String, boot_id: String, predicate_hash: String, created_at_ms: u64, matched_sources: u32 }`
- `SubscriptionPullResponse { events: Vec<SubscriptionEvent>, liveness: Vec<SourceLiveness>, lagged: bool, truncated: bool }` (NO next_state in Phase 1)
- `SubscriptionListResponse { subscriptions: Vec<SubscriptionSummary>, truncated: bool }`
- `SubscriptionCloseResponse { closed: bool }`

- [ ] Steps: failing serde test -> add variants+structs+caps -> PASS -> commit `feat(ipc): subscription_open/pull/list/close protocol types`.

---

## Task 10: Daemon handlers + dispatch arms

**Files:**
- Create: `crates/daemon/src/ipc/handlers/subscription.rs` — `handle_subscription_open/pull/list/close` (`pub(in crate::ipc::server)`)
- Modify: `crates/daemon/src/ipc/handlers/mod.rs` (`pub mod subscription;`)
- Modify: `crates/daemon/src/ipc/server.rs` (`dispatch` — 4 match arms; pull arm is `async`, uses `.await`; clamp `timeout_ms` to `MAX_PULL_TIMEOUT_MS`)
- Test: `crates/daemon/tests/subscription_ipc.rs` (e2e via `build_server` + `DaemonClient`, multi-thread `rt()`)

- [ ] **Step 1: e2e failing test** — open `{severity_min:high, sources:all}`, start two noisy commands, pull -> events from both, tagged by source, bounded (AC1); auto-join future command (AC2); idle -> empty+liveness (AC5); two opens same predicate -> distinct sub_ids, independent offsets (AC8).

- [ ] **Step 2: Handlers** — `open` -> `state.subscriptions.open(predicate)` -> response with `state.boot_id`; `pull` -> clamp timeout, call `subscriptions::pull::pull(state, id, max, timeout).await`; `list` -> bounded; `close` -> bool. Map registry errors (already typed `IpcError`).

- [ ] **Step 3: Dispatch arms** (server.rs `dispatch`), e.g.:

```rust
IpcRequest::SubscriptionPull(p) => {
    let p = p.clone();
    match handlers::subscription::handle_subscription_pull(state, &p).await {
        Ok(r) => ("subscription_pull", IpcResult::Ok { response: r }),
        Err(e) => ("subscription_pull", IpcResult::Err { error: e }),
    }
}
```
(plus open/list/close — open/list/close may be sync.)

- [ ] **Step 4: Test + commit** `feat(daemon): subscription_open/pull/list/close handlers + dispatch`. (No Windows dispatch edit — `dispatch_envelope` delegates.)

---

## Task 11: MCP tools (4) + dedicated long-poll client + contract tests

**Files:**
- Modify: `crates/mcp/src/tools.rs` — protocol imports; 4 `ToolCatalogueEntry`; 4 `Mcp*Params` (JsonSchema); 4 `#[tool]` fns; update `into_mcp_error` (done Task 1); update the 2 contract tests (`catalogue_lists_thirty_two_live_tools` -> thirty_six + vec; `tool_router_exposes_all_live_tools` -> vec).
- Modify: `crates/mcp/src/tools.rs` (the pull tool MUST use a pull-scoped daemon client with `with_timeout(Duration::from_secs(12))` — > the 8s server cap; build/hold it on the server struct or per-call) — see spec MUST-ADD #7.
- Test: `crates/mcp/tests/mcp_subscriptions_e2e.rs` (full stack via `paired_against_live_daemon`).

- [ ] **Step 1: Failing e2e** — call `subscription_open` then `subscription_pull` over the rmcp client; assert SUCCESS empty+liveness on idle (NEVER -32603 — AC13); pull unknown -> invalid_params `unknown_subscription`.

- [ ] **Step 2: Tools** — follow the 5-step shape (`ensure_daemon_available` -> build IPC params -> `self.daemon.call` -> match `Ok(IpcResponse::X)` into `json_tool_result` -> `unexpected_variant`/`into_mcp_error`). For `subscription_pull`, route through the long-timeout client field so an 8s idle pull does NOT hit the 5s default and surface -32603.

- [ ] **Step 3: Update both contract tests** (count name + both expected vecs — add the 4 names in catalogue order + sorted router order).

- [ ] **Step 4: MCP source guard** — confirm no `fs`/`Command`/socket added. Run the two grep guards from `scripts/linux-gate.sh`.

- [ ] **Step 5: Test + commit** `feat(mcp): subscription_open/pull/list/close tools (pull uses long-poll client)`.

---

## Task 12: Bounded ledger surface (folds the deferred round-2 item)

**Files:**
- Modify: `crates/ipc/src/protocol.rs` (`RuntimeStateResponse` — per-vec `limit`+`truncated` for its THREE vecs probes/buckets/active_rules; `ProbeListResponse`, `RegistryListActiveResponse`, `SubscriptionListResponse` single-vec `limit`+`truncated`)
- Modify: `crates/daemon/src/ipc/handlers/runtime.rs` + `registry.rs` (apply caps + set truncated)
- Modify: `crates/mcp/src/tools.rs` (surface `limit` param + truncated on the 4 list tools)
- Test: each list tool truncates + flags.

- [ ] Steps: failing test (over-cap input -> truncated true, bounded len) -> apply per-vec caps (`runtime_state` bounds each of 3 vecs independently) -> PASS -> commit `feat: bound list/snapshot tools (runtime_state 3 vecs, probe_list, registry_list_active, subscription_list)`.

---

## Task 13: Dual-OS gate + AC sweep

**Files:** none (verification).

- [ ] **Step 1: Linux gate (WSL)**

Run: `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && CARGO_TARGET_DIR=\$HOME/tc-linux-target bash scripts/linux-gate.sh"`
Expected: fmt clean, clippy -D warnings clean, `cargo nextest run --workspace` all green, TC47 load gate green, MCP grep guards pass.

- [ ] **Step 2: Windows gate**

Run (pwsh): `pwsh -File scripts/windows-gate.ps1`
Expected: windows_no_console + windows_spawn_site_coverage green; the new subscription tests compile+run on the named-pipe path.

- [ ] **Step 3: AC checklist** — tick every AC1-AC8, AC10-AC13 against a real run (AC9 stream is Phase 2, skip). Confirm: consumer isolation (AC8), enroll-before-recheck losslessness (AC3), fairness cap (AC4), eviction clamp (AC12), idle SUCCESS not -32603 (AC13), liveness incl. cancelled (AC5).

- [ ] **Step 4: Commit** any gate-driven fixups. Final: `chore(subscriptions): Phase 1 dual-OS green`.

---

## Self-Review (run before execution)

- **Spec coverage:** MUST-ADD #1 (Task 2), #2 (Task 4), #3 (Task 5), #5 (Task 1), #6 (Task 3), #7 (Task 11); registry (Task 7); pull/enroll/fairness/timeout (Task 8); 4 tools (Tasks 9-11); bounded ledger (Task 12); dual-OS (Task 13). Predicate/model (Task 6). All Phase 1 spec sections mapped.
- **Out of scope (Phase 2/3, NOT here):** stream bridge, Monitor/Stop-hook, MCP notification (Phase 2); tags, seek, proportional fairness (Phase 3).
- **Type consistency:** `Liveness` (Task 5) is the single union reused in `SourceLiveness`/pull (Tasks 8-9); `sub_id` is `Uuid` internally, `String` on the wire; `predicate_hash` `u64` internal, `String` on the wire; `BucketSource` (Task 4) consumed by predicate `bucket_in_scope` (Task 6) and `scope_snapshot` (Task 8).
- **Gate discipline:** every task ends code -> code-reviewer -> test-runner; dual-OS only at Task 13 (and any task touching `#[cfg]` forks: Tasks 4, 5, 10).

---

## Canonical "add an IPC-method-backed MCP tool" checklist (from grounding)

1. `crates/ipc/src/protocol.rs`: `*Params`/`*Response` structs; `IpcRequest::X`/`IpcResponse::X` variants; caps; (new) `IpcErrorCode` variant + serde test.
2. `crates/daemon/src/state.rs`: new shared state field + construct in `bootstrap` (thread `Arc::clone`).
3. `crates/daemon/src/ipc/handlers/<domain>.rs`: `pub(in crate::ipc::server) fn handle_x(state, params) -> Result<IpcResponse, IpcError>`; (probe field also edits `collect_probes` all 3 sites).
4. `crates/daemon/src/ipc/server.rs`: ONE `dispatch` match arm (Windows shares via `dispatch_envelope` — no second edit).
5. `crates/mcp/src/tools.rs`: import types; `ToolCatalogueEntry`; `McpXParams` (JsonSchema); `#[tool]` fn (5-step shape); classify any new `IpcErrorCode` in the exhaustive `into_mcp_error`.
6. `crates/mcp/src/daemon_client.rs`: usually none; long-poll needs a `with_timeout` client.
7. Tests: the 2 catalogue contract tests (count name + 2 vecs); daemon IPC test (`rt()` multi-thread + `build_server`); MCP e2e (`paired_against_live_daemon` + `call_tool`).
8. Verify: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo nextest run --workspace`; MCP source guards.
