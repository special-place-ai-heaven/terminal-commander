# Wave 5 — Remote Federation (Multi-Host Omni)

**Goal:** Same MCP tool surface works against local daemon, remote SSH host, or container — LLM does not need a different tool per environment.

**Depends on:** Waves 1–2 stable (shell + platform parity on targets).

**Acceptance:** O-09, O-10.

---

## Design principle

**Remote TC daemon, not remote raw shell.**

Each host runs `terminal-commanderd`. Federation connects MCP adapter to the correct daemon endpoint. Combing, policy, audit stay on the target host.

```text
Cursor MCP → terminal-commander-mcp
                  ↓
         target_router (local default)
           ↙         ↘
    local IPC    SSH -L / forwarded pipe
                      ↓
              remote terminal-commanderd
```

---

## Workstreams

### WS-5A — Target registry (TC66)

**Daemon-local config** (`~/.config/terminal-commander/targets.toml`):

```toml
[[targets]]
id = "prod-server"
transport = "ssh_forward"
host = "user@prod.example.com"
identity_file = "~/.ssh/id_ed25519"
remote_socket = "~/.local/share/terminal-commanderd/terminal-commanderd.sock"

[[targets]]
id = "dev-container"
transport = "docker_exec"
container = "my-dev-box"
# runs terminal-commander-mcp inside container OR forwards to inner daemon
```

**New MCP parameter (optional on all daemon-backed tools):**

```json
{"target_id": "prod-server", ...existing params...}
```

Default omitted → local daemon (backward compatible).

---

### WS-5B — SSH forward transport (TC67)

1. MCP adapter (or supervisor) establishes `ssh -L local:remote_sock` tunnel.
2. IPC client connects to local forward endpoint.
3. Peer identity: remote daemon uid check + host key pinning in config.
4. Policy: `remote_targets.allow[]` in profile.

**Security:**

- No public TCP listener on remote.
- SSH keys managed by operator; never in LLM context.
- `target_id` must be pre-registered — LLM cannot arbitrary SSH.

---

### WS-5C — Container attach (TC68)

Options (pick in design review):

- **A:** Sidecar daemon in container (preferred) — `docker run` with TC pre-installed image.
- **B:** `docker exec` into running container to invoke local MCP one-shot (heavier, worse audit).

Document bootstrap: `terminal-commander setup container`.

---

### WS-5D — Federation discover (TC69)

`system_discover` extended:

```json
{
  "targets": [
    {"id": "local", "available": true, "platform": "linux"},
    {"id": "prod-server", "available": true, "platform": "linux", "tc_version": "0.2.0"}
  ]
}
```

Cross-target `subscription_pull` — phase 2 (multiplex remote buckets) if needed.

---

## New tools (summary)

| Tool | Purpose |
|---|---|
| `target_list` | Registered targets + reachability |
| `target_probe` | Health check on remote daemon |
| `target_add` / `target_remove` | admin_cli or gated MCP |

Most existing tools gain optional `target_id`.

---

## Effort

| Goal | Weeks |
|---|---|
| TC66 target registry | 1.5 |
| TC67 SSH transport | 3 |
| TC68 container attach | 2 |
| TC69 discover + e2e | 1.5 |
| **Total** | **~8 weeks** |

---

## Risks

| Risk | Mitigation |
|---|---|
| Latency on remote IPC | degrade gracefully; longer wait_ms defaults |
| Version skew local/remote | `replace_if_stale` per target; min version in discover |
| SSH key exposure | keys never in MCP args; config file 0600 |

---

## Phasing option (reduce scope)

**5-minimum:** SSH target support only (O-09).  
**5-full:** SSH + container (O-09 + O-10).

Ship 5-minimum for omni v1.0 if calendar pressure; container in v1.1.
