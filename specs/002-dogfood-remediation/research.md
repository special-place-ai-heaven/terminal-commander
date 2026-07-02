# Phase 0 Research: Dogfood Remediation Batch

**Feature**: `specs/002-dogfood-remediation` | **Date**: 2026-07-02

**Method**: four parallel read-only code-mapping passes over the live
workspace (MCP facade layer; registry + suggest; file ops + policy + WSL;
IPC pipe + subscriptions + buckets). Every decision below is anchored to
current code with `file:line` references so an implementing agent can jump
straight to the seam.

## Spec correction discovered during research

The US1 example in the spec had the `sub_pull` field names reversed.
Ground truth: `McpSubscriptionPullParams` (`crates/mcp/src/tools.rs:5336`)
carries `sub_id`, `max`, `timeout_ms` — `timeout_ms` IS the honored knob
(forwarded to the daemon, clamped to [1, 8000] ms). `wait_ms` is not a field
of that action and is silently dropped by serde. The dogfood agent passed
`wait_ms` (the knob it knew from `run_and_watch`/`sh_exec`), believed it had
set a 30 s wait, and had set nothing. The spec has been corrected in place;
the remediation contract (reject unknown-for-action fields, name the
counterpart) is unchanged in substance.

---

## D1. Facade strictness (US1, FR-001/002/003)

