# Spec: WSL Cleanup Dogfood Findings -- TC Ergonomics + Operability Gaps

Status: Design (findings from a live dogfood session 2026-05-27, no open
questions on the findings themselves; fix decisions locked where noted,
flagged OPEN where a design choice remains). Language: ASCII only.

## Objective

A real cleanup task -- reclaim disk in WSL (cargo/temp/docker/apt/journal/
fstrim) -- was driven end-to-end strictly through Terminal Commander (TC)
against a live daemon. The run succeeded (about 14 GB reclaimed plus 2.1 GiB
fstrim), but it exposed eight concrete operability and ergonomics gaps that
make TC harder to trust and slower to use than it should be. This spec
records each gap with verified root cause and a fix decision, and defines the
SUDO setup discipline that made privileged cleanup work without leaking a
credential. It is the input to
`docs/superpowers/plans/2026-05-27-wsl-cleanup-dogfood-fixes.md`.

## Why this matters

TC's value proposition is "run noisy/long/privileged commands, get back only
structured signal, never flooded context." The dogfood proved that works for
the happy path (full cargo build returned one clean-exit receipt; a forced
compile failure returned one `command_failed` critical event). But four of the
eight gaps directly tax the very LLM the product is meant to serve: it was
forced to author a regex rule before it could read the output of any
unfamiliar command, and a rule authored mid-job silently captured nothing.
Two more gaps are operability footguns around `update`/restart that cost a
human a confused round-trip. Fixing these is what moves TC from "works if you
already know its model" to "an LLM prefers it over raw Bash."

## Evidence base (verified this session)

All findings are from live TC tool calls against daemon build 0.1.18 on
Windows 11 host with a WSL2 (Ubuntu) distro, user `robert`. Representative
proofs:

- No-rule command `cargo --version` returned a bounded receipt
  (`command_exited`, exit 0, 35 bytes, 131 ms) -- the no-silence guarantee
  held.
- `cargo build --workspace` returned only the synthetic clean-exit event
  (13.8 s); raw output never entered context. Value proven.
- `cargo check -p nonexistent-crate-xyz` returned `command_failed`, severity
  critical, exit 101. Failure signal proven.
- `wsl.exe -- df -h` with a hand-authored `disk_usage` regex rule captured
  per-mount usage (root `/dev/sdd` 512G/1007G). Capture mechanism proven.
- `registry_activate` reported `jobs_rebound` counting future jobs, not the
  in-flight one; a rule authored after `command_start_combed` captured
  nothing from that job.
- `terminal-commander update` (npm wrapper) ran `npm install`; daemon uptime
  kept climbing (27746 -> 27833 s), proving it never touched the daemon.
- `terminal-commanderd update` (daemon binary) returned
  `update skipped: reachable daemon, no pidfile, no killable pid found`,
  proving the installed daemon writes no pidfile.
- After a manual `taskkill` + respawn, `WSLENV` inside the daemon's WSL child
  was empty, proving the long-lived daemon never re-read host env.
- A scoped NOPASSWD sudoers file made `sudo -n fstrim` return `SUDO_OK`
  through TC, and `fstrim -av` then reported `/ ... 2.1 GiB ... trimmed`.

## Findings and fix decisions

### P1 -- blocks fluent use by an LLM

**F1. No raw-stdout escape hatch.**
Root cause: `command_start_combed` only surfaces lines that match an active
rule, plus the synthetic lifecycle receipt. For an exploratory command whose
output format is unknown (`docker system df`, `pip cache purge`, `df -h`), the
caller must author a regex rule *before* it can read anything. During the
dogfood this forced six rule upserts just to read normal command output.
Decision (LOCKED): add a bounded, rule-free read path
`command_output_tail(job_id, max_lines, max_bytes, stream?)` that returns the
last N lines (default 50, hard cap 200; byte cap 65536, reusing the existing
`file_window_bytes` policy ceiling) of a finished or running job's captured
stream, truncation-flagged. It does NOT change the default suppression
behavior; it is an explicit opt-in escape hatch. OPEN: whether
`event_context` via an event pointer already partially covers this -- the plan
must probe it first and only build `command_output_tail` if the pointer path
cannot return arbitrary tail lines without a matched event.

**F2. `registry_activate` does not bind already-running jobs.**
Root cause: activation rebinds future jobs only; `jobs_rebound` counts jobs
not yet started. A rule authored mid-job captures nothing from that job, so
the proof line (`pip ... Removed N files`, `Total reclaimed space: X`) was
lost and a re-measure was required.
Decision (LOCKED): two parts. (a) Ship a curated `cleanup` seed rule pack
(df/du/docker-df/fstrim/reclaimed/freed patterns, exactly the rules
hand-authored this session) so the common cleanup signals are active out of
the box and never need mid-job authoring. (b) Document explicitly (tool
description + error/teaching text) that activation is future-only, and that
the supported pattern is "activate, THEN start the command." OPEN (deferred,
not in this plan): true hot-rebind of a running probe -- larger change, parked
as a follow-up.

