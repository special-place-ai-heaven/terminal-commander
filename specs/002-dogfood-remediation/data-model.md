# Data Model: Dogfood Remediation Batch

**Feature**: `specs/002-dogfood-remediation` | **Date**: 2026-07-02

No database migration in this feature. All new state is either in-memory
(per-subscription liveness baseline), derived at runtime (facade action
schemas, pack membership), or additive wire fields. Entities below are the
conceptual units; exact wire shapes live in [contracts/ipc-wire.md](./contracts/ipc-wire.md).

## E1. FacadeActionSchema (US1 — derived, runtime-only)

The validation unit for facade strictness. Derived per facade from the SAME
schemars-generated schema advertised via `tools/list` — never
hand-maintained.

| Field | Type | Source |
|---|---|---|
| `action` | string | variant tag of the facade enum (`facades.rs`) |
| `required` | set of field names | variant schema `required` array |
| `properties` | set of field names | variant schema `properties` keys |
| `aliases` | map alias -> canonical | static allow-list of runtime-only aliases (today exactly one: `samples` -> `sample_lines` on registry `suggest_from_samples`) |
| `counterparts` | map unknown-field -> remedy field | static table (seed: `wait_ms` <-> `timeout_ms` pairs per action) |

**Validation rules**: a call is valid iff its `action` is a known variant,
every `required` field is present, and every present field is in
`properties` or `aliases`. Violations aggregate into ONE error naming the
action, ALL missing fields, and ALL unknown fields (with counterpart/sibling
remedies). Valid calls proceed to today's serde deserialization unchanged.

**State transitions**: none (stateless, recomputed/cached from schema).

## E2. RuleIdentity (US2 — comparison rule, not stored)

Identity of a pack rule against the latest stored version of the same
`rule_id`.

- Basis: full `RuleDefinition` (`crates/core/src/rule.rs:267-307`) compared
  with derived `PartialEq` after normalization.
- **Excluded from identity**: `version` (store always overwrites with
  `latest+1`) and `status` (lifecycle state, mutated by the
  `activate=true` import path and by operator activation).
- **Included**: `id, kind, severity, event_kind, stream, description,
  pattern, keywords, captures, summary_template, tags, rate_limit_per_min,
  redact, context_hint, examples`.
- Outcome: `identical` -> `skipped` (no row created, no `failed` entry);
  `different` or `absent` -> imported as today (new version row).

## E3. BulkDeactivateSelector + Outcome (US2)

| Field | Type | Rule |
|---|---|---|
| `pack` | Option<String> | exactly one selector must be present |
| `rule_ids` | Option<Vec<String>> | exactly one selector must be present |
| `scope` | ActivationScope | required; ONE scope per call (mixing = teaching error) |

Per-rule outcome (response list entry):

| Field | Type | Values |
|---|---|---|
| `rule_id` | String | echoed |
| `version` | Option<u32> | the version acted on (absent for `unknown_rule`) |
| `outcome` | enum | `deactivated` \| `not_active` \| `unknown_rule` |

Partial success is explicit: every requested rule appears exactly once in
`outcomes`. `jobs_rebound` is reported ONCE for the whole call (rebinding
runs after the loop, not per rule). Pack membership resolves from the
embedded seed-pack JSON (`resolve_pack_json`), never from id-prefix or tag
conventions.

## E4. DirectoryEntry (US3)

The discovery unit of the files facade.

| Field | Type | Rule |
|---|---|---|
| `name` | String | entry file name only (no path) |
| `kind` | enum | `file` \| `dir` \| `symlink` — from `symlink_metadata`, NEVER followed |
| `size_bytes` | Option<u64> | files only; omitted otherwise |
| `mtime_ms` | Option<i64> | millis since epoch; omitted when unavailable (stat race) |

**Listing invariants**: single level, no recursion, no globbing; sorted
dirs-first then files, each group lexicographic by `name`; capped at
`MAX_FILE_LIST_ENTRIES` (500, default 200) with `total_entries` +
`truncated` flag; entries that vanish mid-enumeration are omitted or
partial, never a whole-listing error; policy gate and error shape identical
to `file_read` (same `resolve_and_authorize_file` path); listing a file (not
a dir) errors naming the `read` action as remedy.

