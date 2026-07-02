# Terminal Commander - Embeddable Engine (`terminal-commander-embed`)

Status: Design APPROVED (brainstorming complete). Pending: spec review, then
writing-plans -> phased implementation.
Date: 2026-06-26.
Scope: structural design for an engine-only, daemon-free library crate so a
sibling project (AAP) can link the run/comb/PTY/session engine in-process on the
host and inside a musl Firecracker microVM guest-agent. ASCII only.

---

## 1. Objective

Make Terminal Commander embeddable as an engine-only Rust library. A sibling
project links the run/comb/PTY/shell-session engine IN-PROCESS - on the host AND
inside a musl guest-agent - with ZERO daemon, ZERO IPC socket, and ZERO MCP
surface. Reach the same posture symforge is already at.

"Done" looks like: AAP adds

```toml
terminal-commander-embed = { git = "...", default-features = false }
```

and, in-process, with no daemon and no socket: constructs an engine instance,
runs a one-shot command and collects exit + rule matches; starts a long-running
/ PTY command and writes stdin + reads combed (structured-signal) output;
registers/activates sifter rules; reads events + a bounded output tail; and it
compiles for `x86_64-unknown-linux-musl`.

### Non-goals

- Do NOT delete or weaken daemon/mcp/cli - the external MCP server stays for
  non-AAP harnesses.
- Do NOT change the MCP tool surface.
- Do NOT put any AAP-specific logic in TC - AAP wraps the generic engine in its
  own `aap-terminal` adapter.
- Do NOT vendor TC into AAP - it stays its own git-dep repo.

---

## 2. Context

Three sibling repos under `E:\project\`, all ours:

- **symforge** (`../symforge`) - ALREADY at the target. Single crate,
  `default = ["server"]`, `embed = []` (an empty gate). Every server dep
  (rmcp, axum, clap, reqwest, socket2, notify, tracing-subscriber) is
  `optional = true` and gated behind `server`; engine modules are always
  compiled and exposed via a flat `symforge::embed` re-export facade, pinned by
  a `#[cfg(test)] mod contract`. A CI `embed-musl` job cross-compiles
  `--no-default-features --features embed` to `x86_64-unknown-linux-musl`.
  Edition 2024, toolchain 1.96.

