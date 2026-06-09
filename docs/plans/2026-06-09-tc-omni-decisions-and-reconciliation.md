# TC Omni-Tool — Decisions + Reconciliation (Claude x Cursor)

**Date:** 2026-06-09
**Status:** DESIGN LOCKED — awaiting user sign-off, then writing-plans (Wave 1 / TC49).
**Inputs reconciled:**
- Claude gap analysis: `.planning/cursor_report_03.md` (code-verified).
- Claude shell draft: `.planning/PLAN-allow-shell.md` (now SUPERSEDED — see Decision 2).
- Cursor master program: `docs/plans/2026-06-09-tc-omni-tool-master-program.md` + waves 1-6.

This doc is the single source of truth for the cross-decisions. Where it differs from any
wave doc, THIS doc wins until the wave docs are amended in writing-plans.

---

## Verification of Cursor's program (do not trust, verify)

All load-bearing citations checked against live code/docs at v0.1.47:

| Cursor claim | Check | Result |
|---|---|---|
| Shell seam `command.rs:73-77` (`allow_shell`/`CommandShellStart`) | read | EXACT |
| `PolicyAction` enum exists, `CommandStart` variant | read | EXACT (`policy.rs:59`) |
| `COMMANDS_DENY` privilege list | read | byte-exact |
| Wave-4 helper derives from `PRIVILEGE_MODEL.md §5` | read | FAITHFUL — §5 "privileged-helper question (deferred)" pre-binds "separate goal / small single-purpose / closed allow-list, never a generic shell" |
| 8 program docs written to disk | ls | all present (untracked) |
| `runtime.rs`, `SPEC.md`, `BACKLOG.md`, `COMMAND_RUNTIME.md`, `EVIDENCE_REPORT_RUNTIME.md` | ls | all exist |

**Verdict:** Cursor's master program is grounded and accurate. ADOPTED as the program spine
(3 layers, O-01..O-14 acceptance matrix, 6 waves, doc-amendment list, "what NOT to do").

Minor corrections folded below: effort figures are judgment (not measured); Wave-1 wiring target is
`state.rs` (VERIFIED: `bootstrap` constructs `CommandRuntime`/`WatchRuntime`/`PtyRuntime` at
`state.rs:228-247` — `ShellRuntime` hangs off `DaemonState` here), NOT `runtime.rs` (which is TC36
self-check / foreground-idle, `runtime.rs:8-16`). Cap schema lives in `config.rs` `[policy]`/`[caps]`.
New `shell.rs` mirrors the `pty_command.rs`/`command.rs` split. `lib.rs` re-exports the new types.

---

## Locked decisions

### Decision 1 — Trust model: HYBRID
Granular opt-in capabilities, ALL default-false, deny-first preserved:
`allow_shell`, `allow_privileged` (gates the Wave-4 helper, NOT generic sudo), `allow_remote`,
`allow_session`. PLUS a convenience profile **`full_access`** that bundles all caps ON (audited)
for the trusted/unrestricted case. Existing four profiles unchanged; safe-by-default intact.

**`full_access` guardrails (round 2, Cursor — binding):**
1. NEVER default. Requires explicit TOML config + daemon restart; `developer_local` stays safe.
2. NOT MCP-toggleable. Config/TOML only (same as profile switching today) — no tool flips it.
3. Bundle = all caps ON, NOT audit OFF. Shell/privileged/remote stay AllowWithAudit; the policy
   engine is never short-circuited.
4. `policy_status` EXPOSES the caps (`allow_shell:true`, ...) — no opaque "full_access magic".
5. Scary docs: README warns trusted-machine / single-operator only.

Implementation: the `full_access` profile sets the four caps true in `[caps]`; NO fifth code path
that bypasses `evaluate()`.

### Decision 2 — Shell mechanism: `shell_exec` ONLY in v1; alt door DEFERRED (revised round 2)
Round 1 picked "both doors, one lock". Cursor round 2 made the YAGNI/code-superiority case to drop
the alt door from v1, and it wins (it beats the round-1 pick on the correctness-first meta-rule).

