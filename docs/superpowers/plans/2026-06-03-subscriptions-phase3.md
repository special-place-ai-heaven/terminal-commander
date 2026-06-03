# Subscriptions Phase 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Coding agent: **rust-pro**. Each task ends gated: code -> code-reviewer -> test-runner -> dual-OS gate.

**Goal:** Ship the Phase 3 optional refinements on top of the merged Phase 1 core + Phase 2 accelerators: (1) per-bucket **tags** as an AND-filter on the predicate (auto-join still works because tags ride the source side-table); (2) **`subscription_seek`** for explicit re-read (clamped, never an error); (3) **proportional (lag-weighted) fairness** inside `drain_fair`, replacing the flat per-bucket share. All three are ergonomic/expressiveness refinements, not correctness (spec §1 tags, §3 fairness note, §3 seek deferral, §"Phasing" Phase 3).

**Architecture:**
- **Tags** = a per-BUCKET AND-filter FIELD `tag: Option<String>` on `Predicate` (NOT a 5th `SourceSel` variant). It rides the existing bucket source side-table: add `tag: Option<String>` to `BucketSource` (`crates/daemon/src/subscriptions/source.rs:34`), populated at the 3 record sites (`crates/daemon/src/command.rs:478`, `crates/daemon/src/file_watch.rs:283`, `crates/daemon/src/pty_command.rs:289`); add `tag: Option<String>` to `Predicate` (`crates/daemon/src/subscriptions/model.rs:82`); extend `bucket_in_scope` (`crates/daemon/src/subscriptions/model.rs:137`) to AND the tag against the bucket source; fold `tag` into `normalized_hash` (`crates/daemon/src/subscriptions/model.rs:97`). Wire it on `SubscriptionPredicate` (`crates/ipc/src/protocol.rs:1562`) + the 3 `*StartParams` (`CommandStartParams` `crates/ipc/src/protocol.rs:710`, `FileWatchStartParams` `:1121`, `PtyCommandStartParams` `:1192`) flowing through the start handlers into `start_combed`/`start`/`start`; surface `tag` on the MCP start tools + `subscription_open`. Auto-join is PRESERVED because the side-table dirty epoch bumps on every record, forcing a routing rebuild that re-evaluates the tag predicate (spec §1 routing index).
- **`subscription_seek`** = a new IPC method `SubscriptionSeek { sub_id, bucket_id, seq } -> { clamped_seq, lagged }`. Handler resolves the sub via `with_sub_mut` (`crates/daemon/src/subscriptions/registry.rs:130`), reads the bucket's `BucketState` (`head_seq`/`tail_seq`, `crates/core/src/bucket.rs:101`), and inserts `offsets.insert(bucket_id, seq.clamp(head_seq.saturating_sub(1), tail_seq))`; `lagged = requested < head_seq.saturating_sub(1)`. An unknown sub is `UnknownSubscription` (via `with_sub_mut`'s built-in miss path); out-of-range is CLAMPED, not an error. New MCP tool (catalogue + the 36->37 contract-test count + the router vec). NO new `IpcErrorCode`.
- **Proportional fairness** = replace the flat `let per = (cap / n).max(1);` in `drain_fair` (`crates/daemon/src/subscriptions/pull.rs:264`) with a lag-weighted per-bucket share: `per_i = max(1, cap * backlog_i / sum(backlog))` where `backlog_i = tail_seq_i - off_i`; hard-stop at `cap`; rotate `rr_start` for ties; fall back to round-robin when backlogs are equal/zero. Internal-only, NO wire change.

**Tech Stack:** Rust (existing `subscriptions` module + `tokio::sync` bucket primitives, `serde`/`schemars` for wire + MCP params), rmcp MCP SDK, `cargo nextest`, dual gate (`scripts/linux-gate.sh` via WSL + `scripts/windows-gate.ps1`).

**Spec:** `docs/superpowers/specs/2026-06-02-subscriptions-design.md` — read §1 (predicate grammar incl. `tag`; routing index/dirty-flag), §3 step 6 (fairness; "Proportional-to-lag deferred to Phase 3") + the `subscription_seek` deferral note, §"Phasing" Phase 3. The locked decisions below override the spec's "deferred/confirm" language.

**Constraints (do not violate):**
- MCP crate (`crates/mcp/src`) forbids `Command::new`/`spawn`, `TcpListener`/`UdpSocket`, `std::fs`/`tokio::fs`/`File::open`/`read_to_string` (the linux gate greps for it). Tools are pure IPC forwards. The new `subscription_seek` tool is a thin forward.
- `IpcErrorCode` is a CLOSED set. Phase 3 adds NO new error codes: `UnknownSubscription` covers a missing sub; out-of-range seeks CLAMP (not errors). The `into_mcp_error` exhaustive match (`crates/mcp/src/tools.rs:1550`) is UNCHANGED.
- Every new Rust source file starts with the PolyForm SPDX header:
  ```
  // SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
  // Copyright 2026 The Terminal Commander Authors
  ```
