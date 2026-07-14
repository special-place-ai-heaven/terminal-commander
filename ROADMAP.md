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

## Runtime chain (TC33 - TC48)

Successor chain `terminal-commander-runtime` covering the runtime
P0 items the MVP wave left open. All goals below have landed on
`main`. Per-goal commit hashes and detailed evidence live in
`EVIDENCE_REPORT_RUNTIME.md`.

| Goal  | Wave                                | Outcome |
|-------|-------------------------------------|---------|
| TC33  | runtime-pivot research              | research, no code |
| TC34  | runtime-pivot pivot                 | chain pivot, no code |
| TC35  | persistent audit log V0003          | `PersistentAudit` is the production audit sink |
| TC36  | daemon runtime bootstrap + config   | `DaemonState::bootstrap` wires every subsystem |
| TC37  | UDS IPC + peer identity             | local UDS IPC, PeerCred per connection |
| TC38  | command runtime + shell-bridge guard | argv-only, sudo/doas/su default-deny, shell-interpreter deny |
| TC39  | signal-retrieval API over UDS       | bounded bucket / context APIs, notify-based wait |
| TC40  | rmcp stdio MCP adapter              | `terminal-commander-mcp` forwards through UDS |
| TC41  | MCP command + bucket tool surface   | command_start_combed + bucket_* tools live |
| TC42  | registry hot activation             | rule binding through MCP / UDS |
| TC42b | live rule rebind                    | running-stream rebind without draft loss |
| TC42c | scoped registry rule bindings       | Global / Bucket / Job / Probe scope |
| TC42d | explicit scope on activate / deactivate | `IpcErrorCode::ScopeInvalid`; nextest gate |
| TC43  | file probe search / watch / bounded read | file_read_window, file_search, file_watch_* tools |
| TC44  | POSIX PTY spawn + bounded stdin     | `pty-process = "=0.5.3"`, secret-prompt deny |
| TC45  | aggregate runtime view              | runtime_state / probe_list / probe_status |
| TC46  | provider-harness smoke              | local daemon + MCP stdio smoke + Codex / Claude Code configs |
| TC47  | load / noise / backpressure gate    | 8 stress tests, no product-code changes |
| TC48  | beta gate evidence review           | this report; recommendation: `Conditional Go` |

Out-of-runtime-chain (still deferred):

- Native notify / inotify file-watch backend (TC43 polling remains).
- Windows-native ConPTY (TC44 is Unix-only).
- Daemon-side `frames_suppressed` counter (BACKLOG P1.1).
- Live provider-harness validation against Codex CLI and Claude Code
  on a host where both binaries work (BACKLOG P1.2 / P1.3).

Successor chain after TC48 is operator-driven beta exercise, not a
new code chain.

## Windows + WSL bridge chain (WWS01–WWS09)

Added by WWS08 (docs-only). The WWS chain landed JS-only
Windows control-plane surfaces wrapping the existing Linux/WSL2
runtime. No `crates/**` change. No package version change. No
new MCP tool. No workflow change. The publish floor recommended
by WWS01 §14.1 was WWS02 + WWS04 + WWS05 + WWS06 + WWS08 — all
landed at this commit.

| Goal  | Surface | Commit |
|-------|---------|--------|
| WWS01 | Windows + WSL install UX contract; D-01..D-15 binding decisions | `6220eb2` |
| WWS02 | Root npm package `os: ["linux", "win32"]`; bridge-required resolver branch | `1da40f3` |
| WWS03 | WSL discovery + read-only doctor helpers (`lib/wsl/{distro-name,detect,doctor}.js`) | `ec8441e` |
| WWS04 | Windows → WSL bridge shim (`lib/wsl/spawn.js`) | `d86e73f` |
| WWS05 | Cursor MCP config writer (`lib/cursor/{config,write,index}.js`) | `ae37878` |
| WWS06 | Setup / doctor / pair CLI (`lib/cli/**`) | `4936904` |
| WWS07 | Windows bridge smoke script (`scripts/smoke/verify-windows-bridge-smoke.ps1`) | `785d410` |
| WWS08 | Public README + release contract + checklist + backlog + risk + roadmap updates | (this commit) |
| WWS09 | Pre-publish readiness review | Pending |

Out-of-WWS-chain (deferred):

- First live npm publish (operator-driven; BACKLOG WWS-B1).
- Windows → WSL MCP bridge round-trip live evidence (BACKLOG
  WWS-B2; blocked on WWS-B1 + inside-WSL install).
- Cursor provider GUI live smoke transcript (BACKLOG WWS-B3;
  required for `Conditional Go` → `Go` promotion).
- `setup cursor-wsl --uninstall` rollback (BACKLOG WWS-B4).
- Multi-distro interactive ask-once prompt (BACKLOG WWS-B5).
- Full WSL-side `pair accept` handshake (BACKLOG WWS-B6).
- Safe credential broker for `--install-wsl-runtime`
  permission failures (BACKLOG WWS-B7; explicitly does NOT
  forward LLM-supplied credentials).
