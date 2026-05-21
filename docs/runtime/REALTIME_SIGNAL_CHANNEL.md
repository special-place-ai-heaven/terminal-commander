# Realtime Signal Channel — Product Contract

Status: Locked at TC34. Normative for TC35-TC48 and all subsequent
runtime goals.
Anchored by: `docs/audits/runtime-gap-audit.md`,
`docs/audits/runtime-source-map.md`,
`docs/audits/runtime-tool-surface-gap.md` (TC33).
Language: ASCII only.

## 1. Product identity

Terminal Commander is a **realtime signal channel and tool-control
abstraction layer for LLM agents**.

It is NOT:

- A CLI that runs commands on behalf of the user.
- A log shipper, log indexer, or observability backend.
- A general terminal multiplexer.
- A knowledge base of stored documents.
- An MCP root shell.

It IS:

- A local daemon and MCP server that lets an LLM **state intent** and
  receive **only structured signal** in response.
- A **probe runtime** that performs the terminal and filesystem toil
  on the LLM's behalf.
- A **sifter / filter / index layer** that converts raw streams into
  cursor-addressable bucket events.
- A **bounded-context provider** that exposes the raw frames behind
  any event only through pointer-anchored windows, never as a stream.

The LLM should never read a raw terminal stream or a raw file to make
progress through Terminal Commander. If the LLM must read raw text to
make a decision, Terminal Commander has failed at its job.

## 2. The intent / signal exchange

The expected interaction loop is:

```text
LLM intent:
  "run this command, watch stdout, stderr, and these files,
   use these registry rules, notify me only when something
   useful happens, give me context around event X if I need
   to look closer."

Terminal Commander does:
  spawn / observe / normalize / sift / index / store / serve.

Terminal Commander returns:
  structured signal events,
  bounded context windows,
  bounded file slices,
  policy decisions,
  registry hits.

Terminal Commander does NOT return:
  raw streams,
  whole files,
  unbounded blobs,
  shell results without policy gating,
  unfiltered network output.
```

## 3. Required capabilities for beta

The system is beta-ready when an LLM, attached via a provider-neutral
MCP client, can perform the following flow without ever reading raw
output:

1. Call a `command_*` tool to start a policy-approved command. Get
   back a bucket id (and a job id) referring to the streamed work.
2. Call `bucket_wait` with a cursor and `severity_min` and receive
   structured signal events as they emerge.
3. Call `event_context` against any returned event and receive a
   bounded window of frames around the anchor.
4. Call a file tool (`file_search`, `file_read_window`, or
   `file_watch`) to perform bounded filesystem intelligence without
   loading whole files.
5. Search the rule registry, optionally create a new rule version,
   test it on sample input, and activate it. The activation MUST
   take effect on **already-running** probes (hot rebind).
6. Receive a heartbeat on `bucket_wait` timeout. Never a raw dump.

A capability that requires the LLM to "just read the log" or "tail
this file" is a contract violation, not a workaround.

## 4. Subsystems and their roles