- **AAP** (`Agent_Army_Professionals`) - the consumer. MIT, edition 2024,
  MSRV 1.93, `unsafe_code = "forbid"` workspace-wide (sole exception: the
  guest-agent's vsock fd code). It depends on symforge as a branch-tracked git
  dep, `default-features = false, features = ["embed"]`, and statically links it
  into the `x86_64-unknown-linux-musl` guest-agent. Its blueprint
  (`docs/handover/tool-execution-kernel.md`) already prescribes: EMBED TC
  `core` + `probes` (daemon-free) behind a thin `aap-terminal` adapter; drive
  `ProcessProbe::spawn/wait/cancel` directly (NOT the daemon `CommandRuntime`,
  which it calls "binary-only"); consume the sifter event stream,
  `BucketManager::events_since`, `ContextRingManager`, and
  `JobRecord.last_output_at`. No AAP code touches TC yet - the integration is
  designed, not built.

- **terminal-commander** (THIS repo) - a 9-crate workspace
  (`core, sifters, probes, store, ipc, daemon, mcp, cli, supervisor`),
  edition 2024, rust-version 1.92, `unsafe_code = "forbid"` (per-crate
  overrides only for Win32 FFI in daemon/probes). Two processes today:
  `mcp` -> IPC -> `daemon`. There is no way to use the engine without running
  the daemon and speaking IPC/MCP.

---

## 3. Findings (recon, evidence-backed)

These reframe the work from "heavy extraction" to "moderate-leaning-light":

1. **The engine crates are already clean.** `core`, `sifters`, `probes`,
   `store` carry zero server deps (no rmcp/clap/axum/socket2/
   tracing-subscriber). `probes` is the run/PTY engine (tokio process +
   `pty-process` on unix + `portable-pty`/Job-Objects on windows); `core` holds
   the bucket/context-ring/job managers; `sifters` is the comb engine; `store`
   is rusqlite-bundled.

2. **The orchestration is daemon-free in all but file location.**
   `CommandRuntime`, `PtyRuntime`, `ShellSessionRuntime`, `WatchRuntime` take
   engine handles in their constructors, hold ZERO reference to the daemon
   `State`/IPC/config (only a doc comment mentions `DaemonState`), and return
   plain Rust types that a SEPARATE `ipc/handlers/*` layer maps to the wire
   protocol. `run_and_watch` is not even in the daemon - it is an MCP poll loop
   over `command_start` + `command_status`.

3. **The `ipc` crate already splits** pure wire types (`protocol`, `framing`)
   from the cfg-gated transport (`client::DaemonClient` /
   `pipe_client::DaemonClient`). The engine can depend on `ipc`'s pure types
   without ever touching the socket.

4. **musl is clean for the engine path.** No `unsafe` and no Windows-only code
   in the orchestration; `linux-musl` compiles only the unix arms. All Win32
   Job-Object/ConPTY `unsafe` lives in `probes` under `#[cfg(windows)]`.

The unit that moves out of the daemon (~9.3k-10k LOC): `command`,
`pty_command`, `shell`, `shell_session`, `file_watch`, plus the private support
`policy`, `store_actor`, `activation`, `router`, `audit`,
`subscriptions::source`. What STAYS in the daemon: `runtime.rs` (lifecycle/
pidfile/signals), `state.rs` (assembler), `ipc/*` (server + handlers),
`config.rs` (clap), `main.rs`, and `supervisor`.

---

## 4. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| License of the embed graph | **No change - stays PolyForm-Noncommercial-1.0.0** | AAP is the consumer and is non-commercial internal use, which PolyForm-NC permits. AAP's MIT governs AAP's own code; the dependency constrains the combined work to noncommercial, which AAP's use satisfies. Caveat: if AAP ever ships commercially, dual-license the engine graph (`MIT OR PolyForm-Noncommercial-1.0.0`) at that time. |
| Facade scope (v1) | **Full** | First-class: run+watch, PTY (start/stdin/read/stop), long-running shell session (start/exec/workspace/stop), file watching, sifter rule register/activate, events + bounded tail. |
| Shared wire types | **Reuse `terminal-commander-ipc` pure types** | Engine returns enums (`Liveness`, `SessionState`, `ProbeKind`, the `*Response` data) that live in `ipc::protocol`. embed depends on `ipc` for those; the cfg-gated `DaemonClient` transport is never used (dead code, no socket). Passes acceptance: the tree has no rmcp/clap/socket2/axum/supervisor/daemon-IPC-server. |
| Crate topology | **One new crate** (`terminal-commander-embed`) | The prompt delegated this. The workspace already quarantines server deps by crate boundary, so one crate holding the lifted orchestration + the facade (with the daemon wrapping it) mirrors symforge and avoids a speculative engine-vs-facade split. |

---

## 5. Crate topology

```
terminal-commander-embed   (NEW crate: crates/embed)
  ├─ lifted orchestration:  command, pty_command, shell, shell_session,
  │                         file_watch, policy, store_actor, activation,
  │                         router, audit, subscriptions::source
  ├─ Engine facade:         curated public entrypoint (full scope)
  ├─ re-exported primitives: CommandRuntime, PtyRuntime, ShellSessionRuntime,
  │                         WatchRuntime, BucketManager handles (AAP low-level path)
  ├─ #[cfg(test)] mod contract: pins the public ABI (symforge-style)
  └─ deps: core, sifters, probes, store, ipc(pure types), tokio
     NOT: supervisor, rmcp, clap, axum, socket2, tracing-subscriber, daemon-IPC-server
     license: PolyForm-Noncommercial-1.0.0 (unchanged)

terminal-commanderd  (daemon, now a THIN wrapper)
  = depends on embed + keeps: runtime.rs (lifecycle/pidfile/signals),
    state.rs (assembler -> builds embed handles), ipc/* (server + handlers
    that map wire <-> embed types), config.rs (clap), main.rs, supervisor

terminal-commander-mcp   - UNCHANGED (still depends on daemon)
```

### Quarantine via crate boundary (no feature flag)

symforge feature-gates server deps inside one crate because it IS one crate. TC
already quarantines every server dep by crate boundary (rmcp/clap/etc. live only
in daemon/mcp/cli/supervisor). So the embed crate needs NO `embed`/`server`
feature - it is engine-only by construction; the crate graph is the gate.
`default = ["server"]` is irrelevant here because daemon/mcp/cli are separate,
always-built crates.

---

## 6. The `Engine` facade (illustrative; exact types nailed in the plan)

```rust
// terminal_commander_embed
pub struct Engine { /* owns assembled engine handles; NO socket/pidfile/boot_id */ }

pub struct EngineConfig {
    pub data_dir: PathBuf,       // sqlite + state; no XDG/daemon assumptions
    pub policy: PolicyProfile,   // reuse existing PolicyProfile
    pub limits: Limits,          // bounded reads/caps (reuse ipc MAX_* defaults)
}

impl Engine {
    pub async fn start(config: EngineConfig) -> Result<Self, EngineError>; // multiple instances coexist; NO global singleton

    // run + comb
    pub async fn run_combed(&self, req: CommandStartRequest) -> Result<CommandHandle, EngineError>;
    pub async fn command_status(&self, job: JobId) -> Result<CommandStatus, EngineError>;
    pub async fn run_and_watch(&self, req: CommandStartRequest, watch: WatchOpts) -> Result<RunWatchOutcome, EngineError>; // in-proc poll loop; no socket

    // PTY
    pub async fn pty_start(&self, req: PtyStartRequest) -> Result<PtyHandle, EngineError>;
    pub async fn pty_write_stdin(&self, job: JobId, bytes: &[u8]) -> Result<PtyWriteResponse, EngineError>;
    pub async fn pty_stop(&self, job: JobId) -> Result<(), EngineError>;

    // long-running shell session (full scope)
    pub async fn session_start(&self, req: SessionStartRequest) -> Result<SessionHandle, EngineError>;
    pub async fn session_exec(&self, id: SessionId, line: &str) -> Result<SessionExecOutcome, EngineError>;
    pub async fn session_stop(&self, id: SessionId) -> Result<(), EngineError>;

    // file watching (full scope)
    pub async fn watch_start(&self, req: WatchStartRequest) -> Result<WatchHandle, EngineError>;
    pub async fn watch_stop(&self, id: WatchId) -> Result<(), EngineError>;

    // sifter rules
    pub async fn register_rule(&self, rule: RuleDraft) -> Result<RuleId, EngineError>;
    pub async fn activate_rule(&self, id: RuleId) -> Result<(), EngineError>;

    // events + bounded tail
    pub async fn events_since(&self, bucket: BucketId, cursor: Cursor, limit: usize) -> Result<Vec<SignalEvent>, EngineError>;
    pub async fn bucket_wait(&self, bucket: BucketId, cursor: Cursor, timeout: Duration) -> Result<BucketWaitOutcome, EngineError>;
    pub async fn output_tail(&self, job: JobId, limit: TailLimit) -> Result<OutputTail, EngineError>;
    pub fn event_context(&self, event_id: EventId, before: usize, after: usize) -> Result<EventContext, EngineError>;
}
```

The facade is a thin layer over the lifted runtimes + `BucketManager` reads - it
replicates `state.rs::bootstrap` MINUS boot_id, shutdown_tx, the subscription
registry, the idle reaper, and the socket. A `#[cfg(test)] mod contract` pins
every facade signature (symforge-style), so an ABI drift fails TC's own test
suite. Primitives are re-exported for AAP's lower-level path.

---

## 7. Daemon rewiring (behavior-preserving)

`ipc/handlers/*` change `crate::command::CommandRuntime` ->
`terminal_commander_embed::CommandRuntime` (mechanical path rewrite).
`state.rs::bootstrap` constructs the embed handles (or an `Engine`) and layers
the daemon-only concerns (boot_id, shutdown_tx, subscription registry, reaper,
ipc server) on top. `mcp` is untouched. `cargo test --workspace` staying green
is the proof the daemon's behavior is unchanged.

The `subscriptions` module is SPLIT: only `source.rs` (`BucketSourceTable`)
travels into embed; `registry.rs`/`pull.rs`/`model.rs` stay in the daemon (the
subscription IPC feature is daemon-only). Recon shows `source.rs` is clean
(156 LOC, no back-ref to `registry`).

---

## 8. musl posture + the one real risk

Engine path has zero `unsafe` and zero Windows-only code; `linux-musl` compiles
only the unix arms. `rusqlite` is bundled (honors target `CC`). **The one thing
symforge never had to solve: `pty-process` (unix PTY; uses libc/nix) on musl.**
Mitigation: a Phase-0 spike builds it for musl in WSL/CI. Contingency: if it
will not build/run on musl, add a default-on `pty` feature so a musl build can
drop the PTY lane and still offer non-PTY run+comb. We do NOT pre-build that
feature (YAGNI) unless the spike fails.

---

## 9. Acceptance criteria (prove each, paste evidence)

- `cargo build -p terminal-commander-embed` succeeds (no `--no-default-features`
  needed - there is no server feature to disable).
- `cargo tree -p terminal-commander-embed` shows NO rmcp/clap/socket2/axum/
  supervisor/daemon-IPC-server. (It WILL show `terminal-commander-ipc` + tokio
  net - acceptable per the wire-types decision.)
- `cargo build -p terminal-commander-embed --target x86_64-unknown-linux-musl`
  succeeds (CI job, mirrors symforge's `embed-musl`; the Windows dev box has no
  musl linker).
- `cargo test --workspace` stays green - daemon + MCP behaviour unchanged.
- `examples/embed_run.rs` (committed): with no daemon and no socket, runs
  `echo hi` and collects exit + a rule match, then starts a PTY command, writes
  stdin, and reads combed output. Runs via `cargo run --example embed_run`.
- The embed public API is documented (AAP is the only consumer; minimal
  surface).

---

## 10. Phasing (workspace green at each step)

0. **Baseline + spikes** - confirm `file_watch` + `subscriptions::source` are
   engine-shaped (no supervisor/ipc-transport/State back-refs); musl
   `pty-process` spike. No code moves.
1. **Scaffold embed + move leaf support** - router, audit, activation,
   store_actor, subscriptions::source, policy. Daemon re-exports from embed so
   it keeps compiling. Verify `cargo test --workspace`.
2. **Move orchestration** - command, pty_command, shell, shell_session,
   file_watch. Daemon handlers call `terminal_commander_embed::*`. Verify.
3. **Engine facade + assembler + contract test**; rewire `state.rs`. Verify.
4. **musl CI job + `examples/embed_run.rs` + cargo-tree assertion + API docs**
   (+ the `pty` feature only if the Phase-0 spike failed).

Each phase is committed to a review branch; no push/merge without approval.

---

## 11. Risks

- `pty-process` on musl - the only genuine unknown (Phase-0 spike + `pty`
  feature contingency).
- `policy.rs` (~2.2k LOC) and `store_actor.rs` (~0.85k LOC) are the heavy
  movers - self-contained, just large.
- `subscriptions` must be split (only `source.rs` travels) - confirmed
  low-risk by recon.
- Keeping `terminal-commander-ipc` in the embed graph means `cargo tree` shows
  `ipc` + tokio-net. Acceptable per the wire-types decision; optional future
  polish is to relocate the ~5 shared enums into `core` so the embed tree is
  fully ipc-free.