Wave-1 ships ONE shell shape:

- **`shell_exec { shell_line, shell?, cwd, env, rules, wait_ms }`** + new `ShellRuntime`. Dedicated
  field; daemon spawns `[shell,"-lc",shell_line]` ONLY on an Allow/AllowWithAudit
  `PolicyAction::CommandShellStart` verdict; separate audit action `command_shell_start`.
- The argv lane (`command_start_combed`) keeps `SHELL_INTERPRETERS_DENY` a HARD DENY — unchanged.

**Alt door (`command_start{argv, allow_shell}`) — DEFERRED / harness-compat-only.** Dropped from v1:
a second door doubles the test matrix, splits the OMNI playbook (agents pick the wrong door), needs
extra argv-shape validation that `shell_line` expresses for free, and risks handler drift. Add it
ONLY when a concrete harness CANNOT send a `shell_line` field AND the cost is measured.

**IF the alt door is ever added (binding constraints):**
1. Normalize at the IPC handler boundary into ONE internal `ShellExecSpec { shell, line, cwd, env }`
   — no separate spawn path.
2. Both doors hit the SAME `CommandShellStart` verdict (the "one lock" invariant survives).
3. ONE parameterized test suite (`door: shell_exec | allow_shell_argv`).
4. OMNI_PLAYBOOK documents `shell_exec` ONLY; the alt door is harness-compat, not LLM-facing.

`.planning/PLAN-allow-shell.md` is SUPERSEDED; its `allow_shell`+argv design survives only as the
deferred alt-door spec.

### Decision 3 — Multi-host: IN SCOPE, LAST
Remote federation stays part of 100% but sequences LAST (Wave 5, after 1-4). Model: a remote TC
daemon per host, connected over an authenticated SSH tunnel (`ssh -L`) — NO public TCP listener.
Tools gain optional `target_id` (default local). Federation connects daemons; never SSH-exec
without combing.

### Decision 4 — TC50 session model: daemon-stateful, stateless subprocess per line (round 3; Cursor "Decision 5")
A `shell_session` is DAEMON-OWNED state, not a long-lived shell. Long-lived PTY `bash -l` is reserved
for true interactive use (existing `pty_command_*`); an optional `shell_session_mode: interactive`
may bridge later.

- `shell_session_start` -> `session_id` + `bucket_id`; record `{ cwd, env, shell, rules, bucket_id }`.
- `shell_session_exec(session_id, line)`:
  1. Policy: `CommandShellStart` (the SAME lock as `shell_exec`).
  2. Spawn a FRESH `[shell,"-lc",line]` with `current_dir(session.cwd)` + env overlay.
  3. Comb output -> session bucket; capture `exit_code`. Each line = an isolated job, clear exit, no
     cross-line shell pollution.
  4. Persist cwd via the sentinel mechanism below.
- PTY long-lived rejected as default: hidden shell state (vars/shopt/functions/job-control) is
  unsnapshotable; prompt/line-sync races; `secret_prompt_active` blocks LLM stdin; daemon-owned
  `{cwd,env}` is fully serializable for TC51 snapshots.

**Claude correctness fix to Cursor's step 4:** a SEPARATE `pwd` spawn CANNOT recover cwd — `cd /tmp`
runs in a fresh child whose chdir DIES with that subprocess; a later `pwd` child (current_dir = the
still-old `session.cwd`) returns the old dir and O-02 fails. cwd MUST be captured IN THE SAME `-lc`
invocation via a sentinel, then stripped before combing:

```text
[shell,"-lc", line + " ; __rc=$?; printf '\036TC_CWD:%s\036' \"$PWD\"; exit $__rc"]
# daemon strips the \036...\036 marker from the stream; session.cwd = captured $PWD; exit preserved
```

