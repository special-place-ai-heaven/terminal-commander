# Wave 4 — Privileged Helper (Gated System Operations)

**Goal:** Enable system-level operations (package install, services, journal read) without generic sudo — closed allow-list, operator opt-in, full audit.

**Depends on:** Wave 1 policy framework (`CommandShellStart` pattern reuse).

**Acceptance:** O-06.

**Authority:** Pre-bound constraints in `docs/security/PRIVILEGE_MODEL.md` §5.

---

## Architecture

```text
LLM → MCP → daemon → policy gate → privileged helper (local IPC)
                                              ↓
                                    closed allow-list ops only
                                              ↓
                                    audit (high severity) + optional human approve
```

**Separate binary:** `terminal-commander-privileged`

- Installed by operator (`setup privileged-helper` — NOT automatic).
- Runs setuid root OR uses polkit — **design choice in TC61 threat review**.
- Speaks narrow IPC: `{op: "apt_install", packages: ["foo"], confirm_token?: "..."}`.
- **Never** accepts `{shell_line: "..."}`.

---

## Allowed operations (v1 closed set)

| Op ID | Description | Audit |
|---|---|---|
| `apt_install` | apt-get install -y named packages | high |
| `apt_update` | apt-get update | medium |
| `systemctl_start` | named unit | high |
| `systemctl_stop` | named unit | high |
| `systemctl_restart` | named unit | high |
| `journal_read_window` | bounded journal lines (like file_read_window) | medium |
| `npm_global_install` | system-wide npm (alternative to user prefix) | high |

Windows v1 ops (separate list):

| Op ID | Description |
|---|---|
| `winget_install` | named package id |
| `sc_start` / `sc_stop` | named service |

---

## Policy integration

**New profile or extend `admin_debug`:**

```toml
[privileged_helper]
enabled = false
socket_path = "/run/user/{uid}/terminal-commander-privileged.sock"
allowed_ops = ["apt_install", "journal_read_window"]
require_human_approve = true
```

**New MCP tools:**

| Tool | Purpose |
|---|---|
| `privileged_exec` | `{op, args}` — policy + optional approve token |
| `privileged_list_ops` | discover allowed ops for active profile |
| `privileged_approve` | admin_cli only — mint approve token for pending op |

LLM never reads audit log; operator uses `terminal-commander audit` for review.

---

## Human approval flow (when enabled)

```text
1. LLM calls privileged_exec {op: apt_install, packages: [curl]}
2. Daemon returns {status: pending_approval, approval_id, expires_at}
3. Operator: terminal-commander privileged approve approval_id
4. LLM retries with approval_token
5. Helper executes; combed output in bucket
```

---

## Goal chain

| Goal | Outcome |
|---|---|
| TC61 | Threat model amendment + PRIVILEGE_MODEL.md update |
| TC62 | Helper binary + IPC protocol |
| TC63 | Daemon bridge + MCP tools + policy |
| TC64 | Operator setup docs + NOPASSWD/sudoers templates |
| TC65 | Live e2e O-06 + negative tests (deny by default) |

---

## Effort

**4–6 weeks** — threat review is gating; do not skip.

---

## Risks

| Risk | Mitigation |
|---|---|
| Privilege escalation | closed op set; no shell; per-call audit |
| Operator fatigue on approve | default off; auto-approve list for low-risk ops in admin profile only |
| WSL vs native Linux | helper runs in WSL Linux context; document Windows host boundary |

---

## Explicit non-goals

- Generic `sudo bash -c`
- LLM-readable audit log
- Auto-install helper without operator action
