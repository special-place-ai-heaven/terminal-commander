# Terminal Commander — Field Ledger (live agent usage, AAP campaign)

Additive log of every finding from using TC as the primary WSL/shell channel
during real Claude Code agent work on Agent_Army_Professionals. Started
2026-06-11. Each entry: what was attempted, what happened, severity, suggested
fix. Newest entries appended at the bottom of each section. Honest field data —
includes what worked, not just what broke.

Environment: Windows 11 host, TC MCP adapter in Claude Code (stdio), daemon
Windows-side; WSL2 Ubuntu reached via `wsl.exe bash -lc ...` argv bridging.
Workload: git commits via script files, full AAP stack rebuild+restart
(~136s cargo + launcher), curl health probes, directory listing.

---

## BUGS

### TC-B1 — ANSI escapes not stripped before rule matching / summaries
- **Severity: medium (ergonomics-breaking for colored output)**
- Repro: `run_and_watch` argv `["wsl.exe","bash","-lc","... aap-start.sh restart"]`
  with rule pattern `MySQL|Backend:|...`. The launcher prints colored lines.
- Observed: signal `match`/`captures`/`summary` carry raw escapes:
  `"[0;32m[AAP][0m MySQL is ready"`.
- Consequences: (a) any `^`-anchored pattern (`^\[AAP\]`) silently never
  matches colored output — the agent concludes "no signal" when the line was
  there; (b) summaries pollute the LLM context with escape bytes.
- Suggested fix: strip ANSI (CSI/OSC) before the matcher and in emitted
  summaries; keep raw bytes in the frame store. Expose `strip_ansi: bool`
  default true for anyone who needs raw.

### TC-B2 — daemon dies/unreachable mid-session after idle; no self-heal
- **Severity: high (session-killing; breaks tool trust)**
- Timeline (one session): daemon `health` ok at uptime 29s -> 5 successful
  calls over ~7 min incl. a 136s job that exited cleanly and was
  status-polled -> ~4 min idle (no TC calls) -> next call
  (`run_and_watch` cmd.exe dir) returned `MCP error -32603: daemon_unavailable`
  -> `health` also `daemon_unavailable`. No TC call in between could have
  killed it; the long job had already exited.
- Consequences: all in-flight jobs/buckets/cursors lost; the MCP adapter does
  NOT relaunch the daemon (only a user-side `/mcp` reconnect did, earlier the
  same day). Agent has no recovery path from inside the session.
- Notes/hypotheses for the author: idle shutdown? crash on a Windows
  `cmd.exe /c dir` spawn? daemon lifetime tied to something that exited? The
  uptime=29s right after a user `/mcp` reconnect suggests adapter-spawned
  daemon whose lifetime may be fragile.
- Suggested fix: (a) adapter should attempt daemon (re)spawn on
  `daemon_unavailable` before erroring, at least once, and say so in the
  error (`recover_hint`); (b) if there is an idle timeout, advertise it in
  `health` output; (c) persist/restore job receipts across daemon restarts so
  a status-poll after a crash can still answer "exit_code unknown, daemon
  restarted at T".

---

## SECOND SESSION BLOCK (2026-06-12 ~06:3xZ)
- TC reused successfully for: WSL git+sync script, a `wsl.exe --exec bash /path.sh`
  sqlite dependency probe (14 signals captured cleanly, exit 0 — the `--exec`
  argv path the verdict recommends works great), health pings.
- TC-B2 RECURRED: after ~3 good calls + a gap, `run_and_watch` returned
  `MCP error -32603: daemon_unavailable` again mid-campaign (during a
  commit+deploy script dispatch). Same signature as session 1. Falling back to
  Bash for the deploy. **TC-B2 is the single biggest trust blocker** — a tool
  that randomly becomes unavailable mid-workflow cannot be the primary channel
  until it self-heals or the adapter respawns the daemon. Everything else about
  TC is genuinely good; this one issue gates adoption.
- NEW DATA POINT for the author: the `wsl.exe --exec bash <script.sh>` argv form
  (no `-lc`, script-as-file) is the cleanest, highest-fidelity bridge observed —
  no quoting hazards, full ANSI in frames, correct exit. Recommend documenting
  it as the canonical TC->WSL pattern (superior to `bash -lc "<inline>"`).