Semantically correct too: `(cd /tmp)` / `cd x &` in a subshell leave `$PWD` unchanged, so the
sentinel reports no change — matching real shell scope, which fragile `cd`-string parsing gets wrong.
Exported-env capture (same trick: `declare -x` / `env -0`) is v2; skip in TC50 v1.

### Decision 5 — Cap schema: `[policy.caps]` nested, no top-level `[caps]` (round 3; Cursor "Decision 6")
Add `caps: Option<PolicyCapsSection>` to `PolicySection` (`config.rs:129`) beside `commands`/`paths`/
`probes` — VERIFIED that pattern exists. TOML:

```toml
[policy]
profile = "developer_local"
[policy.caps]
allow_shell = true       # explicit opt-in on a base profile; still AllowWithAudit
allow_session = false
allow_privileged = false
allow_remote = false
```

Rationale: single trust surface (operator reads `[policy]` for "what may this daemon do"); caps are
inputs to `evaluate()` like `profile`, not a separate subsystem; `policy_status` shape is naturally
`{ profile, caps:{...} }`; mirrors existing `[policy.commands]` doctrine; profile name != cap set, so
a power user flips one cap on `developer_local` without inventing a profile. `full_access` is a loader
preset that sets all four true — never a top-level orphan section, never an `evaluate()` bypass.

### Decision 6 — TC50 cwd sentinel wire format (round 4; Cursor "Decision 6")
Locks the cwd-capture format from Decision 4.

- **Marker:** minted ONCE at `shell_session_start` (`tc_cwd_<session-hex>`) and baked LITERALLY into
  the per-session `-c` script string by the daemon at spawn — **NEVER an env var** (round-5 binding
  fix: a child env var like `__TC_CWD_MARKER` is `unset`/overwritable by the eval'd user line, so the
  line could suppress or SPOOF the cwd frame; a literal in the daemon-built script is untouchable).
  Never surfaced to the LLM. Per-session, not per-line.
- **Frame:** `\036<marker>:<pwd>\036` (RS-framed) on **stderr** (keeps the stdout signal stream clean).
- **Strip:** in `ShellSessionRuntime` BEFORE EventSink/comb (same layer as PTY prompt handling), never
  via sifter rules — the LLM never sees a frame.
- **Wrapper = CONSTANT `-c` string; user line = POSITIONAL arg** (injection-safe — a line with `}` /
  unbalanced quotes / `exit` cannot break the frame; worst case marker is skipped -> keep old cwd):

```text
[shell, "-c",
 'eval "$1"; __rc=$?; printf "\036tc_cwd_<session-hex>:%s\036" "$PWD" >&2; exit $__rc',
 "_", line]
# daemon templates tc_cwd_<session-hex> into the script LITERALLY; user line stays positional $1
```

Refines Cursor across rounds: positional `$1` keeps the user line out of the frame (injection-safe);
unconditional `__rc=$?` right after `eval` avoids the printf-failure edge; the marker is a literal in
the daemon-built script (round 5), NOT an env var, so the line cannot unset/spoof it. The wrapper's
`printf` is the FINAL stderr write after `eval`, so "last frame wins" deterministically takes the
wrapper's own frame.

- **Parse:** anchor the FULL `\036<marker>:…\036`; LAST match in the exec's stderr wins; strip all
  frames; marker MISSING -> keep `session.cwd`, do NOT fail (valid for `(cd x)`, failed `cd`, `exit`);
  `$PWD` with `:` is fine; `$PWD` with RS is pathological + documented.
- **Sourcing:** session exec uses `-c` (NOT `-lc`); capture the login env ONCE at `shell_session_start`
  so each line does not re-source login profiles (perf + env-reset avoidance).
- **v1.1 hardening (optional):** length-prefix instead of RS-end for exotic `$PWD`.
- Fixed `TC_CWD:` alone is an acceptable v1 fallback, but the session marker is one struct field and
  removes "user echoed our protocol" as a realistic bug.

### Decision 7 — TC49 implementation confirmations (round 5/6, code-verified)
Locks the implementation shape before execution; all four verified against code 2026-06-09.

