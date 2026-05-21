# Terminal Commander - Roadmap

Status: Baseline (TC01 wave 0 deliverable).
Source: `.agent/goals/terminal-commander-mvp/GOAL_CHAIN_INDEX.md` (32 goals).
Language: ASCII only.

This document groups the 32 goals into nine waves and names which goals
are MVP-required versus nice-to-have. Wave boundaries are derived from
the dependency graph in the chain index. The wave grouping does not
reorder goals; it organizes them for planning and reporting.

## Wave 0 - Research and discipline (TC01 - TC03, with TC01a reconcile)

Land the verifiable product baseline, security doctrine, and test
methodology before any code lands. No Rust source ships in this wave.

| Goal | Outcome |
|---|---|
| TC01 | Research product baseline and source map. SPEC.md, ARCHITECTURE.md, ROADMAP.md, CONTRIBUTING.md, source-map and assumptions registers. |
| TC01a | Reconcile README, LICENSE, NOTICE, and CONTRIBUTING with TC01 locked decisions (Apache-2.0, 7-crate list). Blocks TC04. |
| TC02 | Security, privilege, and policy doctrine. Locks the policy profiles, the audit-log mandate, the default-deny list, and the privilege boundaries. |
| TC03 | Test methodology and fixture plan. Locks the verification commands, fixture taxonomy, evidence rules, and the no-mock invariant in test form. |

## Wave 1 - Repository foundation (TC04 - TC05)

Stand up the workspace, toolchain, and contract schemas. Still no
product behavior.

