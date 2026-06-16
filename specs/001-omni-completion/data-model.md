# Phase 1 Data Model: Omni Completion Program

Entities are described by purpose, fields, relationships, and lifecycle. Exact
Rust types and serde shapes are bound in `tasks.md` / implementation; this model
is the contract-level view.

## ShellSession (P1)

- **Purpose**: A long-lived interactive shell preserving cwd/env across sends.
- **Fields**: `session_id` (stable id), `bucket_id` (signal bucket), `shell`
  (resolved interpreter path), `cwd` (current working dir), `env_snapshot`
  (bounded key/value map), `state` (Starting | Live | Exited | Failed),
  `created_at`, `last_active_at`, `peer_subject` (audit identity).
- **Relationships**: owns one PTY handle and one bucket; produced/consumed by the
  `shell_session_*` tools; gated by `PolicyAction::SessionStart`.
- **Lifecycle**: Starting -> Live -> (Exited | Failed). Idle past `idle_ttl_secs`
  -> reaped (Exited). Sends to a non-Live session fail loudly.
- **Invariants**: bounded `env_snapshot` size; `max_sessions` cap enforced before
  spawn; output combed only.

## WorkspaceSnapshot (P1)

- **Purpose**: A saved, restorable workspace (cwd + bounded env).
- **Fields**: `snapshot_id`, `cwd`, `env` (bounded), `created_at`, `source_session_id`.
- **Relationships**: created from a ShellSession; applied into a (new) ShellSession.
- **Lifecycle**: Created -> Applied (N times). Persisted in SQLite.
- **Invariants**: env size bounded; no secrets persisted unredacted.

## JobReceipt (P1, TC-B3)

- **Purpose**: Survive daemon restart so a status poll is honest.
- **Fields**: `job_id`, `bucket_id`, `terminal_state` (Exited | Cancelled |
  SpawnFailed | Unknown), `exit_code` (nullable), `final_signal_counts`,
  `restarted_at` (set when read after a restart), `created_at`.
- **Relationships**: one per command/shell job; written on every completion path;
  read by `command_status` when the in-memory job is absent.
- **Lifecycle**: Written at terminal transition; read post-restart returns a
  restart-marked terminal result, never a bare error.
- **Invariants**: append/replace keyed by `job_id`; bounded row size.

## RuleSuggestion (P2)

- **Purpose**: Candidate parsing rules derived from samples; never self-activating.
- **Fields**: `proposed_rules` (list of rule drafts), `confidence`
  (heuristic-grade label), `next_steps` (ordered: registry_test ->
  registry_upsert -> registry_activate).
- **Relationships**: input = output samples + intent; output feeds the
  test/activate loop. NO direct link to the active registry until explicit activate.
- **Lifecycle**: Generated -> (optionally) Tested -> Upserted -> Activated by
  explicit operator/agent action only.
- **Invariants**: activation impossible from this entity alone (FR-008).

## RulePack (P2)

- **Purpose**: A named bundle of rules for a tool family.
- **Fields**: `pack_id` (e.g. `docker`, `kubectl`, `git`), `rules`, `version`.
- **Relationships**: imported via `registry_import_pack`; surfaced by pack hints.
- **Lifecycle**: Built-in (shipped) -> Imported (active) on demand.
- **Invariants**: >=25 built-in packs at release (SC-003).

## CapabilityMatrix (P6)

- **Purpose**: Discovery-time map of omni capabilities on this host.
- **Fields**: per-capability `{ available: bool, reason?: string, platform?:
  string }` for shell_exec, sessions, pty (+ platform), privileged_helper,
  remote_targets (count, reachable).
- **Relationships**: assembled by the daemon, surfaced via `system_discover.omni_status`.
- **Lifecycle**: Computed per discovery call from live runtime + config state.
- **Invariants**: reports honest reasons for unavailable capabilities.

## RemoteTarget (P5)

- **Purpose**: A registered host reachable via a tunnel to its local daemon socket.
- **Fields**: `target_id`, `transport` (ssh_forward), `host`, `identity_file`,
  `remote_socket`, `reachable` (probe result).
- **Relationships**: referenced by optional `target_id` on daemon-backed tools.
- **Lifecycle**: Registered (targets.toml) -> Probed -> Used.
- **Invariants**: no public TCP listener; default local when unset.

## PrivilegedOperation (P4, plan-only)

- **Purpose**: A named closed-allow-list privileged op with approval.
- **Fields**: `op` (allow-list member), `params` (named args), `approval_state`
  (PendingApproval | Approved | Denied), `approval_id`, `approval_token`,
  `audit_subject` (redacted).
- **Relationships**: requested via `privileged_exec`; approved via admin-CLI
  `privileged_approve`; executed by the helper binary.
- **Lifecycle**: Requested -> PendingApproval -> Approved -> Executed (or Denied);
  token single-use, expirable.
- **Invariants**: off-list op refused regardless of approval; audit before exec;
  NO code this run (threat-review gated).

## Signal projection: compact + captures (P1, TC-E1/E4)

- **Purpose**: Presentation shape of a signal event for token economy.
- **Compact fields**: `{ summary, stream, seq, severity }` (ids omitted unless
  `compact:false`).
- **Capture canonicalization**: one canonical matched-text field + named captures
  map; the redundant `0`/`line`/`match` triple is collapsed.
- **Invariants**: projection only; the event store retains the full record.
