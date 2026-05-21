# Source Map - terminal-commander-mvp

Status: Updated in TC01 wave 0 to apply R2-delta reclassifications
and to incorporate R1-alpha, R1-beta, R2-gamma, R2-delta evidence.
Language: ASCII only.

This file separates user-provided source material, external
research-backed evidence, and open decisions. Every row cites either
a repository file with line range or a research document under
`docs/research/`. Per the project's no-mock invariant, claims with no
citation are not eligible to be treated as verified.

## 1. User-provided source material

Claims taken directly from the user request, the goal-chain index,
or the user-added README.md.

| Claim | Source |
|---|---|
| Target repository URL: `https://github.com/special-place-administrator/terminal-commander.git` | Goal chain frontmatter; `_R1-alpha-summary.md` |
| Repository is new/empty except the initial README.md added by the user | `GOAL_CHAIN_INDEX.md` "Source Evidence Used" |
| Product name + working terminology (Terminal Commander, Live Signal Comber, probes, sifters, registry, signal buckets, source pointer, context windows) | `README.md:1-12`, `README.md:86-209` |
| LLMs interact with an MCP-operated abstraction layer, not by tailing/grepping/reading large terminal output | `README.md:31-69` |
| Every terminal line or file update is processed continuously by local probes, not by periodic LLM polling | `README.md:31-55` |
| Dynamic registry of regex/keyword/condition rules with search/create/test/edit/activate by id | `README.md:96-99`, `README.md:150-152` |
| Signal buckets must expose timestamped, pointer-backed, vetted events and bounded context windows | `README.md:154-209` |
| System supports terminal streams, command execution, files, directories/artifacts, parallel probes, MCP operation, provider neutrality | `README.md:73-99`, `README.md:120-130` |
| Branch-safe, evidence-driven, numbered `/goal` files runnable linearly | `README.md:299-311` |
| Two-process architecture: `terminal-commander-mcp` + `terminal-commanderd` | `README.md:213-239` (architecture diagram); `README.md:354-360` (crate list); `README.md:284-297` (safety model). Reclassified from inference to user-provided per `_R2-delta-summary.md` finding F2. |
| Planned MCP tool surface (20 names) | `README.md:243-266`. User-provided. |
| Signal event schema sample and required fields | `README.md:171-197`. User-provided. |
| 6 probe types: `process_probe`, `terminal_probe`, `file_probe`, `directory_probe`, `journal_probe`, `artifact_probe` | `README.md:120-130`. User-provided. |
| 11 sifter types: keyword, regex, numeric condition, multiline block, progress detector, prompt detector, stall detector, dedupe, suppression, correlation, artifact parser | `README.md:132-148`. User-provided. |
| `bucket_wait` is the keystone tool; sample request payload; heartbeat (never raw output dump) on no-signal | `README.md:268-281`. User-provided. |
| `event_context(event_id, before, after)` primitive for bounded context | `README.md:204-209`. User-provided. |
| Safety default-deny list: private keys, password files, credential stores, token caches | `README.md:294-297`. User-provided. |
| Rule pack file names: `generic.terminal.json`, `apt.json`, `cargo.json`, `npm.json`, `pytest.json`, `gcc.json` | `README.md:367-372`. User-provided. |
| Original 17-item ordered chain (expanded by architect to 32 TCxx goals; expansion is permitted per `README.md:308`) | `README.md:312-331`. User-provided. |
| MVP target list (7-step LLM workflow) | `README.md:333-343`. User-provided. |
| Seven-crate canonical list (TC04) - architect extension of README's six-crate list | `_USER_DECISIONS.md`; locked by TC04 goal file; documented contradiction below. |

## 2. External evidence (research-backed)

Claims promoted from "inference" to "evidence-backed" by the R1/R2
research wave. Each row cites the research document and a primary
upstream URL where one exists.

