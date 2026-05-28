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

## Reality corrections (post-codex-review, 2026-05-28)

A codex spec-review against the actual code corrected several first-draft
claims. The design SHAPE survives; these are the honest implementation facts the
plan must carry (so effort is not under-estimated as "reuse"):

- **`IpcRequest::Shutdown` does not exist** (`crates/daemon/src/ipc/protocol.rs`).
  Self-reap + `session reap` need a NEW protocol variant + dispatcher arm +
  runtime shutdown source + client method. Net-new, not reuse.
- **`Health` returns only `uptime_secs`** (`protocol.rs`, `server.rs:~390`); it
  does NOT carry `idle_secs`/`version`. Adding `idle_secs` is a wire-shape change
  and the new probe MUST tolerate a daemon that returns the OLD Health shape
  (partial-upgrade: a current daemon answering the legacy Health is still "ours"
  — absence of `idle_secs` ⇒ treat idle as unknown, not "not our daemon").
- **Health is NOT side-effect-free today**: every accepted IPC request emits a
  persistent audit row (`server.rs:~553` → `audit.rs` → DB insert). The
  "non-bumping peek" therefore must ALSO skip the audit write, not merely skip
  `last_activity` — otherwise `session list` polling spams the audit log.
- **Graceful drain is net-new infra**: accept loops spawn per-connection tasks
  and do not track/join them; `shutdown()` joins only the accept loop
  (`server.rs`, `pipe_server.rs`). Draining in-flight work requires new tracking,
  not a timer detail.
- **`read_pidfile` returns `None` for dead pids** (`pidfile.rs:46-52`), discarding
  the contents `session list` needs to show a STALE entry. Enumeration needs a
  RAW pidfile parse + separate `pid_alive` classification, not `read_pidfile`.
- **`TC_SOCKET`-tier daemons are OUT OF SCOPE for list/reap** (decision): a
  custom full-path socket override keeps `state_dir` at the base
  (`paths.rs:57-63`), so several would overwrite the one base pidfile and all
  appear as `default`. `TC_SOCKET` is the explicit power-user escape hatch — the
  operator named the socket and owns its lifecycle. `session list/reap` manage
  TC_SESSION-keyed sessions + the single default; they do NOT track arbitrary
  TC_SOCKET daemons. Documented, not worked around with a central index.

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
- Uses a RAW pidfile parse (read + deserialize the JSON) + a separate
  `pid_alive` check — NOT `pidfile::read_pidfile`, which returns `None` for dead
  pids and would hide exactly the stale entries `list` must show.
- Scopes to TC_SESSION sessions + the default only. Does not attempt to discover
  `TC_SOCKET` (custom-path) daemons (see Reality corrections).
- **`default` is a reserved label (codex):** the unseeded session is labeled
  `"default"`, but the F1 validator currently ACCEPTS `default` as a literal
  `TC_SESSION` token — a seeded `TC_SESSION=default` would collide with the
  default session's label/selector in `list`/`reap`. Resolve by reserving it:
  the session module rejects `default` (case-insensitively) as a session token
  (and the JS `isValidSessionToken` / mint mirror the reservation), OR `list`
  disambiguates (e.g. `default` for the base, `session:default` for a seeded
  one). Reservation is preferred — simpler, and a token literally named
  "default" has no legitimate use.

No connection to daemons here — this layer only reads the filesystem. Idle time
is layered on top by the CLI via the handshake (Unit 3).

Stale-pidfile cleanup is **compare-before-delete**, never a blind unlink: re-read
the pidfile immediately before removing it and delete only if it still names the
same dead pid. This closes the race where a daemon restarts (writing a fresh
pidfile) between the stale classification and the cleanup — a blind
`remove_pidfile` would otherwise delete a LIVE daemon's pidfile.

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
  → well-formed Health response  → OUR daemon (idle_secs if present, else unknown)
  → garbage / timeout / closed   → NOT our daemon (treat as not-running)
```

- The `health` response struct gains an OPTIONAL `idle_secs` (additive to the
  existing `uptime_secs`). **Concrete wire shape (codex):** the field is decoded
  as `Option<u64>` with `#[serde(default)]` (or the probe deserializes into a
  lenient struct where missing fields are allowed), so a LEGACY Health response
  (no `idle_secs`) still parses as a well-formed Health — it just yields
  `idle_secs = None` (unknown). The accept test is "deserializes as a Health
  response," never "has idle_secs." This avoids a new client rejecting an old
  daemon and racing into a double-bind.
