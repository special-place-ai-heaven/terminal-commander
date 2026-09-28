# MCP Tool Control Surface - Locked Contract

Status: current MCP-facing contract. Live catalogue: 60 tools; compact surface: six facades.
Anchored by: `crates/mcp/src/tools.rs`, `docs/runtime/REALTIME_SIGNAL_CHANNEL.md`.
Language: ASCII only.

This document is the authoritative MCP tool contract for Terminal
Commander clients. A tool that ships without bounded outputs, honest
availability metadata, policy gating where applicable, and audit on
mutation is a contract violation.

## 1. Discovery and availability

`system_discover` is always callable. It returns adapter metadata, daemon
reachability, and the live tool catalogue:

```json
{
  "adapter_version": "<installed version>",
  "mcp_spec": "2026-07-28",
  "daemon_available": false,
  "daemon": null,
  "daemon_error": "daemon ipc error [...]: ...",
  "tools": [
    {
      "name": "system_discover",
      "status": "live",
      "requires_daemon": false,
      "available": true,
      "unavailable_reason": null
    },
    {
      "name": "health",
      "status": "live",
      "requires_daemon": true,
      "available": false,
      "unavailable_reason": "daemon_unavailable"
    }
  ]
}
```

When the daemon is reachable, `daemon.environment` is a fresh bounded snapshot,
not a platform guess. It reports terminal markers, installed shell/PowerShell
paths and versions, WSL state, and common tool probes. Its `access_routes` list
contains confirmed-only routes filtered through the active command-probe,
shell, and command policy. `repo_only` uses its configured repository root as
the representative discovery cwd; it never depends on the daemon process cwd.
Each route has a stable id, absolute executable, family, evidence, and exact
`argv_template`: `direct_argv` and `wsl_argv` end in `{args...}`, while
shell-backed routes end in `{command}`. `beachhead` repeats the highest-ranked
surviving route so an LLM can establish a working execution path without
synthesizing flags. Concrete calls are always rechecked with their real argv and
cwd. WSL is promoted only after a sentinel command succeeds within the probe
deadline.

Route actions are part of the contract: use `run` or `run_and_watch` with the
returned template for `direct_argv`, `wsl_argv`, and `wsl_shell`; use `exec` with
the returned shell executable and a `shell_line` for native `shell` routes.

Availability rules:

- `system_discover` and `target_list` do not require the daemon.
- Every other full-surface MCP tool requires the daemon.
- When the daemon is unavailable, daemon-backed tools report
  `available: false` and `unavailable_reason: "daemon_unavailable"`.
- Daemon-backed calls must return a structured `daemon_unavailable`
  error when startup status says the daemon is unavailable. They must
  not leak raw pipe/socket errors as the primary client contract.
- The advertised list and the registered rmcp router are tested to stay
  aligned. The catalogue pin in `crates/mcp/src/tools.rs` is
  `catalogue_lists_sixty_live_tools`; that test asserts the live
  60-tool list. `tool_router_exposes_all_live_tools` checks the router.

Session availability:

- Persistent shell sessions and workspace snapshots
  (`shell_session_*`, `workspace_snapshot_*`) require a reachable daemon,
  a UNIX session runtime, and the `allow_session` capability. If any gate is
  closed, `system_discover` reports them unavailable:
  - `system_discover.omni_status.matrix.sessions.available` is true only when
    all three gates are open. This row carries only the boolean; the
    human-readable reason rides the per-tool catalogue.
  - each session/snapshot entry in `system_discover.tools[]` reports
    `available: false` with the specific daemon, platform, or capability reason.
- The PTY command lane (`pty_command_*`) is dual-backend (POSIX +
  Windows ConPTY) and stays available on Windows; only the session
  layer built on top of it is unix-only. See
  `docs/runtime/SHELL_SESSION.md` section 3.

Machine-readable fixture:
`tests/fixtures/contracts/mcp-tools/system_discover.v1.json`.

## 2. Live tool catalogue

The full rmcp stdio surface exposes 60 live tools. With
`TC_SURFACE=compact`, the adapter instead advertises six action-dispatched
facades: `command`, `files`, `recipe`, `registry`, `session`, and `status`.
Both surfaces route to the same handlers and policy boundary. `recipe` is
the argv recipe registry. It is separate from `registry` (signal rules).
Recipe actions are not accepted on the `registry` facade.

