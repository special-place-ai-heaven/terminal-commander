# Runtime Gap Audit — TC33

Goal: `TC33-code-reality-audit-and-runtime-pivot.md`
Branch: `main`
Audited commit: `a667010f43807dd3da53bfdc11b89ecc3b7b7825`
Audit date: 2026-05-21
Scope: classify every audited surface as live, partial, scaffold-only,
test-only, degraded, blocked, deferred, or unknown. No code is changed
by TC33.

## Baseline verification

| Command | Result |
|---|---|
| `git diff --check` | clean (no output) |
| `cargo metadata --no-deps` | parses; 7 workspace members |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo nextest run --workspace` | 189 / 189 PASS |

Baseline is green. TC33 does not need to fix anything. The chain is
test-clean but runtime-incomplete.

Metadata note: `rust_version` reads `1.92` in `Cargo.toml`
(`workspace.package.rust-version`). The doctrine amendment of
2026-05-XX selected `1.95.0` as the local toolchain (see
`docs/research/_USER_DECISIONS.md`). This is a documentation drift,
not a build failure; `rust-toolchain.toml` is the authority for the
toolchain that actually runs.

## Surface classification

Legend:

- **live** — implementation is real, exercised by code paths the LLM
  would actually traverse if a real MCP client attached.
- **partial** — present but missing a sub-capability named in the spec
  or backlog.
- **scaffold-only** — file exists, exits / refuses immediately, never
  performs runtime work.
- **test-only** — exists only inside `#[cfg(test)]` or only callable
  from in-process tests.
- **degraded** — works but on a substitute path (e.g. polling instead
  of inotify, in-process router instead of UDS).
- **deferred** — explicitly punted by a previous goal lock, named in
  `BACKLOG.md` P0/P1/P2/P3.
- **blocked** — would need code, config, or platform that is not
  available here.

### Binaries

| Surface | File | Status | Evidence |
|---|---|---|---|
| `terminal-commanderd` binary | `crates/daemon/src/main.rs` | scaffold-only | `eprintln!("terminal-commanderd: scaffold only (TC04). ...Refusing to start.");` exits `EX_USAGE=64`. No tokio runtime, no Router, no IPC listener. |
| `terminal-commander-mcp` binary | `crates/mcp/src/main.rs` | scaffold-only | Same pattern: `eprintln!("terminal-commander-mcp: scaffold only (TC04). ...Refusing to start.");` exits 64. No rmcp stdio attach. |
| `terminal-commander` CLI binary | `crates/cli/src/main.rs` | partial (parses; offline) | Clap subcommands present (`status`, `doctor`, `rules`, `buckets`, `jobs`, `probes`, `policy`, `audit`). Bodies print `(offline CLI; daemon attach deferred)` placeholders. Does not connect to a daemon. |

### Daemon library + router

| Surface | File | Status | Evidence |
|---|---|---|---|
| `Router` in-process API | `crates/daemon/src/router.rs` | live (in-process only) | `Router::new` wires `Arc<BucketManager>`, `Arc<ContextRingManager>`, `Arc<JobManager>`, `Arc<SifterRuntime>`. Methods `bucket_create`, `bucket_append`, `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context`, `job_start/finish/cancel/get` all functional. |
| `AuditPlaceholder` | same | scaffold-only seam | `Mutex<Vec<AuditRecord>>` in memory. Every emit writes `decision = "allow_placeholder"`. No durability, no schema. Module doc explicitly says "TC22 wires the real audit log". |
| `PolicyEngine` | `crates/daemon/src/policy.rs` | live | Four profiles (`developer_local`, `repo_only`, `read_only_observer`, `admin_debug`). Closed-set `PolicyDecision { Allow, Deny, AllowWithAudit, Error }`. `COMMANDS_DENY` for sudo / doas / su / pkexec / kexec / polkit-agent / polkit-auth-agent-1. 14 default-deny path suffixes. |
| Local API transport (UDS / JSON-RPC) | none on `main` | deferred / blocked | TC21 mini-spec names UDS as the eventual transport. Not in code. CLI says `daemon attach deferred`. |

### MCP tool surface

| Surface | File | Status | Evidence |
|---|---|---|---|
| `ToolSurface` struct | `crates/mcp/src/lib.rs` | live (in-process only) | Wraps `Arc<Router>` + `PolicyEngine`. Tools implemented: `system_discover`, `bucket_events_since`, `bucket_wait`, `bucket_summary`, `event_context`, `file_read_window`, `registry_search`, `registry_get`, `registry_create`, `registry_activate`. Policy is consulted on every call. |
| `system_discover.tools[]` list | same | partial | Advertises 5 tools. Other implemented tools (`file_read_window`, `registry_*`) are not in the list. |
| Bounded output | same | live | `MAX_FILE_WINDOW_BYTES = 64 * 1024`. `bucket_events_since` capped via `MAX_READ_LIMIT`. `event_context` capped by ring `max_bytes`. |
| rmcp 1.7.0 stdio adapter | none on `main` | deferred / scaffold-only binary | `crates/mcp/src/main.rs` is scaffold-only. Lib doc: "rmcp stdio adapter is deferred to a follow-up". |
| Command tools (`command_start_combed`, `command_status`, `command_write_stdin`, `command_send_signal`) | none on `main` | not implemented in `ToolSurface` | README + ROADMAP list them; the in-process `ToolSurface` does not expose them. |
| Probe tools (`probe_create`, `probe_bind_rules`) | none on `main` | not implemented in `ToolSurface` | Same. |
| Registry test (`registry_test`) | none on `main` | not implemented in `ToolSurface` | Same. |
| File tools (`file_search`, `file_watch`) | none on `main` | not implemented in `ToolSurface` | Only `file_read_window` ships. |