- `tag` is a per-BUCKET SOURCE field, DISTINCT from `RuleDefinition.tags: Vec<String>` (a per-rule concept, `crates/core/src/rule.rs`). Do not conflate them. The seek clamp + lag flag are NEVER errors.
- Auto-join MUST survive the tag change: the side-table `record` bumps the dirty epoch, so a tagged future probe re-routes on the next pull's rebuild.
- No push/merge without the operator's approval (commit to a review branch and stop).
- `env` is OVERLAY (untouched by this work).

---

## Task 0: Integration branch + baseline gate

**Files:** none (git only).

- [ ] **Step 1: Branch off the current `main` (Phase 1 + 2 merged)**

```bash
git switch main
git pull --ff-only
git switch -c feat/subscriptions-phase3
```

- [ ] **Step 2: Baseline gate (must be green BEFORE feature work)**

Run (WSL): `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && CARGO_TARGET_DIR=\$HOME/tc-linux-target bash scripts/linux-gate.sh"`
Expected: PASS. If red, STOP — the base is broken, not your change.

---

## Task 1: Tags — per-bucket AND-filter on the predicate

This task threads ONE field end to end. It touches more than 5 files, so split into two phases per the edit-safety rule: **Phase 1a (daemon: side-table + model + record sites)**, then **Phase 1b (wire + handlers + MCP)**.

### Phase 1a — daemon side-table + model

**Files:**
- Modify: `crates/daemon/src/subscriptions/source.rs` — add `pub tag: Option<String>` to `BucketSource` (`:34`).
- Modify: `crates/daemon/src/subscriptions/model.rs` — add `pub tag: Option<String>` to `Predicate` (`:82`); extend `bucket_in_scope` (`:137`); fold `tag` into `normalized_hash` (`:97`).
- Modify: `crates/daemon/src/command.rs:478`, `crates/daemon/src/file_watch.rs:283`, `crates/daemon/src/pty_command.rs:289` — set `tag` at each `self.sources.record(...)` site (from the start request; `None` until the wire field exists in Phase 1b — wire a local `tag: Option<String>` plumbed in 1b, default `None` for 1a so this compiles standalone).

- [ ] **Step 1: Failing test FIRST (model)**

In `crates/daemon/src/subscriptions/model.rs` tests mod (alongside `bucket_in_scope_all_is_always_true`, `:275`):

```rust
#[test]
fn tag_predicate_matches_only_tagged_source() {
    let mut tagged = src(); // helper builds a BucketSource
    tagged.tag = Some("deploy".to_owned());
    let untagged = src(); // tag: None
    let id = BucketId::new();

    let pred = Predicate {
        severity_min: None,
        kind: None,
        sources: SourceSel::All,
        tag: Some("deploy".to_owned()),
    };
    assert!(pred.bucket_in_scope(id, &tagged), "tag matches");
    assert!(!pred.bucket_in_scope(id, &untagged), "untagged excluded");

    // A None tag predicate ignores the tag dimension entirely.
    let pred_any = Predicate { tag: None, ..pred.clone() };
    assert!(pred_any.bucket_in_scope(id, &tagged));
    assert!(pred_any.bucket_in_scope(id, &untagged));
}

#[test]
fn normalized_hash_includes_tag() {
    let base = Predicate { severity_min: None, kind: None, sources: SourceSel::All, tag: None };
    let tagged = Predicate { tag: Some("a".to_owned()), ..base.clone() };
    assert_ne!(base.normalized_hash(), tagged.normalized_hash());
    // Same tag -> same hash (stable).
    let tagged2 = Predicate { tag: Some("a".to_owned()), ..base.clone() };
    assert_eq!(tagged.normalized_hash(), tagged2.normalized_hash());
}
```

(Update the `src()` helper at `crates/daemon/src/subscriptions/model.rs:217` to set `tag: None` so existing constructions still compile.)

- [ ] **Step 2: Run — expect FAIL** (`Predicate` has no `tag`; `BucketSource` has no `tag`).

Run: `cargo test -p terminal-commanderd subscriptions::model::tests`

- [ ] **Step 3: Add `tag` to `BucketSource`** (`crates/daemon/src/subscriptions/source.rs:34`):

```rust
    /// Optional per-bucket tag for predicate AND-filtering (Phase 3). Set at
    /// probe start from the start request; None when the caller omits it.
    pub tag: Option<String>,
```

Update the `cmd_source()` test helper (`crates/daemon/src/subscriptions/source.rs:100`) + all existing `BucketSource { .. }` literals to add `tag: None`.

- [ ] **Step 4: Add `tag` to `Predicate`** (`crates/daemon/src/subscriptions/model.rs:82`):

```rust
    /// Per-BUCKET tag AND-filter (Phase 3). `None` = ignore the tag dimension.
    /// Matches only buckets whose source `tag` equals this value.
    pub tag: Option<String>,
```

