# Omni Playbook -- the agent decision tree

Audience: an LLM agent (or the human configuring one) deciding WHICH
Terminal Commander tool to reach for. This is a how-to guide: pick the
lane, run it, read the bounded result. It is not a reference for every
field -- that is `docs/mcp/TOOL_CONTROL_SURFACE.md`.

"Omni" is the program promise: an agent should never need a separate raw
terminal tool. This page is the map from intent to tool. Every lane
returns BOUNDED, COMBED signals -- never raw stdout/stderr.

Language: ASCII only.

## The one-screen decision tree

```text
Need to run or observe something?
|
+-- Run a single program (argv), known signal?
|     +-- It has a rule pack (cargo, pytest, npm, docker, kubectl, git, ...)
|     |     -> registry_import_pack <pack>   (once)   then
|     |     -> run_and_watch argv=[...] rules=[...]    OR rely on active pack rules
|     +-- No pack, but you know what to match
|           -> run_and_watch argv=[...] rules=[{"pattern":"ERROR"}]
|
+-- One-shot pipeline / compound / redirect (grep ... | wc -l, make 2>&1 | tee)?
|     -> shell_exec { shell_line: "..." }      (needs allow_shell; default off)
|
+-- Multi-step work that shares cwd/env (cd build; cmake ..; make)?
|     -> shell_session_start  then  shell_session_exec (one line per step)
|        (needs allow_session; default off; UNIX-ONLY)
|        save/restore with workspace_snapshot_create / _apply
|
+-- Interactive program / REPL (python, psql, a prompt that asks back)?
|     -> pty_command_start  then  pty_command_write_stdin
|        (dual backend: unix pty-process + Windows ConPTY)
|
+-- Output whose format you do NOT know yet?
|     -> run it, then command_output_tail  (bounded, no rule needed)
|        -> registry_suggest_from_samples { samples: [...] }   (DRAFT rules)
|        -> registry_test  ->  registry_upsert  ->  registry_activate
|        (suggest NEVER auto-activates; you drive the loop)
|
+-- Read or write a file (not run a program)?
|     +-- Read a bounded window / search one file
|     |     -> file_read_window { path, start_line?, max_lines? }
|     |     -> file_search { path, query }           (bounded matches + pointers)
|     +-- Write content to a file
|           -> file_write { path, content, create_dirs? }
|              (policy-gated by paths.write_allow; audited BEFORE the write;
|               bounded size; atomic temp+rename, no torn writes; MUTATING /
|               non-idempotent -- the client never auto-retries it. read_only_observer
|               denies it; repo_only confines it to $REPO_ROOT.)
|
+-- Run any of the above on a REMOTE host?
|     -> add target_id=<id> to the tool call  (needs allow_remote; default off;
|        reached only via an operator-established ssh -L forward, no public TCP)
|
+-- A privileged system op (install a package, restart a service)?
      -> privileged_* tools  WHEN ENABLED.  As of this program: NOT AVAILABLE
         (P4 is plan-only; omni_status reports available:false,
          reason:"threat_review_pending"). Until then this branch is closed.
```

## 1. Run something known: import a pack, then run

For a tool that has a curated rule pack, import the pack once, then run.
The pack supplies expert signal extraction so you do not hand-author
JSON.

```text
registry_import_pack pack="cargo"
run_and_watch argv=["cargo","test"] rules=[{"pattern":"FAIL"}]
```

25 packs ship (import by name): `ansible`, `apt`, `bundler`, `cargo`,
`choco`, `cleanup`, `docker`, `dotnet`, `gcc`, `generic.terminal`,
`git`, `go`, `kubectl`, `make`, `msbuild`, `npm`, `pip`, `pnpm`,
`pytest`, `ssh`, `systemd`, `terraform`, `uv`, `winget`, `yarn`.

