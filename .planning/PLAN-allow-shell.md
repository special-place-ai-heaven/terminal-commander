# PLAN — P0 `allow_shell` capability (shell-passthrough opt-in)

**Status:** SUPERSEDED (2026-06-09) by `docs/plans/2026-06-09-tc-omni-decisions-and-reconciliation.md`
Decision 2 (BOTH doors, one lock). The `allow_shell`+argv design below survives as the **alt door**;
the primary is Cursor's `shell_exec`+`shell_line`+`ShellRuntime`. Both route to ONE
`PolicyAction::CommandShellStart` gate. Kept for the threat-model + phasing detail.

**Original status:** SPEC / awaiting design confirmation
**Version base:** 0.1.47
**Origin:** `cursor_report_03.md` Gap 1 (biggest "anything a human can do" gap)
**Invariant being reversed:** `POLICY.md:181` `shell_passthrough = false`; README "not a generic shell bridge"

---

## Mini-spec

**objective:** Add an explicit, off-by-default, per-call `allow_shell` capability that lets a
trusted policy profile run a shell interpreter as `argv[0]` (e.g. `["bash","-lc","cat a | grep b > c"]`),
unlocking pipelines / compounds / globs / redirects — WITHOUT widening the default surface.

**non_goals:**
- NOT a daemon-constructed shell. Caller supplies the interpreter + flags as argv; daemon never builds `sh -c`.
- NOT shell-line content filtering (false security — undecidable on shell strings).
- NOT enabled in `repo_only` or `read_only_observer`. NOT default-on anywhere.
- NOT PTY shell in the first cut (the guard is duplicated in `pty_command.rs`; defer to a follow-up phase).
- NO new crate dependencies.

**allowed_area:** `crates/daemon/src/policy.rs`, `crates/daemon/src/command.rs`,
`crates/daemon/src/ipc/handlers/*`, `crates/ipc/src/protocol.rs`, `crates/mcp/src/tools.rs`,
`POLICY.md`, relevant tests + contract fixtures.

**forbidden_files:** `crates/mcp/src/*` must NOT gain any guard literal (`Command::new`/`spawn`/
`TcpListener`/`UdpSocket`/`tokio::fs`/`std::fs`/`File::open`/`read_to_string`/`read_to_end`).
Live machine daemon untouched during dev.

**contracts_or_interfaces:**
- `CommandStartRequest { .. , allow_shell: bool }` (serde default false).
- `PolicyAction::CommandShellStart { argv, cwd }` (new variant).
- `CommandStartParams { .. , allow_shell: bool }` (IPC, serde default false).
- MCP `command_start_combed` / `run_and_watch` params gain optional `allow_shell` (default false).

**invariants:**
1. `allow_shell` absent / false  =>  byte-identical behavior to today (shell `argv[0]` still `ShellInterpreterDenied`).
2. `allow_shell` true is only honored when the active profile's `CommandShellStart` verdict is Allow/AllowWithAudit.
3. `repo_only` + `read_only_observer` ALWAYS deny `CommandShellStart` regardless of the flag.
4. Every honored shell start emits an audit row (AllowWithAudit), capturing redacted argv head.
5. `COMMANDS_DENY` still applies to `argv[0]`; the threat that a shell STRING can carry `sudo`
   is accepted + documented, and is the reason the capability is trusted-profile-only.
6. cwd / path policy gate still applies (CommandShellStart carries `cwd`, reuses the CommandStart cwd checks).

**acceptance_criteria:**
- Default profile (and explicit `allow_shell:false`) still returns `ShellInterpreterDenied` for `sh`/`bash`/… — existing tests stay GREEN.
- `developer_local` + `allow_shell:true` spawns `bash -lc "<pipeline>"` and combs its output normally.
- `repo_only` + `allow_shell:true` => Deny verdict (`PolicyDenied`), never spawns.
- AllowWithAudit profile => one audit row per honored shell start; argv redaction applied.
- End-to-end MCP: `run_and_watch {argv:["bash","-lc","echo a | tr a-z A-Z"], allow_shell:true}` returns a signal/receipt; same call without the flag errors.
- WSL `clippy --workspace --all-targets` EXIT=0; `nextest --workspace` (excl session_reap) green; Windows clippy green.

