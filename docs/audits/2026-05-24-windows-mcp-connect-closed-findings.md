# Terminal Commander — Windows MCP "Connection Closed" Audit

Date: 2026-05-24
Auditor: Claude (Opus 4.7, 1M ctx) using SymForge index
Repo: `C:\AI_STUFF\PROGRAMMING\terminal-commander` (HEAD as of audit)
Scope: triage of `MCP error -32000: Connection closed` on Windows host + the
companion complaint that `npm install -g terminal-commander@latest` does not
deliver a working binary on a clean Windows machine.

This is a **findings report**, not a fix plan. Findings are stated with
file/line evidence so a planning agent (e.g. Codex 5.5 Extended Pro) can
turn them into a roadmap. No implementation recommendations here.

---

## 0. Symptom on the user's box

Cursor/Claude Code MCP log on every connect attempt:

```
[V2] Handling CreateClient action
[V2 FSM] connection:connect_start: conn=idle,auth=unknown -> conn=connecting
Connection failed: MCP error -32000: Connection closed
[V2 FSM] connection:connect_failure
```

Connect fails ~200-300 ms after `CreateClient`. Same outcome under both Cursor
and Claude Code. Host: Windows 11, no WSL involved in the failing path.

Manual reproduction (already executed during triage):

* Spawning `terminal-commander-mcp` directly and writing a JSON-RPC
  `initialize` while keeping stdin open → server **does** return a valid
  `initialize` response. Confirms the binary is functional in isolation.
* Spawning the shim with stdin closed → exits silently. Demonstrates the
  shim's session lifecycle is brittle around stdin EOF on Windows.
* `npm view @terminal-commander/windows-x64 versions` → **404 Not Found**.
* The only reason the user has a usable binary at all is that the local
  repo was junctioned into the global Node install via `npm link`.

---

## 1. Vision (for grounding subsequent reviewers)

From `SPEC.md` §1-2 and `ARCHITECTURE.md` §1-2:

* Two-process MCP control plane for LLM coding agents.
  * `terminal-commander-mcp` — thin per-session stdio MCP adapter. No
    network, no `Command::spawn` of user code. Translates MCP tool calls
    into daemon IPC.
  * `terminal-commanderd` — persistent per-user daemon. Owns probes,
    sifters, registry, signal buckets, context spool, policy engine,
    SQLite audit log.
* "Raw terminal/file/PTY output goes in. Only vetted, structured signal
  events come out. Context stays available by pointer." Bounded JSON
  only; no raw stream dump in the model context.
* Operator promise (README §"Install (zero-touch)"): one command —
  `npm install -g terminal-commander@latest` — gives a working MCP
  server on every detected harness (Cursor, Codex CLI, Claude Code,
  Claude Desktop) after the harness reloads.
* `SPEC.md` §3 explicitly lists "macOS native and Windows-native
  shipping" as **non-goals**; the README simultaneously documents a
  Windows-native parent + Windows-x64 platform package as live. The
  project is in conflict with its own SPEC on this point (see §3 below).

---

## 2. Packaging / distribution findings

### 2.1 Windows platform package is declared but not published

* `packages/terminal-commander/package.json:37-41` declares the
  optionalDependency `@terminal-commander/windows-x64: "0.1.4"`.
* `lib/resolve-binary.js:41-45` lists `{ platform: "win32", arch: "x64",
  pkg: "@terminal-commander/windows-x64" }` as a `SUPPORTED_TARGETS`
  entry that returns `reason: "ok"` when the package is present.
* `scripts/release/sync-optional-dependencies.js:29-33` and
  `scripts/release/verify-optional-dependencies.js:11` both expect
  `@terminal-commander/windows-x64` to exist.
* The npm registry currently serves only:
  ```
  npm view terminal-commander optionalDependencies
  → @terminal-commander/linux-x64:   0.1.4
  → @terminal-commander/linux-arm64: 0.1.4
  ```
  `npm view @terminal-commander/windows-x64` → 404.
* `docs/release/npm-bootstrap-first-publish.md` §3 + §5.12 confirms the
  Windows-x64 bootstrap publish is a manually dispatched workflow that
  has not been fired. `docs/release/npm-distribution-final-report.md` §2
  records "two operator-driven actions Pending".

