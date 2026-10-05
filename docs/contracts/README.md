# Contract Schemas - Terminal Commander

Status: MVP draft (TC05 wave-0 deliverable), updated as later TC
phases land.
Scope: golden JSON examples + naming + versioning rules. Live MCP
fixtures and Rust contracts are now tracked by the fixture map.

Language: ASCII only.

## 1. Purpose

This directory documents the over-the-wire shapes Terminal Commander
will produce and consume:

- signal events stored in buckets,
- bucket cursor reads,
- rule definitions persisted in the registry,
- probe descriptors,
- job status,
- source pointers and event-context windows,
- policy decisions and audit records,
- MCP tool request and response payloads.

Every fixture is a small, deterministic example. Tests reference
these as the golden truth for what the wire format MUST look like.

## 2. Layout

```text
docs/contracts/
  README.md                            # this file
  enums/                               # enum domain documents (no JSON)
    severity.md
    event-kind.md
    sifter-type.md
    probe-kind.md
    policy-decision.md
    audit-action.md

tests/fixtures/
  contracts/                           # versioned wire-shape examples
    event.signal.v1.json
    rule-definition.v1.json
    forbidden/                         # negative examples
      missing-pointer.v1.json
    mcp-tool-fixture-map.v1.json       # live/obsolete MCP fixture catalogue
    mcp-tools/                         # one file per live MCP tool fixture
      system_discover.v1.json
      health.v1.json
      policy_status.v1.json
      self_check.v1.json
      command_start_combed.v1.json
      command_status.v1.json
      bucket_events_since.v1.json
      bucket_wait.v1.json
      bucket_summary.v1.json
      event_context.v1.json
      registry_search.v1.json
      registry_get.v1.json
      registry_upsert.v1.json
      registry_test.v1.json
      registry_activate.v1.json
      registry_deactivate.v1.json
      registry_list_active.v1.json
      file_read_window.v1.json
      file_search.v1.json
      file_watch_start.v1.json
      file_watch_stop.v1.json
      file_watch_list.v1.json
      pty_command_start.v1.json
      pty_command_write_stdin.v1.json
      pty_command_stop.v1.json
      pty_command_list.v1.json
      runtime_state.v1.json
      probe_list.v1.json
      probe_status.v1.json
```

The `mcp-tools/` list above is the TC05 set; the directory now holds one
file per live tool (`mcp-tool-fixture-map.v1.json` is the inventory).

Each fixture starts with a `_meta` field describing it (fixture id,
schema anchor, status). The `_meta` field is part of the FIXTURE
shape only; live wire payloads do not carry it.

## 3. Naming and versioning

### 3.1 File naming

`<contract>[.<sub-kind>].v<N>.json`

- `<contract>` is the bare contract name (`event`, `bucket-read-
  response`, `rule-definition`, ...).
- `<sub-kind>` is optional and used to disambiguate variants in a
  single contract family (e.g. `event.signal.v1.json` vs a future
  `event.heartbeat.v1.json`).
- `<N>` is a non-negative integer. There is no `v0`.

### 3.2 Versioning rule: SemVer-strict

Locked 2026-05-21 by operator at TC05.

A fixture file `v<N>` bumps to `v<N+1>` when ANY of the following
changes happen to its schema:

- a required field is added,
- a required field is removed,
- a required field changes type,
- an enum domain loses a previously valid value,
- the meaning of a field changes such that an old consumer cannot
  use it correctly.

Additive optional fields stay in `v<N>`. If a consumer can ignore the
new field and still get correct behavior, it is additive. If not,
it is breaking and forces a bump.

When `v1` and `v2` coexist, BOTH files live in this directory until
the transition is complete; the `_meta.status` field on the older
file flips to `superseded-by:v2`.

### 3.3 Schema anchors

Every fixture references a schema anchor in `_meta.schema_anchor`,
of the form `TC<NN>/<contract>-v<N>`. Example: `TC05/event-v1`. The
anchor is what code-level (TC06+) JSON-Schema files will be tagged
with.

## 4. Required and optional fields

For each contract, the required vs optional fields are documented
inline in the fixture (via the `_meta.required` and `_meta.optional`
arrays). The convention:

