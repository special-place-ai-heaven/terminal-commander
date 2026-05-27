# Per-Harness Session Endpoint (F1) — Design

Date: 2026-05-27
Status: Approved (brainstorming)
Audit ref: `docs/audits/2026-05-27-full-spectrum-flakiness-fragility-audit.md` (F1)
Builds on: F2 `EnvSource` seam (`crates/supervisor/src/paths.rs`),
F6 fatal-vs-transient pipe-create classification.

## Problem

The daemon endpoint is derived from the OS **user**, not from the **harness**:

- Windows pipe: `\\.\pipe\terminal-commander-{USERNAME}` (`config.rs:261-268`),
  ignores the data dir entirely.
- Unix socket: `<data_dir>/terminal-commanderd.sock` (`config.rs:253-257`), and
  the default data dir is per-user (`$HOME/.local/share/terminal-commanderd`).

So two harnesses / agents run by the **same OS user** on one machine collide on
a single shared endpoint. The product requires **one daemon per harness**: each
harness/agent/terminal gets its own daemon + endpoint, like SymForge serving
multiple terminals.

F6 already made the colliding second daemon fail fast (fatal `ALREADY_EXISTS`
instead of an infinite 100ms retry spin), so this is no longer a hang — but the
second harness still cannot get its own daemon. F1 closes that.

## Decision

Model: **independent session-id endpoint, no central registry.** Each harness's
daemon and its `mcp`/`cli` clients independently resolve the **same** endpoint
from an inherited environment token. No router, no aggregator, no coordination.

### Three-tier endpoint resolution

Both the daemon (at bind) and every client (`mcp`, `cli`, at connect) resolve
the endpoint through one shared function with identical precedence:

```
1. TC_SOCKET set & non-empty   -> use verbatim as the full endpoint
                                  (explicit override / escape hatch; today's behavior)
2. else TC_SESSION set & non-empty -> token = sanitize(TC_SESSION)
3. else                         -> token = stable per-user default
```

Then the token maps to a platform endpoint:

```
Windows pipe : \\.\pipe\terminal-commander-{token}
Unix         : data_dir = <base>/{token}    (socket = data_dir/terminal-commanderd.sock,
                                              which already derives from data_dir)
```

`TC_SESSION` is an **opaque token**, not a path. The launcher (session
supervisor) sets one short string; it never needs to know the `\\.\pipe\...`
syntax or the `.sock` layout. `TC_SOCKET` remains the full-path escape hatch at
highest precedence.

### Backward compatibility (the auto-seed-stable-default decision)

When neither `TC_SOCKET` nor `TC_SESSION` is set, the token is a **stable
per-user default** that reproduces today's endpoint exactly. The
username-derived path flows through the *same* new code path (one path, not a
special case), so existing single-harness installs and manual `cli` use are
unchanged. Per-harness isolation is opt-in via `TC_SESSION`.

The per-user default token:
- Windows: `u-{USERNAME}` so the pipe is `\\.\pipe\terminal-commander-u-{USERNAME}`.
  NOTE: this is a one-time endpoint-name change vs the current
  `terminal-commander-{USERNAME}`. See Migration.
- Unix: the existing default data dir is preserved by mapping the default token
  to today's `<base>` directly (no extra `{token}` subdir for the default case),
  so the socket path is byte-identical to today. Mechanically: `endpoint_for_token`
  takes a flag (or sentinel) marking the per-user default, and on unix returns the
  bare `<base>` for the default while appending `/{token}` only for an explicit
  `TC_SESSION`. The implementer MUST NOT add a `default/` subdir — that would
  break backward-compat invariant #2.

### Token sanitization

`TC_SESSION` is attacker-adjacent (it names a kernel object on Windows and a
filesystem path on unix). Sanitize before use:

- Allowed: `[A-Za-z0-9._-]`, length 1..=64.
- Reject (fall through to per-user default + log a warning) if it contains a
  path separator, `\\.\pipe\`, `..`, NUL, or exceeds length. This prevents pipe
  squatting via a crafted token and path traversal in the unix data dir.

## Components touched

| Unit | Change | Why |
|---|---|---|
| `supervisor::paths` (new fns) | `session_token(env)` + `endpoint_for_token(token)` resolution | single source of truth both sides call |
| `supervisor::paths::resolve_socket_path_with` | route through token resolution | client (mcp/cli) discovery |
| `supervisor::paths::resolve_state_dir_with` | unix: token subdir (non-default) | per-harness DB/log isolation |
| `daemon::config::DaemonConfig::pipe_name` | route through token resolution | daemon bind name |
| `daemon::config::DaemonConfig::socket_path` | unchanged (derives from data_dir) | already correct on unix |

The resolution function lives in **one place** (`supervisor::paths`, already the
shared crate both daemon and clients depend on) and is consumed identically by
both sides. This is the invariant that makes "no coordination" work: same input
env → same endpoint, computed by the same code.

## Invariants

1. Daemon bind endpoint == client connect endpoint, for identical env. (The
   whole feature fails if these diverge — enforced by both calling one fn.)
2. Unseeded (no `TC_SOCKET`, no `TC_SESSION`) Windows pipe and unix socket are
   deterministic and documented; unix socket is byte-identical to pre-F1.
3. `TC_SOCKET` (full override) always wins over `TC_SESSION` (token).
4. A malformed/hostile `TC_SESSION` never produces a path-traversal data dir or
   a squattable pipe name; it falls back to the per-user default with a warning.
5. Per-harness state (DB, logs, socket) is isolated when `TC_SESSION` differs —
   no two sessions share a SQLite file.

## Error handling

- Malformed `TC_SESSION`: log a one-line warning, use per-user default. Never
  hard-fail (preserves backward-compat tier).
- Windows pipe-name length: the token is capped at 64 chars; the full pipe name
  stays well under the ~256 limit.
- Two harnesses that accidentally set the **same** `TC_SESSION`: they share a
  daemon (by design — same session = same daemon). The F6 fatal-`ALREADY_EXISTS`
  path still protects against a true double-bind race.

## Testing

- `session_token()` precedence: TC_SOCKET > TC_SESSION > default, via the F2
  `EnvSource` fake env (no process-global mutation).
- Sanitization: rejects `../evil`, `\\.\pipe\x`, NUL, over-64-char; accepts
  `abc-123`.
- Endpoint determinism: same env → same endpoint string, both the daemon-side
  (`pipe_name`/`socket_path`) and client-side (`resolve_socket_path_with`)
  produce equal results for the same fake env (the cross-side invariant #1).
- Backward-compat: unseeded unix socket path == the pre-F1 literal.
- Windows pipe name shape under each tier (cfg(windows) test).

## Migration

The Windows unseeded default pipe name changes from
`terminal-commander-{USERNAME}` to `terminal-commander-u-{USERNAME}`. A daemon
from a prior version binds the old name; a new client resolves the new name and
would report `daemon: unavailable` against an old running daemon. Because the
daemon-replace path (`replace.rs`) and autostart will both be on the new
version together (shipped in one release), and a stale daemon is killed +
respawned on version mismatch, this self-heals on the next `ensure_daemon`.
Document in the changelog. Unix is unaffected (socket path unchanged for the
default token).

## Non-goals

- Session **lifecycle management** (minting ids, per-session pidfiles, reaping
  idle daemons). That is a future milestone (the "session supervisor" story);
  this spec only makes endpoints per-harness-addressable. The launcher owns
  setting `TC_SESSION` for now.
- Any central router / aggregator / proxy. Explicitly rejected in favor of
  independent computation.
- Changing the `TC_SOCKET` escape-hatch semantics.

## Out of scope / deferred

- Auto-deriving `TC_SESSION` from the OS console/tty session (was considered;
  fragile for backgrounded daemons). Launcher sets it explicitly.