Effect on the user-facing promise: `npm install -g terminal-commander@latest`
on Windows installs the JS shim but cannot resolve the platform package, so
`resolveBinary` returns `platform_package_missing`. The shim writes a single
stderr line and exits 64. The harness sees "Connection closed".

### 2.2 Local junction install masks the registry gap

`C:\Program Files\nodejs\node_modules\terminal-commander` and
`C:\Program Files\nodejs\node_modules\@terminal-commander\windows-x64`
are both Junction reparse points into the repo at
`C:\AI_STUFF\PROGRAMMING\terminal-commander`. Locally built `.exe` binaries
live in `packages/terminal-commander-windows-x64/bin/`. This is why the
maintainer sees a partially working install while every clean machine fails.

### 2.3 Placeholder artifacts coexist with real binaries

`packages/terminal-commander-windows-x64/bin/` contains both:

* Real PE32+ binaries: `terminal-commander-mcp.exe`,
  `terminal-commanderd.exe`, `terminal-commander.exe`.
* Tracked `*.placeholder` files (174-184 bytes each).

`scripts/release/verify-platform-binaries.js` is the prepack guard.
With both kinds of files present the published tarball composition
depends on which file the verifier picks, which itself depends on
build order. Same concern in `packages/terminal-commander-linux-x64`
and `-linux-arm64` (`*.placeholder` files present there too).

### 2.4 `.cmd` shim coverage on Windows

`Get-Command terminal-commander-mcp` resolves to
`C:\Program Files\nodejs\terminal-commander-mcp.ps1` only. No `.cmd`
launcher was generated. Some MCP clients spawn commands without
invoking pwsh resolution, so a missing `.cmd` shim is a silent failure
mode for any harness that bypasses `PATHEXT` PowerShell handling.

### 2.5 Stray triage artifacts in the working tree

`tmp-mcp-err.txt` (0 bytes) and `tmp-mcp-init.json` sit at the repo
root. Not in `.gitignore`.

---

## 3. Vision-vs-implementation incoherence on Windows

* `SPEC.md` §3 (non-goals): "macOS native and Windows-native shipping.
  Linux native and WSL2 are primary."
* `docs/research/_USER_DECISIONS.md`: same stance.
* `crates/daemon/src/ipc/mod.rs:14-16`:
  ```
  //! - Windows native: NOT SUPPORTED. The IPC modules compile (so the
  //!   workspace builds), but `Server::bind` returns
  //!   `IpcError::UnsupportedPlatform`. WSL2 is the Windows story.
  ```
* `docs/adr/ADR-parent-environment-runners.md` (later) and
  `docs/release/npm-binary-packaging-contract.md` §13c (2026-05-23
  amendment) reverse this: native Windows daemon + named-pipe IPC +
  `@terminal-commander/windows-x64` platform package + parent-on-Windows
  with optional WSL runners.
* The crate code follows the ADR (not the SPEC):
  `crates/daemon/src/runtime.rs:234-260` is a `#[cfg(windows)]`
  `run_ipc_server` that binds `\\.\pipe\terminal-commander-…` via
  `tokio::net::windows::named_pipe::ServerOptions`
  (`crates/daemon/src/ipc/pipe_server.rs:11`).

Net: SPEC, the in-crate module doc, and the ADR/contract docs disagree
about whether Windows native is supported. The Node shim
(`packages/terminal-commander/...`) acts as if it is supported; the
runtime acts as if it is supported; the SPEC says it is not. Nothing in
the repo arbitrates.

---

## 4. Shim ↔ daemon lifecycle findings

All references below are in `packages/terminal-commander/lib/daemon/session_supervisor.js`.

### 4.1 MCP child runs with `stdio: "inherit"`

`runHarnessMcpSession` (L202-292) spawns the MCP child with
`stdio: "inherit"` (L268). On Windows that hands the child the parent's
stdin/stdout handles directly. When the harness closes one end of the
JSON-RPC stream (transiently or after a probe), the MCP child sees EOF
and exits; `mcp.on("exit")` (L273-278) fires `cleanup()`, which
`killProcessTree`s the daemon (L226) and `fs.rmSync`-recursively wipes
the session base directory (L162, L227). The harness then logs
"Connection closed" and the session leaves no diagnostic state behind.

