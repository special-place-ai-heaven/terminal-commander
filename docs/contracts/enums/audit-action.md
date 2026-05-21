# Audit-action enum (closed set, MVP)

The closed set of action strings emitted in the `audit-record.vN`
contract. New entries require a doctrine amendment first
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