| Claim | Source |
|---|---|
| Language: Rust, edition 2024 | `docs/research/language-choice.md`; `_USER_DECISIONS.md` |
| Async runtime: tokio (rmcp 1.7.0 Cargo.toml depends on `tokio = "1"`) | `docs/research/async-runtime.md`; `docs/research/mcp-rust-sdk.md` |
| MCP SDK: rmcp `=1.7.0` exact pin (MSRV Rust 1.92, edition 2024); protocol revision 2025-11-25 | `docs/research/mcp-rust-sdk.md`; `docs/research/msrv.md`; `_USER_DECISIONS.md` |
| Storage: rusqlite 0.39 `bundled` (ships FTS5 by default) + refinery 0.9 + WAL | `docs/research/sqlite-fts5.md` |
| File watcher: notify 8.2 + notify-debouncer-full 0.7 with explicit per-target transport (native inotify, `PollWatcher` for 9P, `ReadDirectoryChangesW` on Windows) | `docs/research/file-watcher.md` |
| WSL2 `/mnt/c` (9P) silently breaks inotify; `inotify_add_watch` succeeds, no events delivered | `docs/research/wsl-boundary.md`; microsoft/WSL#4739 (open since 2019-12-06) |
| WSL2 9P detection via `/proc/self/mountinfo` at probe construction (not via runtime back-off) | `docs/research/wsl-boundary.md` sections 2.2-2.3 |
| PTY (MVP): pty-process 0.5.3 with `async` feature (POSIX); portable-pty deferred for Windows native | `docs/research/pty-crate.md`; `_USER_DECISIONS.md` |
| Daemon model: foreground supervisor + PID file; optional systemd USER unit; `fork` crate for optional `--daemonize` | `docs/research/daemon-lifecycle.md` |
| `daemonize` 0.5.0 is unmaintained on lib.rs; `fork` 0.7.0 is maintained | `docs/research/daemon-lifecycle.md` |
| Process cleanup: `process-wrap` (supersedes `command-group`) + process groups + SIGTERM-then-SIGKILL | `docs/research/process-cleanup.md` |
| WSL2 systemd is opt-in (requires WSL 0.67.6 + `/etc/wsl.conf`); not assumed | `docs/research/daemon-lifecycle.md` |
| Apache-2.0 SPDX id `Apache-2.0`; OSI-approved permissive; Category A allowlist covers MIT, BSD 2/3-clause, CC0, Unicode-3.0 | `docs/research/license-decision.md`; https://spdx.org/licenses/Apache-2.0.html; https://www.apache.org/legal/resolved.html |
| `notify` core is CC0-1.0 (must be on cargo-deny allowlist explicitly) | `docs/research/license-decision.md`; `docs/research/tooling-baseline.md`; https://github.com/notify-rs/notify |
| Workspace: flat virtual manifest, `members = ["crates/*"]`, resolver 3, `[workspace.package]` + `[workspace.dependencies]` + `[workspace.lints]` inheritance | `docs/research/workspace-layout.md` |
| `[workspace.lints]` table stabilized in Rust 1.74 | `docs/research/tooling-baseline.md`; https://blog.rust-lang.org/2023/11/16/Rust-1.74.0/ |
| Tooling: rustfmt + clippy (rustup), cargo-deny 0.19, cargo-machete 0.9, cargo-hack 0.6, cargo-nextest 0.9; 7-step CI sequence | `docs/research/tooling-baseline.md` |
| Linux Landlock available since kernel 5.13 (June 2021); enabled on every WSL2 kernel since 5.15.57.1 (2022); current WSL2 branch is linux-msft-wsl-6.18.y | `docs/research/policy-prior-art.md`; `docs/research/_R1-beta-summary.md`; https://docs.kernel.org/userspace-api/landlock.html |
| Policy enforcement framing: advisory in-process + audit log for MVP; kernel enforcement (Landlock + seccomp-bpf) named as post-MVP hardening | `docs/research/policy-prior-art.md`; `docs/research/_R1-beta-summary.md` |
| Prior art: TC's closest direct analog is honeycombio/honeytail (daemon tailing files with parser stack), NOT any agent/terminal product; no surveyed product combines MCP + streaming daemon + runtime-mutable rule registry | `docs/research/prior-art.md`; `docs/research/_R2-delta-summary.md` finding F1, F4 |
| Claude Code already uses a "wait for signal" pattern (file write / log entry / stdout match) that validates TC's `bucket_wait` primitive | `docs/research/_R2-delta-summary.md` finding F5 |
| rmcp is in an Apache-2.0 relicensing transition (Apache-2.0 going forward, legacy MIT, docs CC-BY-4.0) | `docs/research/mcp-rust-sdk.md`; https://github.com/modelcontextprotocol/rust-sdk/blob/main/LICENSE |
| sled is functionally abandoned for production (last release 2021-09-12); rejected in favor of rusqlite | `docs/research/_R1-beta-summary.md`; https://github.com/spacejam/sled |
| macOS FSEvents has a 32-subpath coalescing limit (qualitative behavior confirmed across bindings; specific 32 figure pending primary-source citation) | `docs/research/file-watcher.md`; `docs/research/_R1-beta-summary.md` |

## 3. Open decisions and contradictions

Tracked here so downstream goals can find them. Each row names the
goal that will resolve the question.

| Item | Status | Resolver |
|---|---|---|
| README lists 6 crates (`README.md:354-360`); TC04 + this SOURCE_MAP lock 7 crates (adds `terminal-commander-store`) | Contradiction documented; seven-crate list is authoritative for implementation. README is expected to be updated post-TC04 in a dedicated goal. | Post-TC04 doc goal (not yet assigned). |
| README says "License is not selected yet" (`README.md:384-386`); user decision locks Apache-2.0 | Contradiction documented; Apache-2.0 is authoritative. A LICENSE file + README license-section update is not in TC01 scope. | Dedicated license goal (not yet assigned). |
| IPC transport between MCP server and daemon | Plan of record: `interprocess` v2 local socket + JSON-RPC 2.0 (per `docs/research/mcp-transport-pattern.md`). NOT locked. | TC21 (daemon local API and router). |
| SQLite client crate selection (rusqlite vs sqlx) | Locked: rusqlite 0.39 bundled per `_USER_DECISIONS.md`. tokio-rusqlite bridging decisions deferred to the store goal. | TC04 declares the dependency; TC12 finalizes integration. |
| Daemonize support (`--daemonize` via `fork` crate) | Plan of record: skip. Foreground-only is acceptable for MVP. | TC25 (or explicitly skipped). |
| Per-user vs per-machine daemon | Plan of record: per-user. | TC26 (installer / service / WSL startup). |
| Encryption at rest (sqlcipher feature) | Deferred. | Post-MVP. |
| Kernel-enforced policy (Landlock + seccomp-bpf) | Deferred. Advisory only in MVP. | Post-MVP hardening goal. |
| macOS / Windows-native port | Deferred. PTY abstraction kept feature-flagged so a future port becomes a goal, not a rewrite. | Post-MVP. |
| FSEvents 32-subpath coalescing limit primary source | Open low-priority follow-up. | Post-MVP if/when macOS work scopes. |

## 4. Verified project state before goal generation

- The user reports the GitHub repository exists and the initial
  README.md has been added.
- No local clone or repository contents were inspected during goal
  generation.
- As of TC01 execution: branch `feature/terminal-commander-mvp`
  exists and the branch guard passes; documentation evidence under
  `docs/research/` is present (R1-alpha, R1-beta, R2-gamma, R2-delta
  outputs and `_USER_DECISIONS.md`).
