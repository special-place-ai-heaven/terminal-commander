# TC Amendment Plan (TC01 propagation)

Synthesized from 3 reader-agent runs over all 32 TC files.
Maps research findings -> required edits per TC.

Date: 2026-05-21
Scope: amend TC02-TC32 mini-specs with research-discovered constraints so the chain is internally consistent before any code goal runs.
Authority: user-authorized scope expansion of TC01 (mini-spec only allows TC01 file edits; user explicitly approved propagating findings into the chain).

## Systemic amendments (apply to every TC that touches the relevant area)

### S1. Crate version pins
Every TC whose `allowed_files_or_area` includes `crates/<name>/**` MUST cite the locked version in `contracts_or_interfaces`:
- `rmcp = "=1.7.0"` (TC23, TC24, TC27)
- `rusqlite = "0.39"` with `bundled` feature (TC12, TC13, TC14)
- `refinery = "0.9"` (TC12, TC13)
- `notify = "8.2"` + `notify-debouncer-full = "0.7"` (TC18, TC20, TC25)
- `pty-process = "0.5.3"` with `async` feature (TC19)
- `process-wrap = "9.1"` (TC15, TC16)
- `cap-std` (TC22)
- `tokio = "1"` (TC07, TC17, TC21)

### S2. SPDX license
Every TC that creates `Cargo.toml` must include `license = "Apache-2.0"` (or `license.workspace = true` after TC04 lands). Affects: TC04, TC06, TC07, TC09, TC10, TC12, TC13, TC15, TC18, TC19, TC21, TC22, TC23, TC24, TC25.

### S3. Verification command - 7-step CI
Replace bare `cargo test -p X` with the cargo-nextest path + clippy gate:
```
cargo fmt --check
cargo clippy -p <crate> --all-targets -- -D warnings
cargo nextest run -p <crate>
```
Full 7-step CI (`fmt -> clippy -> deny -> hack --each-feature -> hack --rust-version -> nextest+doc -> machete`) lives in CONTRIBUTING.md / TC03; per-TC verification cites a per-crate subset.

### S4. Crate-count reconciliation
TC04 has 7 crates; README has 6 (missing `terminal-commander-store`). README cannot be edited from TC01 (mini-spec forbids). Resolution paths:
- (a) Spawn a new mini-goal `TC01a-readme-license-reconcile` that updates README's crate list AND adds the LICENSE file. Both are blocking TC04.
- (b) Update TC04 to drop store crate (NOT recommended; TC12/TC13 depend on it).
- (c) Document the gap in SPEC.md as authoritative (already done by architect); later goal patches README.

Recommended: (a) — create a small license-and-readme goal.

### S5. WSL `/mnt/c` 9P silent-fail (microsoft/WSL#4739)
Any TC touching file/dir watching MUST force `PollWatcher` on 9P mounts and detect via `/proc/self/mountinfo`. Affects: TC18 (file probe), TC20 (dir probe), TC24 (MCP file_watch tool), TC25 (doctor), TC30 (E2E demos). TC12/TC13 must REJECT placing DB on `/mnt/c`.

### S6. Advisory-policy framing
TC22, TC24, TC29, TC30 must explicitly state policy enforcement is ADVISORY in MVP (in-process + audit log). Landlock + seccomp-bpf documented as post-MVP. Avoid "enforces" without qualifier.

## Per-TC amendments

### TC02 - Security Privilege And Policy Doctrine
- Add: "Policy enforcement is ADVISORY in MVP; Landlock (kernel 5.13+, WSL2 5.15.57.1+) and seccomp-bpf are roadmap" per `_R1-beta-summary.md`
- Add to RISK_REGISTER mitigations: cap-std for FS handles
- Cite default-deny list from README:294-297

### TC03 - Test Methodology
- TESTING.md must enumerate 7-step CI sequence from `_R2-gamma-summary.md`
- Add fixture category `tests/fixtures/probes/wsl-mountinfo/` with sample 9P vs native mountinfo
- Forbid >256-line / >16KB raw terminal capture fixtures
- Bound platform: WSL2 with no systemd, no network, no Windows-specific tooling
- GAP: ANSI test corpus crate (vte / strip-ansi-escapes); snapshot framework (insta vs hand-rolled)

### TC04 - Rust Workspace Scaffold
- Pin: edition 2024, resolver "3", rust-toolchain 1.92
- Workspace fields: workspace.package, workspace.dependencies, workspace.lints (1.74+)
- Add to allowed_files: `LICENSE` and `NOTICE` (per S4 needs decision)
- Cargo.toml SPDX: `license = "Apache-2.0"`
- 7 crates locked; reconcile README gap (see S4)
- cargo-deny allowlist must include CC0-1.0 (notify is CC0)

