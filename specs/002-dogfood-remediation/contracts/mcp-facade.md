# Contract: MCP Facade Deltas

**Layer**: `crates/mcp` — the five facade tools (`command`, `session`,
`files`, `registry`, `status`) and their flat action schemas. The MCP tool
COUNT does not change: everything below is actions/fields on existing
facade tools, plus behavior. Legacy granular tools keep their current rmcp
`Parameters<T>` behavior and are NOT covered by the strictness contract.

## F1. Strict parameter validation (US1 — all five facades)

**Pipeline change**: each facade handler validates the raw call object
BEFORE deserializing the action enum. The validator's source of truth is
the SAME schemars-generated facade schema served by `tools/list` — required
fields from the variant's `required` array, allowed fields from its
`properties` keys. It is therefore self-updating for every field/action
added elsewhere in this batch.

**Error contract** (single `invalid_params` response, one per failed call):

| Condition | Error must contain |
|---|---|
| Unknown/missing `action` | the invalid value and the full list of valid actions for that facade |
| Missing required fields | the action name + EVERY missing required field (not just the first) |
| Unknown-for-action fields | the action name + EVERY unknown field by name |
| Unknown field with a known counterpart | the counterpart named as the remedy (static table; seed pairs: `wait_ms` -> `timeout_ms` on `sub_pull` and `wait`; `timeout_ms` -> `wait_ms` on `run_and_watch`, `sh_exec`, `pty_stdin`) |
| Unknown field that exists on a sibling action | that sibling action named as a hint |

Missing + unknown violations aggregate into the SAME single error.

**Alias allow-list** (runtime-only aliases absent from the schema — the
validator must accept them, and this list is the contract):
- `samples` -> `sample_lines` on registry action `suggest_from_samples`
  (deliberate schema asymmetry; schema advertises only `sample_lines`).

`rules_json` is an advertised (deprecated) schema property — no
special-casing; it keeps working wherever it works today (FR-003).

**Zero-cost guarantee** (FR-001..003 scenario 3): a call that passes
validation is deserialized and dispatched EXACTLY as today — byte-identical
behavior, including all lenient value coercions (`de_opt_*_lenient`,
`de_scope_lenient`) which operate on known fields' values, not field names.

**Canonical strictness examples** (from the dogfood evidence):

```jsonc
// registry: one error, ALL missing fields
{"action": "deactivate", "rule_id": "cargo.compile-error"}
// -> invalid_params: action 'deactivate' is missing required field(s): scope.
//    (version is optional; omitted = latest)

// command: unknown-for-action field with counterpart remedy
{"action": "sub_pull", "sub_id": "sub_...", "wait_ms": 30000}
// -> invalid_params: action 'sub_pull' does not accept field(s): wait_ms.
//    Did you mean timeout_ms (the bounded-wait field for sub_pull)?
```

## F2. `files` facade — new `list` action (US3)

New variant `FilesFacadeCall::List(McpFileListDirParams)`:

```jsonc
{"action": "list", "path": "E:/project/foo", "max_entries": 200}
```

Params: `path: String` (required, absolute), `max_entries: Option<u32>`.
Forwards 1:1 to `IpcRequest::FileListDir`. Payload mirrors the wire
response: `{path, entries: [{name, kind, size_bytes?, mtime_ms?}],
total_entries, truncated}`. Policy denial is the SAME error shape as
`read` of the same path. Listing a file errors naming `read` as the remedy.

## F3. `registry` facade — `deactivate` gains bulk selectors (US2)

The `deactivate` action accepts EXACTLY ONE of `{rule_id, rule_ids, pack}`
plus the (already required) `scope`:

```jsonc
// pack-level, one call:
{"action": "deactivate", "pack": "cargo", "scope": {"kind": "global"}}
// multi-rule:
{"action": "deactivate", "rule_ids": ["a.b", "c.d"], "scope": {"kind": "global"}}
// single-rule: unchanged, byte-identical path
{"action": "deactivate", "rule_id": "a.b", "version": 3, "scope": {"kind": "global"}}
```

Adapter routing: `rule_id` -> existing `RegistryDeactivate` (unchanged);
`rule_ids`/`pack` -> `RegistryDeactivateBulk`. Zero or multiple selectors ->
strictness-style teaching error naming all three selectors. `version` is
only meaningful with `rule_id` (unknown-for-selector otherwise — teaching
error). Bulk payload surfaces `outcomes` per rule + `jobs_rebound`.

## F4. `command` facade — `wait` / `events` gain `compact` (US4)

`McpBucketWaitParams` and `McpBucketEventsSinceParams` gain
`compact: bool` (default false). When true, each returned signal is
projected through the EXISTING `project_signal_compact` — exactly
`{summary, stream, seq, severity}` — and the payload echoes
`"compact": true`. Full records remain fetchable by re-issuing the same
cursor read with `compact` omitted. `compact` never changes which events
match (filters compose untouched).

## F5. `command` facade — `sub_pull` liveness delta (US4)

The adapter ALWAYS sets `liveness_delta: true` on the wire request. Payload
rule: the `liveness` section is included only when non-empty (first pull
after open/seek = full snapshot; steady idle state = omitted). This is the
agent-facing behavior change SC-004 measures. No new facade field.

## F6. `command` facade — `event_context` by `event_id` alone (US5)

`McpEventContextParams.bucket_id` becomes optional:

```jsonc
{"action": "event_context", "event_id": "evt_0198..."}
```

Supplied `bucket_id` = today's exact behavior (including EventNotFound when
the event is not in that bucket). Absent = daemon resolves the owning
bucket. Unknown event errors identically in both addressing modes.

## F7. `session` facade — `pty_stdin` bounded wait (US5)

`McpPtyCommandWriteStdinParams` gains `cursor: Option<u64>`,
`wait_ms: Option<u64>`:

```jsonc
{"action": "pty_stdin", "job_id": "job_...", "bytes": "1+1\n", "wait_ms": 3000}
```

With `wait_ms`: payload adds the combed batch
(`cursor_in, next_cursor, has_more, dropped_count, events`) — same shape
family as `sh_exec`. Without: today's payload, byte-identical. Secret-prompt
denial unchanged.

## F8. `files` facade — `write` gains `append` (US6)

`McpFileWriteParams` gains `append: bool` (default false):

```jsonc
{"action": "write", "path": "E:/notes/log.md", "content": "one line\n", "append": true}
```

Same policy gate, same size cap, `bytes_written` = bytes appended.

## F9. Fixture and description obligations

Any change above that alters a tool schema or payload MUST, in the same
change:
- update the versioned contract fixtures under
  `crates/mcp/tests/fixtures/contracts/mcp-tools/` (at minimum:
  `registry_deactivate.v1.json`, `registry_import_pack.v1.json`, the
  bucket wait/events fixtures, pty stdin, subscription pull, event_context;
  NEW fixture for files `list`) plus `mcp-tool-fixture-map.v1.json`;
- keep the facade description constants in sync (guarded by
  `facade_consts_match_tool_attribute_descriptions`);
- keep the `system_discover` payload/fixture consistent with the new
  actions/fields;
- leave the tool COUNT anchors untouched (no tool added or removed).
