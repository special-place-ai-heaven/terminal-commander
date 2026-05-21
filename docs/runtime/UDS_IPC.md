# Daemon UDS IPC (TC37)

Status: Live (TC37) on Linux / WSL2 / macOS / BSD.
Status: Unsupported on Windows native (use WSL2).
Crate paths: `crates/daemon/src/ipc/{mod,protocol,peer,server,client}.rs`.

## 1. Purpose

Local transport between operator tools (and, after TC40, the rmcp
adapter) and the long-running daemon. Replaces the
"library-only in-process Router" state from TC21/TC36 with a real
cross-process boundary that:

- Speaks only Unix domain sockets.
- Captures peer identity (uid/gid/pid where available).
- Refuses unidentified peers on Linux/WSL.
- Bounds every request and every response.
- Audits every accepted request through the TC35 PersistentAudit
  sink.
- Exposes a minimal, safe method set.

This is the transport layer for the realtime signal channel. It is
not a shell bridge.

## 2. What ships in TC37

Method set (deliberately small):

| Method | Purpose |
|---|---|
| `system_discover` | Version, MCP spec, active profile, callable methods. |
| `health` | Liveness ping; returns uptime seconds. |
| `policy_status` | Active profile + daemon-side per-call caps. |
| `self_check` | Tiny synthesized report (data dir, profile, audit count). |

What TC37 does NOT ship (intentional; later goals):

- `command_*` (TC38 wires `command_start_combed`).
- `bucket_*` over IPC (TC39 daemon API + TC41 MCP tool surface).
- `event_context` over IPC (TC39).
- `file_*` (TC43).
- `registry_*` (TC42 hot rebind + TC41 MCP surface).
- rmcp stdio adapter (TC40).

## 3. Wire protocol

Length-prefixed JSON frames over a single UnixStream.

```text
+-----+---------------------------+
| 4B  | N bytes of UTF-8 JSON     |
+-----+---------------------------+
  big-endian u32 = N
```

- `MAX_FRAME_BYTES = 256 KiB`. A length prefix above this is rejected
  before the payload is even read.
- `MAX_REQUEST_BYTES` and `MAX_RESPONSE_BYTES` mirror the frame cap
  today.

### 3.1 RequestEnvelope

```json
{
  "correlation_id": 42,
  "request": {
    "method": "system_discover"
  }
}
```

Other methods: `"health"`, `"policy_status"`, `"self_check"`. None
of the TC37 methods take parameters; the `params` field is omitted.

### 3.2 ResponseEnvelope (success)

```json
{
  "correlation_id": 42,
  "result": {
    "kind": "ok",
    "response": {
      "method": "system_discover",
      "version": "0.0.0",
      "mcp_spec": "2025-11-25",
      "policy_profile": "DeveloperLocal",
      "methods": ["system_discover", "health", "policy_status", "self_check"]
    }
  }
}
```

### 3.3 ResponseEnvelope (error)

```json
{
  "correlation_id": 42,
  "result": {
    "kind": "err",
    "error": {
      "code": "frame_too_large",
      "message": "frame 300000 bytes > MAX_FRAME_BYTES 262144"
    }
  }
}
```

### 3.4 Closed-set error codes

| `code` | Meaning |
|---|---|
| `frame_too_large` | Length prefix exceeds `MAX_FRAME_BYTES`. |
| `malformed_json` | Payload not valid UTF-8 JSON. |
| `schema_mismatch` | Payload parsed but did not match the wire schema. |
| `unknown_method` | Method name not in the dispatcher. |
| `policy_denied` | Policy engine denied the request. |
| `internal` | Daemon-side I/O / serialization error. |
| `peer_credential_failure` | Peer creds unavailable; connection refused. |
| `unsupported_platform` | Windows native; use WSL2. |

Adding a variant requires a goal-file amendment.

## 4. Peer identity

| Platform | Source | Fields |
|---|---|---|
| Linux / WSL2 / Android | `SO_PEERCRED` via `getsockopt` | uid, gid, pid |
| macOS / BSD | `getpeereid` | uid, gid (pid = None) |
| Windows native | — | unsupported (the `ipc::server` module is `#[cfg(unix)]`) |

Fail-closed rule: on Linux/Android, if peer credentials cannot be
obtained, the server emits an `ipc_connect` audit row with
`decision = "deny"` and `reason = "peer credentials unavailable
(Linux/WSL fail-closed)"`, sends a `peer_credential_failure`
error to the client, and closes the connection without dispatching
any request.

On macOS/BSD the absence of `pid` is normal and is NOT a refusal.

Peer credentials are surfaced into every per-request audit row's
`metadata_json` as the JSON object `{"uid":N,"gid":N,"pid":N|null}`.

## 5. Concurrency model