- **Lane enum named `StartLane`, not `StartMode`** — `main.rs:90` already has `enum StartMode { IpcServer,
  ForegroundIdle }`. Approach = thread `StartLane{Argv|Shell}` through `start_combed_inner` (3 branch
  points: shell-guard, policy action, audit), NOT a spawn-core extraction (premature for TC49; TC50 can
  add a mode later if it duplicates real logic). Task-4 regression test locks argv behavior.
- **`MAX_SHELL_LINE_BYTES = 4096`** = `MAX_ARGV_ITEM_BYTES`. `validate_argv` (`command.rs:405-418`) caps
  every argv item at 4096 and the line is argv[2]; 16 KiB would be rejected there. 16 KiB later needs a
  lane-aware validator (exempt argv[2] under Shell) — deferred, not TC49.
- **`start_combed` is SYNC** (`command.rs:488`); `ShellRuntime::exec` is sync; async MCP/IPC handlers call
  it inline.
- **Dedup unchanged** — `(peer, argv, cwd, tag)` fallback (`command.rs:352`) already includes argv[2]=
  shell_line, so distinct lines do not collapse. Add a distinct-lines test.
- **`full_access`**: `base || full` forces all caps on (subset => use a base profile + explicit caps).

Execution: subagent-driven on review branch `tc49-shell-exec`; per-task code-review + test gate; argv
regression is the hard gate; NO push/merge without human approval.

---

## Definition of done (unchanged from Cursor master)

100% = Layer A (human parity) + Layer B (LLM supersets) + Layer C (self-reliance), proven by the
O-01..O-14 omni acceptance matrix as automated smokes across Linux / WSL / native Windows / macOS.

---

## Sequenced program (with decisions applied)

| Wave | Track | Decisions applied | Status |
|---|---|---|---|
| 0 | Trust hardening | — | mostly shipped; verify green |
| 1 | Shell + session | D2 (both doors/one lock), D1 (`allow_shell` cap) | LEAD — TC49-52 |
| 2 | Platform parity (ConPTY, macOS) | — | after 1; parallel with 3 |
| 3 | Parse omni (suggest + packs) | — | parallel with 2 |
| 4 | Privileged helper | D1 (`allow_privileged` gates helper), PRIVILEGE_MODEL §5 | after 1 |
| 5 | Remote federation | D3 (in-scope-last, SSH tunnel, `target_id`) | after 1+2 |
| 6 | Omni certification + doc realign | D1 (`full_access`), all | continuous; final gate |

---

## Cross-cutting invariants (every wave honors)

1. Combed output always — even with shell/remote, NO unbounded raw stream to the LLM (only the
   bounded `command_output_tail` exploratory path).
2. New tools MUST update the hard-coded tool-count anchors (`mcp/tests/mcp_live_daemon.rs`,
   `system_discover` contract) — currently 38; estimate 46-53 at omni-complete.
3. `crates/mcp/src` stays free of guard literals (`Command::new`/`spawn`/`TcpListener`/`UdpSocket`/
   `tokio::fs`/`std::fs`/`File::open`/`read_to_string`/`read_to_end`).
4. Every gated capability use emits an audit row (AllowWithAudit) with redacted argv/line.
5. No new crate deps without explicit approval (windows-sys feature adds OK).
6. Both-OS gate: WSL `clippy --workspace --all-targets` + `nextest` authoritative; Windows clippy
   is blind to `#![cfg(unix)]` bodies.
7. Accepted residual risk: an enabled shell cap can reach `sudo` inside a `shell_line` on a
   permissive host. This is intended (trusted-profile cap); Wave 4 keeps privilege a SEPARATE
   closed helper so the DEFAULT privilege path is never generic shell.

---

## Next step

writing-plans on Wave 1 (TC49 — one-shot shell, both doors/one lock). Cursor's
`2026-06-09-tc-omni-wave1-shell-session.md` is the base; amend it for Decision 2 (add the
`allow_shell` alt door + the one-lock invariant) during plan authoring.
