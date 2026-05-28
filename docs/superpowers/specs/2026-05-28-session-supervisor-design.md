# Session Supervisor — Design

Date: 2026-05-28
Status: Approved (plan-ready)
Builds on: F1 per-harness session endpoint
(`docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md`).
Promotes scope deferred as F1 non-goals — see the companion ADR
`docs/adr/ADR-session-supervisor-scope.md`.

## Problem

F1 made per-harness daemon endpoints **addressable**: an opaque `TC_SESSION`
token (minted in JS, resolved in `supervisor`, bound by the daemon) gives each
harness its own daemon. F1 deliberately deferred **lifecycle**: nothing
enumerates sessions, nothing reaps idle daemons, and two real trust gaps were
left for a later milestone:

1. **No daemon-identity handshake.** `supervisor::ensure::probe_endpoint` treats
   ANY connectable pipe/socket as "our daemon already running." A pre-bound or
   stale endpoint, or an unrelated process squatting the name, is accepted as
   ready — suppressing a legitimate spawn or impersonating readiness. (Found in
   F1 code review by codex.)
2. **WSL credential leak via inherited `WSLENV`.** The Win→WSL bridge
   (`lib/wsl/spawn.js`) forwards whatever the parent `WSLENV` lists into the
   Linux process. Real-install verification observed `WSL_SUDO_CREDENTIAL/u`
   crossing the boundary unfiltered — exactly the credential forwarding the
   audit's F-cluster set out to prevent. `buildFilteredEnv` strips secret-shaped
   env *keys* but never sanitizes the `WSLENV` var itself.

Without lifecycle, leaked idle daemons accumulate (closed laptop, abandoned
agent) with no automatic reclamation and no operator visibility.

## Reframing (what F1 + version-replace already provide)

This milestone is **enumerate + reap + harden**, NOT "build a lifecycle
subsystem." Already shipped and reused:

- `state_dir` is already per-session (F1: base dir for the default/unseeded
  session, `<base>/<token>` for seeded sessions).
- A per-session pidfile already exists at `<state_dir>/terminal-commanderd.pid`
  carrying `RunningDaemon { pid, version, endpoint }`
  (`crates/supervisor/src/pidfile.rs`, from the version-replace work).
- Cross-platform `pid_alive` and the M4 identity guard
  `pid_belongs_to_daemon(pid, state_dir)` already exist and are verified.

Consequence: **the filesystem of session dirs + their pidfiles IS the registry.**
No central registry, no new persisted store, no new always-on process.

## Decision

Four units with clear boundaries.

### Unit 1 — `supervisor::sessions` (new Rust module): enumeration

Pure read. `enumerate(base_dir) -> Vec<SessionEntry>`:

- Read `base_dir/terminal-commanderd.pid` → the `default` session (label
  `"default"`), if present.
- For each immediate subdirectory of `base_dir` containing
  `terminal-commanderd.pid` → a seeded session (label = subdir name = token).
- Each entry: `{ label, pid, version, endpoint, alive }` where `alive =
  pid_alive(pid)`. A pidfile whose pid is dead is an orphan (label as `stale`).

No connection to daemons here — this layer only reads the filesystem. Idle time
is layered on top by the CLI via the handshake (Unit 3).

### Unit 2 — daemon self-reap (in `terminal-commanderd`)

The daemon owns its own idle lifecycle. No external watcher.

- In-memory `last_activity: Instant`, **bumped on every real IPC dispatch** (a
  command/bucket/registry/etc. request). NOT bumped by the non-bumping health
  peek (see Unit 3).
- An idle-timer task (tokio interval, ~60s) checks `now - last_activity > TTL`.
  On trip: log the reason, begin **graceful drain** (stop accepting new
  connections; finish in-flight requests; new connections during drain get a
  retryable "shutting down" error so the client cold-spawns a fresh daemon —
  the cheap F1 path), remove own pidfile, exit 0.