- One tokio task per accepted connection.
- Per-connection serial request/response.
- Accept loop is shutdown-aware via `tokio::sync::Notify`; a
  graceful shutdown drains in-flight connections and removes the
  socket file.
- Drop of `ServerHandle` is a best-effort cleanup (notify, abort,
  unlink); operators should call `handle.shutdown().await` for an
  orderly stop.

## 6. Audit emission

Every accepted request emits exactly one audit row through the TC35
`PersistentAudit` sink:

- `action = "ipc_<method_name>"` (e.g. `ipc_system_discover`,
  `ipc_health`).
- `subject = "uid=...;gid=...[;pid=...]"` from peer credentials.
- `decision = "info"` (TC37 method set is read-only).
- `actor = "ipc"`.
- `metadata_json = {"uid":N,"gid":N,"pid":N|null}`.

Refused connections emit `action = "ipc_connect"` with
`decision = "deny"` and the refusal reason in `reason`.

## 7. Bounded outputs

Per-frame: hard cap `MAX_FRAME_BYTES = 256 KiB` on length-prefix
read AND on response encode (the server replaces an oversize
response with a `frame_too_large` error envelope rather than
truncating silently).

Per-request: every method's response shape is statically small
(version strings, integer counts, fixed method list). There is no
raw-stream lane today; TC39 will add `bucket_events_since` with its
own `MAX_READ_LIMIT` cap.

## 8. Socket path resolution

```text
daemon.socket_path (TOML)               -> use as-is
otherwise                              -> <data_dir>/terminal-commanderd.sock
```

The socket file is created with the daemon's UID and the default
filesystem umask. Operators who want stricter ACLs should place
the socket under a directory they own with `chmod 700`.

## 9. Subcommand surface

```bash
terminal-commanderd start                       # default: ipc-server on Unix
terminal-commanderd start --mode ipc-server     # explicit
terminal-commanderd start --mode foreground-idle # no IPC (pre-TC37 fallback)
terminal-commanderd check                       # self-check + exit (no IPC)
terminal-commanderd print-config                # emit resolved config
```

## 10. Security posture

- No TCP listener. No UDP. No HTTP. No WebSocket. No network
  listener of any kind. (`terminal-commanderd::security` test suite
  already enforces no `TcpListener` in `crates/mcp`; the structural
  argument extends to the daemon crate.)
- No `Command::spawn` from the IPC dispatcher.
- No raw stream surface.
- No setuid, no polkit, no privileged helper.
- Peer credential check is mandatory on Linux/WSL.
- Audit row is mandatory on every accepted request.

## 11. Source-status

| Component | Status |
|---|---|
| Wire protocol (TC37 method set) | live |
| UnixListener accept loop | live (Unix only) |
| SO_PEERCRED (Linux/WSL/Android) | live |
| `getpeereid` (macOS/BSD) | live (pid = None) |
| Windows native | unsupported (use WSL2) |
| Bounded length-prefixed framing | live |
| Closed-set error codes | live |
| Audit on every accepted request | live |
| Graceful shutdown + socket cleanup | live |
| Cross-host UDS | NEVER (local-only) |
| TLS over UDS | NEVER (UDS peer cred is the trust root) |

## 12. Test coverage

Unit (`crates/daemon/src/ipc/protocol.rs`):
- `encode_decode_envelope_round_trip`
- `malformed_json_rejected_with_typed_code`
- `schema_mismatch_is_malformed_json_today`
- `frame_too_large_rejected_before_serialize_attempt`
- `response_envelope_err_round_trips`

Unit (`crates/daemon/src/ipc/peer.rs`, Unix only):
- `audit_string_with_pid`
- `audit_string_without_pid`

Integration (`crates/daemon/tests/ipc_roundtrip.rs`, Unix only):
- `system_discover_round_trip`
- `health_round_trip`
- `policy_status_reports_active_caps`
- `self_check_method_returns_report`
- `malformed_json_returns_typed_error_and_closes`
- `oversized_frame_rejected`
- `peer_credentials_recorded_in_audit_metadata` (Linux only)
- `shutdown_removes_socket_file`

## 13. Recorded gaps (NOT fixed in TC37)

- TC37 dispatcher does not yet route bucket / event-context / file
  / command / registry methods (TC38-TC44 own those).
- `system_discover.methods[]` advertises only what the dispatcher
  actually serves today. Once TC38+ expands the dispatcher, the
  advertised list MUST be re-tied to the live handler set; the
  contract is set out in `docs/mcp/TOOL_CONTROL_SURFACE.md` §5.
- No request/response correlation beyond the per-call envelope id;
  multiplexed in-flight requests on one connection are not
  supported yet.
- No backpressure cap on concurrent connections; bounded by the
  OS open-file limit. TC47 owns the load gate.
- `docs/contracts/enums/audit-action.md` closed set still does not
  list `ipc_*` actions (TC35 doctrine tension; outside TC37 scope).