| Subsystem | Role | Owner crate (live source) |
|---|---|---|
| MCP adapter | Speaks the MCP transport, forwards every tool call to the daemon. NEVER spawns commands, NEVER opens sockets, NEVER reads files outside its own config. | `crates/mcp` |
| Daemon runtime | Owns probes, buckets, context rings, job manager, sifter runtime, policy engine, audit log. Holds the privileged code path. | `crates/daemon` + workspace crates it composes |
| Probes | Convert real-world sources (process, terminal, file, directory) into normalized `SourceFrame`s. They DO NOT decide significance. | `crates/probes` |
| Sifters | Decide significance. Rules are typed (keyword, regex, condition, multiline, dedupe, suppression, stall, progress, prompt, correlation, artifact). | `crates/sifters` |
| Buckets | Cursor-addressable streams of `SignalEvent`. Per-bucket retention by count and TTL. Drop-oldest with `dropped_count`. | `crates/core::bucket` |
| Context rings | Per-probe ring buffers of `SourceFrame`. Anchored windows by `FrameId`. | `crates/core::context` |
| Event store | Durable SQLite, manual migration runner, FTS5 search lane. | `crates/store` |
| Registry | Versioned, tagged rule library. Search, get, create, test, activate, bind. | `crates/store::registry` |
| Policy engine | Evaluates allow / deny / allow-with-audit for every gated action. Four locked profiles. | `crates/daemon::policy` |
| Audit log | Durable record of policy-relevant runtime actions. Persistent SQLite-backed since TC35. | `crates/daemon::audit::PersistentAudit` + `crates/store::audit` |
| Daemon runtime bootstrap | Loads config, opens store, applies V0003, wires Router with PersistentAudit, idles in foreground. | `crates/daemon::state::DaemonState`, `crates/daemon::runtime` |
| Daemon UDS IPC | Unix-domain socket transport with SO_PEERCRED / getpeereid peer identity, length-prefixed JSON frames, bounded `MAX_FRAME_BYTES`, closed-set error codes, audit on every accepted request. Method set today: `system_discover` / `health` / `policy_status` / `self_check`. | `crates/daemon::ipc::{server,client,protocol,peer}` |
| Command runtime | argv-only `command_start_combed`. Policy gate -> ProcessProbe -> DaemonEventSink -> Router::bucket_append (PersistentAudit). Bounded response (job_id / bucket_id / probe_id / cursor). Lifecycle events written to bucket. No raw stdout/stderr returned. | `crates/daemon::command::CommandRuntime` |
| Daemon signal-retrieval API | `bucket_events_since` (cursor read), `bucket_wait` (Notify-backed, heartbeat-on-timeout), `bucket_summary`, `event_context` (bounded window resolved by `(bucket_id, event_id)`). All bounded, all audited through PersistentAudit. | `crates/daemon::ipc::server` + `protocol` |

## 5. Locked invariants

These hold across every later runtime goal. A change to any of these
requires an amendment to this document and to
`docs/research/_USER_DECISIONS.md`, not a silent rewrite.

1. **No unrestricted root shell.** The MCP-facing crate MUST NOT
   contain `Command::spawn` and MUST NOT open a network listener.
   TC29 structural tests enforce this.
2. **No setuid, no polkit, no installed system service** unless a
   later explicit goal authorizes it.
3. **No network listener** on the MCP-facing crate. Local-only
   transport: stdio for the MCP adapter, UDS / named pipe for the
   daemon, per platform.
4. **Bounded outputs everywhere.** `MAX_FILE_WINDOW_BYTES = 64 KiB`.
   `MAX_READ_LIMIT = 10000` events per call. Context windows are
   bounded by `before`, `after`, and `max_bytes` (hard cap
   `MAX_WINDOW_BYTES`).
5. **No raw output by default.** Bucket reads return `Vec<SignalEvent>`,
   not `Vec<String>`. Context windows return frames bounded by the
   ring cap. File reads return capped UTF-8-lossy slices.
6. **Pointer-or-reason for severity >= Medium.** Every `SignalEvent`
   at Medium or higher carries a `SourcePointer` OR a
   `pointer_unavailable_reason`. The TC02 negative-contract fixture
   `forbidden/missing-pointer.v1.json` and `SignalEvent::validate`
   enforce this.
7. **Closed-set enums** for `Severity`, `PolicyDecision`,
   `PolicyProfile`, `PolicyAction`, `RuleType`, `RuleStatus`,
   `SourceStream`, `SourceType`. Open-set `event_kind` is the
   intentional escape valve for new event types.
8. **Source-status honesty.** Scaffold-only, deferred, degraded, or
   test-only behavior MUST be labeled as such. A live status claim
   without a code path that exercises the live behavior under a real
   transport is a defect.
9. **Default-deny sensitive paths.** 14 path suffixes (private keys,
   credential stores, sudoers, token caches) deny by default in every
   profile.