- **`session list` data sources (codex — pick one per field):** `version` comes
  from the PIDFILE (enumeration, Unit 1), NOT the handshake; the handshake only
  supplies `idle_secs` (and liveness). So a stale/unresponsive daemon still shows
  its pidfile `version`; only `idle` is blank when the handshake yields no
  `idle_secs`.
- **The health request is a non-bumping, AUDIT-FREE peek**: serving it MUST NOT
  update `last_activity` AND MUST NOT emit the per-request persistent audit row
  that ordinary IPC requests write (`server.rs:~553`). Rationale
  (correctness-critical): the handshake is used by both `ensure_daemon` (cold
  start) and `session list`; if it bumped activity, any periodic `session list` /
  monitoring cron would reset every daemon's idle timer and **self-reap would
  never fire** (the feature would silently defeat itself); and if it audited, the
  same polling would spam the audit log. Inspection observes without perturbing
  lifecycle OR the audit trail. Real command IPC still bumps and audits.
- **Identity scope (honest):** the handshake proves "a live terminal-commander
  daemon answered at this endpoint," NOT "the daemon that owns this session /
  state_dir." That is sufficient for the realistic threat (stale bind, accidental
  collision, a non-tc process squatting the name) and for `ensure`/`list`. The
  STRONGER "this exact session's daemon" check exists only on the force-kill path
  via `pid_belongs_to_daemon` (which matches image + state_dir in the cmdline).
  The spec does not claim session-binding identity on the list/ensure paths.
- Reused by `ensure_daemon` (a connectable-but-not-ours endpoint no longer
  suppresses spawn) and by `session list` (idle/version display).

### Unit 4 — `session list|reap` CLI + WSLENV allowlist

**`terminal-commander session list`:** call `enumerate`, then handshake each
ALIVE session for idle/version. Handshakes run **concurrently** (not serial) with
a per-daemon timeout (~500ms) so total latency is bounded by the slowest single
daemon, not `N × 500ms` (codex). Classification after the handshake:
- well-formed Health → `alive` (+ idle/version)
- pid alive but handshake times out / garbles → `unresponsive`
- pidfile pid was alive at enumerate but the daemon EXITED before the handshake
  (connect refused / pidfile now gone) → `gone` (re-check `pid_alive`; do not
  mislabel a cleanly-exited daemon as `unresponsive`)

Render:

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
  send IpcRequest::Shutdown        // daemon ACKs, then graceful-drains in-flight work
  wait up to GRACE for endpoint unreachable   // GRACE generous, see below
  if still reachable after GRACE:
     if pid_belongs_to_daemon(pid, state_dir): // existing M4 guard
        hard_kill(pid)                          // force fallback, identity-confirmed
     else: report "endpoint occupied by non-daemon, refusing"
for each STALE (dead-pid) entry: compare-before-delete the orphan pidfile (no IPC)
```

**Shutdown wire shape (codex — net-new, must be specified by the plan):**
- `IpcRequest::Shutdown` — new request variant (`crates/daemon/src/ipc/protocol.rs`).
- `IpcResponse::ShutdownAck { draining: bool }` — new response variant: the
  daemon sends it IMMEDIATELY on receipt (this is the ACK that distinguishes
  "draining" from "wedged"), then stops accepting new connections and drains.
- New `IpcErrorCode::ShuttingDown` — returned to any NEW connection that arrives
  during the drain window, so the client treats it as retryable and cold-spawns
  a fresh daemon (the cheap F1 path), never a hard failure.
The dispatcher gets a `Shutdown` arm wired to a runtime shutdown source (new,
alongside the existing OS-signal path in `crates/daemon/src/runtime.rs`); the
client (`McpDaemonClient` / CLI) gets a `shutdown()` method.

Reap targets a socket (Shutdown), not a pid — the graceful path has no
cross-process kill. The force fallback (for a daemon that is WEDGED — never ACKs
Shutdown, never drains) reuses the already-shipped `pid_belongs_to_daemon`
identity guard, so it introduces no new pid-reuse TOCTOU.

**GRACE must not race a legitimately-draining long request** (codex): a daemon
that ACKed Shutdown and is draining a slow in-flight command is NOT wedged.
Distinguish the two: on Shutdown the daemon (a) immediately ACKs and stops
accepting new connections, then (b) drains. Reap waits for the endpoint to close;
if the daemon ACKed, reap keeps waiting (the daemon is making progress) up to a
generous cap; force-kill fires only when there was NO ACK within a short window
(truly wedged) or the drain exceeds a hard ceiling. The Shutdown ACK is the
signal that separates "draining" from "wedged" — without it, a fixed ~3s would
kill a daemon mid-legitimate-drain. (Note `hard_kill` itself already escalates
TERM→KILL after 800ms on Unix; the spec's GRACE governs WHEN we resort to it.)

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
WSLENV = "TC_SESSION/u"     (when TC_SESSION set; else WSLENV unset)
                            // ambient WSLENV entries dropped entirely
```