- TTL from `TC_IDLE_TTL_SECS`, read once at startup. Default **1800** (30 min).
  `0` disables self-reap (never idle-exit). Conservative + configurable.

In-memory only: no disk write on the IPC hot path.

### Unit 3 — liveness handshake (`supervisor::ensure` + daemon `health`)

Replace "connectable == our daemon" with a real handshake:

```
probe_endpoint(ep):
  connect(ep)
  send IpcRequest::Health
  recv (short bounded timeout)
  → well-formed Health response  → OUR daemon (return idle_secs, version)
  → garbage / timeout / closed   → NOT our daemon (treat as not-running)
```

- The `health` response struct gains `idle_secs: u64`.
- **The health request is a non-bumping peek**: serving it MUST NOT update
  `last_activity`. Rationale (correctness-critical): the handshake is used by
  both `ensure_daemon` (cold start) and `session list`; if it bumped activity,
  any periodic `session list` / monitoring cron would reset every daemon's idle
  timer and **self-reap would never fire** — the feature would silently defeat
  itself. Inspection observes without perturbing. Real command IPC still bumps.
- Reused by `ensure_daemon` (a connectable-but-not-ours endpoint no longer
  suppresses spawn) and by `session list` (idle/version display).

### Unit 4 — `session list|reap` CLI + WSLENV allowlist

**`terminal-commander session list`:** call `enumerate`, then for each ALIVE
session run the health handshake (bounded per-daemon timeout, e.g. 500ms;
pid-alive-but-unresponsive → `unresponsive`). Render:

```
SESSION   PID   STATE         IDLE   ENDPOINT
default   4128  alive         3m     \\.\pipe\terminal-commander-poslj
tc-2e53…  9132  alive         41m    \\.\pipe\terminal-commander-tc-2e53…
tc-cbd6…  7251  stale         -      (orphan pidfile)
```

**`terminal-commander session reap [<token> | --idle | --all]`:** graceful-first,
force-fallback:

```
for each target ALIVE daemon:
  send IpcRequest::Shutdown
  wait bounded (~3s) for endpoint unreachable
  if still reachable:
     if pid_belongs_to_daemon(pid, state_dir):  // existing M4 guard
        hard_kill(pid)                           // force fallback, identity-confirmed
     else: report "endpoint occupied by non-daemon, refusing"
for each STALE (dead-pid) entry: remove the orphan pidfile (no IPC)
```

Reap targets a socket (Shutdown), not a pid — the graceful path has no
cross-process kill. The force fallback (for a wedged daemon that never reads
Shutdown) reuses the already-shipped `pid_belongs_to_daemon` identity guard, so
it introduces no new pid-reuse TOCTOU.

Selectors: `<token>` reaps one named session; `--all` reaps every session;
`--idle [SECS]` reaps sessions whose `idle_secs` (from the handshake) exceeds an
explicit threshold — `SECS` if given, else a CLI default (e.g. 1800). This
threshold is the CLI's own, deliberately INDEPENDENT of each daemon's
`TC_IDLE_TTL_SECS`: `reap --idle` is a manual override that must work even when
a daemon has self-reap disabled (`TC_IDLE_TTL_SECS=0`). A daemon with self-reap
on will usually have exited before `reap --idle` ever sees it; the flag exists
for the self-reap-disabled and force-cleanup cases.

**WSLENV allowlist (`lib/wsl/spawn.js`):** rebuild `WSLENV` from a TC-only
allowlist instead of preserving ambient entries:

```
WSLENV = join(":", [
  "TC_SESSION/u"            (when TC_SESSION set),
  "TC_WSL_DISTRO/u"         (when TC_WSL_DISTRO set),
])   // ambient WSLENV entries dropped entirely
```