### 4.2 Daemon-readiness wait is fire-and-forget by design

`runHarnessMcpSession` spawns the daemon (L235) but explicitly does
**not** await IPC readiness (L246-256). The MCP child therefore starts
talking JSON-RPC before the daemon has bound the named pipe. Any tool
call that hits the daemon during the first ~hundreds of ms returns an
error from `McpDaemonClient`. The design comment at L244-245 justifies
this on Linux (`20s wait yields "Connection closed"`), but the Windows
named-pipe path has a different ready latency and there is no
Windows-specific bound at all.

### 4.3 IPC-readiness diagnostics gated on env var

`waitForIpc` (L113-120) probes the endpoint every 100 ms for up to
`STARTUP_TIMEOUT_MS = 20_000`. On Windows the probe is `net.connect`
with a 500 ms per-attempt timeout (L88-104). If the daemon never binds
the pipe, the only output is a single stderr line gated by
`TC_DEBUG_SESSION === "1"` (L247-251). In the default install path the
user gets no signal that the daemon failed to come up.

### 4.4 Cleanup fires on every exit, including normal harness teardown

* `process.on("exit", cleanup)` (L233) ensures `fs.rmSync(paths.base, …)`
  runs on **every** process exit — including code 0 after a clean
  shutdown. Session state, manifest, sqlite, sock/pipe state — all gone.
* The session base path is per-process (`SESSIONS_ROOT/h<pid><rand>`,
  L36-40), so there is no longitudinal session to preserve across
  reconnects; every harness reconnect spawns a brand-new session and
  daemon.

### 4.5 Stale-session cleanup uses POSIX liveness check on Windows

`isProcessAlive` (L78-86) is `process.kill(pid, 0)`. On Windows
`process.kill(pid, 0)` returns true for any pid that exists in the
NT process table regardless of access rights and false otherwise,
without the EPERM-means-alive nuance the function depends on
(L84: `return e && e.code === "EPERM"`). The branch will silently
misclassify some live processes as dead on Windows.

`cleanupStaleSessions` (L168-190) runs unconditionally at the top of
every shim invocation (L205) and `fs.rmSync`s session dirs whose
supervisor AND daemon both look dead. Combined with §4.4 and the per-pid
session base, there is a race where two concurrent shim starts can each
delete each other's freshly written manifest before liveness can be
established.

### 4.6 Session manifest pids never reach the manifest until late

`manifest.supervisor_pid = process.pid` is written immediately (L211)
but `manifest.daemon_pid` stays `null` until `waitForIpc` succeeds
(L252-255). If the daemon dies before IPC binds, the manifest never
records `daemon_pid` at all and `cleanupStaleSessions` on the next run
will rely on the absent value being treated as "not alive" by
`isProcessAlive(undefined)` (L79: returns false for pid ≤ 0).

### 4.7 No back-pressure on daemon spawn failures

`daemon.on("error")` (L236-242) only writes stderr when
`TC_DEBUG_SESSION === "1"`. The MCP child still spawns. The harness
sees a working MCP that returns errors from every tool call instead of
a fast failure with a clear message.

---

## 5. Resolver findings

`packages/terminal-commander/lib/resolve-binary.js`:

* `resolveBinary` (L70-130) on Windows-x64 with the package present
  returns `reason: "ok"` and a binary path that points into
  `@terminal-commander/windows-x64/bin/`. Since the platform package is
  not on the registry (see §2.1), every clean install actually takes
  the `platform_package_missing` branch instead.
* The Windows branch (L112-122) checks for `binary` + `.exe` and falls
  back to a no-suffix file. The Linux artifacts are also named without
  extension, so the post-check is necessary; it is implemented but it
  only fires after `pkgJsonPath` resolved, masking the upstream packaging
  bug from the resolver's perspective.