- A field listed in `_meta.required` MUST be present in every valid
  wire payload of this contract version.
- A field listed in `_meta.optional` MAY be omitted by a producer
  and MUST be tolerated by a consumer.
- A field NOT listed in either is "ignored if present" - producers
  MUST NOT emit it; consumers MUST NOT fail if a non-conformant
  producer includes it. Strict-mode consumers MAY warn.

## 5. Forbidden shapes (negative contract examples)

`tests/fixtures/contracts/forbidden/` holds payloads that look
plausible but MUST NEVER be produced by Terminal Commander. They
exist to drive negative tests in TC23/TC29.

Current entries:

- `missing-pointer.v1.json`: an event with `severity >= medium` that
  has neither a `pointer` field nor a `pointer_unavailable_reason`
  explanation. Forbidden per the TC02 invariant (every signal event
  preserves a bounded source pointer or explains why no pointer can
  exist).

## 6. Enum domains

Per the contract requirements in TC05 the following enums are
canonical. Each domain is reserved at MVP-draft status; concrete
behavior lands in the goal noted.

### 6.1 Severity (7 values)

`trace`, `debug`, `info`, `low`, `medium`, `high`, `critical`. Documented
in `enums/severity.md`; the `Severity` enum is in
`crates/core/src/severity.rs`.

### 6.2 Sifter type (11 discriminators)

Per TC05 contract: `keyword`, `regex`, `prompt`, `exit_code`,
`stream_marker`, `progress_collapse`, `dedupe`, `threshold`,
`sequence`, `anchor`, `custom`.

Note: this list is the CANONICAL enum domain for the rule contract
schema. README.md sections "Planned sifter types" (lines 136-148)
list user-facing sifter NAMES, not discriminators (e.g. "numeric
condition" is realized as `threshold`; "multiline block" is realized
as `sequence`; "stall detector" is realized as a `threshold` variant;
"suppression rule" is realized as a `dedupe` variant; "correlation
rule" is realized as `sequence`; "artifact parser" is realized as
`custom`). The mapping is documented in `enums/sifter-type.md`.
Fixtures MAY stub unimplemented discriminators with status
`reserved-not-implemented`.

### 6.3 Event kind

Open string, recommended canonical set listed in `enums/event-kind.md`:
`command_started`, `command_failed`, `command_exited`,
`missing_package`, `compile_error`, `test_failed`,
`permission_denied`, `password_prompt`, `stalled`, `file_changed`,
`artifact_generated`, `repeated_collapsed`, `threshold_crossed`,
`prompt_detected`, `policy_denied`. Producers MAY emit new kinds;
consumers MUST treat unknown kinds as data (display + log), not as
errors.

### 6.4 Probe kind

`process`, `terminal`, `file`, `directory`, `journal`, `artifact` are the
values of an event's `source.source_type` (`SourceType` in
`crates/core/src/source.rs`). The runtime probe kinds listed by
`probe_list` and used by `[policy.probes]` are different: `command`,
`file_watch`, `pty` (`ProbeKind` in `crates/ipc/src/protocol.rs`). No
directory, journal or artifact probe ships.

### 6.5 Policy decision

`allow`, `deny`, `allow_with_audit`, `error` (`PolicyDecision` in
`crates/daemon/src/policy.rs`). Audit rows additionally accept `info`
(`ALLOWED_AUDIT_DECISIONS` in `crates/store/src/audit.rs`).

### 6.6 Audit action

Listed in `enums/audit-action.md`. The store enforces a closed set only
for the decision string; action strings are free text, and the daemon
emits more than that file lists (see its status note).

## 7. MCP tool fixtures

The live MCP tool surface is `terminal_commander_mcp::tools::tool_catalogue()`.
`tests/fixtures/contracts/mcp-tools/system_discover.v1.json` mirrors that live
catalogue. `tests/fixtures/contracts/mcp-tool-fixture-map.v1.json` is the
authoritative live/obsolete fixture inventory and drift boundary.

The map classifies each live tool as:

- `covered_live`: current live per-tool fixture exists.
- `placeholder_for_live_tool`: a live tool still has an old reserved fixture.
- `missing_fixture`: a live tool has no per-tool fixture yet.