- [ ] **Step 5: Extend `bucket_in_scope`** (`crates/daemon/src/subscriptions/model.rs:137`) — AND the tag onto the existing source match:

```rust
    pub fn bucket_in_scope(&self, id: BucketId, src: &BucketSource) -> bool {
        // Tag is an AND-filter: a Some(tag) predicate requires the bucket's
        // source tag to equal it; None ignores the tag dimension entirely.
        if let Some(want) = &self.tag {
            if src.tag.as_ref() != Some(want) {
                return false;
            }
        }
        match &self.sources {
            SourceSel::All => true,
            SourceSel::Buckets(ids) => ids.contains(&id),
            SourceSel::Jobs(jobs) => src.job_id.is_some_and(|j| jobs.contains(&j)),
            SourceSel::Probes(probes) => src.probe_id.is_some_and(|p| probes.contains(&p)),
        }
    }
```

- [ ] **Step 6: Fold `tag` into `normalized_hash`** (`crates/daemon/src/subscriptions/model.rs:97`) — add a stanza mirroring the `severity_min` shape (presence byte + value), AFTER the `kind` stanza and BEFORE `self.sources.hash_normalized(&mut h)`:

```rust
        // tag (presence byte + value, like severity_min)
        match &self.tag {
            Some(t) => {
                1u8.hash(&mut h);
                t.hash(&mut h);
            }
            None => 0u8.hash(&mut h),
        }
```

- [ ] **Step 7: Set `tag` at the 3 record sites** (default `None` for 1a; the start request plumbs it in 1b). At `crates/daemon/src/command.rs:478` (the `BucketSource { kind: Command, job_id, probe_id, path: None }` literal) add `tag: req.tag.clone()` (after 1b adds `tag` to `CommandStartRequest`; for 1a, add `tag: None` and revisit in 1b). Same at `crates/daemon/src/file_watch.rs:283` and `crates/daemon/src/pty_command.rs:289`.

- [ ] **Step 8: Run — PASS.** `cargo test -p terminal-commanderd subscriptions`; `cargo check --workspace`.

- [ ] **Step 9: GATE — code -> code-reviewer -> test-runner.** Commit:

```bash
git add crates/daemon/src/subscriptions/source.rs crates/daemon/src/subscriptions/model.rs crates/daemon/src/command.rs crates/daemon/src/file_watch.rs crates/daemon/src/pty_command.rs
git commit -m "feat(subscriptions): per-bucket tag AND-filter on Predicate + BucketSource (Phase 1a)"
```

### Phase 1b — wire + handlers + MCP surface

**Files:**
- Modify: `crates/ipc/src/protocol.rs` — add `tag: Option<String>` to `SubscriptionPredicate` (`:1562`) + `CommandStartParams` (`:710`) + `FileWatchStartParams` (`:1121`) + `PtyCommandStartParams` (`:1192`), each `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Modify: `crates/daemon/src/command.rs` — add `tag: Option<String>` to `CommandStartRequest` (`:132`); thread it in `handle_command_start_combed` (`crates/daemon/src/ipc/handlers/command.rs:15`) and into the record site (Phase 1a Step 7 becomes `req.tag.clone()`).
- Modify: file-watch + pty start request structs + handlers (`crates/daemon/src/ipc/handlers/file.rs:201`, `crates/daemon/src/ipc/handlers/pty.rs:27`) likewise.
- Modify: `crates/daemon/src/ipc/handlers/subscription.rs` — `predicate_from_wire` (`:28`) maps `tag` into `Predicate`.
- Modify: `crates/mcp/src/tools.rs` — add `tag` to `McpSubscriptionOpenParams` (`:2783`) -> `into_predicate` (`:2800`); add `tag` to `McpCommandStartParams` (`:1972`), `McpFileWatchStartParams` (`:2616`), `McpPtyCommandStartParams` (`:2653`) -> their `into_ipc`/construction.
- Test: `crates/daemon/tests/subscription_ipc.rs` — a tagged probe is matched by a tag predicate; an untagged one is not (e2e via the live UDS server).

- [ ] **Step 1: Failing e2e test FIRST** (`crates/daemon/tests/subscription_ipc.rs`):

```rust
#[test]
fn tagged_probe_matched_only_by_matching_tag_predicate() {
    // 1. open {sources:all, tag: Some("deploy")}.
    // 2. start a command WITH tag="deploy" -> emits a high-sev event.
    // 3. start a command WITHOUT a tag -> emits a high-sev event.
    // 4. pull -> events come ONLY from the tagged probe.
    // (drives the live daemon end to end via DaemonClient, like AC1)
}
```

- [ ] **Step 2: Run — expect FAIL** (`SubscriptionPredicate`/`CommandStartParams` have no `tag`).

- [ ] **Step 3: Add `tag` to the 4 wire structs** (`crates/ipc/src/protocol.rs`):

`SubscriptionPredicate` (`:1562`):
```rust
    /// Per-BUCKET tag AND-filter. Omitted = ignore the tag dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