| Goal | Outcome |
|---|---|
| TC04 | Rust workspace and toolchain scaffold. Seven crates under `crates/<short>/`, `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `.config/nextest.toml`, baseline CI workflow. |
| TC05 | Contract schemas and golden fixtures. Versioned JSON examples for events, buckets, rules, probes, jobs, source pointers, context windows, policy decisions. |

## Wave 2 - Core types (TC06)

| Goal | Outcome |
|---|---|
| TC06 | Core identifiers, events, and source pointers. Typed ULID-style IDs, severity enum, event model, source descriptor, source pointer; round-trip serialization tests against TC05 fixtures. |

## Wave 3 - In-memory primitives (TC07 - TC08)

Behavior lands but only in memory. Persistence still deferred.

| Goal | Outcome |
|---|---|
| TC07 | In-memory bucket manager. Monotonic cursors, severity filtering, bounded reads, summaries. |
| TC08 | Context ring and bounded context windows. Per-probe bounded ring buffer; `before`/`after` retrieval by source pointer. |

## Wave 4 - Rule and sifter runtime (TC09 - TC11)

| Goal | Outcome |
|---|---|
| TC09 | Rule model, validation, and templates. Rule definition type, validation rules, capture mapping, summary-template rendering, rule test types. |
| TC10 | Keyword and regex sifter runtime. First runtime evaluating keyword and regex rules against normalized frames; emits drafts. |
| TC11 | Noise suppression, dedupe, progress basics. Dedupe, suppression, repeated-event collapse, progress-noise classification. |

## Wave 5 - Persistence and registry (TC12 - TC14)

| Goal | Outcome |
|---|---|
| TC12 | Persistent event store and bucket cursors. rusqlite 0.39 + refinery 0.9 + WAL; cursor queries preserving bounded reads. |
| TC13 | Registry store and rule CRUD. Persistent rule registry CRUD, versioning, search (FTS5), test metadata, activation records. |
| TC14 | Seed rule packs and registry import. Initial bundles for `generic.terminal`, `apt`, `cargo`, `npm`, `pytest`, `gcc` per `README.md:367-372`. |

## Wave 6 - Probes, jobs, waiter (TC15 - TC20)

This wave does the source-to-event integration end-to-end, but the
daemon and MCP server are still not assembled.

| Goal | Outcome |
|---|---|
| TC15 | Process probe streaming stdout/stderr. Non-interactive process probe; normalized frames; feeds sifter runtime. |
| TC16 | Job manager and command exit events. Process lifecycle, cancellation, exit codes, non-zero exit events, runtime metadata. |
| TC17 | Realtime bucket waiter. `bucket_wait` semantics: block for matching events by cursor + severity + kind + timeout. Heartbeat, no raw output dump. |
| TC18 | File probe (follow, create, rotate). File scan/follow with create-after-watch, truncation, rotation; emits events and context. **MUST include the WSL2 9P forced-polling acceptance criterion per `docs/research/wsl-boundary.md`.** |
| TC19 | Terminal PTY probe and prompt detection. Adds PTY support with ANSI/CR normalization, prompt detection, bounded stdin writing. |
| TC20 | Directory and artifact probes. Directory watching + initial artifact detectors (seed). |

## Wave 7 - Daemon, policy, MCP (TC21 - TC24)

Assemble the two-process product surface.

| Goal | Outcome |
|---|---|
| TC21 | Daemon local API and router. Daemon binary, local API surface, IPC transport choice locked (per `docs/research/mcp-transport-pattern.md`). |
| TC22 | Policy engine and audit log. First real policy engine (advisory) + audit log per TC02 doctrine. |
| TC23 | MCP server: discovery, jobs, buckets. rmcp 1.7.0 stdio server exposing `system_discover`, `policy_status`, `command_start_combed`, `command_status`, `command_write_stdin`, `command_send_signal`, `bucket_create`, `bucket_events_since`, `bucket_wait`, `event_context`. |
| TC24 | MCP server: registry, probe, file tools. Adds `probe_create`, `probe_bind_rules`, `registry_*`, `file_read_window`, `file_search`, `file_watch`. |

## Wave 8 - Operator tooling (TC25 - TC27)

| Goal | Outcome |
|---|---|
| TC25 | Admin CLI and doctor commands. `status`, `doctor`, `rules`, `buckets`, `jobs`, `probes`, `policy`, `audit` subcommands; does not bypass daemon policy. |
| TC26 | Installer, service, and WSL startup docs. Safe installer + service + config + WSL startup; no unintended privileged behavior. Per-user vs per-machine daemon decision locked here. |
| TC27 | Provider-harness integration examples. MCP integration notes for Claude Code, Codex CLI, generic MCP clients. No hardcoded secrets or machine-specific paths. |

## Wave 9 - Validation and release (TC28 - TC32)

| Goal | Outcome |
|---|---|
| TC28 | Load, performance, backpressure tests. Deterministic load/scale/backpressure proofs. |
| TC29 | Security hardening and fuzz-like tests. Policy enforcement, regex safety, path denial, bounded outputs, prompt secrecy, malformed input handling. |
| TC30 | End-to-end MVP demo scenarios. Verified workflows for command execution, realtime bucket waiting, dynamic registry creation, file watching, bounded context retrieval. |
| TC31 | Beta packaging and release checklist. Packaging metadata, release checklist, versioning notes, install verification. No automatic publishing. |
| TC32 | Evidence review and backlog refinement. Consolidate evidence, identify gaps, produce next backlog. |

## MVP completion definition

The MVP is "complete" when TC30 passes its acceptance criteria. The
MVP-required set is:

TC01, TC02, TC03, TC04, TC05, TC06, TC07, TC08, TC09, TC10, TC11,
TC12, TC13, TC14, TC15, TC16, TC17, TC18, TC19, TC20, TC21, TC22,
TC23, TC24, TC25, TC26, TC27, TC28, TC29, TC30.

Nice-to-have but not blocking MVP-declaration:

- TC31 (beta packaging) - required for an external beta but not for
  declaring the MVP code-complete.
- TC32 (evidence review) - retrospective; valuable but not behavior.

The MVP-target list at `README.md:333-343` is satisfied when:

1. An LLM can start a command through MCP (TC23).
2. Continuous stdout/stderr probes attach (TC15 + TC23).
3. Registry-backed sifters activate (TC10, TC13, TC14, TC24).
4. The LLM receives only structured signal events (TC11, TC23).
5. The LLM can wait for new bucket events by cursor (TC17, TC23).
6. The LLM can request bounded context around an event (TC08, TC23).
7. The LLM never needs to read large raw terminal output (enforced
   by the no-unbounded-output invariant, validated in TC29 and TC30).

## Out-of-MVP

Explicitly deferred:

- macOS native port. PTY abstraction will keep this addressable but
  no goal currently scopes it.
- Windows-native port (ConPTY via `portable-pty`). Same status.
- Encryption at rest (sqlcipher feature on rusqlite).
- Kernel-enforced policy via Landlock and seccomp-bpf.
- Cross-host federated audit log.
- Journal probe (`journal_probe` per `README.md:128`).
- Correlation rules and artifact-parser sifters beyond the TC11 /
  TC20 seeds.
- Multi-tenant daemons or per-project daemons. The model is
  per-user, per-host.

Any of the above becoming in-scope requires a new goal in this chain
or a successor chain.