| Group | Tools |
|---|---|
| Discovery, health, and audit | `system_discover`, `health`, `policy_status`, `self_check`, `audit_since` |
| Commands and buckets | `command_start_combed`, `run_and_watch`, `command_status`, `command_stop` (forced-kill-only; CommandSignal-gated), `shell_exec` (gated by `allow_shell`; combed, never raw), `command_output_tail`, `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context` |
| Subscriptions | `subscription_open`, `subscription_pull`, `subscription_list`, `subscription_close`, `subscription_seek` |
| Rule registry | `registry_search`, `registry_get`, `registry_upsert`, `registry_test`, `registry_activate`, `registry_import_pack`, `registry_deactivate`, `registry_list_active`, `registry_suggest_from_samples` (proposals only; NEVER auto-activates) |
| Recipe registry | `recipe_search`, `recipe_get` (`tombstoned` is true when the id was retired by `recipe_tombstone`), `recipe_upsert`, `recipe_test` (dry-run; does not activate or start a job), `recipe_activate`, `recipe_deactivate`, `recipe_list_active`, `recipe_run`. Separate from rules. Compact facade `recipe` actions: `search`, `get`, `upsert`, `test`, `activate`, `deactivate`, `list_active`, `run`. MCP `recipe_activate` / `recipe_deactivate` are open under the default `full_access` profile; a hardened profile denies them while `llm_can_activate_recipes` is false (its default) with `recipe_activate_requires_admin`. The grant is the `terminal-commander` peer image, not a caller `from_mcp` bit; omitting that field is not admin, and the MCP adapter image cannot claim it. The operator CLI is `terminal-commander recipes activate`, `recipes deactivate`, `recipes tombstone`, and `recipes import [--activate]` (global scope only). `recipe_run` runs an activated recipe on the argv lane. A recipe with `timeout_ms` or `rule_pack_ids` returns a watched response (signals, resume cursor, `degraded` / `recover_hint`, same contract as `run_and_watch`). `rule_pack_ids` only select that path and do not load packs; combing uses registry rules already active on the job. Otherwise the start is `command_start_combed`. Never `shell_exec`. |
| Sessions and workspace | `shell_session_start`, `shell_session_exec`, `shell_session_status`, `shell_session_stop`, `shell_session_list`, `workspace_snapshot_create`, `workspace_snapshot_apply` (gated by `allow_session`, on under the default `full_access`; unix-only; combed, never raw) |
| Files | `file_read_window`, `file_search`, `file_write` (policy-gated by `paths.write_allow`; audited before write; bounded size; atomic; mutating / non-idempotent), `file_watch_start`, `file_watch_stop`, `file_watch_list` |
| PTY | `pty_command_start`, `pty_command_write_stdin`, `pty_command_stop`, `pty_command_list`, `credential_request` (asks the owner for a password a PTY job waits on; status only) (POSIX + Windows ConPTY). Compact `session` action: `credential_request`. |
| Remote | `target_list`, `target_probe` (routes to an operator-forwarded local socket; remote use gated by `allow_remote`; no public TCP) |
| Runtime | `runtime_state`, `probe_list`, `probe_status` |

`command_output_tail` accepts optional `strip_ansi`; stripping affects only the
returned rendering and never mutates the raw frame store. `file_search` accepts
either an absolute regular file or directory. Directory searches are recursive,
deterministic, policy-checked per candidate file, symlink-safe, and bounded by
match, byte, and visited-entry ceilings.

PTY password prompts (owner credential path):

- TC44 is unchanged: while a PTY job is at a secret prompt,
  `pty_command_write_stdin` returns `SecretInputDenied` and writes nothing.
  The deny message names `credential_request` with the job id.
- `command_status`, `pty_command_list`, and `pty_command_write_stdin` carry
  `awaiting_credential: {kind: "sudo"|"ssh"|"password", since_ms}` for a job
  at such a prompt; the field is omitted otherwise.
- `credential_request {job_id}` makes the daemon ask the OWNER through a
  channel the model cannot read: Windows CredUI; on a unix desktop the first
  of `$SSH_ASKPASS`, `ssh-askpass`, `zenity`, `kdialog`, `pinentry`. The
  daemon types the answer plus Enter into that job, masks an echoed copy on
  the next output line, and audits `credential_provided {kind, source}`
  (never the value or its length). The model receives only
  `{job_id, status}`: `provided`, `declined`, `timeout` (not answered in
  60 s; the prompt stays open and a repeat call keeps waiting),
  `owner_action_required` (plus `command`), or `not_awaiting`. One owner
  prompt per password prompt; repeats replay, never re-ask.
- Without a native prompt the owner runs
  `terminal-commander credential provide <job_id>` in their own terminal. It
  shows which job asks, reads the password with echo off, and sends
  `credential_provide`, which the daemon accepts only from the admin CLI
  image outside its own process tree. There is no MCP tool for it.
- `run_and_watch` and `command_start_combed` add `credential_hint` when the
  launched program is `sudo`, `su`, `doas`, or `ssh`: a pipe cannot answer
  a password prompt, so run it with `pty_command_start` instead. Argv is
  never rewritten.

Remote routing surface (`target_id`):

- The optional `target_id` parameter (remote federation, gated by
  `allow_remote`) is wired on the COMMAND lane only:
  `command_start_combed`, `run_and_watch`, `command_status`,
  `command_stop`, plus `target_list` / `target_probe`.