## E5. LivenessBaseline (US4 — in-memory, per subscription)

New field on `Subscription` (`crates/daemon/src/subscriptions/model.rs`):

```text
last_liveness: HashMap<BucketId, Liveness>
```

**Rules**:
- Empty map = baseline pending -> next pull sends the FULL snapshot.
- On each delta-mode pull: diff full snapshot against map; send only changed
  entries; write full snapshot back at the same commit point where pull
  offsets advance (no transition skippable — a change lands in exactly the
  next pull).
- `sub_seek` clears the map (fresh baseline). A reopened subscription is a
  new `Subscription` (empty map) — baseline naturally.
- Wire opt-in: daemon behavior unchanged unless `liveness_delta: true` is
  set on `SubscriptionPullParams`; the MCP adapter always sets it.

## E6. PtyStdinSettle (US5 — request/response extension)

Mirrors the `shell_session_exec` settle contract onto pty stdin writes.

| Direction | Field | Type | Rule |
|---|---|---|---|
| in | `cursor` | Option<u64> | default 0; bucket cursor to read from |
| in | `wait_ms` | Option<u64> | absent = today's immediate return, byte-identical; present = bounded settle window, daemon-clamped like existing settle windows |
| out | `cursor_in`, `next_cursor` | Option<u64> | present only when `wait_ms` supplied |
| out | `has_more`, `dropped_count` | Option | present only when `wait_ms` supplied |
| out | `events` | Option<Vec<SignalEvent>> | combed signals produced after the write |

Secret-prompt denial fires BEFORE the write and is unchanged by the new
fields.

## E7. NestedShellClassification (US8 — enum, daemon-internal)

```text
WslArgvClass =
  NotWsl                      -- argv[0] is not a wsl carrier; existing path
| Management                  -- wsl management flag, no command payload; runs
| NonShellPayload             -- payload program not in the interpreter list; runs
| NestedShell { interpreter } -- payload is a recognized shell (or empty payload
                              --   = distro default shell); allow_shell governs
| UnknownConstruction         -- unrecognized flag in payload position;
                              --   FAIL CLOSED under allow_shell=false
```

**Enforcement matrix** (argv lane and PTY lane, same classifier):

| Class | allow_shell=false | allow_shell=true |
|---|---|---|
| NotWsl / Management / NonShellPayload | runs as today | runs as today |
| NestedShell | DENY, teaching error (interpreter + carrier + gate) | runs; audit metadata `nested_shell` |
| UnknownConstruction | DENY, teaching error (fail closed) | runs; audit metadata notes unknown construction |

Audit: no schema change — classification rides `metadata_json` and `reason`
on the existing audit rows (allow: command start row; deny:
`command_rejected`).

## E8. Heuristic.stream becomes optional (US7)

`Heuristic.stream: StreamKind` -> `Option<StreamKind>` in
`crates/sifters/src/suggest.rs`. Existing six heuristics keep `Some(...)`
(zero behavior change); the two new entries (`npm-error`, `ts-error`) use
`None` because `sample_lines` carries no stream evidence (FR-050). A
proposal with `stream: None` never triggers `stream_mismatches` in
`registry_test`.

## E9. Wire-type delta summary (details in contracts/ipc-wire.md)

| Type | Change | Compat rule |
|---|---|---|
| `SubscriptionPullParams` | + `liveness_delta: bool` | `#[serde(default)]` — absent = today |
| `EventContextParams` | `bucket_id` -> `Option<BucketId>` | `#[serde(default, skip_serializing_if)]` — supplied = today's path |
| `PtyCommandWriteStdinParams` | + `cursor`, `wait_ms` | default/skip |
| `PtyCommandWriteStdinResponse` | + settle fields (E6) | all Option + skip — no-wait response byte-identical |
| `FileWriteParams` | + `append: bool` | `#[serde(default)]` |
| `IpcRequest`/`IpcResponse` | + `FileListDir`, + `RegistryDeactivateBulk` | new variants (additive) |
| `RegistryImportPackResponse` | none (semantics: identical -> `skipped`, no `failed` entry) | shape untouched |
| protocol constants | + `MAX_FILE_LIST_ENTRIES = 500`, `DEFAULT_FILE_LIST_ENTRIES = 200` | alongside existing file caps |