### Probes

| Surface | File | Status | Evidence |
|---|---|---|---|
| Process probe | `crates/probes/src/process.rs` (462 LOC) | live (non-interactive) | `tokio::process::Command`, concurrent stdout / stderr line readers, sifter feed, `EventSink`, `InMemorySink`, `ProcessProbeMetrics`, `DEFAULT_GRACE = 10s`. |
| Process-group integration (`process-wrap`) | none on `main` | deferred | BACKLOG P1. Cancellation is forced kill; SIGTERM ladder absent. |
| PTY spawn | none on `main` | deferred / partial | `crates/probes/src/pty.rs` ships `AnsiNormalizer` (vte 0.15) + `PromptDetector` only. Doc: "The actual pty-process spawn path is deferred to a POSIX harness". |
| File probe | `crates/probes/src/file.rs` (414 LOC) | live (degraded — polling) | `DEFAULT_POLL_INTERVAL = 250ms`. `FileProbeMode::{ScanOnce, FollowEnd, FollowBeginning}`. Handles create-after-start, truncation, rotation. notify backend deferred (BACKLOG P1). |
| Directory probe | `crates/probes/src/directory.rs` (359 LOC) | live (degraded — polling) | `DEFAULT_DIR_POLL_INTERVAL = 500ms`. Created / modified / deleted events. Move events surface as delete+create. JUnit-XML summary present. inotify deferred (BACKLOG P1). |
| Journal probe | none on `main` | not implemented | Not in MVP. |
| Artifact probe (general) | none on `main` | partial | Only JUnit-XML summary inside directory probe. |

### Store + registry

| Surface | File | Status | Evidence |
|---|---|---|---|
| `EventStore` | `crates/store/src/lib.rs` (946 LOC) | live | rusqlite 0.39 bundled, manual migration runner over `schema_migrations` table, V0001 initial schema, V0002 registry, WAL + pragmas, VACUUM INTO backup, in-memory open, cursor reads, severity_min filter, count cap eviction. |
| Migration V0001 | `crates/store/migrations/V0001__initial_schema.sql` | live | Creates `buckets`, `events`. |
| Migration V0002 | `crates/store/migrations/V0002__registry.sql` | live | Creates `rules`, `rule_versions`, `rule_tags`, `rule_activations`. |
| Migration V0003 (audit) | none on `main` | deferred | BACKLOG P0. Audit table does not exist. |
| Registry persistence | `crates/store/src/registry.rs` | live | Versioned rule create, latest / specific version lookup, tag search, activation log. |
| Rule pack import | `crates/store/src/import.rs` | live | Imports JSON rule packs with regex + DFA size limits (`RULE_PACK_REGEX_SIZE_LIMIT`, `RULE_PACK_DFA_SIZE_LIMIT`). |
| Audit table + writes | none on `main` | deferred / blocked | No DDL, no `INSERT INTO audit_*`, no SELECT. The `AuditPlaceholder` in the daemon has no persistence target. |
| Audit hash chain | none on `main` | deferred (BACKLOG P1) | Post-MVP. |

### Buckets + context

| Surface | File | Status | Evidence |
|---|---|---|---|
| `BucketManager` | `crates/core/src/bucket.rs` (949 LOC) | live | `Arc<RwLock<HashMap<BucketId, Arc<RwLock<BucketInner>>>>>`. Per-bucket `Notify`. `BucketReadRequest`, `BucketWaitRequest`, summary, severity histogram. |
| `bucket_wait` realtime channel | same | live | `tokio::sync::Notify` — no polling. Race between `notified.as_mut()` and `tokio::time::timeout`. Returns heartbeat on timeout, events on signal. |
| Per-bucket retention | same | live | Count cap (`DEFAULT_BUCKET_MAX_EVENTS = 100_000`) + TTL (`DEFAULT_BUCKET_TTL = 24h`). Drop-oldest with `dropped_count`. |
| Pointer / pointer-reason invariant | `crates/core/src/event.rs` | live | `SignalEvent::validate` enforces severity >= Medium implies SourcePointer OR `pointer_unavailable_reason`. TC02 negative-contract fixture forbidden / missing-pointer.v1.json. |
| `ContextRingManager` | `crates/core/src/context.rs` (704 LOC) | live | Per-probe rings of `SourceFrame`. Anchored windows by `FrameId`. `anchor_missing` flag on miss. `max_bytes` cap. |

### Sifters