If you run a known tool WITHOUT its pack, a command-start response may
carry a `hint: { kind: "pack_available", pack, action:
"registry_import_pack" }` -- that is the daemon telling you a pack
exists. Prefer SCOPED activation (`{"kind":"job","job_id":...}`) or
per-command inline rules over global activation, so a `warning:` matcher
meant for one tool does not fire on every command.

`run_and_watch` is the one-shot workhorse: start, wait (bounded) for
rule signals + exit, return both. A quiet command (zero matches) returns
a RECEIPT (exit code, lines suppressed, short tail), never a bare empty
success. For longer jobs use `command_start_combed` + `bucket_wait`.

## 2. One-shot pipeline: shell_exec

Some real work is irreducibly a pipeline or compound. The argv lane
cannot express it (shell interpreters are denied as `argv[0]`). Use
`shell_exec` for ONE shell line:

```text
shell_exec { shell_line: "grep -r TODO src | wc -l" }
```

- Gated by `allow_shell` (default OFF; operator config TOML, not an
  MCP parameter). On the default profile it returns `PolicyDenied`.
- Output is combed exactly like the argv lane -- never raw.
- Details and the residual-risk discussion: `docs/runtime/SHELL_RUNTIME.md`
  and `POLICY.md` section 4.1.

## 3. Multi-step shared state: shell sessions

When steps share working directory and environment, use a persistent
session instead of re-passing `cwd`/`env` on every call.

```text
shell_session_start { }                         -> { session_id, ... }
shell_session_exec  { session_id, line: "cd build" }
shell_session_exec  { session_id, line: "cmake .." }
shell_session_exec  { session_id, line: "make" }
shell_session_status{ session_id }              -> { state, cwd, env_snapshot, ... }
workspace_snapshot_create { session_id }        -> { snapshot_id }
workspace_snapshot_apply  { snapshot_id, session_id }
shell_session_stop  { session_id }
```

- Gated by `allow_session` (default OFF) and UNIX-ONLY: on a non-unix
  daemon the session tools return `UnsupportedPlatform`.
- `cd` state persists because the shell process is persistent.
- `status.cwd` is BEST-EFFORT (it tracks plain `cd <single-arg>` only).
  To read the authoritative directory, `shell_session_exec` a `pwd` and
  read the combed signal. See `docs/runtime/SHELL_SESSION.md` section 6.
- A start past `max_sessions` is refused loudly (`SessionLimitExceeded`),
  never a silent hang.

## 4. Interactive / REPL: pty_command

For a program that prompts and reads back -- a REPL, an installer that
asks a question -- use the PTY lane:

```text
pty_command_start { argv: ["python3"] }         -> { job_id, bucket_id, ... }
pty_command_write_stdin { job_id, data: "print(1+1)\n" }
pty_command_stop { job_id }
```

- Dual backend: unix `pty-process` and Windows ConPTY. `system_discover`
  reports per-host availability and the `omni_status.pty.platform`
  string (`posix`, `windows_conpty`, or `unavailable`).
- The secret-prompt guard refuses LLM-supplied input while a password
  prompt is detected.
- Honest host caveat: live Windows ConPTY child-output end-to-end is
  gated behind `TC_CONPTY_E2E=1` and not yet fully closed on every dev
  host; check `system_discover` before relying on it on native Windows.

## 5. Unknown output: tail, suggest, test, activate

When you do not know the output format yet, do NOT guess a rule. Run the
command, read a bounded tail, then let the daemon PROPOSE draft rules
from the samples -- and you drive them to activation.

```text
run_and_watch argv=["df","-h"] rules=[]         (or command_start_combed)
command_output_tail { job_id }                  -> bounded last lines (200 / 64 KiB)
registry_suggest_from_samples { samples: [<tail lines>] }
   -> { proposed_rules: [...], confidence: "heuristic",
        next_steps: ["registry_test","registry_upsert","registry_activate"] }
registry_test    { ... }                        (does the draft match?)
registry_upsert  { ... }                        (persist a new immutable version)
registry_activate{ ... }                        (scope it: job/bucket/probe/global)
```