10. **Sudo / doas / su / pkexec / kexec / polkit-agent /
    polkit-auth-agent-1 deny in every profile.**
11. **Hot rebind of rules.** Registry activation MUST take effect on
    running probes. A rule activation that only persists to the
    database and does not affect a live `SifterRuntime` is a defect.
    Implementation lands in TC42.

## 6. Non-goals (re-anchored from SPEC §3)

The product is not, and should not become, any of these without an
explicit user decision:

- A hosted SaaS or cross-host service. Local single-machine only.
- A replacement for the LLM harness or MCP client.
- A general log shipper or metrics pipeline.
- A kernel-sandbox enforcer at MVP. Landlock / seccomp-bpf is post-MVP
  hardening per `BACKLOG.md` P2.
- A macOS / Windows-native binary at MVP. Linux native + WSL2 first;
  macOS / Windows deferred per `_USER_DECISIONS.md`.
- A TUI or GUI. Operator surface is the admin CLI; LLM surface is MCP.
- A privileged default install. Privilege paths only behind explicit
  later goals.
- An encryption-at-rest, secret-vaulting, or credential-manager
  product. Deferred per `_R1-beta-summary.md`.

## 7. Bounded-context contract

Raw frames remain available **only** through:

- `event_context(probe_id, anchor, before, after, max_bytes)` returns
  a bounded window. `anchor_missing = true` when the anchor frame is
  no longer in the ring.
- `file_read_window(path, offset, max_bytes)` returns capped bytes
  with `truncated`, `next_offset`, and `total_size` set.
- Future `file_search` (TC43) returns matches with bounded snippets.
- Future `file_watch` (TC43) returns structured change events, never
  raw deltas.

There is no `stream_tail`, no `command_read_stdout`, no
`file_read_all`. A tool that streams raw text is a contract violation.

## 8. Heartbeat contract for `bucket_wait`

When `bucket_wait` times out without matching events, it MUST return
a heartbeat (`heartbeat = true`, empty `events`, `next_cursor` ==
input cursor). It MUST NEVER return raw stream data as the timeout
fallback. The forbidden negative-contract fixture
`tests/fixtures/contracts/forbidden/raw-stream-as-events.v1.json` is
the structural test oracle.

## 9. Registry semantics

The registry holds versioned, tagged rules. Each version is immutable.
Activation produces an `activation` row with the (rule_id, version,
profile, source) tuple. Search uses FTS over rule content and tags.

Operations:

- `registry_search(query, limit)` — bounded result set with default
  and max limits.
- `registry_get(rule_id)` — latest version.
- `registry_create(rule_definition)` — appends version N+1.
- `registry_test(rule_definition, sample_input)` — dry-run against
  sample input, returns matches and captures, NEVER persists. TC42
  is the implementation goal.
- `registry_activate(rule_id, version, profile)` — records activation
  AND rebinds running probes. TC42 enforces the rebind requirement.

A registry update that does not affect live probes is a defect.

## 10. Probe semantics

Probes are sources of `SourceFrame`s. They do not classify, they
normalize.

| Probe | Source | Status on `main` | Goal that lands real behavior |
|---|---|---|---|
| `process_probe` | Non-interactive child process via `tokio::process::Command`. | live | n/a (live since TC15) |
| `terminal_probe` / PTY | Interactive PTY-attached process. | normalizer-only on `main` (`AnsiNormalizer` + `PromptDetector` live; spawn deferred) | TC44 |
| `file_probe` | A single file, follow / scan / create-after-start / rotation. | live (polling backend; native notify deferred) | n/a; notify swap is BACKLOG P1 |
| `directory_probe` | A directory, create / modify / delete events; JUnit-XML summary. | live (polling backend) | n/a; native inotify is BACKLOG P1 |
| `journal_probe` | systemd journal. | not implemented | post-MVP |
| `artifact_probe` | Arbitrary structured reports beyond JUnit-XML. | partial (JUnit only) | post-MVP |

## 11. Policy contract