### P2 -- operability footguns

**F3. Two colliding `update` commands.**
Root cause: `terminal-commander update` is the npm CLI wrapper and runs
`npm install` (reinstall the package); `terminal-commanderd update` is the
daemon binary subcommand that does version-aware replacement. A user ran the
former expecting the latter; the daemon was never replaced.
Decision (LOCKED): the npm-facing verb for daemon replacement becomes
`terminal-commander restart` (and/or `terminal-commander update --daemon`),
which proxies to `terminal-commanderd update`/`--force`. Package self-update
(npm reinstall) moves to `terminal-commander upgrade`. Help text disambiguates
both. Keep `terminal-commander update` as a deprecated alias that prints a
one-line "did you mean restart/upgrade?" notice.

**F4. `update` is version-gated with no force path.**
Root cause: `replace_if_stale` returns `UpToDate` when running version ==
installed version, so it cannot restart a daemon to pick up a changed env var
or config when the version is unchanged. Verified in
`crates/supervisor/src/replace.rs` (`is_stale`) and
`crates/daemon/src/main.rs` (`run_update`).
Decision (LOCKED): add `terminal-commanderd update --force` that kills and
respawns regardless of version equality, and surface it via
`terminal-commander restart`. `--force` still uses the same safe kill path
(pidfile pid, else OS-query scoped to our `--data-dir`), never a blind name
kill.

**F5. Installed daemon writes no pidfile.**
Root cause: `replace_if_stale` fell back to OS-query and then reported
"no pidfile, no killable pid found" for the npm-installed daemon, while the
workspace build writes one. Strong suspicion: the pidfile is written under a
`state_dir`/`data_dir` that differs between the workspace run and the
Windows npm install, so reads miss it. Must be confirmed by inspecting where
`write_daemon_pidfile` writes vs where `read_pidfile` reads on a Windows
install (see `crates/daemon/src/runtime.rs`, `crates/supervisor/src/pidfile.rs`).
Decision (LOCKED, pending confirmation): make the pidfile path a single
resolved function shared by writer and reader, anchored to the same
`resolve_state_dir()` the adapter/CLI use, and add a test that the path the
daemon writes equals the path the supervisor reads under the installed-mode
data dir. If the root cause is instead that the windows_subsystem="windows"
daemon cannot run its post-bind pidfile write, fix that instead -- the plan
task starts with a diagnosis step, not a blind edit.