```
`CommandStartParams` (`:710`), `FileWatchStartParams` (`:1121`), `PtyCommandStartParams` (`:1192`):
```rust
    /// Optional per-bucket tag for subscription routing (Phase 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
```

- [ ] **Step 4: Thread the start `tag` through the daemon** — `CommandStartRequest` (`crates/daemon/src/command.rs:132`) gains `pub tag: Option<String>`; `handle_command_start_combed` (`crates/daemon/src/ipc/handlers/command.rs:15`) sets `tag: params.tag.clone()` in the `CommandStartRequest { .. }` literal; the record site (`crates/daemon/src/command.rs:478`) becomes `tag: req.tag.clone()`. Mirror for file-watch (`crates/daemon/src/ipc/handlers/file.rs:201` -> the watch start request -> `crates/daemon/src/file_watch.rs:283`) and pty (`crates/daemon/src/ipc/handlers/pty.rs:27` -> `crates/daemon/src/pty_command.rs:289`).

- [ ] **Step 5: Map `tag` in `predicate_from_wire`** (`crates/daemon/src/ipc/handlers/subscription.rs:28`) — add `tag: wire.tag.clone()` (or move-by-value) into the `Predicate { .. }` it builds.

- [ ] **Step 6: Surface `tag` on the MCP tools** — add `#[serde(default)] pub tag: Option<String>` to `McpSubscriptionOpenParams` (`crates/mcp/src/tools.rs:2783`) and pass it through `into_predicate` (`:2800`) into `SubscriptionPredicate`. Add the same to `McpCommandStartParams` (`:1972`) `into_ipc` (`:2015`), `McpFileWatchStartParams` (`:2616`), `McpPtyCommandStartParams` (`:2653`) -> the wire `*StartParams` they build. (Note: `subscription_open` is the primary consumer; the start tools let callers TAG a probe so a tag predicate can route to it.)

- [ ] **Step 7: Run — PASS + check.** `cargo test -p terminal-commanderd --test subscription_ipc tagged_probe`; `cargo check --workspace`; serde round-trip for the 4 changed wire structs (a tag round-trips; omitted -> `None`).

- [ ] **Step 8: GATE — code -> code-reviewer -> test-runner.** REVIEW FOCUS: `tag` is `Option` everywhere with `skip_serializing_if` (wire-compat: old clients omit it -> `None`); auto-join still works (record bumps the dirty epoch -> rebuild re-evaluates the tag); tag is NOT confused with `RuleDefinition.tags`; pty path stays `#[cfg(unix)]`. Commit:

```bash
git add crates/ipc/src/protocol.rs crates/daemon/src/command.rs crates/daemon/src/file_watch.rs crates/daemon/src/pty_command.rs crates/daemon/src/ipc/handlers/ crates/mcp/src/tools.rs
git commit -m "feat(subscriptions): wire + handlers + MCP surface for per-bucket tag predicate (Phase 1b)"
```

---

## Task 2: `subscription_seek` — explicit clamped re-read

**Files:**
- Modify: `crates/ipc/src/protocol.rs` — `SubscriptionSeekParams { sub_id: String, bucket_id: BucketId, seq: u64 }`; `SubscriptionSeekResponse { clamped_seq: u64, lagged: bool }`; `IpcRequest::SubscriptionSeek(SubscriptionSeekParams)` (after `SubscriptionClose`, `:298`); `IpcResponse::SubscriptionSeek(SubscriptionSeekResponse)`.
- Modify: `crates/daemon/src/ipc/handlers/subscription.rs` — `handle_subscription_seek` (mirror `handle_subscription_close` `:179` for sub resolution, but mutate via `with_sub_mut`).
- Modify: `crates/daemon/src/ipc/server.rs` — one `dispatch` arm (after the `SubscriptionClose` arm, `:623`).
- Modify: `crates/mcp/src/tools.rs` — `McpSubscriptionSeekParams`; `subscription_seek` `#[tool]` fn; a `ToolCatalogueEntry` (after `subscription_close`, `:285`); update `catalogue_lists_thirty_six_live_tools` -> `..._thirty_seven_...` + add `"subscription_seek"` to BOTH expected vecs (catalogue order at `:3042`; sorted router order at `:3101`).
- Test: `crates/daemon/tests/subscription_ipc.rs` (e2e seek behavior) + the MCP contract-test count bump.

> **Locked clamp:** `offsets.insert(bucket_id, seq.clamp(head_seq.saturating_sub(1), tail_seq))`; `lagged = requested_seq < head_seq.saturating_sub(1)` (the requested seq was evicted). Out-of-range is CLAMP, NOT error. Unknown sub -> `UnknownSubscription` (via `with_sub_mut`). NO new `IpcErrorCode`. `BucketState.head_seq`/`tail_seq` from `crates/core/src/bucket.rs:101`.