* `bridge_required` is now annotated as "Deprecated" (L10-15) but
  `formatResolveError` (L143-163) still emits a Windows-specific message
  for it (L150-154). The only code path that can still trigger it is the
  `TC_USE_LEGACY_WSL_BRIDGE=1` legacy bridge gate at
  `bin/terminal-commander-mcp.js:46`. README §"Generated Cursor stanza
  (Windows bridge)" instructs the user to set `TC_WSL_DISTRO` but **not**
  `TC_USE_LEGACY_WSL_BRIDGE=1`, so following the docs verbatim does not
  reach the legacy bridge path.

---

## 6. Daemon runtime / IPC findings

### 6.1 Named-pipe accept loop creates one server at a time

`crates/daemon/src/ipc/pipe_server.rs::accept_loop` (L67-110) calls
`ServerOptions::new().first_pipe_instance(true).create(&pipe_name)` once
per accepted connection. `first_pipe_instance(true)` is the wrong flag
to keep on every iteration after the first; subsequent iterations will
fail with `ERROR_ACCESS_DENIED` if anything else opened the pipe in the
window between accept and re-create. The loop's reaction to a create
failure is `break` (L86), which kills the entire accept loop silently.

### 6.2 Pipe peer credentials are hard-coded to root

`crates/daemon/src/ipc/pipe_server.rs::handle_pipe_connection`
(L112-135) hands a `PeerCred { uid: 0, gid: 0, pid: None }` (L118-122)
to `dispatch_envelope`. README §"Safety posture" advertises "peer
credentials checked"; on the Windows transport that check is a constant.

### 6.3 No equivalent of UDS file-mode hardening

UDS path applies 0o700 perms (`crates/daemon/src/runtime.rs::run_ipc_server`
at L202-230 wires PeerCred via `SO_PEERCRED`). The named-pipe path
binds with `ServerOptions::new()` defaults — no ACL, no SDDL, no
explicit user-only restriction. Any local user can connect.

### 6.4 Daemon prints to its own stderr but supervisor discards it

`spawnDaemonHidden` uses `stdio: "ignore"` (L128). All the `eprintln!`
lines in `crates/daemon/src/runtime.rs` (pipe bind notice at L243-245,
shutdown messages at L256-259) go to /dev/null. The only Windows-side
record of the daemon's own diagnostics is whatever it writes through
the audit subsystem after it manages to come up.

### 6.5 PTY paths are Unix-only by design but the MCP tool catalogue lists
them unconditionally

`crates/probes/src/pty.rs:188-194` is `#[cfg(unix)]` only; the
non-Unix path is supposed to surface
`IpcErrorCode::UnsupportedPlatform`
(`crates/daemon/src/ipc/server.rs:1624-1630`). `crates/mcp/src/tools.rs::tool_catalogue`
(L83-231) still advertises `pty_command_start` (L196) and the related
`pty_command_*` tools as live to every MCP client, including on Windows
hosts where they return `UnsupportedPlatform`. The discovery surface
should reflect the runtime gating but does not.

---

## 7. Documentation findings

### 7.1 README Windows stanza missing required env var

`README.md:179-191` shows the generated Cursor MCP stanza as:

```json
{
  "mcpServers": {
    "terminal-commander": {
      "type": "stdio",
      "command": "terminal-commander-mcp",
      "env": { "TC_WSL_DISTRO": "Ubuntu-24.04" }
    }
  }
}
```

With the current resolver behaviour, this stanza on a clean Windows
machine fails (no Windows-x64 platform package on the registry; no
`TC_USE_LEGACY_WSL_BRIDGE=1` to take the legacy WSL path). The README
does not explain which mode the stanza is supposed to activate.

### 7.2 README claims Windows install is `Live`

`README.md` Status table (§"Status", lines ~376-384) lists "Windows
install bootstrap + bridge" as **Live**. The npm registry state
(§2.1 above) and `docs/release/npm-distribution-final-report.md` §2
disagree.

### 7.3 `terminal-commander doctor` not yet covering the missing-package
case

`packages/terminal-commander/lib/cli/doctor.js` (L1-?) exists but the
audit did not find a check that reports "Windows-x64 platform package
missing from registry" or "shim resolves to junction but no public
package would resolve". A user running `terminal-commander doctor`
after a failed Cursor connect has no obvious next step.