- CAP01 capability-registry contract — future doctrine
  carry-forward (BACKLOG WWS-B9). The chain consistently
  describes "tentacles = programmable probes = policy-gated
  capability executors" as future doctrine; the formal
  registry contract is unscheduled.

After WWS09 closes, the next operator-driven milestone is the
first live npm publish via the existing NPM07 trusted-publishing
workflow (no token, no PAT). `npm-bootstrap-publish.yml`
remains the one-time bootstrap fallback per NPM10 and stays
committed-but-undispatched.

## Omni completion chain (TC49-TC74)

Successor program `001-omni-completion`. Goal: take Terminal
Commander from a signal-combing tool (39 MCP tools) to a 100%
self-reliant **omni** terminal tool for LLM agents -- an agent never
needs a separate raw shell. Delivered as six independently-shippable
priority slices (P1-P6) mapped to omni acceptance gates O-01..O-14,
plus folded field-ledger trust/ergonomics fixes. The live surface is
now **49 MCP tools** and **25 rule packs**. Spec and per-gate detail
live in `specs/001-omni-completion/`; the agent lane map is
`docs/mcp/OMNI_PLAYBOOK.md`.

Per-slice outcome and HONEST status:

| Slice | Goals | Outcome | Status |
|-------|-------|---------|--------|
| P1 (US1) | TC49-TC52 | Persistent PTY shell sessions (sticky cwd/env), workspace snapshots (SQLite), `allow_session` cap + `SessionStart` gate + audit. Plus folded field-ledger fixes (ANSI strip default-on with raw kept, CRLF-aware normalizer, compact response mode, `wait_until:"exit"` honest cap + `poll_hint_ms`, canonical capture, SQLite job-receipt restart-marked status). | DONE -- sessions are UNIX-ONLY; non-unix returns `UnsupportedPlatform`. |
| P2 (US2) | TC53-TC55 | `registry_suggest_from_samples` (pure-Rust heuristics; NEVER auto-activates; loop is suggest -> test -> activate), config-gated universal extractors (`sifters.universal_extractors`), rule-pack set grown 8 -> 25, `pack_available` hints. | DONE. |
| P3 (US3) | TC56-TC58 | Platform parity: Windows ConPTY backend (`portable-pty`, dual-backend behind PtyProbe), event-driven file-watch (notify; poll retained for WSL `/mnt/c` 9P), graceful SIGTERM->SIGKILL terminate ladder shared by command/PTY/session stop. | DONE on unix + Windows lifecycle. BLOCKED: live ConPTY child-output e2e gated behind `TC_CONPTY_E2E=1` (env 0xC0000142 DLL-init on the dev host; must run on CI/desktop to close O-07). macOS parity is code + smoke script only, BLOCKED-no-Mac-host. |
| P4 (US4) | TC61-TC65 | Operator-gated privileged helper: separate `terminal-commander-privileged` binary, closed named-op allow-list, human-approval flow, `allow_privileged` cap, audit-before-exec. | PLAN-ONLY by decision. NO code shipped. BLOCKED on a threat review (`docs/security/PRIVILEGE_HELPER_THREAT_REVIEW.md`). `omni_status.privileged_helper` reports `available:false, reason:"threat_review_pending"`. |
| P5 (US5) | TC66-TC69 | Remote federation: `target_list` / `target_probe`, `target_id` routing on the command path, `allow_remote` cap + audit. Transport is an operator-established `ssh -L` forward to the remote daemon's LOCAL socket (NO public TCP; adapter never spawns ssh). | SIM-VERIFIED via a second-local-socket simulation. BLOCKED: real-SSH transit NOT tested (no sshd in the smoke env). `target_id` is wired on the command path, not yet all 51 full-surface tools. |
| P6 (US6) | TC70-TC74 | Certification: `system_discover.omni_status` honest capability matrix; `scripts/smoke/verify-omni-{linux,wsl,windows,macos}` running gates O-01..O-14. | DONE -- runnable gates pass; host-blocked gates (O-06 privileged, O-07 ConPTY, O-09/O-10 remote, O-13 fault-injection) are LOUDLY skipped, not faked. |

Out-of-omni-chain (deferred / blocked):

- P4 privileged helper code -- blocked on threat-review sign-off.
- Live ConPTY child-output e2e on native Windows (`TC_CONPTY_E2E=1`;
  run on CI/desktop to close O-07).
- macOS live runtime verification (no Mac host; closes the P3 macOS
  gate when a Mac smoke run lands).
- Real-SSH remote-federation transit (needs a host with sshd;
  closes O-09/O-10).
- `target_id` threaded through every one of the 51 full-surface tools (currently
  on the command path).

The goal-numbering above (TC61-TC74) follows the slice plans in
`docs/plans/2026-06-09-tc-omni-wave*.md`; the canonical task list is
`specs/001-omni-completion/tasks.md`.