- [ ] **Step 1: Failing tests FIRST** (`crates/daemon/tests/subscription_ipc.rs`):

```rust
#[test]
fn seek_within_range_repositions_offset() {
    // open {sources:all}; start a command that emits N events; pull once;
    // seek(sub, bucket, earlier_seq) -> clamped_seq == earlier_seq, lagged==false;
    // next pull re-delivers from earlier_seq+1.
}

#[test]
fn seek_into_evicted_territory_clamps_and_sets_lagged() {
    // force FIFO eviction so head_seq > requested; seek(sub, bucket, 0) ->
    // clamped_seq == head_seq-1, lagged == true (NOT an error).
}

#[test]
fn seek_unknown_sub_is_unknown_subscription() {
    // seek a random uuid -> Err(UnknownSubscription).
}
```

- [ ] **Step 2: Run — expect FAIL** (no `SubscriptionSeek`).

- [ ] **Step 3: Add the wire types** (`crates/ipc/src/protocol.rs`):

```rust
pub struct SubscriptionSeekParams {
    pub sub_id: String,
    pub bucket_id: BucketId,
    /// Requested re-read position. Clamped to
    /// `[head_seq.saturating_sub(1), tail_seq]`; never an error.
    pub seq: u64,
}

pub struct SubscriptionSeekResponse {
    /// The offset actually stored after clamping.
    pub clamped_seq: u64,
    /// True when the requested seq was below `head_seq-1` (events evicted).
    pub lagged: bool,
}
```
Add `IpcRequest::SubscriptionSeek(SubscriptionSeekParams)` (after `:298`) and `IpcResponse::SubscriptionSeek(SubscriptionSeekResponse)` (after `:365`). Serde round-trip test for the pair.

- [ ] **Step 4: Add the handler** (`crates/daemon/src/ipc/handlers/subscription.rs`, after `handle_subscription_close` `:179`):

```rust
/// `subscription_seek` -> reposition this consumer's offset for ONE bucket.
/// The requested seq is CLAMPED to `[head_seq-1, tail_seq]` (never an error);
/// `lagged` flags a request below the surviving head (events evicted).
///
/// # Errors
/// [`IpcErrorCode::UnknownSubscription`] if the sub is unknown (via
/// `with_sub_mut`'s miss path). A malformed `sub_id` is likewise unknown.
pub(in crate::ipc::server) fn handle_subscription_seek(
    state: &Arc<DaemonState>,
    params: &SubscriptionSeekParams,
) -> Result<IpcResponse, IpcError> {
    let sub_id = parse_sub_id(&params.sub_id)?; // -> UnknownSubscription on parse fail
    let st = state.buckets.state(params.bucket_id).map_err(map_bucket)?;
    let floor = st.head_seq.saturating_sub(1);
    let lagged = params.seq < floor;
    let clamped = params.seq.clamp(floor, st.tail_seq);
    state
        .subscriptions
        .with_sub_mut(sub_id, |s| {
            s.offsets.insert(params.bucket_id, clamped);
        })?; // -> UnknownSubscription if the sub is gone
    Ok(IpcResponse::SubscriptionSeek(SubscriptionSeekResponse {
        clamped_seq: clamped,
        lagged,
    }))
}
```