Invariant: `registry_suggest_from_samples` is pure-Rust heuristics and
NEVER auto-activates. The loop is always suggest -> test -> activate, and
activation is your explicit step. `command_output_tail` is for one-off
exploration; for a recurring signal, define a rule instead of tailing
every time.

Optional always-on baseline: the operator can set
`sifters.universal_extractors = true`, which emits bounded LOW-severity
error/warning/exit/progress signal for a command that has NO
tool-specific rule. That is a config behavior, not a tool you call.

Important and intentional: universals are a FALLBACK, not an additive
layer. They emit ONLY when no scoped active rule and no inline rule
apply to the command. If ANY pack rule or inline rule is in play, the
flag adds nothing -- a real pack always out-ranks the baseline, and the
universals are NOT merged alongside it. So enabling the flag does not
sprinkle baseline LOW signal onto pack-covered commands; it only covers
the otherwise-uncovered ones. This conservative behavior is by design.

## 6. Remote hosts: target_id

To run a daemon-backed tool against a remote host, add `target_id`:

```text
target_list { }                                 -> { targets: [{ target_id, host, reachable }] }
target_probe { target_id }                      -> { reachable, daemon_version? }
run_and_watch argv=[...] rules=[...] target_id="build-box"
```

- Gated by `allow_remote` (default OFF).
- Transport is an operator-established `ssh -L` forward to the REMOTE
  daemon's LOCAL socket. There is NO public TCP listener, and the
  adapter never spawns ssh -- the operator sets up the tunnel.
- Combing happens on the remote daemon, so the bounded/structured signal
  contract is identical local vs remote.
- Honest caveat: federation is proven via a second-local-socket
  simulation; real-SSH transit is not yet exercised in CI (no sshd in
  the smoke env). `target_id` is wired on the command path; it is not
  yet threaded through all 50 tools.

## 7. Privileged system ops: NOT AVAILABLE (plan-only)

Installing a package or restarting a service would use the `privileged_*`
tools WHEN ENABLED. They are NOT shipped in this program: P4 is
plan-only, blocked on a threat review. `system_discover` reports
`omni_status.privileged_helper = { available: false, reason:
"threat_review_pending" }`. Do not attempt this branch until that
review is signed off. Design and rationale:
`docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`.

## 8. Cross-cutting rules

These apply to every lane:

- `wait_exhausted: true` means STILL RUNNING -- poll `command_status`,
  do not treat it as finished.
- `degraded: true` means an IPC error interrupted a wait -- follow the
  `recover_hint` (re-attach with the returned `job_id`).
- Pass `compact: true` on signal-returning tools for a smaller
  projection when you only need summary/stream/seq/severity. Compact is
  PRESENTATION-ONLY and DROPS the id and rule metadata (`event_id`,
  `bucket_id`, `source`, `pointer`, rule fields); the event store keeps
  the full record. If you need an `event_id` -- e.g. to call
  `event_context` -- do NOT read it off a compact response (it is not
  there): re-fetch the full event via `bucket_events_since` /
  `bucket_wait`. Do not drive automated actions off compact alone.
- Pass `wait_until: "exit"` on wait-capable tools to wait for exit
  (server-capped; a still-running response carries `poll_hint_ms`).
- `strip_ansi` defaults true on `command_start_combed` / `run_and_watch`
  (the raw frame is kept internally for context).
- Never ask for unbounded output. Every response is intentionally
  capped and truncation is always flagged.

## 9. See also

- `docs/mcp/TOOL_CONTROL_SURFACE.md` -- the full per-tool contract.
- `docs/runtime/SHELL_RUNTIME.md` -- `shell_exec` (one-shot lane).
- `docs/runtime/SHELL_SESSION.md` -- persistent sessions + snapshots.
- `POLICY.md` section 4.1 -- the `[policy.caps]` capabilities.
- `README.md` -- the omni tool surface and safety posture.