**Decision**: Add a schema-driven validation pass in the five facade tool
handlers, run BEFORE serde deserialization of the action enum. The facade
tools change their rmcp parameter type from `Parameters<XxxFacadeCall>` to a
raw JSON object; a shared validator resolves the `action` tag, then checks
the call against the SAME schemars-generated schema that `tools/list`
advertises: (a) collect ALL missing required fields from the variant's
`required` array, (b) collect all unknown fields (keys absent from the
variant's `properties`), (c) attach remedies, (d) only then deserialize into
the existing enum and dispatch unchanged.

**Rationale**:
- serde's derived deserializer raises `missing_field` for the FIRST missing
  field only and stops (proven by tests
  `activate_without_scope_fails_deserialization`,
  `crates/mcp/src/tools.rs:6462`). No aggregation exists anywhere.
- `#[serde(deny_unknown_fields)]` is documented-ineffective in combination
  with internally-tagged enums (the `action` tag strips into a `Content`
  buffer before the variant struct deserializes), so unknown-field rejection
  CANNOT be a serde attribute. The only occurrence of that attribute in the
  crate is on `RuleInput` (`tools.rs:3819`), which is not a facade enum.
- Validating against the schemars schema (already produced per-facade; see
  test `command_facade_action_map_is_total`, `tools.rs:6134`) makes the
  validator self-updating: any new action/field added later (including the
  new fields in this very batch — US2..US6) is picked up with zero validator
  edits, and the validator can never drift from the advertised contract.

**Mechanics the implementer must honor**:
- Runtime-only aliases are NOT in the schema and must be allow-listed:
  `samples` -> `sample_lines` on the registry `suggest_from_samples` action
  (deliberate schema asymmetry documented at `tools.rs:4800-4806`,
  `facades.rs:88-91`). `rules_json` IS a schema property (deprecated but
  advertised) and needs no special-casing.
- Counterpart remedies come from a small static table consulted for unknown
  fields; seed entries: `wait_ms` -> `timeout_ms` (on `sub_pull`, `wait`),
  `timeout_ms` -> `wait_ms` (on `run_and_watch`, `sh_exec`, `pty_stdin`).
  Secondary hint: if the unknown field exists on a sibling action of the
  same facade, name that action.
- Unknown `action` value: teaching error listing the valid actions (serde
  already enumerates variants; the validator should preserve that quality).
- Well-formed calls must be byte-identical: validation passes -> the
  existing enum deserialization and dispatch run exactly as today. Lenient
  value coercions (`de_opt_*_lenient`, `de_scope_lenient`, `tools.rs:5616+`
  tests) operate at value level and are unaffected.
- Scope: the five facades only (`command`, `session`, `files`, `registry`,
  `status` — handlers at `tools.rs:2788/2820/2845/2869/2894`). Legacy
  granular tools keep rmcp `Parameters<T>` behavior.

**Alternatives considered**:
- `deny_unknown_fields` on variant structs — rejected: structurally
  ineffective with internally-tagged enums (above).
- Hand-written `Deserialize` impls per param struct — rejected: ~30 structs,
  enormous diff, drifts from schema, violates surgical-change discipline.
- Static hand-maintained required/allowed tables — rejected: drifts from the
  schemars schema; the schema is already in memory and is the single source
  of truth the agent sees via `tools/list`.

## D2. import_pack idempotency (US2, FR-010)

**Decision**: Skip-on-identical implemented in the `import_parsed_pack` loop
(`crates/store/src/import.rs:162-182`), NOT in `create_rule_version`.
For each pack rule: fetch `get_latest_rule(id)`
(`crates/store/src/registry.rs:261`); normalize both sides (set `version` to
a common value, set `status` to a common value); compare with the existing
`PartialEq` on `RuleDefinition` (`crates/core/src/rule.rs:266` derives
`PartialEq, Eq`). Equal -> report in `skipped`, create nothing. Different or
absent -> import as today.

**Rationale**:
- The defect is confirmed: `create_rule_version`
  (`crates/store/src/registry.rs:140`) unconditionally computes
  `latest_version + 1` and inserts; `import_parsed_pack` calls it for every
  valid rule. No content comparison exists at any layer (no hash column, no
  canonical compare).
- The guard belongs in the import loop because `registry_upsert` is MEANT to
  version (immutable-versions contract, test
  `registry_immutable_versions_no_mutation`,
  `crates/store/src/registry.rs:930`). Placing the skip in the import loop
  leaves that store-level contract and its test untouched — the two do not
  conflict.
- `version` must be excluded from identity because the store overwrites it
  with `latest+1` before persisting (`registry.rs:195-197`).
- `status` must be excluded because the `activate=true` import path rewrites
  incoming status to Active (`import.rs:147-151`) and an operator activation
  changes the stored status; a rule whose only difference is lifecycle state
  is the same content. Skipping (rather than minting a Draft v2 over an
  operator-activated rule) is also the safe direction for the spec's
  local-edit edge case: identity compares against the pack's own latest
  stored version content, and a genuinely edited definition WILL differ in
  content fields and import normally.

**Response semantics** (no wire shape change): identical rules land in the
existing `skipped: Vec<String>` and do NOT get a `failed` entry
(`RegistryImportPackResponse`, `crates/ipc/src/protocol.rs:1486`;
`failed` reasons at `:1514`). Discriminator for a reader: `skipped` with a
`failed` entry = invalid; `skipped` without = identical/unchanged. The
existing `empty_failed_is_omitted_from_serialized_response` regression
(`crates/daemon/src/ipc/handlers/registry.rs:849`) stays green.

**Alternatives considered**: content-hash column (schema migration + hash
maintenance for a compare the JSON already answers — rejected as heavier);
guard inside `create_rule_version` (breaks the intended upsert versioning
contract — rejected).

## D3. Bulk / pack deactivate (US2, FR-011)

**Decision**: New IPC pair `IpcRequest::RegistryDeactivateBulk` /
`IpcResponse::RegistryDeactivateBulk`. Params carry EXACTLY ONE selector —
`pack: Option<String>` or `rule_ids: Option<Vec<String>>` — plus a required
`scope: ActivationScope`. Response: `outcomes: Vec<{rule_id, version:
Option<u32>, outcome: deactivated | not_active | unknown_rule}>` plus a
single `jobs_rebound` count. The existing single-rule
`RegistryDeactivate` wire contract is untouched.

On the MCP facade, the existing `deactivate` action gains the two optional
selector fields; the adapter enforces exactly-one-of `{rule_id, rule_ids,
pack}` (a teaching error otherwise), routes `rule_id` to the unchanged
single path and the other two to the bulk request.

**Rationale**:
- Activation identity is the triple `(rule_id, version, scope)`
  (`crates/core/src/activation.rs:41-63`); durable rows in
  `rule_activations` (V0002 + V0004 scope columns); in-memory authority in
  `ActivationRegistry` (`crates/daemon/src/activation.rs:118`). The reusable
  per-rule primitive already exists
  (`handle_registry_deactivate`, `crates/daemon/src/ipc/handlers/registry.rs:404`;
  `deactivate_rule_scoped`, `crates/store/src/registry.rs:503`), and
  `active_versions_for_scope` (`registry.rs:387-402`) answers "which
  versions are active" per rule.
- Pack membership is NOT in the database (no pack column in any migration;
  id prefixes and tags are conventions, not guarantees). The authoritative
  resolver is the embedded seed-pack JSON:
  `resolve_pack_json(name)` (`crates/store/src/import.rs:99`) /
  `SEED_PACKS` (`import.rs:62-95`). Pack-scope deactivate resolves
  membership from there; unknown pack name = the same teaching error
  `import_pack` gives.
- Rebinding live jobs runs ONCE after the loop (the single-rule handler
  rebinds per call, `registry.rs:485-490`; N rebind passes for N rules would
  be waste).
- Per-rule outcomes make partial success explicit (spec scenario 4: two
  deactivated + one named unknown), matching the house pattern
  (`one_failed_activation_yields_partial_success_not_bare_error`,
  handlers/registry.rs:768).

**Alternatives considered**: overloading the existing `RegistryDeactivate`
wire params with optional selectors — rejected (mutating the semantics of an
existing mutating request; a new variant is the additive path). A
pack-membership table — rejected (schema change to answer a question the
embedded JSON already answers; revisit only if user-authored packs land).

## D4. Directory listing (US3, FR-020/021)

**Decision**: New files-facade action `list` -> new IPC pair
`FileListDir` (method name `file_list_dir`). Daemon handler
`handle_file_list_dir` in `crates/daemon/src/ipc/handlers/file.rs` reuses
`resolve_and_authorize_file(state, path, false)`
(`crates/daemon/src/ipc/handlers/common.rs:212`) — the identical
canonicalize-then-`PolicyAction::FileRead` gate `file_read_window` uses,
which satisfies FR-021 (same policy error shape) by construction. Then one
`std::fs::read_dir` pass, entries capped, sorted dirs-first then files, each
group lexicographic by name.

**Constants** (in `crates/ipc/src/protocol.rs:1577-1612` alongside the other
file caps): `MAX_FILE_LIST_ENTRIES = 500`, `DEFAULT_FILE_LIST_ENTRIES = 200`
(mirrors the `MAX/DEFAULT_FILE_SEARCH_MATCHES` pattern; clamp shape
`.unwrap_or(DEFAULT).min(MAX)` as at `file.rs:63-70`).

**Entry shape**: `name` (file name only), `kind` (`file`/`dir`/`symlink` —
from `symlink_metadata`, never followed), `size_bytes` (files only,
omitted otherwise), `mtime_ms` (millis since epoch, omitted when
unavailable). Response carries `total_entries` + `truncated: bool` so a
capped listing is truthful (Constitution III). Races: an entry that
disappears between enumeration and stat is omitted or reported with partial
metadata — never an error for the whole listing. A file (not dir) target
errors with a teaching message naming the `read` action.

**Audit**: dispatch-level `ipc_file_list_dir` row via the existing
`emit_audit` path (`common.rs:16`) — consistent with read/search, which have
no domain-level audit row.

**Alternatives considered**: recursion/globbing — rejected (spec pins
single-level; deeper discovery composes by repeated calls); piggybacking on
`file_search` — rejected (different contract, search requires a query and
scans content).

## D5. Compact projection on wait/events (US4, FR-030)

**Decision**: MCP-adapter-side only; ZERO daemon change. Add
`compact: bool` (`#[serde(default)]`) to `McpBucketWaitParams`
(`crates/mcp/src/tools.rs:4509`) and `McpBucketEventsSinceParams`
(`tools.rs:4472`); in `bucket_wait` (`tools.rs:1629`) and
`bucket_events_since` (`tools.rs:1612`) map events through the existing
`project_signal_compact` (`tools.rs:3254-3261` — exact field set
`{summary, stream, seq, severity}`) and echo `"compact": true` in the
payload, exactly as `run_and_watch_result` does (`tools.rs:3312-3321`).

**Rationale**: the daemon already returns full `SignalEvent` records
(`BucketWaitResponse`/`BucketEventsSinceResponse`,
`crates/ipc/src/protocol.rs:970/932`); the one existing compaction lives on
the adapter with the explicit doc "PRESENTATION ONLY: the event store
retains the full record". Reusing the same helper guarantees the spec's
"exactly the field set run_and_watch established" and keeps full records
re-fetchable by cursor with `compact` omitted. Filters compose untouched
(`compact` changes projection only, never matching).

## D6. Liveness delta on sub_pull (US4, FR-031)

**Decision**: Daemon-side, wire-opt-in. Add
`liveness_delta: bool` (`#[serde(default)]`) to `SubscriptionPullParams`
(`crates/ipc/src/protocol.rs:2491`). Add
`last_liveness: HashMap<BucketId, Liveness>` to `Subscription`
(`crates/daemon/src/subscriptions/model.rs:189-212`). In the pull path,
`liveness_for` (`crates/daemon/src/subscriptions/pull.rs:422-458`) still
computes the full snapshot; when the flag is set, the handler diffs against
`last_liveness`, sends only changed entries (all entries when the map is
empty = first-pull baseline), and writes the full snapshot back through
`with_sub_mut` (`registry.rs:130-137`) at the same commit point where
offsets advance. `handle_subscription_seek`
(`crates/daemon/src/ipc/handlers/subscription.rs:224-257`) additionally
clears the map, so a seek forces a fresh baseline (spec edge case); a
reopened subscription is a new `Subscription` whose empty map yields the
baseline naturally.

The MCP adapter's `sub_pull` (`crates/mcp/src/tools.rs:2639`) always sets
`liveness_delta: true` — the agent-facing behavior change IS the feature
(SC-004) — and omits the liveness section from its payload when the delta
is empty.

**Rationale**:
- Confirmed: liveness is rebuilt and sent in full on every pull return path
  (`pull.rs:554/567/576`), one entry per in-scope bucket, no diffing
  anywhere; `SubscriptionPullResponse.liveness` is a plain Vec.
- The wire flag (default false) keeps the daemon response byte-identical
  for any non-adapter client and for old adapters against a new daemon —
  the additive-evolution rule. The behavior change rides only in the
  adapter, whose payload contract this spec explicitly evolves.
- "No transition skippable": the stored map is updated at the same
  commit point as pull offsets, so a transition not sent in this pull
  (because it happened after the diff) is guaranteed present in the next.
- Wire posture per house rules: new param field
  `#[serde(default, skip_serializing_if = "...")]`; `Liveness` already
  implements `Default` (`protocol.rs:2160-2167`).

**Alternatives considered**: delta-by-default on the wire — rejected
(changes an existing response's meaning for existing callers; violates the
additive-evolution assumption). A per-entry `changed` marker field —
rejected (still sends the full array; saves nothing).

## D7. event_context by event_id alone (US5, FR-040)

**Decision**: `EventContextParams.bucket_id` becomes
`Option<BucketId>` (`#[serde(default, skip_serializing_if)]`,
`crates/ipc/src/protocol.rs:1017-1033`). In `handle_event_context`
(`crates/daemon/src/ipc/handlers/bucket.rs:103-241`):
- `bucket_id` supplied -> exactly today's single-bucket page scan
  (`bucket.rs:112-133`) — byte-identical, including `EventNotFound` when the
  event is not in that bucket (which IS the spec's "contradicting bucket_id
  is an error").
- `bucket_id` absent -> iterate `state.buckets` bucket ids and run the same
  bounded page scan per bucket until the event is found; not found anywhere
  (including evicted) -> the same honest `EventNotFound` teaching error,
  phrased without a bucket name.

MCP side: `McpEventContextParams.bucket_id` -> `Option<String>`
(`crates/mcp/src/tools.rs:4558-4574`, `into_ipc` at `:4576`).

**Rationale**: `EventId` is UUIDv7-backed, globally unique
(`crates/core/src/ids.rs:64-87`) — one event can live in at most one
bucket. There is NO global event-id index; buckets are bounded rings with
eviction (`crates/core/src/bucket.rs:308-355`), so the all-bucket scan is
bounded by (bucket count x ring cap) using only existing machinery. An
`EventId -> BucketId` side-table was rejected: it must hook every append
AND both eviction paths to stay truthful, a standing correctness liability
to optimize a per-call convenience lookup; add only if profiling ever shows
the scan hurting.

## D8. pty_stdin bounded wait (US5, FR-041)

**Decision**: Mirror the `shell_session_exec` shape
(`crates/mcp/src/tools.rs:2365-2402`, params `:5172-5188`) onto
`pty_command_write_stdin`. Additive fields:
- Wire params `PtyCommandWriteStdinParams`: `cursor: Option<u64>`,
  `wait_ms: Option<u64>` (both `#[serde(default, skip_serializing_if)]`).
- Wire response: optional `cursor_in`, `next_cursor`, `has_more`,
  `dropped_count`, `events` — present only when `wait_ms` was supplied
  (`skip_serializing_if` keeps the no-wait response byte-identical).
- Daemon handler: perform the stdin write exactly as today (secret-prompt
  denial fires BEFORE the write and is untouched); when `wait_ms` is
  present, run the same bounded settle-window read over the PTY job's
  bucket that `ShellSessionExec` uses, from `cursor` (default 0), with the
  same daemon-side clamp.
- MCP `McpPtyCommandWriteStdinParams` (`tools.rs:5118-5124`) gains the same
  two optional fields; the handler (`tools.rs:2240-2268`) returns the combed
  batch fields when present.

**Rationale**: `pty_command_write_stdin` is fire-and-forget today (returns
`{job_id, bytes_written, secret_prompt_active}` only); every REPL
interaction costs a second `wait` call. The settle-window pattern is
established, daemon-clamped, and bounded (Constitution III). Omitted
`wait_ms` = today's behavior, field-for-field.

## D9. npm/TS suggest heuristics (US7, FR-050)

**Decision**: Two new entries in the ordered `heuristics()` table
(`crates/sifters/src/suggest.rs:103-179`), inserted after `coded-error` and
BEFORE `error-prefix` (specific before generic):
- `npm-error`: detector `^npm ERR! (?P<message>.+)$`, event_kind error-class,
  High severity.
- `ts-error`: detector `^error TS(?P<code>[0-9]+): (?P<message>.+)$`,
  event_kind `compile_error`.

Plus one structural change: `Heuristic.stream` becomes
`Option<StreamKind>`; the six existing heuristics keep their current
`Some(...)` values (zero behavior change), the two new entries use `None`.

**Rationale**:
- The table is the single extension point; `build_proposal`
  (`suggest.rs:262-286`) turns any entry into a Draft proposal
  automatically. One proposal per heuristic; bounds: table is
  priority-ordered with early break at `DEFAULT_MAX_PROPOSED_RULES = 8`
  (`suggest.rs:48/222-234`); patterns must pass `compile_bounded_regex`
  (guarded by `proposed_patterns_compile_under_bounded_regex`,
  `suggest.rs:520`).
- `stream: None` on the new entries is forced by FR-050's "no stream filter
  without stream evidence": `suggest_from_samples` input is
  `sample_lines: Vec<String>` — plain strings, NO stream evidence exists
  (npm ERR! goes to stderr but tsc writes diagnostics to STDOUT; guessing
  would produce `stream_mismatches` failures in `registry_test`,
  handlers/registry.rs:132/176-184). Existing heuristics keep their
  hardcoded streams — changing them would alter shipped behavior outside
  this spec's scope.
- Never-activate/never-persist is structural (the handler takes no `&state`,
  `crates/daemon/src/ipc/handlers/registry.rs:543`) and is untouched.

## D10. WSL nested-shell gate (US8, FR-060/061)

**Decision**: One new classifier function in `crates/daemon/src/command.rs`
next to `shell_interpreter_basename` (`command.rs:500-522`), taking the FULL
argv: `classify_wsl_nested_shell(argv: &[String]) -> WslArgvClass` with
variants `NotWsl`, `Management`, `NonShellPayload`,
`NestedShell { interpreter }`, `UnknownConstruction`. Both argv-lane call
sites route through it: the command guard block (`command.rs:732-746`) and
the PTY lane (`crates/daemon/src/pty_command.rs:267`).

Classification rules (argv-only, file contents never inspected):
- argv[0] basename matches `wsl` / `wsl.exe` (case-insensitive, bare name or
  any path to it) -> WSL carrier; otherwise `NotWsl` (existing argv[0]
  check unchanged).
- Recognized management flags with no command payload (`--list`, `--status`,
  `--version`, `--help`, `--shutdown`, `--terminate`, `--set-default`,
  `--set-version`, `--export`, `--import`, `--mount`, `--unmount`,
  `--update`, `--install`, `--manage`) -> `Management` (runs as today).
- Payload introducers and selectors are skipped to find the payload:
  `-d`/`--distribution <name>`, `-u`/`--user <name>`, `--cd <dir>`,
  `--system`, `-e`/`--exec`, `--`. First payload token's basename is matched
  against the existing `SHELL_INTERPRETERS_DENY` list (`command.rs:80-97`)
  extended semantically to Unix-side spellings it already contains
  (`sh`, `bash`, `dash`, `zsh`, `fish`, `ksh`, `csh`, `tcsh`, `ash`,
  `busybox`) -> `NestedShell` or `NonShellPayload`.
- EMPTY payload (bare `wsl.exe`, or selectors with no command) launches the
  distro's default shell -> classified `NestedShell` (interpreter
  "default shell").
- Any unrecognized flag in payload position -> `UnknownConstruction`.

Enforcement: `NestedShell` and `UnknownConstruction` are denied when
`allow_shell=false` (`caps_allow_shell()`,
`crates/daemon/src/policy.rs:452`) with a teaching error naming the nested
interpreter, the `wsl` carrier, and the `allow_shell` gate — reusing wire
code `IpcErrorCode::ShellInterpreterDenied` with an extended message (the
existing mapping lives at `handlers/common.rs:98-106`). Under
`allow_shell=true` they run, and the audit row carries the classification in
`metadata_json` (e.g. `"nested_shell": "bash"`) — there is no typed audit
column (`AuditEntry`, `crates/store/src/audit.rs:54-62`), so metadata +
reason text is the house mechanism. Denials emit the existing
`command_rejected` audit row as today (`command.rs:735-744`).

**Documentation (FR-061)**: the stance — argv-only inspection, the
fail-closed rule for unknown constructions, and the rationale that the shell
capability follows the shell across the WSL boundary (WSL is this host's
boundary, not a remote machine) — is added to the policy documentation
alongside the existing `SHELL_INTERPRETERS_DENY` description
(`docs/security/POLICY.md`).

**Rationale**: confirmed gap — `shell_interpreter_basename` receives ONLY
`argv[0]` and `wsl`/`wsl.exe` appears in neither `SHELL_INTERPRETERS_DENY`
nor `COMMANDS_DENY` (`policy.rs:166`), so `wsl.exe -e bash -lc <arbitrary>`
passes both checks. The constitution (Principle II: no argv smuggling) makes
the default non-negotiable; the spec settles the stance. Reusing the
existing interpreter list and error code keeps the teaching-error contract
coherent for agents that already learned it.

**Alternatives considered**: adding `wsl.exe` to `SHELL_INTERPRETERS_DENY`
wholesale — rejected (breaks all legitimate non-shell WSL usage:
`wsl.exe -e cargo build`, `wsl --list`; the dogfood transcript depends on
these). Inspecting the Linux-side binary — rejected (argv-only is the
spec'd and constitutional boundary; file inspection is unreliable across
the WSL boundary anyway).

## D11. Pipe instance pool (US9, FR-070, optional)

**Decision**: If implemented: restructure the Windows accept loop
(`accept_loop`, `crates/daemon/src/ipc/pipe_server.rs:170-306`) from
one-pending-instance-then-recreate to a small fixed pool (N = 4) of
concurrently pending `NamedPipeServer` instances (a `JoinSet` of accept
futures; `first_pipe_instance(true)` only on the very first instance ever
created; each accepted instance is replaced immediately so the pending
count stays at N). Per-connection behavior is untouched — peer identity is
resolved per accepted connection inside `handle_pipe_connection`
(`pipe_server.rs:308-365`), and the shutdown path must additionally cancel
the N idle pending accepts (the existing `tokio::select!` on
`shutdown.changed()` per accept arm covers this), then drain in-flight
handlers under the existing `PIPE_DRAIN_CEILING`.

This mirrors the Unix UDS loop, which already accepts unboundedly
(`crates/daemon/src/ipc/server.rs:153-222`) — the change makes the two
transports behaviorally symmetric. The 0.1.72 client retry
(`crates/ipc/src/pipe_client.rs:124-153`, ERROR_PIPE_BUSY +
ERROR_FILE_NOT_FOUND, 50 x 20 ms) stays as the backstop.

**Skip path**: US9 is explicitly optional. Skipping with a written rationale
in the implementation report is a compliant outcome; the accept path is
load-bearing for everything else and must not be destabilized.

## Verification environment facts (for the gate)

- Gate commands (Constitution VI): `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo nextest run --workspace`.
- The Linux gate MUST be run in WSL before push (session-established
  practice: `CARGO_TARGET_DIR=$HOME/tc-linux-target`, `~/.cargo/bin/cargo`
  directly — NOT the `~/.aap-bin` flock wrapper). cfg-gated code (US8 has
  `#[cfg(windows)]` surface, US9 is Windows-only) has burned two clippy gate
  failures this cycle; verify both platforms before push.
- Test placement conventions: cross-platform behavior (typed errors, caps,
  policy denial) ungated; POSIX-path/symlink proofs `#[cfg(unix)]`;
  Windows pipe tests in `crates/daemon/tests/pipe_*.rs` with
  `unique_pipe_name(tag)`; MCP e2e `#[tokio::test(flavor = "multi_thread",
  worker_threads = 2)]` via the `paired_against_live_daemon` harness.
- Contract fixtures (`crates/mcp/tests/fixtures/contracts/mcp-tools/*.v1.json`
  + `mcp-tool-fixture-map.v1.json`) must be updated in the same change as
  any tool schema change; the MCP tool COUNT does not change (facade
  actions are not tools), so the catalogue count anchors are unaffected —
  but the `system_discover` fixture enumerates facades/actions and must be
  checked.
