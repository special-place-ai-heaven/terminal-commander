# Contract: IPC Wire Deltas

**Layer**: `crates/ipc/src/protocol.rs` (adapter <-> daemon local IPC).
**Envelope**: unchanged. `IpcRequest` is `#[serde(tag = "method",
content = "params", rename_all = "snake_case")]`; `IpcResponse` is tagged by
`method`. No `deny_unknown_fields` anywhere on the wire (unknown fields
ignored on decode) — that is the existing posture and stays.

**House serde rules for every addition** (violating any of these is a wire
break):
- new optional input field: `#[serde(default, skip_serializing_if = "Option::is_none")]`
- new bool input field: `#[serde(default)]`
- new optional list output: `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
- new output Option field: `#[serde(default, skip_serializing_if = "Option::is_none")]`
- existing required field relaxed to optional: `#[serde(default, ...)]` so
  old senders (always supply it) and new senders (may omit) both decode.

Every request variant added below must also be classified in
`IpcRequest::is_idempotent` (`protocol.rs:439-540`) — the match is
exhaustive, so the compiler enforces this.

---

## W1. FileListDir (NEW pair — US3)

Method name: `file_list_dir`. Idempotency: **idempotent read** (groups with
`FileReadWindow`/`FileSearch`).

```rust
// IpcRequest::FileListDir(FileListDirParams)
pub struct FileListDirParams {
    /// Absolute path of the directory to list.
    pub path: String,
    /// Cap on returned entries; clamped to
    /// [1, MAX_FILE_LIST_ENTRIES], default DEFAULT_FILE_LIST_ENTRIES.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u32>,
}

// IpcResponse::FileListDir(FileListDirResponse)
pub struct FileListDirResponse {
    /// Canonicalized directory that was listed.
    pub path: String,
    /// Sorted: dirs first, then files/symlinks; each group lexicographic.
    pub entries: Vec<DirEntry>,
    /// Total entries present in the directory (>= entries.len()).
    pub total_entries: u64,
    /// true iff total_entries > entries.len().
    pub truncated: bool,
}

pub struct DirEntry {
    pub name: String,                 // file name only, no path
    pub kind: DirEntryKind,           // file | dir | symlink (snake_case)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,      // files only
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_ms: Option<i64>,        // millis since epoch; None on stat race
}
```

Constants (alongside the file caps at `protocol.rs:1577-1612`):
`MAX_FILE_LIST_ENTRIES: usize = 500`, `DEFAULT_FILE_LIST_ENTRIES: usize = 200`.

**Errors** (same shapes as `file_read_window`):
- policy-denied path -> `IpcErrorCode::PathDenied` (identical to a
  `file_read` denial of the same path — FR-021).
- relative path -> the existing absolute-only invalid-params error.
- path is a file -> teaching error naming the files facade `read` action as
  the remedy.
- missing directory -> the existing typed not-found error shape.

**Behavior contract**: single level; `symlink_metadata` only (symlinks and
reparse points reported by kind, never followed); entries deleted between
enumeration and stat are omitted or returned with `size_bytes`/`mtime_ms`
absent — never a whole-listing error. Audit: dispatch-level
`ipc_file_list_dir` row.

## W2. RegistryDeactivateBulk (NEW pair — US2)

Method name: `registry_deactivate_bulk`. Idempotency: **mutating** (groups
with `RegistryDeactivate`). The existing `RegistryDeactivate`
(single-rule) wire contract is byte-for-byte untouched.

```rust
// IpcRequest::RegistryDeactivateBulk(RegistryDeactivateBulkParams)
pub struct RegistryDeactivateBulkParams {
    /// Selector 1: deactivate every member of this seed pack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<String>,
    /// Selector 2: deactivate these rule ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_ids: Option<Vec<String>>,
    /// Required. ONE scope per call.
    pub scope: ActivationScope,
}
```

**Validation** (daemon, teaching errors):
- exactly one of `pack` / `rule_ids` must be present; zero or both ->
  invalid-params naming both selectors.
- unknown `pack` -> same teaching error `registry_import_pack` gives for an
  unknown pack (lists known pack names).
- membership resolution: embedded seed-pack JSON only
  (`resolve_pack_json`), never id-prefix or tag heuristics.

```rust
// IpcResponse::RegistryDeactivateBulk(RegistryDeactivateBulkResponse)
pub struct RegistryDeactivateBulkResponse {
    /// One entry per requested rule, always, in request order
    /// (pack order = pack file order).
    pub outcomes: Vec<BulkDeactivateOutcome>,
    /// Live jobs rebound ONCE after the loop.
    pub jobs_rebound: u64,
}

pub struct BulkDeactivateOutcome {
    pub rule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,         // version acted on; None for unknown_rule
    pub outcome: BulkOutcomeKind,     // deactivated | not_active | unknown_rule
}
```