- It is NOT a parameter on `shell_exec`, `pty_*`, `file_*`,
  `registry_*`, or `recipe_*` (except `target_list` / `target_probe`). Passing
  `target_id` to one of those tools does NOT route the call remotely;
  the field is unknown to the schema and the call runs against the LOCAL
  daemon as if no target were named.
- This is NOT an `allow_remote` bypass. The remote gate lives on the
  command path; tools without `target_id` simply have no remote path to
  gate. To run other lanes remotely, run them inside a remote
  `command_start_combed` / `run_and_watch`, not by tagging an unsupported
  tool. See `docs/mcp/OMNI_PLAYBOOK.md` section 6.

Each catalogue entry returned by `system_discover.tools[]` includes:

- `name`
- `status`
- `description`
- `requires_daemon`
- `available`
- `unavailable_reason`

When the daemon is up and `allow_shell` is off, the `shell_exec` catalogue
row is `available: false` with `unavailable_reason` `allow_shell capability
is off in the active policy profile`. Discover's catalogue `steer` and
`omni_status.matrix.shell_exec.steer` stay the argv default (`recover_hint`
`retry_with_argv`, `intended_tool` `run_and_watch`, `intended_example`
`{"argv":["git","status"]}`). A shell-misuse deny follows the daemon
`ShellTeach`: when `recipe_id` and `recipe_scope` are set (exactly one
runnable scope), `recover_hint` is `retry_with_recipe`, `intended_tool`
is `recipe_run`, and `intended_example` is
`{"recipe_id":"...","scope":{"kind":"global"}}`. Pass that object
unchanged. Otherwise the envelope keeps `retry_with_argv` /
`run_and_watch`. Alternatives still list argv tools.
The remedy is that hint, not "enable shell". See
`docs/integrations/recipe-registry.md`.

## 3. Tools not exposed

| Anti-tool | Why it must not exist |
|---|---|
| `command_read_stdout` | Would surface raw stream text. |
| `command_read_stderr` | Would surface raw stream text. |
| `file_read_all` | Unbounded file output. |
| `stream_tail` | Raw stream tail. |
| `network_listen` | No network listener is allowed in the MCP-facing crate. |
| `policy_override` | Policy decisions are not bypassable by clients. |

Any later goal that needs a capability shaped like one of these must
stop and propose a bounded alternative.

## 4. Bounded-output rules

Every response carries explicit limits or returns references/cursors
instead of raw streams.

| Surface | Required behavior |
|---|---|
| Command start | Returns ids and metadata, not stdout/stderr dumps. |
| Bucket reads | Cursor-based and capped by daemon/store limits. |
| Bucket wait | Returns events or a heartbeat, not an unbounded tail. |
| Event context | Returns a bounded window around a pointer. |
| File reads | Windowed by line/byte limits; no whole-file dump tool. |
| File search | Bounded matches and capped snippets. |
| Registry search/test | Bounded hit/sample counts. |
| Recipe search/test | Bounded hits. Test validates argv and does not activate or start a job. |
| Runtime/probe status | Bounded JSON snapshots. |

`run_and_watch.cursor` is a resume cursor, not simply the last internally
observed event. When `max_signals` caps a response, the cursor remains before
omitted matches so a later `wait` from that cursor recovers every capped signal.

`bucket_wait` must return one of two response shapes:

```text
{ heartbeat: true,  events: [],         next_cursor: <input> }
{ heartbeat: false, events: [...non-empty], next_cursor: <max(seq)> }
```

A response with raw stream text in `events` is invalid. The forbidden
fixture `tests/fixtures/contracts/forbidden/raw-stream-as-events.v1.json`
is the structural test oracle.

## 5. Policy and audit

Tools that read or mutate daemon state must route through the daemon-side
policy/audit boundary. The MCP adapter is a transport and validation
layer; it must not become an alternate command executor, policy bypass,
or hidden shell bridge.

Known policy action families include command start/stdin/signal,
file read/watch, probe create, registry create/activate, recipe activate
(MCP-gated by `llm_can_activate_recipes` on a hardened profile), bucket wait/read,
and event context. A new tool that needs a new policy action must add the
closed-set variant in the same goal that adds the tool.

## 6. Forbidden expansions

A future goal must stop and surface a blocker rather than:

- Add a tool that returns raw stream text.
- Add a tool that bypasses policy evaluation.
- Add a tool that opens a TCP listener.
- Add a tool that spawns commands from the MCP crate.
- Replace `bucket_wait` heartbeat with a partial raw dump.
- Replace `event_context` bounded windows with an unbounded mode.
- Expose command or PTY mutation without audit.

## 7. References

- `crates/mcp/src/tools.rs` - live rmcp tool registration and discovery.
- `docs/runtime/REALTIME_SIGNAL_CHANNEL.md` - product contract.
- `docs/mcp/README.md` - adapter overview.
- `docs/integrations/recipe-registry.md` - recipes vs rules, `recipe_run`, teach.
- `docs/security/PRIVILEGE_MODEL.md` - privilege boundaries.
- `docs/contracts/README.md` - wire-shape fixtures.