---

## 8. Test/CI findings

### 8.1 No Windows-native smoke test exercises the full path

`scripts/smoke/` contains `verify-runtime-smoke.sh` and
`verify-npm-local-install.sh` (bash). README mentions a
`scripts/smoke/verify-windows-bridge-smoke.ps1` but no such file is
indexed under `scripts/smoke/`. The smoke harness for the failure mode
the user actually hits — "fresh Windows machine, `npm install -g`,
Cursor connects" — does not exist.

### 8.2 mcp_live_command_e2e tests run against UDS only

`crates/mcp/tests/mcp_live_command_e2e.rs`,
`crates/mcp/tests/mcp_live_daemon.rs`, and
`crates/mcp/tests/mcp_stdio.rs` exercise the daemon over UDS. There is
no equivalent test wiring `pipe_server` end-to-end. The named-pipe
path has no live test coverage in the workspace.

### 8.3 npm bootstrap-publish workflow is manual-dispatch only

`.github/workflows/npm-bootstrap-publish.yml` is the bootstrap path
for the first publish per `docs/release/npm-bootstrap-first-publish.md`.
It is a `workflow_dispatch` and has not been fired for `windows-x64`.
There is no CI assertion that the Windows publish has happened before
the README's "Live" claim takes effect.

---

## 9. Stub / placeholder density in the daemon stack

* No `todo!()` or `unimplemented!()` in `crates/` (good).
* `crates/cli/src/main.rs:87` still prints
  `"state         : not running (TC25 stub; IPC arrives in TC21 follow-up)"`
  from the operator CLI's status output.
* `crates/sifters/src/lib.rs::build_draft` (L377-447) comment at L403
  marks a probe-layer hook as a placeholder pending TC15+.
* `crates/mcp/src/lib.rs:34` and `crates/daemon/src/audit.rs:6`
  reference a historical `AuditPlaceholder` seam; the seam is gone but
  the doc references remain.

These are minor on their own; collectively they suggest the operator
CLI surface and parts of the sifter draft path lag the MCP surface
that already advertises 29 live tools.

---

## 10. Severity-ranked failure chain that produces the user's
"Connection closed"

In order of how directly each finding causes the symptom:

1. **§2.1** — `@terminal-commander/windows-x64` not on npm. Any
   clean-install user hits `platform_package_missing` in
   `resolveBinary` and the shim exits 64 before doing anything.
   (User's "npm install does not work" is exactly this.)
2. **§4.1 + §4.4** — Even with the local junction install giving
   `reason: "ok"`, the MCP child runs `stdio: "inherit"` and any
   stdin EOF triggers `cleanup()` which destroys the session.
   (User's "MCP error -32000: Connection closed" within ~200 ms is
   consistent with this teardown after the harness probes the stream.)
3. **§4.2 + §4.3** — Even if the MCP child survives, the daemon
   may not have bound its named pipe before the first tool call; any
   readiness failure is silent unless `TC_DEBUG_SESSION=1`.
4. **§6.1 + §6.2 + §6.3** — Named-pipe accept loop is fragile and the
   security model is weaker than the UDS path, but these do not cause
   the connect-time symptom directly.
5. **§3** — Unresolved Windows-native decision. Until SPEC, ADR, crate
   docs, and README agree, fixes are guesses.
6. **§5, §7, §8, §9** — Surface-level issues that compound diagnosis
   difficulty.

---

## 11. Cross-references for a follow-on planning agent

Files most worth pulling into a planning context:

* Node shim:
  * `packages/terminal-commander/bin/terminal-commander-mcp.js`
  * `packages/terminal-commander/lib/resolve-binary.js`
  * `packages/terminal-commander/lib/daemon/session_supervisor.js`
  * `packages/terminal-commander/lib/bootstrap/orchestrator.js`
  * `packages/terminal-commander/lib/cli/doctor.js`
  * `packages/terminal-commander/scripts/install.js`
* Rust runtime:
  * `crates/daemon/src/runtime.rs`
  * `crates/daemon/src/ipc/mod.rs`
  * `crates/daemon/src/ipc/pipe_server.rs`
  * `crates/daemon/src/ipc/pipe_client.rs`
  * `crates/daemon/src/ipc/framing.rs`
  * `crates/daemon/src/ipc/peer.rs`
  * `crates/daemon/src/ipc/server.rs`
  * `crates/mcp/src/main.rs`
  * `crates/mcp/src/tools.rs`
  * `crates/mcp/src/daemon_client.rs`
* Vision + contract:
  * `SPEC.md` §1-§5
  * `ARCHITECTURE.md` §1-§3
  * `docs/adr/ADR-parent-environment-runners.md`
  * `docs/release/npm-binary-packaging-contract.md` §13c
  * `docs/release/npm-bootstrap-first-publish.md` §3-§5
  * `docs/release/npm-distribution-final-report.md` §2-§4
* CI:
  * `.github/workflows/npm-binary-build.yml`
  * `.github/workflows/npm-bootstrap-publish.yml`
  * `.github/workflows/release-please.yml`
  * `.github/workflows/release-pr-sync.yml`

---

## 12. What this report deliberately does NOT do

* Does not propose specific code edits, refactors, or API choices —
  that is the planning agent's job.
* Does not prejudge whether Windows-native should ship at all — that
  is a vision-level decision flagged at §3.
* Does not run the daemon or rmcp binary; all findings are derived
  from static read of the indexed source plus the triage commands
  already executed during the conversation.

---

## 13. Vision realignment input (operator decision, 2026-05-24)

The operator has stated the target distribution model explicitly, which
resolves §3's three-way conflict and reframes several other findings.
This section captures the decision so the planning agent does not
re-litigate it.

### 13.1 Decision

* **Tier-1 targets:** Windows-x64 native + Linux-x64 native (the latter
  also covers WSL Ubuntu — WSL is not a separate target).
* **Tier-3 build target:** macOS (arm64 + x64). Binaries built and
  released alongside tier-1, but explicitly not prioritized for QA,
  smoke, or doctor coverage in the current planning horizon. Mac
  shipping exists because the cross-compile is essentially free once
  Win/Linux builds are green.
* **Implementation language:** Rust 2024 edition, toolchain 1.95.
  Single workspace, no per-OS forks.
* **Runtime dependency policy:** the running MCP adapter and the
  running daemon are native binaries. Once installed, nothing on the
  runtime path depends on Node, Python, WSL, PowerShell, or shell
  scripts.
* **Install dependency policy:** the install step may use whatever
  delivers the one-command, set-and-forget UX best — npm postinstall
  downloader, winget, scoop, cargo install, or a curl one-liner. Node
  is acceptable during install if `npm install -g terminal-commander`
  is the chosen front door.
* **UX target (the actual operator ask):** one install command on a
  clean machine, never have to think about it again. Whatever
  packaging mechanism delivers that is acceptable. The reference
  feeling is SymForge's "it just works" install — not necessarily
  SymForge's specific implementation.

### 13.2 Findings this decision resolves

* **§3 (SPEC vs ADR vs code disagreement).** Resolved: SPEC §3
  non-goal text for "Windows-native shipping" is now stale and should
  be struck. ADR + crate code + README converge on the "Windows native
  in scope" side.
* **§2.1 (`@terminal-commander/windows-x64` missing from registry).**
  Subsumed: the npm-platform-package fanout (windows-x64, linux-x64,
  linux-arm64, root shim) is the wrong distribution model under the
  decision. The planning agent should treat this as a packaging
  migration, not a "publish the missing package" task.
* **§2.4 (no `.cmd` shim).** Subsumed: with a native binary on PATH,
  Windows PATHEXT covers `.exe` directly; no Node wrapper means no
  pwsh/cmd asymmetry.
* **§6.5 (PTY `#[cfg(unix)]`-only).** Resolved by adopting a
  cross-platform PTY crate that works on both targets (e.g.
  `portable-pty`, already proven in WezTerm). Tool catalogue stops
  hiding PTY tools per-OS.
* **§3 doc-comment in `crates/daemon/src/ipc/mod.rs:14-16`** ("Windows
  native: NOT SUPPORTED") is now stale and contradicts the decision.

### 13.3 Findings this decision does NOT change

The lifecycle, IPC, security, diagnostics, and CI findings (§4, §6.1,
§6.2, §6.3, §6.4, §7, §8) remain in scope. They are bugs in the
Windows path that exist regardless of how the binary is distributed.
In particular:

* §4.1 / §4.4 — `stdio: "inherit"` + cleanup-on-exit lifecycle. Under
  a Rust-only distribution there is no Node supervisor at all, so the
  lifecycle moves into the MCP binary's own runtime. The class of bug
  has to be re-addressed there, not deleted.
* §6.1 — named-pipe accept loop fragility.
* §6.2 — pipe peer credential hard-coded `{uid:0, gid:0}`.
* §6.3 — pipe ACL/SDDL not restricted to the current user.
* §6.4 — daemon stderr should be captured to a per-session log file
  under both distribution models.
* §8.1 — Windows smoke test still missing.
* §8.2 — named-pipe e2e test still missing.

### 13.4 Findings this decision newly introduces

* **The entire `packages/terminal-commander*` Node tree becomes
  out-of-scope artifact.** The bootstrap orchestrator
  (`lib/bootstrap/`), the WSL bridge (`lib/wsl/`), the session
  supervisor (`lib/daemon/session_supervisor.js`), the harness writers
  (`lib/harness/`), the resolver (`lib/resolve-binary.js`), and the
  install script (`scripts/install.js`) all lose their reason to
  exist. The planning agent should explicitly decide: delete, archive,
  or repurpose any pieces with reusable logic (e.g. the harness-config
  writers, which would still be useful as a Rust subcommand:
  `terminal-commander setup harness`).
* **Per-harness MCP config writers must be re-implemented in Rust.**
  README §"Harness configuration" promises auto-config for Cursor,
  Codex CLI, Claude Code, and Claude Desktop. Currently that lives in
  `packages/terminal-commander/lib/harness/`. Under the decision it
  moves into the Rust CLI's `setup harness` path.
* **Release pipeline switches from npm publish to GitHub Releases +
  optional `cargo install`.** `release-please.yml`,
  `release-pr-sync.yml`, `npm-binary-build.yml`, and
  `npm-bootstrap-publish.yml` need to be re-pointed. The `docs/release/`
  contract docs (NPM01-NPM10) become reference-only history.
* **IPC transport choice should be revisited.** The current code uses
  UDS on Unix + named pipe on Windows. Both work in Rust and both are
  already implemented. Decision-pending: keep that asymmetry (two
  transports, one product) or collapse to a single mechanism that
  works identically on both targets (e.g. loopback TCP + token auth,
  or a single abstraction over UDS / named-pipe with shared framing,
  which the codebase partly has at `crates/daemon/src/ipc/framing.rs`).
  Planning agent should weigh this; both are defensible.

### 13.5 Concrete planning agent must-decide list (from §13)

1. Strike or revise SPEC §3 non-goals to reflect the new tier-1 list.
2. Update `crates/daemon/src/ipc/mod.rs` module doc (L14-16).
3. Pick IPC transport story: keep asymmetric UDS+pipe, or unify.
4. Decide fate of `packages/terminal-commander*` (delete vs. archive).
5. Decide release vehicle: pure GitHub Releases, Cargo, or both.
6. Decide PTY crate (likely `portable-pty`) and remove `#[cfg(unix)]`
   gates from tool catalogue.
7. Decide whether `terminal-commander setup harness` lives in the
   daemon CLI or a separate `terminal-commander` operator binary
   (the latter already exists per `crates/cli/src/main.rs`).
8. Decide whether the Rust binary self-installs autostart on first run
   (Windows: Task Scheduler / startup folder / WinSW; Linux: systemd
   user unit) or whether autostart is opt-in via a CLI subcommand.
9. WSL Ubuntu is a Linux target — confirm no special-case is required
   beyond the Linux-x64 artifact. (No more `TC_WSL_DISTRO`,
   `TC_USE_LEGACY_WSL_BRIDGE`, or `wsl.exe -d` invocations on the
   runtime path.)