### TC05 - Contract Schemas And Golden Fixtures
- Reserve all 11 sifter type discriminators (README:136-148)
- Reserve all 20 MCP tool names as fixture stubs (README:243-266)
- Add fixture: `event-context-request.v1.json` / `event-context-response.v1.json` (README:204-209)
- bucket_wait response fixture: heartbeat shape distinct from raw stdout
- Verify via `python3 -m json.tool` requires python3 in TC03 prereqs (or switch to `jq`)
- GAP: schema-validation crate (jsonschema/schemars/typify); fixture vN bump semantics

### TC06 - Core Identifiers Events Source Pointers
- SPDX header (S2)
- Severity: snake_case lowercase string serde
- u64 seq (matches SQLite i64 for TC12)
- GAP: ID format (UUIDv7 vs ULID vs u64); timestamp crate (time vs chrono); capture-map type
- Verification: add clippy

### TC07 - In-Memory Bucket Manager
- u64 seq pin, mirror TC12 SQLite INTEGER
- Reserve BucketSummary fields: noise_suppressed_count, dedupe_collapsed_count
- Concurrency: parking_lot::RwLock or tokio::sync::RwLock; Send+Sync
- GAP: memory cap/eviction before TC12 persists; backpressure
- Verification: add clippy

### TC08 - Context Ring
- ContextRing default 4096 frames or 1MB (whichever smaller); FIFO eviction; truncation marker
- Per-frame text cap 8192 bytes; `truncated_bytes: u32`
- Frames are post-ANSI-strip; raw escapes belong to TC15
- GAP: ANSI-strip crate pick; per-source vs per-session sharing; spool persistence roadmap

### TC09 - Rule Model Validation
- Pin: `regex` crate (linear-time, DoS-resistant); forbid fancy-regex/backreferences
- RuleType enum reserves all 11 sifter discriminators
- Template syntax: `${name}` captures; `Result<String, MissingCapture>` never panics
- Pattern length cap 4096 bytes
- SPDX header
- GAP: RuleStatus enum variants; tag taxonomy; ContextHint semantics

### TC10 - Keyword And Regex Sifter Runtime
- Pin: `regex` crate, version compatible with MSRV 1.92
- SPDX header
- Emit `EventDraft` (TC06); final IDs assigned by BucketManager
- Fixture tests use TC03 fixture taxonomy + README rule pack names
- GAP: aho-corasick for multi-keyword; RegexSet batching; per-frame byte limit

### TC11 - Noise Suppression Dedupe
- Progress detection operates on post-ANSI-strip frames; CR-collapse in TC15
- Dedupe window: in-memory only; default 5s, configurable per rule
- TC12 persistent schema must allow `count`, `first_seen`, `last_seen` columns
- Fixtures exercise: progress (cargo spinner, apt %) + repeated-warning (gcc dup)
- GAP: dedupe key hash; suppression metadata location

### TC12 - Persistent Event Store
- Replace "choose backend" with locked stack: rusqlite 0.39 bundled + refinery 0.9 + FTS5 + WAL
- Single-writer invariant: daemon-only writes; reads via read-only conns; busy_timeout=5000ms; synchronous=NORMAL
- REJECT DB on `/mnt/c` (9P unreliable for WAL); detect via /proc/self/mountinfo
- Schema test: no BLOB columns (no raw_bytes); only summary TEXT + captures/pointer JSON
- SPDX header
- GAP: FTS5 column schema; retention/compaction; backup/snapshot

### TC13 - Registry Store And Rule CRUD
- Inherit rusqlite + refinery + FTS5 from TC12
- registry_search uses FTS5 virtual table; LIMIT default 50, max 500
- Reject DB on `/mnt/c`
- Advisory-policy framing for activations
- Minimum tables: rules, rule_versions, rule_tags, rule_activations
- GAP: version content-addressed vs monotonic; latest-pointer; row-level JSON-Schema

### TC14 - Seed Rule Packs
- 6 README user-locked rule packs OR explicitly mark make.json as architect-added 7th (parallel to crate gap)
- Rules declare sifter kind from 11-canonical set
- Index imported rules into FTS5 (rule_id, tags, summary)
- GAP: full RuleDefinition schema fields; regex safety validation rules

### TC15 - Process Probe Streaming
- Pin: `process-wrap` 9.1 (tokio1) + process groups + SIGTERM-then-SIGKILL
- Non-goal: Windows-native process spawn (POSIX MVP only)
- Replace vague "policy assumptions" with: advisory placeholder, real policy in TC22

### TC16 - Job Manager And Exit Events
- Cancellation: SIGTERM to process GROUP, configurable grace window, then SIGKILL
- Pin scope: JobManager lives in `terminal-commander-core` (not `terminal-commanderd`; TC21 wires it)
- Decision: in-memory JobManager; persistent recovery deferred
- GAP: grace-window default (5s vs 10s)