If obsolete fixture files reappear, the map lists them as
`obsolete_fixture_present` entries whose tool names are no longer in the live
catalogue. Obsolete entries are explicit debt, not supported tools. The fixture
map, not `_meta.status` alone, is the source of truth for live-vs-obsolete
classification.

A live catalogue change, a `system_discover` fixture change, or a fixture-file
addition/removal MUST update the fixture map in the same patch. The
`fixture_catalogue_contract` test enforces this drift boundary.

## 8. The bucket_wait heartbeat contract

`tests/fixtures/contracts/mcp-tools/bucket_wait.v1.json` documents
the contract that prevents raw-stream leakage. Per the TC05
mini-spec:

- The response carries EITHER a non-empty `events` array OR a
  `heartbeat = true` flag.
- When `heartbeat = true`, the `events` array MUST be empty and
  `next_cursor` MUST equal the input cursor (no progress beyond
  what was already known).
- Raw stdout/stderr text is NEVER a `bucket_wait` success shape. The
  response type carries `Vec<SignalEvent>`, not text; no fixture
  rejects a raw-text payload.

## 9. Source-status

| Fixture set | Status until consumed |
|---|---|
| event.signal, rule-definition | live shape (match `SignalEvent`, `RuleDefinition`) |
| mcp-tool-fixture-map.v1.json | live authoritative live/obsolete inventory |
| mcp-tools/* classified by the map as `covered_live` | current per-tool contract |
| map entries with `placeholder_for_live_tool` or `missing_fixture` | live-tool coverage debt, blocker before use |
| mcp-tools/* classified by the map as `obsolete_fixture_present` | obsolete debt, not supported tools |
| forbidden/missing-pointer | live (negative-test oracle for the pointer-or-reason invariant) |

## 9a. Where the live response shapes are pinned (checked 2026-10-05)

The old top-level drafts for bucket reads, bucket summary, source pointer,
event context, job status, probe descriptor, registry search, policy
decision and audit record were deleted: nothing consumed them and they had
drifted from the code. The shapes are pinned by the per-tool fixtures in
`tests/fixtures/contracts/mcp-tools/`:

| Shape | Fixture | Rust type |
|---|---|---|
| bucket read | `bucket_events_since.v1.json`, `bucket_wait.v1.json` | `BucketEventsSinceResponse`, `BucketWaitResponse` |
| bucket summary | `bucket_summary.v1.json` | `BucketSummaryResponse` (7 severity counters) |
| event context | `event_context.v1.json` | `EventContextResponse` (`anchor_missing`, `unavailable_reason`, `frames`) |
| job status | `command_status.v1.json` | `CommandStatusResponse` |
| probe listing | `probe_list.v1.json`, `probe_status.v1.json` | `ProbeListEntry` (`kind`: `command`, `file_watch`, `pty`) |
| registry search | `registry_search.v1.json` | `RegistrySearchResponse { hits }` |
| audit rows | `audit_since.v1.json` | `AuditRow` fields (`audit_id` integer, `profile`, `metadata_json`) |

All Rust types are in `crates/ipc/src/protocol.rs`. A source pointer is
`SourcePointer` in `crates/core/src/pointer.rs`. There is no policy-decision
wire type. The two kept top-level fixtures match the code:
`event.signal.v1.json` (`SignalEvent`, `crates/core/src/event.rs`, which
adds optional `count`, `first_seen`, `last_seen`, `suppressed`) and
`rule-definition.v1.json` (`RuleDefinition`, `crates/core/src/rule.rs`).
`SignalEvent::validate` enforces the pointer-or-reason invariant that
`tests/fixtures/contracts/forbidden/missing-pointer.v1.json` illustrates.

`CommandStatusResponse`, `CommandReceipt` and the discovery payloads are
being extended in the working tree (receipt `head`/`lines_omitted`,
`elapsed_ms`, `last_output_age_ms`); the per-tool fixtures are the
reference for them.

## 10. Verification

TC05 verification: every `*.json` under `docs/contracts/` and
`tests/fixtures/contracts/` parses with `python3 -m json.tool`.
`scripts/dev/verify-baseline.sh` already enforces this for the
whole `tests/fixtures/` tree.