**F6. Long-lived daemon never re-reads host env.**
Root cause: the daemon inherits a frozen environment from its spawner
(MCP adapter, itself a child of the editor/client process). Setting a Windows
User env var (`WSL_SUDO_CREDENTIAL`) and `WSLENV` after those processes
started never reached the daemon; killing only the daemon was insufficient
because its parent was also stale. Verified: `WSLENV` was empty inside the
daemon's WSL child even after a daemon respawn.
Decision (LOCKED): do NOT rely on env inheritance for privileged operation.
The supported mechanism is scoped NOPASSWD sudoers (see "SUDO setup
discipline"). Additionally (small, safe): on daemon spawn, have the supervisor
read the relevant host vars from the persisted User environment and pass them
explicitly into the spawned daemon's env, so a daemon respawn picks up
freshly-set vars without a full client restart. OPEN: exact var allowlist to
forward (must be a fixed, documented allowlist, never "forward everything").

### P3 -- polish

**F7. NOT A TC BUG -- operator used the wrong template syntax.**
Initial observation: emitted events showed the literal template
(`disk {mount}: {used}/{size}`) instead of substituted values. Investigation
disproved the bug: `crates/sifters/src/lib.rs:386` already calls
`render_summary(&captures)` at emit and only falls back to the raw template on
a render error (test `crates/sifters/src/lib.rs:593` asserts a rendered
`"missing libssl-dev"`). The placeholder syntax is `${name}`, not `{name}`
(`crates/core/src/rule.rs:167`, `render_template` at :479). The dogfood rules
were authored with bare `{mount}`, which is literal text, so it was emitted
verbatim -- correct behavior. `validate()` does not reject bare `{x}` because
it is not a placeholder.
Decision (LOCKED): no code change to rendering. Instead (a) the shipped
`cleanup` seed pack and all docs use the correct `${name}` syntax; (b) the
`registry_test` / rule-authoring docs and the tool description state the
`${name}` syntax explicitly and warn that bare `{name}` is literal; (c)
OPTIONAL lint (small, OPEN): have `validate()` emit a non-fatal warning when a
`summary_template` contains `{name}` where `name` is also a declared capture,
since that is almost always a `${...}` typo. The plan should treat (c) as
optional and only add it if cheap.

**F8. `registry_activate` scope doc/behavior mismatch.**
Root cause: the tool schema says scope is optional and "omitted = global," but
the daemon rejects an omitted scope with `ScopeInvalid: scope is required;
pass {kind:'global'}`. Verified live.
Decision (LOCKED, pick one in the plan): either (a) make the daemon default an
omitted scope to global to match the documented contract, or (b) make the
schema/description require scope and stop advertising the default. (a) is
preferred (matches the published contract; smaller blast radius for callers).

## SUDO setup discipline (user-facing, the supported privileged path)

This is the discipline that makes TC privileged cleanup work WITHOUT putting
a password anywhere it can leak (daemon env, logs, child argv, crash dumps).

Principle: never pipe a sudo password through an env var or stdin for routine
operation. Grant passwordless sudo scoped to ONLY the specific cleanup
binaries via a sudoers drop-in. The credential never travels; the grant is
auditable and narrow.

One-time setup, run by the user inside the WSL distro (interactive sudo once):

```bash
echo "$USER ALL=(root) NOPASSWD: /usr/bin/apt-get, /usr/bin/journalctl, /usr/sbin/fstrim" \
  | sudo tee /etc/sudoers.d/tc-cleanup
sudo chmod 440 /etc/sudoers.d/tc-cleanup
sudo visudo -c -f /etc/sudoers.d/tc-cleanup   # must print: parsed OK
```

Rules:
- Scope tight. List only the exact binaries cleanup needs. Never `NOPASSWD:
  ALL`. To extend, add specific paths, not wildcards.
- `chmod 440` is mandatory -- sudoers ignores group/other-writable drop-ins.
- Always `visudo -c` before trusting the file; a malformed sudoers can lock
  out sudo entirely.
- TC then drives privileged cleanup with `sudo -n <ABSOLUTE-PATH>` (the `-n`
  makes it fail loud instead of hanging on a password prompt). Verified end to
  end: `sudo -n /usr/sbin/fstrim -av` reported `/: 259.1 MiB ... trimmed`
  through TC with no prompt.
- GOTCHA (verified, must be documented + baked into the doctor fix line and any
  cleanup helper): sudoers NOPASSWD grants match by the EXACT command path
  listed. The drop-in lists absolute paths (`/usr/sbin/fstrim`), so TC MUST
  invoke the absolute path. `sudo -n fstrim` (bare name resolved via PATH) does
  NOT match the absolute-path rule and fails with a password demand under
  `-n` -> a false "sudo broken" signal. Always invoke the absolute path that
  appears in the sudoers line.
- This must be documented in `docs/` (integration guide) AND surfaced as a
  doctor check (see plan): if a privileged cleanup tool is denied, the doctor
  tells the user exactly which sudoers line to add.

Explicitly rejected alternative: `WSL_SUDO_CREDENTIAL` env var + `WSLENV`
forwarding + `sudo -S`. It is fragile (frozen process env, needs full client
restart) and leaks the password into the daemon's environment and any child's
view of it. The env-forwarding piece in F6 is for non-secret operational vars
only, never a password.

## Non-goals

- True hot-rebind of rules onto a running probe (parked, F2 follow-up).
- Any change to the default output-suppression behavior (F1 is opt-in only).
- Auto-shrinking the WSL VHDX from inside the daemon. fstrim returns blocks;
  shrinking the .vhdx is a host-side `wsl --shutdown` + `Optimize-VHD`/diskpart
  operation that requires the distro down and is out of scope here. Document
  it; do not automate it.
- Deleting user data caches (huggingface model weights, active browser
  binaries). The cleanup pack and any shipped guidance must treat these as
  protected and never auto-purge them.

## Acceptance criteria (spec-level; the plan refines into per-task tests)

1. An LLM can read the tail of any command's output without authoring a rule
   (F1), proven by a test that starts a no-rule command and reads its last
   lines.
2. The `cleanup` seed pack ships and is importable/activatable by name, and a
   test imports it and asserts the df/du/docker/fstrim rules are present (F2).
3. `terminal-commander restart` replaces a same-version daemon (uptime
   resets), and `terminal-commander update` prints a deprecation pointer (F3,
   F4).
4. The path the daemon writes its pidfile to equals the path the supervisor
   reads under installed-mode data dir, asserted by a test (F5).
5. A daemon respawn picks up a freshly-set allowlisted host var without a full
   client restart (F6), OR -- if that is descoped -- the docs and doctor make
   the sudoers path the unambiguous supported route.
6. The shipped `cleanup` pack and docs use `${name}` syntax; a doc/test
   demonstrates a correctly-rendered summary and the rule-authoring docs warn
   that bare `{name}` is literal (F7 is not a code bug -- rendering already
   works at `sifters/src/lib.rs:386`).
7. `registry_activate` with an omitted scope succeeds as global (or the schema
   no longer advertises the default), asserted by a test (F8).
8. A new integration doc documents the SUDO setup discipline verbatim, and a
   doctor check points at the exact sudoers line when a cleanup binary is
   denied.