Every gated action passes through `PolicyEngine::evaluate`. The four
profiles are closed:

| Profile | Headline behavior |
|---|---|
| `developer_local` (default) | Common operations allow; mutating registry actions require audit; sensitive paths deny. |
| `repo_only` | Same as developer_local with stricter path scoping. |
| `read_only_observer` | All mutations deny; reads allow. |
| `admin_debug` | Broader allow surface; mutating registry actions still deny by default. |

Policy denials surface as `McpError::PolicyDenied(reason)` to the LLM.
Audit emission is REQUIRED for every `AllowWithAudit` decision. The
audit log is durable (TC35) once that goal lands; on `main` today it
is an in-memory placeholder, which is a known scaffold seam.

## 12. Recorded drifts (TC33 evidence, fixed elsewhere — NOT in TC34)

These drifts are real on `main` and are explicitly NOT fixed by TC34:

- **`crates/store/src/lib.rs` doc claims audit log "rides on the same
  database file."** No `audit` table or writer exists on `main`. The
  comment overstates state. Fix lands in TC35 along with the V0003
  migration. TC34 only records this.
- **`Cargo.toml workspace.package.rust-version = "1.92"`** vs the
  locally selected `1.95.0` recorded in
  `docs/research/_USER_DECISIONS.md`. `rust-toolchain.toml` is the
  effective toolchain. The version-string sync is a future doc-only
  follow-up; TC34 does not edit `Cargo.toml` (forbidden by mini-spec).

These drifts do NOT invalidate the contract in this document; they
are scaffold / metadata mismatches that the runtime chain corrects
as it lands real behavior.

## 13. Unresolved contract questions (to be answered by goal-file
amendments, not by this document)

These questions are surfaced by the TC33 audit and the
`docs/audits/runtime-tool-surface-gap.md` checklist. Each must be
answered in the goal file that owns the related implementation.

1. **`event_context` anchor key.** Does the tool key on `event_id`
   (per README example payload) or on `(probe_id, frame_id)` (per
   current `Router::event_context` signature)? Decide before TC39
   ships the daemon API.
2. **`command_start_combed` return shape.** Bucket id, job id, both,
   or a `command_started` event in a bucket? Decide before TC38
   wires the tool to the daemon runtime.
3. **`bucket_create` exposure.** Explicit tool or implicit on
   `command_start_combed` / `probe_create`? Decide before TC41.
4. **`policy_status` shape.** Standalone tool or section of
   `system_discover`? Decide before TC41.
5. **`command_write_stdin` and `command_send_signal` profile gate.**
   `developer_local` and `admin_debug`, or `admin_debug` only? Decide
   before TC44 ships interactive PTY stdin control.

Each answer carries an `as_of YYYY-MM-DD` line in the owning goal
file when it lands.

## 14. Source-status of this document

| Section | Source status |
|---|---|
| 1-3 | live contract; binding on TC35-TC48 |
| 4 | live (each row reflects code paths verified by TC33 audit) |
| 5 | live (each invariant cross-references existing tests or code) |
| 6-11 | live contract; binding on TC35-TC48 |
| 12 | informative recording of TC33 audit drift; fix is in TC35 (audit log) and a future doc-only follow-up (Cargo.toml rust-version) |
| 13 | open questions; bound to the owning goal file noted in each row |

## 15. References

- `docs/audits/runtime-gap-audit.md` — surface-by-surface classification.
- `docs/audits/runtime-source-map.md` — concept-to-file map.
- `docs/audits/runtime-tool-surface-gap.md` — intended vs implemented MCP tool map.
- `SPEC.md` — original product specification.
- `ARCHITECTURE.md` — process / module layout.
- `README.md` — product overview, planned tool list, safety model.
- `BACKLOG.md` — P0/P1/P2/P3 deferrals.
- `docs/security/PRIVILEGE_MODEL.md` — privilege boundaries.
- `docs/security/SECURITY.md` — security envelope.
- `docs/contracts/README.md` — wire-shape fixtures.
