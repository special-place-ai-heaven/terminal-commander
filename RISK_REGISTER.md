# Terminal Commander - Risk Register

Status: TC48 beta gate snapshot.

Tracks open risks against the runtime chain (TC33-TC47) as it
stands on `main` today. Each row carries: ID, area, description,
status, current mitigation, and follow-up reference. No
mitigation is silently dropped; risks marked `accepted` are
explicit operator-level decisions.

Language: ASCII only.

## Active risks

### R-01 — Provider-harness CLI execution not verified end-to-end on this host

**Area:** Provider integration / TC46.
**Description:** Codex CLI fails on this WSL host with
`Missing optional dependency @openai/codex-linux-x64`; Claude
Code has no `claude` binary installed. Both provider harness
smokes are `Not Run` per TC46.
**Status:** Open.
**Mitigation:** Config-only examples ship in
`docs/integrations/codex-cli.md` and
`docs/integrations/claude-code.md`. The local daemon + MCP
stdio smoke (`scripts/smoke/verify-runtime-smoke.sh`) proves the
transport surface independent of any provider. The TC48 beta
recommendation is `Conditional Go` because of this risk.
**Follow-up:** BACKLOG P1.2 (Codex live smoke), BACKLOG P1.3
(Claude Code live smoke).

### R-02 — `frames_suppressed` counter is absent from the daemon

**Area:** Observability / TC47.
**Description:** Probes do not surface a dedicated
`frames_suppressed` counter. Test code can derive noise reduction
where the test owns both input volume and matching rule; real
beta operators inspecting `runtime_state` / `bucket_summary`
cannot see suppression counts directly.
**Status:** Open.
**Mitigation:** TC47 documented this in its final report and
filed BACKLOG P1.1. The existing counters (`frames_total`,
`events_emitted`, `bytes_total`) plus `BucketSummary.dropped_count`
expose enough state to detect aggregate noise even without the
explicit suppression counter.
**Follow-up:** BACKLOG P1.1.

### R-03 — File-watch backend is poll-based at 120 ms

**Area:** Realtime latency / TC43.
**Description:** `crates/probes/src/file.rs` polls. Sustained
megabyte-per-second append rates are bounded by the polling
interval, not Terminal Commander's pipeline.
**Status:** Accepted for beta.
**Mitigation:** Documented in the TC43 prep amendment. Bounded
caps still hold; nothing leaks. A native notify/inotify backend
is out of scope and would be its own goal.
**Follow-up:** BACKLOG P2.1.

### R-04 — PTY spawn is Unix-only

**Area:** Platform support / TC44.
**Description:** `pty-process = "=0.5.3"` is `cfg(unix)`. The
PTY runtime is gated; the MCP adapter on Windows refuses to start.
**Status:** Accepted for beta.
**Mitigation:** Documented across TC40 / TC44 docs. WSL2 is the
supported Windows path. Non-Unix builds return
`IpcErrorCode::UnsupportedPlatform`.
**Follow-up:** Windows ConPTY is explicitly out of scope per
TC44 non_goals; no follow-up scheduled.

### R-05 — Codex / Claude Code MCP server schema may drift

**Area:** Provider integration / TC46.
**Description:** The two integration docs target the public MCP
server schemas published in late 2025 / early 2026. Provider
docs change; the configs may need adjustment as the providers
evolve.
**Status:** Open.
**Mitigation:** Both docs link the upstream documentation. The
adapter binary (`terminal-commander-mcp`) is stdio-only and
spec-stable per MCP 2024-11-05; changes are likely to be in the
provider-side wrapper config, not the adapter.
**Follow-up:** Operator-driven refresh when providers publish a
schema update.

### R-06 — `command_exited` lifecycle event embeds argv in `summary`

**Area:** Observability / TC38 design choice.
**Description:** The `command_exited` event's `summary` field
embeds the argv list verbatim so an operator can identify which
command exited. The TC47 raw-stream leak check explicitly
exempts the `summary` / `argv` / `argv0` / `subject` /
`summary_template` / `reason` fields from the leak assertion.
This is operator-input, not raw stdout content.
**Status:** Accepted for beta.
**Mitigation:** Documented in the TC47 stress test rationale.
No raw stdout / stderr / file body content reaches the LLM.
**Follow-up:** None.

## Resolved risks (historical context)