Verified (2026-05-28): the WSL-side bridge command is the literal constant
`exec terminal-commander-mcp` (+ literal path/daemon-ensure prefixes) and reads
no Windows env var by name and needs no `/p` path-translated var, so dropping
ambient `WSLENV` breaks nothing the runtime depends on. Allowlist > denylist:
a credential the operator happened to list in `WSLENV` (e.g.
`WSL_SUDO_CREDENTIAL`) no longer crosses the trust boundary.

## Data flow summary

```
session list:  CLI → sessions::enumerate (fs scan) → per-alive: health peek → render
self-reap:     daemon IPC dispatch bumps last_activity; idle-timer drains + exits
ensure:        probe → health handshake → real daemon? reuse : spawn
session reap:  enumerate → Shutdown IPC → (bounded wait) → force-kill if wedged+identity-ok
wsl spawn:     buildFilteredEnv → WSLENV := TC allowlist → wsl.exe
```

## Invariants

1. Backward-compat: with no `TC_SESSION` the default session, its endpoint, and
   its pidfile location are byte-identical to pre-this-milestone. `session list`
   shows the default session; nothing about the unseeded path changes.
2. Precedence `TC_SOCKET > TC_SESSION > per-user default` (F1) is untouched.
3. Every path that acts on a daemon (ensure, list, reap) verifies identity via
   the handshake (and, for the force-kill fallback, `pid_belongs_to_daemon`)
   before trusting or killing it. We never act on a squatter or recycled pid.
4. The health peek never mutates `last_activity`; only real command IPC does.
   Inspection cannot keep an idle daemon alive.
5. Self-reap is graceful: no in-flight request is dropped; a request racing the
   close window gets a retryable error, never a silent loss.
6. No cross-process kill on the graceful reap path; the force fallback is gated
   by the existing identity guard.

## Error handling

- `enumerate`: unreadable/corrupt pidfile → skip that entry (do not fail the
  whole list); a subdir without a pidfile is not a session.
- `session list`: a pid-alive daemon that fails the handshake within the timeout
  → shown as `unresponsive`, not silently dropped.
- `session reap`: graceful Shutdown failing → force fallback; force fallback
  refused when identity does not match → reported, not forced.
- Self-reap: `TC_IDLE_TTL_SECS=0` → timer task is inert (no idle exit).
- WSLENV: malformed/absent ambient WSLENV → allowlist still produces a clean
  `TC_SESSION/u[:TC_WSL_DISTRO/u]`.

## Testing

- **Rust unit:** `sessions::enumerate` (default + seeded + stale, injected fake
  dirs); self-reap idle-timer under virtual time (`start_paused`: TTL elapse →
  exit decision; a bump resets it; a non-bumping peek does NOT reset it);
  handshake accept (well-formed Health) vs reject (garbage/timeout/closed).
- **Rust integration (unix-gated, WSL-verified):** daemon with tiny
  `TC_IDLE_TTL_SECS` self-exits + removes pidfile; `session list` shows-then-gone;
  `session reap <token>` graceful path; wedged-daemon force-fallback (mock a
  non-draining listener bound to the endpoint).
- **JS:** WSLENV rebuilt equals exactly the allowlist with ambient dropped; a
  real `wsl.exe` hop asserts `WSL_SUDO_CREDENTIAL` does NOT appear Linux-side
  (the inverse of the verification that found the leak).
- **E2E smoke:** extend `scripts/smoke/verify-session-isolation-smoke.ps1` —
  start two sessions, `session list` shows both, `session reap --all`, both gone.

## Non-goals (re-affirmed from F1; see ADR)

- No central router / aggregator / proxy. Enumeration is a filesystem scan +
  per-daemon handshake, not a coordinating process. (Still rejected.)
- No always-on supervisor process. Reaping is daemon-self + manual CLI.
- No auto-derive of `TC_SESSION` from the tty/console session. (Still rejected.)
- No cryptographic daemon authentication. Same-user local-IPC trust model: an
  attacker already holding the user's privileges is out of model. The handshake
  defends accidental collision / stale bind / wrong process, not a malicious
  same-user impersonator.
