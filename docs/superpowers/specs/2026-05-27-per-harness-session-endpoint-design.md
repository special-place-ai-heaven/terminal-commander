# Per-Harness Session Endpoint (F1) — Design

Date: 2026-05-27
Status: Approved (plan-ready)
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

The per-user default is **byte-identical to today on BOTH platforms**. The
`{token}` endpoint shape is used ONLY for an explicit (sanitized) `TC_SESSION`;
the unseeded default branch emits the exact legacy endpoint. This avoids any
migration: an old daemon and a new client resolve the same default endpoint, so
there is no same-version partial-upgrade hazard (a new client never silently
misses an old daemon bound to a differently-named pipe).

Mechanically, `endpoint_for_token` distinguishes the per-user default from an
explicit session (via a flag/sentinel) and branches per platform:

- Windows default: legacy pipe `\\.\pipe\terminal-commander-{USERNAME}` (the
  exact current `config.rs:261-268` shape — `USERNAME ?? USER ?? "default"`).
  Explicit session: `\\.\pipe\terminal-commander-{sanitized_token}`.
- Unix default: bare `<base>` data dir (no `{token}` subdir), so the socket is
  byte-identical to today. Explicit session: `<base>/{sanitized_token}`. The
  implementer MUST NOT add a `default/` subdir, and MUST NOT rename the default
  Windows pipe — both would break backward-compat invariant #2.

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
2. Unseeded (no `TC_SOCKET`, no `TC_SESSION`) endpoints are **byte-identical to
   pre-F1 on BOTH platforms** — the Windows pipe stays `terminal-commander-{USERNAME}`
   and the unix socket stays the legacy `<base>/terminal-commanderd.sock`. No
   migration, no partial-upgrade hazard.
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
- Backward-compat: unseeded unix socket path == the pre-F1 literal, AND unseeded
  Windows pipe name == `terminal-commander-{USERNAME}` (the pre-F1 literal).
- Windows pipe name shape under each tier (cfg(windows) test): default → legacy
  name; explicit session → `terminal-commander-{token}`.

## Migration

None. The unseeded default endpoint is byte-identical to pre-F1 on both
platforms (see Backward compatibility + invariant #2), so an old daemon and a
new client always resolve the same default endpoint. Per-harness isolation is
purely additive via `TC_SESSION`. No changelog migration note needed beyond
announcing the new `TC_SESSION` capability.

## Resolution detail (for the plan)

These were raised in spec review; pin them so the plan is unambiguous:

1. **`TC_DATA` vs `TC_SESSION`.** `TC_DATA` sets the data-dir base. `TC_SESSION`
   selects a per-session subdir *under whatever base resolves* (default base or
   a `TC_DATA` custom root). So with both set on unix:
   `<TC_DATA>/{sanitized_token}/terminal-commanderd.sock`. `TC_DATA` does not
   suppress session isolation; it relocates the base the session subdir hangs
   off. (`TC_SOCKET`, being a full endpoint override, still wins over both.)
2. **`daemon.socket_path` / Windows custom pipe.** Today a custom
   `daemon.socket_path` becomes the Windows pipe name verbatim
   (`config.rs:262-263`) and the unix socket path verbatim. This is folded into
   the `TC_SOCKET` tier: `TC_SOCKET` sets `daemon.socket_path`, and an
   explicitly-configured `socket_path` (CLI `--socket` / config file) is treated
   as the same highest-precedence full override. It is NOT a separate fourth
   tier — same semantics, same precedence.
3. **Pidfile + per-session state co-location.** When `TC_SESSION` shifts
   `resolve_state_dir_with` to a subdir, the pidfile, the SQLite DB, and the
   logs MUST all resolve to that same per-session dir. `replace.rs`'s endpoint
   cross-check reads the pidfile from the resolved state dir, so a mismatch here
   would break version-replace. The plan must verify pidfile/DB/log all key off
   the one resolved state dir.

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
