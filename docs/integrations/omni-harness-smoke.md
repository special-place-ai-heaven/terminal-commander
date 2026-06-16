# Omni provider-harness smoke notes

Status: config + procedure doc. The live provider runs described here are
OPERATOR-GATED -- they require a human to drive a real Cursor / Codex CLI /
Claude Code session and observe the tool calls in the transcript. This page
does not run anything; it tells an operator what to drive and what a pass
looks like.

These notes cover the omni surface specifically: the command -> wait ->
status flow, a persistent shell-session check, and a suggest-loop check.
They complement, and do not replace, the per-provider config docs
([`cursor.md`](cursor.md), [`codex-cli.md`](codex-cli.md),
[`claude-code.md`](claude-code.md)) and the local daemon+adapter smoke
(`scripts/smoke/verify-omni-{linux,wsl,windows,macos}` via
`scripts/smoke/omni-o-runner.py`).

Language: ASCII only.

## What this is (and is not)

- It IS the procedure for the O-14 provider-trust gate: a real provider
  invoking real Terminal Commander tools and showing bounded responses in
  the session transcript.
- It is NOT the daemon+adapter O-01..O-13 smoke. That runner
  (`omni-o-runner.py`) drives the MCP adapter over stdio WITHOUT a provider
  in the loop and explicitly marks O-14 as
  `skip:provider-trust-smokes-are-a-separate-pass`. A green local smoke is
  necessary but not sufficient: a provider-harness pass requires the
  provider.

> [!IMPORTANT]
> A provider smoke is "live" only when a real provider session invokes a
> Terminal Commander tool and the bounded response is visible in the
> transcript. Code-reading, a green local runner, or "the config is
> written" are NOT a provider pass. Honesty: until an operator runs the
> steps below in each provider and captures the transcript, the
> provider-harness result is UNVERIFIED.

## Prerequisites

1. Install and write the MCP config for the target provider (see that
   provider's doc):

   ```powershell
   npm install -g terminal-commander@latest
   terminal-commander setup harness --provider cursor       # or codex-cli, claude-code
   terminal-commander doctor harness
   ```

2. Confirm the daemon is reachable: `terminal-commander doctor daemon`.
3. For the session check, the operator must have enabled `allow_session`
   in the daemon's policy config TOML (default OFF) AND be on a unix host
   (sessions are unix-only). If `allow_session` is off, the session check
   is expected to be DENIED -- that denial is itself a valid observation
   (record it as "deny verified"), not a failure of the harness.

## Check A -- command -> wait -> status

The core omni flow. Ask the assistant (in the provider session) to:

1. Call `system_discover`. Confirm it returns the tool catalogue AND an
   `omni_status` matrix. Note `omni_status.privileged_helper` should read
   `{ available: false, reason: "threat_review_pending" }`.
2. Call `run_and_watch` with `argv=["echo","hello"]` and
   `rules=[{"pattern":"hello"}]`. Confirm a bounded signal + `exit_code`
   come back in one call.
3. For the long-running variant: `command_start_combed` with
   `argv=["sh","-c","..."]`-free argv (e.g. a real build command), then
   `bucket_wait` on the returned `bucket_id` with `cursor: 0`, then
   `command_status` on the returned `job_id`.

Pass criteria:

- Every response is bounded JSON. Raw stdout/stderr is NOT pasted into the
  conversation.
- A quiet command returns a RECEIPT (exit code + suppressed count + short
  tail), not a bare empty success.
- `wait_exhausted: true` (if seen) is treated as STILL RUNNING, not done.

## Check B -- persistent shell session

Verifies sticky cwd/env across calls. Unix-only; requires `allow_session`.
Ask the assistant to:

1. Call `shell_session_start`. Record the `session_id`.
2. Call `shell_session_exec` with `line: "cd /tmp"`.
3. Call `shell_session_exec` with `line: "pwd"`.
4. Call `shell_session_status` with the `session_id`.

Pass criteria:

- The `pwd` line returns a combed signal reporting `/tmp` WITHOUT the agent
  re-passing cwd -- i.e. step 2's `cd` persisted into step 3.
- `shell_session_status.cwd` is best-effort and may lag; the authoritative
  directory is the `pwd` combed signal from step 3 (see
  `docs/runtime/SHELL_SESSION.md` section 6). Do not treat a stale
  `status.cwd` as a failure if the `pwd` signal is correct.
- Optional: `workspace_snapshot_create` then `workspace_snapshot_apply`
  into the same (or a fresh) session, confirming cwd/env restore.

Expected-deny variant: with `allow_session` OFF (the default), step 1
should be denied by policy and audited. Record "deny verified" -- that is
the correct default-deny behavior, not a harness failure.

## Check C -- suggest loop (unknown output)

Verifies the suggest -> test -> activate loop and the never-auto-activate
invariant. Ask the assistant to:

1. Run an exploratory command whose format it does not pre-know (e.g.
   `run_and_watch argv=["df","-h"] rules=[]`), then `command_output_tail`
   on the `job_id` to read a bounded tail.
2. Call `registry_suggest_from_samples` with a few of those tail lines as
   `samples`.
3. Confirm the response carries `proposed_rules`, `confidence: "heuristic"`,
   and `next_steps: ["registry_test","registry_upsert","registry_activate"]`.
4. Optionally walk one proposed rule through `registry_test` ->
   `registry_upsert` -> `registry_activate`.

Pass criteria:

- `registry_suggest_from_samples` returns DRAFT rules and does NOT activate
  anything on its own. Activation only happens if the agent explicitly
  calls `registry_activate` in step 4. The "never auto-activates" invariant
  is the load-bearing check here.

## Per-provider notes

The three providers differ only in config location and how the tool list
surfaces; the three checks above are identical across them.

| Provider | Config | Refresh tool list | Doc |
|---|---|---|---|
| Cursor | JSON `mcpServers` | reload window / rename server key | [`cursor.md`](cursor.md) |
| Codex CLI | TOML `~/.codex/config.toml` | restart Codex / rename server key | [`codex-cli.md`](codex-cli.md) |
| Claude Code | JSON `mcpServers` | `/mcp` to list; restart to refresh | [`claude-code.md`](claude-code.md) |

For each provider, capture the transcript showing at least Check A's
`system_discover` + `run_and_watch`. That transcript is the evidence the
provider-harness (O-14) gate requires. Note in the evidence whether Checks
B and C ran, were deny-verified, or were skipped (e.g. non-unix host for
Check B), so a reader cannot mistake a skip for a pass.

## See also

- `docs/mcp/OMNI_PLAYBOOK.md` -- the lane-selection map the checks exercise.
- `docs/runtime/SHELL_SESSION.md` -- the session model behind Check B.
- `scripts/smoke/omni-o-runner.py` -- the local O-01..O-13 runner (no
  provider in the loop).
- `docs/testing/cursor-tc-trust-test-goal.md`,
  `docs/testing/codex-tc-trust-test-goal.md` -- the broader provider trust
  test goals.