| Surface | File | Status | Evidence |
|---|---|---|---|
| `SifterRuntime` | `crates/sifters/src/lib.rs` | live | aho-corasick (keyword DFA) + regex `RegexSet`. Rule kinds present per registry: Keyword, Regex, Condition (numeric), Multiline, Dedupe, Suppression, Stall, Progress, Prompt, Correlation, Artifact. Frame-size cap enforced. Redact list applied to captures. |
| Hot rule activation in live probes | none on `main` | not implemented | `SifterRuntime::build` is one-shot at construction. No rebind path on registry activate. |

### Policy + security

| Surface | Status | Evidence |
|---|---|---|
| `PolicyEngine` four-profile evaluator | live | `policy::tests` cover allow / deny / allow-with-audit per profile. |
| sudo / pkexec deny across all profiles | live | `crates/daemon/tests/security.rs::structural_deny_sudo_all_profiles`, `fully_qualified_sudo_path_also_denied`. |
| 14 default-deny path suffixes | live | `crates/daemon/tests/security.rs::sensitive_path_default_deny_paths_all_variants`. |
| No `Command::spawn` in `mcp` crate | live | `mcp_crate_contains_no_command_spawn` security test. |
| No TCP listener in `mcp` crate | live | `mcp_crate_contains_no_tcp_listener` security test. |
| Audit log write enforcement | scaffold-only | AuditPlaceholder in memory. No durable record of policy decisions. |
| Landlock / seccomp-bpf | deferred | BACKLOG P2. Advisory enforcement only at MVP. |

## End-to-end MCP path on `main`

Question: can current `main` serve an LLM through real MCP stdio +
command execution?

Answer: **no**.

- `crates/mcp/src/main.rs` exits 64 immediately. No rmcp transport
  binds stdin / stdout. An MCP client cannot attach.
- `crates/daemon/src/main.rs` exits 64 immediately. No runtime loop
  spawns probes or holds buckets.
- The CLI parses but every subcommand prints an "offline CLI" string.

What does work today is the in-process `ToolSurface` exercised by
`crates/mcp/tests/e2e.rs`. That test:

1. Constructs `BucketManager`, `ContextRingManager`, `JobManager`,
   `SifterRuntime` directly.
2. Wraps them in `Router`.
3. Wraps `Router` in `ToolSurface` with the default policy engine.
4. Calls `system_discover`, `bucket_events_since`, `bucket_wait`,
   `event_context`, `file_read_window`, `registry_create`,
   `registry_search`, `registry_activate` in process.

This is **not** an MCP runtime path. It is library-level integration
test coverage with no transport.

`command_start_combed` is the most visible missing tool. It is not on
the `ToolSurface`. The product README and roadmap list it as the
headline tool. Process spawning today happens via `ProcessProbe::spawn`
in the probes crate, which has no MCP-facing entry point.

## P0 backlog accuracy

The four P0 items in `BACKLOG.md` are still accurate as of
`a667010`:

1. **rmcp 1.7.0 stdio adapter** — confirmed. `crates/mcp/src/main.rs`
   is scaffold-only; lib.rs comment says "deferred".
2. **pty-process spawn path** — confirmed. `crates/probes/src/pty.rs`
   ships normalizer + prompt detection only.
3. **daemon IPC transport (UDS)** — confirmed. No socket open / accept
   anywhere; CLI says daemon attach is deferred.
4. **Persistent audit log writes** — confirmed. No `audit` table, no
   writer, AuditPlaceholder is a `Mutex<Vec<AuditRecord>>`.

## P0-adjacent gaps (called out for the next chain to assign)

These are not in the current P0 list but are required for a real
realtime signal channel:

- **`command_start_combed` MCP wiring** — process probes exist; the
  MCP tool that starts a probe and binds rules does not.
- **`bucket_wait` reachable from MCP** — `ToolSurface::bucket_wait`
  is in-process only. With no transport, no MCP client can call it.
- **`event_context` reachable from MCP** — same. Library-level only.
- **Hot rule activation rebind in live probes** — `SifterRuntime` is
  one-shot. Registry `activate` does not affect running probes.
- **Multi-probe / multi-bucket routing** — no probe router or rule
  bind table is implemented beyond a single `BucketId` per probe.
- **Provider-neutral smoke harness** — no integration test exercises
  a real stdio MCP client against this codebase.
- **Load / backpressure evidence** — no throughput, ring backpressure,
  or queue-bound test.

## Source-status drift to record

- `crates/store/src/lib.rs` module doc says the persistent audit log
  "rides on the same database file". On `main` there is no audit
  table or writer. The comment overstates the current state.
  Recommend the V0003 audit migration goal correct this.
- `Cargo.toml workspace.package.rust-version = "1.92"` differs from
  the locally-installed `1.95.0` chosen via `_USER_DECISIONS.md`.
  Recommend a future doc-only sync goal align them, but not as part
  of TC33.

## Conclusion

`main` is library-correct (clippy clean, 189 / 189 nextest pass) and
runtime-incomplete. The four P0 items in `BACKLOG.md` accurately
describe the gap between scaffold and shippable MCP behavior. TC34
through TC48 do not need scope correction at this time; they are
already pointed at these gaps. TC33 closes Pending and the next chain
goal is TC34.
