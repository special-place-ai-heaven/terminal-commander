# Embedded engine parity

`terminal_commanderd::embedded::EmbeddedEngine` exposes all current engine
operations using public typed request/response structs. `execute(IpcRequest)` is
the exhaustive operation union; `execute_envelope` is the transport-independent
Serde service boundary. Both reach the same daemon dispatcher. Named methods below
use those same paths; there is no second execution engine or MCP-name router.

| Family | Typed operations | Shared behavior and qualifications |
|---|---|---|
| Identity and discovery | `identity`, `capabilities`, `health`, `system_discover`, `self_check`, `policy_status` | Policy-filtered discovery, API/schema/build/boot identity; health and discovery agree |
| Commands and limits | `command_start_combed`, `command_start_isolated`, `command_status`, `command_stop`, `command_output_tail` | Common policy, resource governor, job lifecycle, bounded receipt, raw observations and CPU; isolated starts opt into cleared env |
| Compound observation | `run_and_watch` | Shared IPC-library orchestration also used by MCP; bounded wall-clock wait, signal cap, resumable IDs/cursor and explicit degradation; no automatic replay |
| Shell execution | `shell_exec` | Existing shell inference and shell policy, same command runtime |
| PTY | `pty_command_start`, `pty_command_write_stdin`, `pty_command_stop`, `pty_command_list` | Unix PTY or Windows ConPTY, interactive input and limits; existing platform restrictions |
| Persistent shell sessions | `shell_session_start`, `shell_session_exec`, `shell_session_status`, `shell_session_stop`, `shell_session_list` | Unix and explicit session capability; unsupported elsewhere |
| Workspace snapshots | `workspace_snapshot_create`, `workspace_snapshot_apply` | Existing workspace/session policy and platform behavior |
| Sifters, buckets and context | inline rules on starts; `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context` | Keyword/regex/pack rules, severities, capture/redaction, bounded context rings and cursors |
| Registry | `registry_search`, `registry_get`, `registry_upsert`, `registry_test`, `registry_activate`, `registry_import_pack`, `registry_deactivate`, `registry_deactivate_bulk`, `registry_list_active`, `registry_suggest_from_samples` | Same validation, activation authority, built-in packs, matching and persistence |
| Recipes | `recipe_search`, `recipe_get`, `recipe_upsert`, `recipe_test`, `recipe_activate`, `recipe_deactivate`, `recipe_list_active`, `recipe_run`, `recipe_list_versions`, `recipe_tombstone`, `recipe_import_seeds` | Same typed parameters, versions, validation, explicit activation/admin gates and execution |
| Files | `file_read_window`, `file_search`, `file_list_dir`, `file_write` | Existing path policy, byte/item caps and write guards |
| File watches | `file_watch_start`, `file_watch_stop`, `file_watch_list` | Same FileProbe, sifters, events, bucket subscriptions and shutdown drain |
| Runtime and probes | `runtime_state`, `probe_list`, `probe_status` | Same lane routing and bounded status |
| Subscriptions and room event consumption | `subscription_open`, `subscription_pull`, `subscription_list`, `subscription_close`, `subscription_seek` | Boot-local consumer cursors and bounded event pulls; new subscriptions start from current tails, restart requires reopening, and seek clamps to retained events; host transports events between rooms |
| Audit | `audit_since` | Shared dispatcher audit trail and real host peer identity |
| Owner credentials | `credential_request`, `credential_provide`, `credential_provide_challenge`, `credential_url` | Embedded host-owned UI with a typed engine/job/prompt challenge; generation-bound completion, existing current-prompt completion and explicit temporary loopback page remain available; trusted local authority required, no serialized admin grant |
| Lifecycle | `quiesce_for_replace`, `shutdown` | Admission, abandonment, cancellation, bounded drain and store closure; no listener needed |
| Low-level integrations | `embedded::{core, probes, sifters}` and existing public runtime APIs | Aligned re-exports; advanced direct calls require the host to own any omitted dispatcher/lifecycle obligations |

MCP's compact `tc`, `omni_*` and action aliases are presentation/routing aliases
for these operations; typed consumers select the corresponding request variant
or method directly. MCP JSON rendering and CLI text formatting remain adapter
responsibilities. The shared `run_and_watch` composition is available explicitly,
including its degraded-result semantics.

Remote `target_list`/`target_probe` and `target_id` routing select another daemon
through the MCP/host transport. They do not create additional engine behavior.
An embedded instance intentionally has no implicit remote attachment: the host
selects the destination and transports the existing typed envelope. Discovery
reports `RemoteTargets: HostTransportRequired`. Host-managed services, code
execution and events can be exposed inside a virtualized room without giving that
room the host's Rust handle or transport credentials.

Capability availability describes whether a family is callable on the platform
and configured policy, not a promise every request is authorized. Hosts retain
the complete API and choose each room's policy and projection. Missing platform
facilities return their existing typed errors; they are never simulated.

Regression coverage lives in `crates/daemon/tests/embedded_engine.rs` and
`crates/ipc/tests/compound.rs`; compile-and-run examples live in
`crates/daemon/examples/embed_in_process.rs` and `embed_isolated.rs`. The daemon
discovery parity test checks the operation-name table against the exhaustive IPC
variant fixture. Standard daemon, IPC, probe and MCP tests exercise the shared
implementations behind this facade.