(Reuse `parse_sub_id` `:94` and the existing `map_bucket`/bucket-error mapping pattern; if the handler module lacks a bucket-error mapper, mirror `pull.rs`'s `map_bucket` `:230`. A bucket the daemon does not know surfaces as the existing `BucketNotFound`, not a new code.)

- [ ] **Step 5: Add the dispatch arm** (`crates/daemon/src/ipc/server.rs`, after the `SubscriptionClose` arm `:623`):

```rust
        IpcRequest::SubscriptionSeek(p) => {
            match handlers::subscription::handle_subscription_seek(state, p) {
                Ok(r) => ("subscription_seek", IpcResult::Ok { response: r }),
                Err(e) => ("subscription_seek", IpcResult::Err { error: e }),
            }
        }
```

(Windows shares dispatch via `dispatch_envelope` — no second edit.)

- [ ] **Step 6: Add the MCP tool** (`crates/mcp/src/tools.rs`):

`McpSubscriptionSeekParams` (near `McpSubscriptionCloseParams` `:2840`):
```rust
pub struct McpSubscriptionSeekParams {
    /// Opaque sub_id from subscription_open.
    pub sub_id: String,
    /// Bucket id (`bkt_<hex>`) to reposition within.
    pub bucket_id: String,
    /// Requested re-read position; clamped to the bucket's live range.
    pub seq: u64,
}
```
The `#[tool]` fn (after `subscription_close` `:1496`, 5-step shape, routed through `self.daemon` not `pull_daemon` — seek is not a long-poll):
```rust
    async fn subscription_seek(
        &self,
        Parameters(params): Parameters<McpSubscriptionSeekParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_daemon_available().await?;
        let bucket_id = parse_id(&params.bucket_id)?; // existing helper :1660
        let ipc = SubscriptionSeekParams { sub_id: params.sub_id, bucket_id, seq: params.seq };
        match self.daemon.call(IpcRequest::SubscriptionSeek(ipc)).await {
            Ok(IpcResponse::SubscriptionSeek(SubscriptionSeekResponse { clamped_seq, lagged })) =>
                json_tool_result(&serde_json::json!({ "clamped_seq": clamped_seq, "lagged": lagged })),
            Ok(other) => Err(unexpected_variant(&other)),
            Err(e) => Err(into_mcp_error(&e)),
        }
    }
```
Catalogue entry (after `subscription_close` `:285`):
```rust
        ToolCatalogueEntry {
            name: "subscription_seek",
            status: ToolStatus::Live,
            description: "Reposition a subscription's offset for one bucket (explicit re-read). The requested seq is clamped to the bucket's live range (never an error); lagged flags an evicted request.",
        },
```
Import `SubscriptionSeekParams, SubscriptionSeekResponse` in the protocol `use` block (`crates/mcp/src/tools.rs:44`).

- [ ] **Step 7: Update the two contract tests** (`crates/mcp/src/tools.rs`):
  - Rename `catalogue_lists_thirty_six_live_tools` (`:3042`) -> `catalogue_lists_thirty_seven_live_tools`; add `"subscription_seek"` to its expected vec (catalogue order: right after `"subscription_close"`).
  - In `tool_router_exposes_all_live_tools` (`:3101`), add `"subscription_seek".to_owned()` to the SORTED vec (alphabetical: between `"subscription_pull"` and `"system_discover"` -> after `subscription_pull`, before `system_discover`; precise position: `subscription_close`, `subscription_list`, `subscription_open`, `subscription_pull`, `subscription_seek`, `system_discover`).
  - Update the module doc comment "36 live tools" (`crates/mcp/src/tools.rs:20`, `:29`) -> 37.

- [ ] **Step 8: MCP facade guard** — confirm no fs/spawn/socket added. Run the two grep guards from `scripts/linux-gate.sh`.

- [ ] **Step 9: Run — PASS + clippy.** `cargo test -p terminal-commanderd --test subscription_ipc seek`; `cargo test -p terminal-commander-mcp catalogue_lists_thirty_seven tool_router_exposes_all`; `cargo clippy --workspace --all-targets -- -D warnings`.

- [ ] **Step 10: GATE — code -> code-reviewer -> test-runner.** REVIEW FOCUS: clamp is `seq.clamp(head_seq-1, tail_seq)`; `lagged = requested < head_seq-1`; out-of-range is NOT an error; unknown sub -> `UnknownSubscription`; NO new `IpcErrorCode`; both contract vecs + count updated; seek routed through the normal (not long-poll) client. Commit:

```bash
git add crates/ipc/src/protocol.rs crates/daemon/src/ipc/handlers/subscription.rs crates/daemon/src/ipc/server.rs crates/mcp/src/tools.rs crates/daemon/tests/subscription_ipc.rs
git commit -m "feat(subscriptions): subscription_seek clamped re-read (37th MCP tool, no new error code)"
```

---

## Task 3: Proportional (lag-weighted) fairness in `drain_fair`

**Files:**
- Modify: `crates/daemon/src/subscriptions/pull.rs` — `drain_fair` (`:264`): replace the flat `let per = (cap / n).max(1);` with a per-bucket lag-weighted share; keep the `cap` hard-stop, the `rr_start` rotation, the eviction clamp, and the round-robin fallback.
- Test: `crates/daemon/tests/subscription_pull_lossless.rs` — a high-backlog bucket gets a larger share than a low-backlog one within one pull (total <= max); equal backlogs behave like round-robin (existing AC4 tests still pass).

> **Locked formula:** `per_i = max(1, cap * backlog_i / sum(backlog))` where `backlog_i = tail_seq_i - off_i` (optionally `+ dropped_count_i` weight); hard-stop at `cap`; rotate `rr_start` for ties; fall back to round-robin when backlogs are equal/zero. Internal-only, NO wire change. The existing eviction clamp (`off = head_seq-1`) and the `events_since` cursor-advance discipline (`crates/daemon/src/subscriptions/pull.rs:264`, lossless) are PRESERVED.

- [ ] **Step 1: Failing test FIRST** (`crates/daemon/tests/subscription_pull_lossless.rs`, alongside `pull_fairness_capped_and_no_starvation`):

```rust
#[tokio::test(flavor = "multi_thread")]
async fn pull_proportional_share_favors_high_backlog_bucket() {
    // Two in-scope buckets: A has a large backlog (e.g. 40 unread), B has a
    // small backlog (e.g. 4 unread). One pull with max=20:
    //   - A receives MORE than B (proportional to backlog),
    //   - total <= 20 (hard cap),
    //   - B is NOT starved (>= 1 event).
}

#[tokio::test(flavor = "multi_thread")]
async fn pull_equal_backlogs_behave_like_round_robin() {
    // Two buckets with equal backlog -> roughly equal shares (the AC4
    // round-robin behavior is preserved). Existing AC4 tests must still pass.
}
```

- [ ] **Step 2: Run — expect FAIL** (current flat share gives equal `per` regardless of backlog).

- [ ] **Step 3: Implement the lag-weighted share** — `replace_symbol_body` on `drain_fair` (`crates/daemon/src/subscriptions/pull.rs:264`). Replace the single `let per = (cap / n).max(1);` with a per-bucket `per_i` vector computed from each bucket's backlog, then use `per_i[i]` (instead of the scalar `per`) inside the visit loop. Keep EVERYTHING else (the `order` rotation by `rr_start`, the `events.len() >= cap` hard-stop, the eviction clamp, the multi-kind post-filter, the cursor-advance discipline, the `next_rr` rotation). Sketch of the new prelude (full code in the edit):

```rust
    // Lag-weighted per-bucket share (Phase 3). backlog_i = tail_seq - off_i
    // (the unread depth). per_i = max(1, cap * backlog_i / sum(backlog)) so a
    // high-backlog bucket drains more within one pull, while a quiet bucket is
    // never starved (>= 1). When all backlogs are equal/zero this collapses to
    // the flat round-robin share (AC4 preserved). The cap is still a hard stop.
    let mut backlog: Vec<u64> = Vec::with_capacity(n);
    for scoped in &scope.buckets {
        let st = state.buckets.state(scoped.bucket_id).map_err(map_bucket)?;
        let off = offsets.get(&scoped.bucket_id).copied().unwrap_or(0);
        // Clamp the floor exactly as the drain loop does so backlog matches
        // what is actually readable.
        let floor = st.head_seq.saturating_sub(1);
        let eff_off = off.max(floor);
        backlog.push(st.tail_seq.saturating_sub(eff_off));
    }
    let total: u64 = backlog.iter().copied().sum();
    let per_i: Vec<usize> = if total == 0 {
        // No backlog anywhere -> flat fallback (round-robin).
        vec![(cap / n).max(1); n]
    } else {
        backlog
            .iter()
            .map(|&b| {
                let share = (cap as u128 * b as u128 / total as u128) as usize;
                share.max(1)
            })
            .collect()
    };
```

Then inside the loop, where it currently reads `let want = per.min(cap - events.len());`, use `per_i[i]` (where `i` is the bucket index in `scope.buckets`; the visit `order` already maps the rotated position back to `i`). The hard-stop `if events.len() >= cap { break 'outer; }` is unchanged, so the sum of `per_i` exceeding `cap` is harmless (the running total caps it). `next_rr` rotation and ties: rotating `rr_start` by one each pull (already done) gives the tie-break across pulls.

(Note: `drain_fair` already re-reads `state.buckets.state(bid)` per bucket inside the loop for the clamp; the backlog prelude adds one extra state read per bucket. Acceptable under the `MAX_BUCKETS_PER_SUBSCRIPTION` = 200 cap. The reviewer may fold the two reads into one if it stays clean.)

- [ ] **Step 4: Run — PASS, including the existing AC4 tests** (`pull_fairness_capped_and_no_starvation`). `cargo test -p terminal-commanderd --test subscription_pull_lossless`; `cargo clippy -p terminal-commanderd --all-targets -- -D warnings`.

- [ ] **Step 5: GATE — code -> code-reviewer -> test-runner.** REVIEW FOCUS: per-bucket share is lag-weighted; `max(1, ...)` prevents starvation; the `cap` hard-stop still bounds the total; equal/zero backlogs fall back to round-robin (AC4 preserved); the eviction clamp + lossless cursor-advance are untouched; NO wire change; backlog uses the SAME clamp floor as the drain so they cannot disagree. Commit:

```bash
git add crates/daemon/src/subscriptions/pull.rs crates/daemon/tests/subscription_pull_lossless.rs
git commit -m "feat(subscriptions): lag-weighted proportional fairness in drain_fair (internal, no wire change)"
```

---

## Task 4: Dual-OS gate + Phase 3 AC sweep

**Files:** none (verification).

- [ ] **Step 1: Linux gate (WSL)**

Run: `wsl.exe -e bash -lc "cd /mnt/e/project/terminal-commander && CARGO_TARGET_DIR=\$HOME/tc-linux-target bash scripts/linux-gate.sh"`
Expected: fmt clean, clippy -D warnings clean, `cargo nextest run --workspace` green (incl. tag, seek, proportional-fairness tests + the updated 37-tool contract tests), MCP grep guards PASS.

- [ ] **Step 2: Windows gate**

Run (pwsh): `pwsh -File scripts/windows-gate.ps1`
Expected: windows_no_console + windows_spawn_site_coverage green; the new tests compile+run on the named-pipe path; the pty `tag` write stays `#[cfg(unix)]`.

- [ ] **Step 3: Phase 3 checklist** — verify against a real run:
  - Tags: a tagged probe is matched ONLY by a matching-tag predicate; an untagged probe is excluded by a `Some(tag)` predicate; `normalized_hash` differs with/without tag; auto-join still routes a tagged FUTURE probe.
  - Seek: within-range repositions; into-evicted clamps to `head_seq-1` + `lagged=true`; unknown sub -> `UnknownSubscription`; no new error code.
  - Fairness: high-backlog bucket gets a larger share, total <= max, quiet bucket not starved; equal backlogs behave like round-robin (AC4 still green).

- [ ] **Step 4: Commit** any gate-driven fixups. Final: `chore(subscriptions): Phase 3 dual-OS green (tags + seek + proportional fairness)`. STOP for operator approval before push/merge (commit to the review branch).

---

## Self-Review (run before execution)

- **Spec coverage:** §1 tags as a per-bucket AND-filter on the predicate riding the side-table (Task 1, both phases) — locked as a `tag` FIELD, NOT a 5th `SourceSel`; §3 `subscription_seek` clamped re-read (Task 2) — locked clamp + lag flag, no new error code; §3 step 6 proportional-to-lag fairness (Task 3) — locked formula, internal-only. §"Phasing" Phase 3 fully mapped.
- **Out of scope (Phase 1/2 done, NOT here):** the pull engine core, registry, severity/kind/sources predicate, liveness, boot_id, the 4 base tools, the 2 IpcErrorCode variants (Phase 1); the stream bridge, MCP notification, Stop-hook (Phase 2). No re-litigation of locked decisions.
- **Type consistency:** `tag: Option<String>` is identical across `BucketSource` (daemon) <-> `Predicate` (daemon) <-> `SubscriptionPredicate` + 3 `*StartParams` (wire) <-> the MCP params; `skip_serializing_if = "Option::is_none"` keeps wire-compat (old clients omit -> `None`). `SubscriptionSeekParams.bucket_id` is `BucketId` on the wire, `String` -> `parse_id` on the MCP boundary; `clamped_seq`/`seq` are `u64` (match `BucketState.head_seq`/`tail_seq`). `tag` (per-bucket source) is DISTINCT from `RuleDefinition.tags` (per-rule). Proportional fairness adds NO type — `per_i: Vec<usize>` is a local in `drain_fair`.
- **No new error code:** `UnknownSubscription` (Phase 1) covers a missing sub on seek; out-of-range seeks clamp; a missing bucket is the existing `BucketNotFound`. `into_mcp_error` is UNCHANGED, so its exhaustive match still compiles. The MCP contract tests bump 36 -> 37 (Task 2 Step 7) — the ONLY tool-count change in Phase 3.
- **Facade discipline:** `subscription_seek` is a thin IPC forward; no fs/spawn/socket; the linux-gate grep guards must still pass (Task 2 Step 8 + Task 4 Step 1).
- **Gate discipline:** each task ends code -> code-reviewer -> test-runner; dual-OS at Task 4 (and any `#[cfg]` fork: the pty `tag` write, Task 1). Task 1 is split 1a/1b to keep each phase <= 5 files + a verify between them.
- **Auto-join invariant:** the tag change preserves auto-join because `BucketSourceTable::record` bumps the dirty epoch on every probe start, forcing `scope_snapshot` to rebuild and re-evaluate the tag predicate against the new bucket (spec §1 routing index, dirty-flag).

---

## Canonical "add an IPC-method-backed MCP tool" checklist (applies to Task 2 `subscription_seek`)

1. `crates/ipc/src/protocol.rs`: `SubscriptionSeekParams`/`SubscriptionSeekResponse`; `IpcRequest::SubscriptionSeek`/`IpcResponse::SubscriptionSeek`; serde round-trip test. (No new `IpcErrorCode`.)
2. `crates/daemon/src/state.rs`: NO new state field — `subscriptions` + `buckets` already on `DaemonState`.
3. `crates/daemon/src/ipc/handlers/subscription.rs`: `handle_subscription_seek` (`with_sub_mut` + the locked clamp).
4. `crates/daemon/src/ipc/server.rs`: ONE `dispatch` arm (Windows shares via `dispatch_envelope`).
5. `crates/mcp/src/tools.rs`: import the two types; `ToolCatalogueEntry`; `McpSubscriptionSeekParams` (JsonSchema); `#[tool]` fn (5-step shape, normal client); NO `into_mcp_error` change (no new code).
6. Tests: the 2 catalogue contract tests (count 36 -> 37 + both vecs); the daemon IPC seek tests (`subscription_ipc.rs`).
7. Verify: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo nextest run --workspace`; MCP source guards; dual-OS.