### TC17 - Realtime Bucket Waiter (KEYSTONE)
- Bump risk_level "medium" -> "high" (this is THE keystone primitive)
- Tighten contract: on timeout return `heartbeat=true`, `events=[]`, `next_cursor=<current bucket cursor>` (not request cursor)
- Pin: tokio `broadcast` or `Notify`; forbid sleep-poll loops

### TC18 - File Probe (CRITICAL)
- ACCEPTANCE CRITERION: detect 9P via /proc/self/mountinfo at probe construction, force PollWatcher; test fixture demonstrates 9P -> polling dispatch
- Pin: `notify 8.2` + `notify-debouncer-full 0.7`; parent-dir watch for create-after-start + rotation
- Replace "where practical for Linux/WSL" with explicit rotation strategy: parent-dir watch on native FS; polling stat on 9P
- macOS/Windows-native deferred (FSEvents 32-subpath limit + ReadDirectoryChangesW caveats noted)

### TC19 - PTY Probe And Prompt Detection
- Pin: `pty-process 0.5.3` with `async` feature (POSIX-only Linux+WSL2); `portable-pty` deferred Windows-native
- PTY processes share TC15/TC16 process-group + SIGTERM/SIGKILL cleanup
- Prompt detection uses sifter rules from canonical 11-set
- Secret redaction: password prompt events MUST NOT include user response or echoed payload; only prompt + redaction marker
- GAP: ANSI normalization crate (vte / strip-ansi-escapes)

### TC20 - Directory And Artifact Probes
- Reuse TC18 watcher abstraction; 9P forces PollWatcher
- Move-event best-effort on 9P (may surface as delete+create)
- Event shapes conform to canonical schema (TC05/TC06)
- GAP: JUnit XML parser crate; coverage JSON format target

### TC21 - Daemon Local API And Router (IPC OWNER)
- CRITICAL: add `implementation_step` for IPC transport decision + implementation. Recommended: `interprocess` v2 UDS on Linux/WSL2. Document in `docs/api/IPC.md`.
- Lifecycle: foreground supervisor + PID file; sd-notify optional, feature-gated, NOT default on WSL
- Policy-decision seam: all side-effecting ops route through policy + emit audit record (placeholder fields acceptable; TC22 fills real)
- API shapes map cleanly to rmcp 1.7.0 tool-call schemas (TC23 wraps)
- GAP: IPC final wire-format decision (interprocess v2 vs JSON-RPC over UDS vs tonic); IPC auth (peer-cred on UDS?)

### TC22 - Policy Engine And Audit Log
- ADVISORY framing explicit: enforced in-daemon via cap-std Dir handles + audit log; NOT kernel-enforced; Landlock + seccomp-bpf roadmap (WSL2 Landlock available since 5.15.57.1)
- Pin: `cap-std` Dir handles for FS access
- Default-deny enumeration: SSH keys, /etc/shadow, .pgpass, ~/.aws/credentials, ~/.config/gcloud, ~/.kube/config, token caches (README:294-297)
- Audit log persistence: rusqlite WAL + refinery; schema: timestamp, session_id, client_id, operation, args_hash, decision, related_*_id
- Output limits live in probe/event-store enforcement; policy provides configured limits + audit on hit
- GAP: policy config format (TOML/JSON/YAML); seccompiler stub-now-or-defer; audit retention

### TC23 - MCP Server Discovery Jobs Buckets
- Pin: `rmcp =1.7.0` against MCP spec rev 2025-11-25
- bucket_wait MCP tool wraps TC17 contract verbatim
- Advisory denial wording in tool docs

### TC24 - MCP Registry Probe File Tools
- Pin: rmcp =1.7.0 + MCP spec rev 2025-11-25
- file_watch inherits 9P detection from daemon; response includes transport-mode field
- Denial responses labeled advisory (daemon-side); no kernel-level claim
- Verification: add `cargo clippy -p terminal-commander-mcp -- -D warnings` + `cargo nextest run -p terminal-commander-mcp`

### TC25 - Admin CLI And Doctor
- Doctor MUST inspect /proc/self/mountinfo on Linux/WSL and warn on probe paths resolving to 9P with active watcher transport
- Doctor probes daemon via TC21 IPC transport; reports transport mode
- Verification: add clippy

### TC26 - Installer Service And WSL Startup
- Install docs document WSL systemd opt-in: WSL 0.67.6+ + `/etc/wsl.conf [boot] systemd=true`
- Default: foreground daemon + PID file; optional systemd USER unit (not system); NEVER assume systemd on WSL

### TC27 - Provider Harness Integration
- Cite: MCP spec rev 2025-11-25 + rmcp =1.7.0
- Tighten: stdio MCP only for examples; local-socket transport is daemon-internal
- Each provider example records provider version observed + drift risk
- GAP: Claude Code MCP config path + JSON shape; Codex CLI MCP attachment syntax