| ID | Area | Resolved by | Note |
|----|------|-------------|------|
| H-01 | Audit durability | TC35 | `PersistentAudit` is the production audit sink. |
| H-02 | Local IPC transport | TC37 | UDS-only; no TCP; PeerCred captured per connection. |
| H-03 | MCP stdio adapter | TC40 | rmcp 1.7.0 stdio adapter, forwards to daemon UDS. |
| H-04 | MCP command spawn | TC41 / TC40 | MCP never spawns; daemon owns spawn. Verified by grep gate. |
| H-05 | MCP file reads | TC43 / TC40 | MCP never reads files directly. Verified by grep gate. |
| H-06 | PTY secret-prompt leak | TC44 | `SecretInputDenied` boundary; audit metadata is bounded `(byte_count, prompt_kind)`. |
| H-07 | Bucket retention loss invisible | TC07 / TC47 | `BucketSummary.dropped_count` surfaces eviction count. |
| H-08 | Probe cross-talk | TC42c / TC47 | Cross-talk impossible: probe_id + bucket_id assertions in TC47 stress test pass. |
| H-09 | Bucket-wait busy-poll risk | TC07 / TC47 | Notify-based; TC47 asserts >=700 ms block for 800 ms timeout. |

Risk policy:

- Active risks move to "Resolved risks" only after a TC goal lands
  the fix AND the relevant tests prove the mitigation.
- A risk marked `accepted` requires operator acknowledgement; the
  status notes name the operator-visible behavior so the
  acceptance is informed.
- Removing a risk row is forbidden. Demote to `Resolved` and keep
  the TC reference instead.

## Active risks — Windows + WSL bridge chain (WWS01–WWS07)

Added by WWS08 to mirror the WWS01 contract §15 R-WWS-01..R-WWS-10
public risk table. None of these block the publish floor; they
guide post-publish hardening.

| ID   | Risk | Mitigation (where landed) | Status |
|------|------|---------------------------|--------|
| R-WWS-01 | English-locale only `wsl.exe -l -v` parser; non-English Windows hosts may not parse | `lib/wsl/detect.js` documented as English-locale only; parser tolerates UTF-16 LE BOM + NUL-padded ASCII + CRLF; future work to support localized headers | **accepted** at WWS03 |
| R-WWS-02 | Cursor MCP config schema may evolve | `lib/cursor/write.js` writes only the documented stanza shape (`type: stdio`, `command`, optional `env.TC_WSL_DISTRO`); refuses unknown top-level keys without `--force`; preserves unrelated entries | **mitigated** at WWS05 |
| R-WWS-03 | `wsl.exe` argv injection via unsafe distro names | Two-step double-validation: `assertSafeDistroName` (whitelist `^[A-Za-z0-9._-]{1,64}$`) + live `detectWsl().distros` membership BEFORE any spawn | **mitigated** at WWS03 / WWS04 |
| R-WWS-04 | Operator supplies a crafted distro name via `--distro` / `TC_WSL_DISTRO` to attempt injection | Whitelist + membership check; argv array passed to spawn (no shell); `shell: false`, no hidden-window option, `stdio: 'inherit'` | **mitigated** at WWS04 / WWS06 |
| R-WWS-05 | Cursor `mcp.json` overwrite clobbers operator's unrelated MCP servers | Refuse-existing-`terminal-commander`-entry without `--force`; always `.bak` before overwrite; refuse pre-existing `.bak` without `--clobber-backup`; atomic write via same-directory tmp file + rename | **mitigated** at WWS05 |
| R-WWS-06 | Token-shaped env var leaks across bridge | `buildFilteredEnv` strips explicit keys (NPM_TOKEN, GITHUB_TOKEN, OPENAI_API_KEY, ANTHROPIC_API_KEY, SLACK_TOKEN, CARGO_REGISTRY_TOKEN, RELEASE_PLEASE_TOKEN, plus the `_TC` suffixed variants) AND pattern-shaped keys (`*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `*_PASS`, `*_API_KEY`, `*_APIKEY`, AWS_SESSION_TOKEN, AWS_SECRET_ACCESS_KEY) before the child env is constructed | **mitigated** at WWS04 |
| R-WWS-07 | `--install-wsl-runtime` becomes a privilege-escalation hole | Locked to ONE constant `npm install -g terminal-commander` invocation; no sudo, no `sudo -S`, no password prompt, no env credential, no LLM-supplied secret; EACCES → `install_permission_required` honestly (no retry under sudo); E404 → `npm_package_unpublished` honestly | **mitigated** at WWS06 |
| R-WWS-08 | Pair code mistaken for a security secret | Documented as operator confirmation only; `pair.json` stores `{ schema_version, pair_id, code, created_at, accepted_at, distro }` — no token, no env, no credentials; the code is never used in any cryptographic decision | **accepted** at WWS06 |
| R-WWS-09 | Cursor live smoke from a Windows host still requires GUI steps (no headless MCP entry point) | WWS07 PowerShell smoke records the GUI smoke status honestly; `Not Run` if unattainable; no promotion to PASS without operator transcript | **accepted** at WWS07 |
| R-WWS-10 | `npm-bootstrap-publish.yml` accidental dispatch | Workflow committed but NOT dispatched; remains the one-time bootstrap fallback per NPM10; BACKLOG WWS-B8 records the disable/rotate follow-up after first publish | **mitigated** by operator discipline |