`TC_WSL_DISTRO/u` is NOT included (codex): the distro is selected HOST-side
before spawn (`lib/wsl/spawn.js` resolveBridgeDistro → `wsl.exe -d <distro>`);
the Linux-side command never reads `TC_WSL_DISTRO`, so forwarding it is dead
weight. Only `TC_SESSION` actually needs to cross.

Verification (2026-05-28, corrected from the first draft): the bridge command is
NOT a bare literal exec — it is `LINUX_PATH_PREFIX + BRIDGE_DAEMON_ENSURE + exec
terminal-commander-mcp`, and `BRIDGE_DAEMON_ENSURE`
(`lib/bootstrap/constants.js:18`) sources `$HOME/.config/terminal-commander/
autostart.sh` inside WSL. That script reads `TC_DATA` (with a `:-default`
fallback). A real `wsl.exe` hop with `WSLENV=TC_SESSION/u` confirmed: (a)
`TC_SESSION` crosses intact; (b) `autostart.sh` was never receiving `TC_DATA`
via WSLENV anyway (it wasn't forwarded pre-change either), so it already uses its
default — dropping ambient WSLENV changes nothing for TC's OWN runtime. The
honest scope of the change (codex): an operator who DELIBERATELY listed extra
vars in their ambient `WSLENV` (e.g. `WSLENV=TC_DATA/u` to push a custom data
dir into WSL) would lose that forwarding. That is acceptable and intended — it is
exactly the unbounded-passthrough the credential leak rode in on — but it is a
real behavior change, not "zero regression." This is a deliberate switch from
"preserve ambient + append" → "TC-only"; the changelog calls it out, and an
operator needing a var in WSL sets it inside the distro rather than via ambient
`WSLENV`.
Allowlist > denylist: a credential the operator listed in `WSLENV` (the observed
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
   `TC_SOCKET`-tier daemons are out of `session list/reap` scope (operator-owned).
3. Every path that acts on a daemon goes through the **liveness handshake**,
   which proves "a live terminal-commander daemon answered" (not "this session's
   daemon" — see Unit 3 identity scope). The force-KILL path additionally gates
   on `pid_belongs_to_daemon` (image + state_dir match), so we never force-kill a
   squatter or recycled pid. (Honest scoping: list/ensure trust liveness, not
   session-binding; only force-kill verifies session ownership.)
4. The health peek never mutates `last_activity` AND never writes an audit row;
   only real command IPC does. Inspection cannot keep an idle daemon alive nor
   pollute the audit trail.
5. Self-reap is graceful: no in-flight request is dropped; a request racing the
   close window gets a retryable error, never a silent loss. (Requires net-new
   in-flight tracking — see Unit 2 / Reality corrections; the current server does
   not track per-connection tasks.)
6. No cross-process kill on the graceful reap path; the force fallback fires only
   on a wedged daemon (no Shutdown ACK / drain exceeds ceiling) and is gated by
   the existing identity guard.

## Error handling

- `enumerate`: unreadable/corrupt pidfile → skip that entry (do not fail the
  whole list); a subdir without a pidfile is not a session.
- `session list`: a pid-alive daemon that fails the handshake within the timeout
  → `unresponsive`. A daemon that EXITED between enumerate and handshake
  (connect refused / pidfile vanished, `pid_alive` now false) → `gone`, NOT
  `unresponsive`. Neither is silently dropped.
- `session reap`: graceful Shutdown failing → force fallback; force fallback
  refused when identity does not match → reported, not forced.
- Self-reap: `TC_IDLE_TTL_SECS=0` → timer task is inert (no idle exit).
- WSLENV: malformed/absent ambient WSLENV → allowlist still produces a clean
  `TC_SESSION/u` (or unset WSLENV when `TC_SESSION` is unset).

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