### TC28 - Load Performance And Backpressure
- Acknowledge: rusqlite WAL throughput baseline NOT MEASURED; tests record numbers, don't gate
- Tests record watcher transport (inotify vs PollWatcher) + target FS type for comparable results
- Switch verification to `cargo nextest run --workspace -E 'test(load)' --no-fail-fast`
- Include registry FTS5 search latency under load

### TC29 - Security Hardening And Fuzz-Like
- Advisory framing in tests/docs
- regex crate is linear-time (not catastrophic-backtracking); failure mode is regex_size_limit / dfa_size_limit; test hostile patterns hit these caps
- Any new dep must pass cargo-deny with Category-A + CC0-1.0 allowlist
- docs/security/HARDENING.md cites Landlock on WSL2 (5.15.57.1+) as post-MVP enforcement path
- GAP: concrete regex DoS test vectors; seccompiler usage patterns

### TC30 - End-To-End MVP Demo
- Demos MUST exercise only the canonical 20 MCP tools (README:243-266); missing tools labeled blocked, not invented
- Probe/sifter types in demos match 6/11 canonical lists
- file-watch demo defaults to native Linux path (~/...) not /mnt/c; if /mnt/c unavoidable, document PollWatcher latency
- Replace `cargo test ... || bash demo` (swallows failures) with sequential `cargo nextest run -E 'test(e2e)'` then `bash scripts/demo/run-all.sh`

### TC31 - Beta Packaging And Release (CRITICAL)
- ADD to allowed_files_or_area: `LICENSE`, `NOTICE`, `crates/*/src/**` (header sweep)
- Implementation step: verify LICENSE at repo root contains full Apache-2.0; NOTICE if needed; `cargo deny check licenses` passes (Category-A + CC0-1.0)
- Verification: add `cargo deny check licenses bans advisories sources`
- RELEASE.md states: rmcp =1.7.0, MSRV 1.92, Edition 2024, MCP spec rev 2025-11-25, Linux+WSL2 targets
- NOTICE/RELEASE notes rmcp Apache-2.0 relicensing transition (MIT OR Apache-2.0 -> Apache-2.0); cargo-deny allowlist covers both
- Install verification documents WSL systemd opt-in prerequisite

### TC32 - Evidence Review And Backlog
- BACKLOG.md inherits deferred items from R1-alpha/R1-beta/_USER_DECISIONS: IPC transport (resolved at TC21 or carried), kernel-level enforcement (Landlock/seccomp), macOS FSEvents probe, encryption-at-rest, federated audit, daemonize choice, portable-pty Windows bridge
- MVP_EVIDENCE_REVIEW.md lists each of 20 MCP tools / 6 probes / 11 sifters with live/partial/blocked/deferred verdict
- Records as-shipped rmcp version pin, MSRV, MCP spec rev, license
- Records most recent green 7-step CI run

## New mini-goal needed

### TC01a - README license and crate reconcile
Tiny gap goal to allow editing README + adding LICENSE file. Both currently outside any TC's allowed_files. Blocking TC04 (workspace scaffold).

Allowed files:
- `README.md` (update crate list 6 -> 7; update license section)
- `LICENSE` (Apache-2.0 full text)
- `NOTICE` (optional; rmcp transition note)
- `.agent/goals/terminal-commander-mvp/TC01a-readme-license-reconcile.md` (the goal file itself)

Verification: README mentions all 7 crate names + Apache-2.0; LICENSE present + Apache-2.0 SHA256 matches official.

## Remaining research gaps (need decisions before specific TCs run)

| TC | Gap | Recommended action |
|---|---|---|
| TC21 | IPC final wire format | Research/decision before TC21 |
| TC22 | Policy config format (TOML/JSON/YAML) | Decision before TC22 |
| TC22 | seccompiler stub-now-or-defer | Architect call |
| TC29 | regex DoS test vectors | Research before TC29 |
| TC29 | seccompiler integration patterns | Research before TC29 |
| TC27 | Claude Code MCP config syntax | Research before TC27 |
| TC27 | Codex CLI MCP attachment | Research before TC27 |
| TC08/11/15/19 | ANSI strip crate (vte vs strip-ansi-escapes) | Decision before TC08 (consumed downstream) |
| TC06 | ID format (UUIDv7 vs ULID vs u64) | Decision before TC06 |
| TC06 | Timestamp crate (time vs chrono) | Decision before TC06 |
| TC03 | snapshot framework (insta) | Decision before TC03 |
| TC03 | JSON-Schema validator (jsonschema/schemars) | Decision before TC05 |

These are NOT TC01 blockers. They are amendments-with-flags carried as `<<DECISION REQUIRED: ...>>` markers in the amended TC mini-spec. Resolved at the TC's own run start.