**Semantics**: for each rule, all active versions under `scope` are closed
(durable `deactivate_rule_scoped` first, then in-memory
`ActivationRegistry::deactivate` — the established durability-first order);
`not_active` = known rule, nothing open under that scope; `unknown_rule` =
rule id not in the registry (or not in the named pack). Partial success is
the NORMAL shape — the call errors only on validation failures, never
because one member was not active. Audit: one row per deactivated rule
(same shape as single deactivate) — the audit stream stays per-action.

## W3. SubscriptionPullParams + liveness delta (US4)

```rust
pub struct SubscriptionPullParams {
    pub sub_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// NEW: when true, `liveness` in the response carries only entries
    /// whose state changed since the last pull (full snapshot on a
    /// subscription's first pull and after any seek). Default false =
    /// today's full-array behavior, byte-identical.
    #[serde(default)]
    pub liveness_delta: bool,
}
```

`SubscriptionPullResponse` shape is UNCHANGED (`events`, `liveness`,
`lagged`, `truncated`). With `liveness_delta: false` the `liveness` array is
the full snapshot exactly as today. With `true`:
- first pull after open or after any `subscription_seek`: full snapshot;
- subsequent pulls: only changed entries (possibly empty Vec);
- a transition is present in EXACTLY the next pull after it is observable
  (baseline map committed at the same point pull offsets advance).

`subscription_seek` contract addition: seek resets the delta baseline (next
delta pull is a full snapshot). No wire change to seek itself.

## W4. EventContextParams — bucket_id optional (US5)

```rust
pub struct EventContextParams {
    /// NOW OPTIONAL. Supplied: exactly today's single-bucket resolution
    /// (event absent from that bucket = EventNotFound — a contradicting
    /// bucket_id is an error, never silently ignored). Absent: the daemon
    /// resolves the owning bucket by scanning in-scope buckets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_id: Option<BucketId>,
    pub event_id: EventId,
    // before / after / max_bytes unchanged
}
```

`EventContextResponse` unchanged. Not found in ANY bucket (including
evicted) -> `IpcErrorCode::EventNotFound` with a message that does not name
a bucket. Old callers always send `bucket_id` -> decode and behavior are
byte-identical.

## W5. PtyCommandWriteStdin settle (US5)

```rust
pub struct PtyCommandWriteStdinParams {
    pub job_id: String,
    pub bytes: String,
    /// NEW: bucket cursor to read the settle window from (default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// NEW: bounded settle window; daemon-clamped like existing settle
    /// windows (shell_session_exec). Absent = immediate return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}
```

Response gains (all `#[serde(default, skip_serializing_if =
"Option::is_none")]`, present ONLY when `wait_ms` was supplied):
`cursor_in: Option<u64>`, `next_cursor: Option<u64>`,
`has_more: Option<bool>`, `dropped_count: Option<u64>`,
`events: Option<Vec<SignalEvent>>`. Existing fields (`job_id`,
`bytes_written`, `secret_prompt_active`) unchanged; a no-wait response
serializes byte-identically to today. Secret-prompt denial is evaluated
before the write, unchanged by the new fields.

## W6. FileWriteParams — append mode (US6)

```rust
pub struct FileWriteParams {
    // path, content, create_dirs ... unchanged
    /// NEW: append `content` to the target instead of replacing it.
    /// Same policy gate (FileWrite), same MAX_FILE_WRITE_BYTES cap per
    /// call, same missing-file creation semantics.
    #[serde(default)]
    pub append: bool,
}
```

`FileWriteResponse` unchanged (`bytes_written` = bytes appended in append
mode). **Atomicity contract**: an append either fully lands or does not
happen; the original content is never modified. Implementation: open with
append mode + a single `write_all` + `sync_all` (payload capped at
`MAX_FILE_WRITE_BYTES` = 192 KiB); OS append-mode offset atomicity is what
serializes racing appenders (spec edge case: both land, order unspecified).
Domain audit row: same `file_write` action, metadata gains
`"append": true`.

## W7. RegistryImportPackResponse — semantics only (US2)

Shape untouched. New meaning: a rule whose normalized definition equals the
latest stored version of the same id appears in `skipped` and does NOT get a
`failed` entry. Reader's discriminator: `skipped` + `failed` entry =
invalid rule (today's meaning); `skipped` alone = identical/unchanged. The
`empty_failed_is_omitted_from_serialized_response` regression stays green.

---

## Contract test obligations (SC-008)

Each W-item lands with at least one test that FAILS against pre-change
behavior (red -> green, proven by the implementing agent). Suggested names
and homes are listed in [quickstart.md](../quickstart.md). Wire-shape
changes must additionally update the MCP contract fixtures
(`crates/mcp/tests/fixtures/contracts/mcp-tools/*.v1.json` and
`mcp-tool-fixture-map.v1.json`) in the same change.
