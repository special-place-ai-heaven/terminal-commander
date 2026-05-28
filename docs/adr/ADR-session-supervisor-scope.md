# ADR: Session supervisor scope (promote lifecycle from F1 non-goals)

Status: Accepted
Date: 2026-05-28
Builds-on: F1 per-harness session endpoint
(`docs/superpowers/specs/2026-05-27-per-harness-session-endpoint-design.md`).
Design: `docs/superpowers/specs/2026-05-28-session-supervisor-design.md`.

## Context

F1 made per-harness daemon endpoints addressable via the opaque `TC_SESSION`
token, and explicitly listed as **non-goals**:

> - Session **lifecycle management** (minting ids, per-session pidfiles, reaping
>   idle daemons). That is a future milestone (the "session supervisor" story).
> - Any central router / aggregator / proxy. Explicitly rejected.
> - Auto-deriving `TC_SESSION` from the OS console/tty session.

The session-supervisor milestone is that future story. It needs to take on some
of those deferred items, so this ADR records WHICH non-goals are promoted to
goals and which stay rejected — so the promotion is deliberate and bounded, not
scope drift.

Two trust gaps surfaced during F1 review/verification also belong to this
milestone (they have no home in F1's addressing-only scope):

- No daemon-identity handshake at probe time (codex code review).
- Inherited `WSLENV` forwards arbitrary vars (observed: `WSL_SUDO_CREDENTIAL`)
  across the Win→WSL boundary unfiltered (real-install verification).

## Decision

**Promoted from F1 non-goals to goals in this milestone:**

1. **Reaping idle daemons** — via daemon self-reap (in-memory idle TTL,
   `TC_IDLE_TTL_SECS`, default 1800s, `0`=off) plus a manual
   `terminal-commander session reap` override. Graceful-shutdown-via-IPC first;
   force-kill fallback only for a wedged daemon, gated by the existing
   `pid_belongs_to_daemon` identity guard.
2. **Session enumeration / management** — `terminal-commander session list|reap`,
   sourced from the filesystem (session dirs + their existing pidfiles) plus a
   per-daemon liveness handshake. No new persisted registry store.

**Promoted trust hardening (no home in F1):**

3. **Daemon-identity handshake** — `probe_endpoint` sends a real `health` IPC and
   requires a well-formed terminal-commander response before treating an
   endpoint as "our daemon."
4. **WSLENV allowlist** — the Win→WSL bridge rebuilds `WSLENV` from a TC-only
   allowlist (`TC_SESSION/u` only — the distro is selected host-side, so
   `TC_WSL_DISTRO` need not cross), dropping ambient entries.

**Still rejected (F1 non-goals that remain non-goals):**

- **No central router / aggregator / proxy.** Enumeration is a filesystem scan
  plus independent per-daemon handshakes — no coordinating process, no shared
  registry daemon. The F1 "no coordination" principle stands.
- **No always-on supervisor process.** Reaping is daemon-self + on-demand CLI.
  We deliberately do NOT introduce a long-lived watcher (a new always-on process
  + new failure mode), which would contradict the F1 decision.
- **No auto-derive of `TC_SESSION` from tty/console.** Still fragile for
  backgrounded daemons; the launcher sets the token explicitly.
- **No cryptographic daemon authentication.** Same-user local-IPC trust model:
  an attacker holding the user's privileges is out of model. The handshake
  defends accidental collision / stale bind / wrong process, not a malicious
  same-user impersonator.

## Consequences

- "Per-session pidfiles" — listed as an F1 non-goal — needs no new STORAGE: the
  version-replace pidfile is already per-session because F1 made `state_dir`
  per-session. But the milestone DOES add new pidfile SEMANTICS (codex): a raw
  stale-pidfile parser (`read_pidfile` hides dead pids), compare-before-delete
  cleanup (vs blind unlink), and the explicit decision that `TC_SOCKET` daemons
  are out of enumeration scope. "Reads existing pidfiles" is accurate; "zero new
  work" was not.
- Backward-compat invariants from F1 hold unchanged: unseeded default is
  byte-identical; `TC_SOCKET > TC_SESSION > default` precedence untouched. NOTE
  the registry model covers TC_SESSION sessions + the single default ONLY;
  `TC_SOCKET`-tier (custom full-path) daemons keep `state_dir` at the base and
  are deliberately OUT of `session list/reap` scope (operator-owned escape
  hatch). The precedence is preserved; the management surface simply does not
  claim to enumerate arbitrary custom-socket daemons.
- Self-reap is default-on (conservative TTL), so existing single-harness installs
  gain automatic idle reclamation; power users / CI set `TC_IDLE_TTL_SECS=0` to
  opt out.
- The "no central coordinator" architecture is preserved: the filesystem is the
  registry, each daemon owns its own lifecycle, and clients compute endpoints
  independently — exactly the F1 model, extended with self-reaping and an
  inspection CLI.