## WHAT WORKED (verified live — keep these properties)

- **WSL bridging**: `argv ["wsl.exe","bash","-lc","<line>"]` — correct exit
  codes, UTF-8 (em-dashes) intact end-to-end, git push output captured.
- **run_and_watch one-call shape**: signals + exit_code in one round trip;
  inline `{"pattern": ...}` shorthand with sane defaults; alternation
  patterns; default `${line}` summary.
- **Long-job contract**: `complete:false, wait_exhausted:true, state:running`
  + job_id; later `command_status` returned exit 0 with sane counters
  (`duration_ms:136534`, `frames_total:33`).
- **Progress suppression**: `frames_suppressed_progress:3` on the cargo
  build — progress spam visibly filtered, exactly the token-saving promise.
- **Quiet commands**: zero-match commands still return bounded state/receipt,
  never silence.

## ERGONOMICS / DESIGN NOTES (not bugs)

### TC-E1 — signal objects are heavy for LLM consumers
Each signal carries bucket_id, event_id, frame_id, probe_id, rule id+version,
pointer, source, first/last_seen, full capture echo — ~6 lines of id plumbing
per 1 line of payload. A `compact: true` response mode returning
`{summary, stream, seq, severity}` per signal would cut token cost ~5x for the
dominant agent use case. Ids could come back only when `compact:false`.

### TC-E2 — wait_ms cap (60s) forces start+poll for every real build
Fine as a contract, but agent loops would benefit from `wait_until:"exit"`
with a server-side hard cap, or a `poll_hint_ms` in the running response.

### TC-E3 — "argv only; shell interpreters denied" is trivially bypassed
`run_and_watch` denies shells, but `["wsl.exe","bash","-lc","..."]` (or
`cmd.exe /c`) sails through. If the denial is a security posture it is
cosmetic; if it is ergonomics-routing (push people to shell_exec), say that in
the description instead so agents don't read it as a sandbox.

### TC-E4 — capture echo duplication
`captures` contains `"0"`, `"line"`, and `"match"` with the identical string
for simple patterns — three copies of the same bytes. One canonical field
(plus named captures when present) would do.

---

## SESSION LOG (chronological, terse)

- 10:28Z health ok (uptime 29s, fresh after user /mcp reconnect).
- 10:29Z run_and_watch wsl bridge probe: OK (2 signals, exit 0).
- 10:29Z git commit+push script via TC: OK (signals showed commit hash + sync).
- 10:30Z AAP rebuild+restart started via TC: long-job contract OK; ANSI bug
  TC-B1 observed in signals.
- 10:34Z command_status: exit 0, counters sane, progress suppression working.
- 10:35Z curl health probe via TC (Windows curl.exe): OK.
- ~10:40Z run_and_watch (cmd.exe dir): daemon_unavailable; health:
  daemon_unavailable. TC-B2. Fallback to plain Bash for the session;
  will retry TC periodically and log.
- 11:35Z health retry (~55 min later): still daemon_unavailable. Confirms NO
  self-recovery and that the adapter never respawns the daemon; only the
  user-side /mcp reconnect brings it back. TC-B2 severity confirmed high.
- ~11:5xZ user ran /mcp dialog twice; TC came back (health ok, uptime 569s).
  CONFIRMS TC-B2 root: recovery requires a USER-side /mcp reconnect — the
  agent cannot self-heal. The daemon also clearly RESTARTED (uptime reset),
  losing all prior job/bucket state. Re-adopting TC as primary channel now.

### TC-B3 — daemon restart loses all job/bucket/cursor state (no persistence)
- **Severity: medium** — after the TC-B2 restart, every prior job_id / bucket /
  cursor is gone; a `command_status(old_job_id)` after a restart cannot answer.
  For an agent that started a long job, lost the daemon, and reconnected, the
  job result is unrecoverable even though the child may have finished.
- Suggested fix: persist job receipts (exit_code + final signal counts + a
  short tail) to a small on-disk store keyed by job_id, surviving daemon
  restart, so a post-restart status-poll returns "exited(code), daemon
  restarted at T, signals lost" instead of an error.