**evidence_required:** per-phase WSL cargo test exit codes + nextest stdout; final both-OS clippy; MCP-guard grep clean on `crates/mcp/src`.

**stop_conditions:** any default-deny test regresses; any privilege-escalator can run via shell in a NON-trusted profile; MCP-guard literal would be introduced; design fork unresolved.

**verification_command (per phase):**
`wsl bash -lc "cd /mnt/e/project/terminal-commander && cargo test -p <crate> <filter> 2>/dev/null; echo EXIT=$?"`
final: `cargo clippy --workspace --all-targets` + `cargo nextest run --workspace -E 'not binary(session_reap)'`.

---

## Threat model (why the design is what it is)

| Threat | Vector | Mitigation |
|---|---|---|
| Privilege escalation via shell string | `bash -lc "sudo X"` — argv[0]=bash, COMMANDS_DENY blind | Trusted-profile-only; off by default; audited. NOT line-filtered (false security). |
| Silent surface widening | flag defaults on / leaks into wrong profile | serde default false; profile matrix denies repo_only/read_only; invariant-1 test |
| Injection from LLM-built strings | unquoted interpolation into `-c` | Caller owns argv structure; daemon never joins/builds the line; we pass argv verbatim |
| Audit blind spot | shell start unlogged | AllowWithAudit mandatory for the capability; redacted argv head recorded |
| PTY guard divergence | pty path still denies; inconsistent UX | Documented: PTY shell deferred; pty_command.rs keeps hard deny this cut |

**Accepted residual risk:** in a trusted profile, `allow_shell` can run anything the daemon's uid can,
including privilege tools reachable without sudo. This is equivalent to handing that profile a shell —
which is the intent. The security boundary is the PROFILE, not per-command string inspection.

---

## Phased plan (each phase <=5 files, verify, then proceed)

### Phase 1 — Policy scaffolding (additive; ZERO behavior change)
Files: `policy.rs` (add `CommandShellStart` variant; `evaluate` arm; profile matrix: developer_local/admin_debug=AllowWithAudit, repo_only/read_only_observer=Deny; update exhaustive matches at ~228, ~426), `command.rs` (add `allow_shell: bool` field to `CommandStartRequest`, serde default false — not yet read).
Tests: policy unit tests for all four profiles x shell action.
Gate: shell still denied at runtime (field unread). Default behavior intact.

### Phase 2 — Runtime bypass (the one risky edit)
Files: `command.rs` (guard at L439/L572: when `req.allow_shell` AND `evaluate(CommandShellStart{argv,cwd})` is Allow/AllowWithAudit, SKIP `SHELL_INTERPRETERS_DENY`; else current deny. Emit audit on the audited verdict before spawn).
Tests: `command_runtime.rs` — default-false => ShellInterpreterDenied (unchanged); allow_shell+developer_local => spawns; allow_shell+repo_only => PolicyDenied.

### Phase 3 — IPC wire
Files: `ipc/src/protocol.rs` (`CommandStartParams.allow_shell`, serde default false), `daemon/src/ipc/handlers/*` (thread flag into `CommandStartRequest`; choose CommandShellStart vs CommandStart for the policy pre-check at server.rs:858).
Tests: `ipc_command.rs` round trip both flag states.

### Phase 4 — MCP surface
Files: `mcp/src/tools.rs` (optional `allow_shell` on command_start_combed + run_and_watch params; forward 1:1; tool_catalogue/schema doc). NO guard literals added.
Tests: `mcp_live_command_e2e.rs` — flagged shell pipeline returns signal; unflagged errors. Grep `crates/mcp/src` for guard literals = clean.

### Phase 5 — Docs + contract truth
Files: `POLICY.md` (`shell_passthrough` is now an opt-in capability, default false; document the profile matrix + accepted residual risk), `command.rs` seam comment (mark implemented), README is/is-not nuance, `system_discover` contract fixture if the tool schema changed.
Gate: both-OS clippy + nextest green; campaign-style honest results note.
