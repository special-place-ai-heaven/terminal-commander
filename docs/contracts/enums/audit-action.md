# Audit-action enum (closed set, MVP)

Status (2026-10-05): not what the daemon emits. Only the decision string is
a closed set in the store (`allow`, `deny`, `allow_with_audit`, `error`,
`info`). Action strings are free text; the daemon writes IPC method names
(for example `file_read_window`, `file_write`, `registry_upsert`,
`pty_command_write_stdin`) plus lifecycle labels (`command_start`,
`command_shell_start`, `command_rejected`, `command_shell_rejected`,
`command_exit`, `pty_command_exit`, `file_watch_exit`,
`shell_session_start`, `credential_provided`,
`recipe_activate_requires_admin`). The names `command_stdin`,
`command_signal`, `file_read`, `probe_create`, `probe_bind`,
`registry_create`, `policy_decision`, `policy_invalid`, `bucket_export` and
`default_deny_override_loaded` below are not emitted by any code path.

The closed set of action strings emitted in the audit rows (`AuditRow`,
`tests/fixtures/contracts/mcp-tools/audit_since.v1.json`). New entries require a doctrine amendment first
(`SECURITY.md` section 4 + `POLICY.md` section 4).

| Action | Implementing goal |
|---|---|
| `command_start` | TC15, TC22 |
| `command_stdin` | TC15, TC22 |
| `command_signal` | TC16, TC22 |
| `file_read` | TC18, TC22 |
| `file_watch` | TC18, TC22 |
| `probe_create` | TC21, TC22 |
| `probe_bind` | TC21, TC22 |
| `registry_create` | TC13, TC22 |
| `registry_activate` | TC13, TC22 |
| `registry_delete` | TC13, TC22 |
| `policy_decision` | TC22 (umbrella for evaluation events) |
| `policy_invalid` | TC22 (load-time profile rejection) |
| `bucket_export` | TC23 |
| `default_deny_override_loaded` | TC22 (per `POLICY.md` section 5) |

Reserved (not yet implemented) but pre-bound so doctrine and
implementation stay aligned:

| Action | Notes |
|---|---|
| `helper_invoke` | Reserved per `docs/security/PRIVILEGE_MODEL.md` section 5. NOT IMPLEMENTED IN MVP. |
| `profile_reload` | Reserved (out of MVP: profile changes are restart-only). |
