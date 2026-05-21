# Assumptions Register - terminal-commander-mvp

Status: Updated in TC01 wave 0 to reflect research-promoted facts
and user-locked decisions. Language: ASCII only.

This file separates: (1) facts the user has confirmed, (2) items
the architect previously assumed that the TC01 research wave has
since promoted to evidence-backed (no longer "to verify"),
(3) decisions explicitly deferred to specific later goals, and
(4) the stack/branch/privilege/destructive/empty-repo register
required by TC01 acceptance criterion 9, plus the correction
protocol.

## 1. Confirmed by user

Locked by direct user input. Architect must treat as binding.
See `docs/research/_USER_DECISIONS.md`.

- Target repository: `https://github.com/special-place-administrator/terminal-commander.git`
- Repository is new/empty except the initial README.md added by
  the user.
- Branch policy:
  - `target_branch`: `feature/terminal-commander-mvp`
  - `prohibited_branches`: `["main", "master"]`
- Goal files are run linearly with `/goal`.
- Work mode: mixed planning and implementation goals.
- License: Apache-2.0 (SPDX `Apache-2.0`). Single license, not dual
  MIT/Apache.
- rmcp pin: `=1.7.0` (exact). MSRV floor Rust 1.92, edition 2024.
- Async runtime: tokio (forced by rmcp 1.7.0).
- Daemon process model: two-process. Thin
  `terminal-commander-mcp` + persistent `terminal-commanderd`. IPC
  details deferred to TC21.
- Workspace: Rust 2024 multi-crate, flat `crates/<short>/` layout,
  seven crates per TC04.
- Storage: rusqlite 0.39 `bundled` (FTS5 included) + refinery 0.9.
- File watcher: notify 8.2 + notify-debouncer-full 0.7.
- WSL `/mnt/c` handling: force `PollWatcher`; detect 9P at probe
  construction via `/proc/self/mountinfo`.
- PTY (MVP): pty-process 0.5.3 with `async` feature (Linux + WSL).
- Policy enforcement (MVP): advisory in-process + audit log.
  Landlock + seccomp-bpf are post-MVP hardening.
- Platforms: Linux native + WSL2 primary. macOS / Windows-native
  deferred.
- Prior-art research: behavior survey only; no copied source.

## 2. Architect assumptions now promoted to evidence

These were originally listed as "to verify" in the pre-TC01 draft.
The TC01 research wave promoted them to evidence-backed status; they
are no longer open assumptions. The originating evidence document is
cited.

| Assumption (original) | Now backed by |
|---|---|
| Stack: Rust workspace for daemon, probes, storage, CLI, and MCP server | `docs/research/language-choice.md`; `docs/research/workspace-layout.md`; `_USER_DECISIONS.md` |
| Storage: SQLite or equivalent embedded local store for registry and event data | `docs/research/sqlite-fts5.md` (rusqlite 0.39 bundled + refinery 0.9 + WAL) |
| MCP transport: provider-neutral local MCP server, stdio or local transport | `docs/research/mcp-rust-sdk.md` (rmcp 1.7.0 stdio); `docs/research/mcp-transport-pattern.md` |
| Security: no privileged helper or sudo/root execution by default | TC02 doctrine pending; daemon is unprivileged by default per `README.md:284-297` and `docs/research/policy-prior-art.md` |
| Platform: Linux and WSL primary; macOS/Windows-native deferred | `_USER_DECISIONS.md`; `docs/research/wsl-boundary.md` |
| Raw output: no unbounded terminal/file output returned by default | Invariant in TC01 mini-spec; preserved in SPEC and ARCHITECTURE |
| MCP can be implemented as a local user-mode process talking to a daemon | `docs/research/mcp-transport-pattern.md`; `docs/research/daemon-lifecycle.md` |
| Linux and WSL share most probe behavior but require documented startup differences | `docs/research/daemon-lifecycle.md`; `docs/research/wsl-boundary.md` |

No items remain in the "architect assumes, must verify in TC01"
category. Items that are still open are decisions (section 3), not
assumptions.

## 3. Open decisions deferred to specific goals

These are not assumptions; they are open decisions whose resolution
is owned by a specific downstream goal.

| Decision | Default behavior until decided | Owning goal |
|---|---|---|
| IPC transport between MCP server and daemon | Plan of record: `interprocess` v2 local socket + JSON-RPC 2.0 per `docs/research/mcp-transport-pattern.md`. Not locked. | TC21 |
| SQLite client integration crate (rusqlite alone vs `tokio-rusqlite` bridge) | rusqlite 0.39 bundled is locked as the backend; the tokio bridging style is open. | TC04 declares the dep; TC12 finalizes integration. |
| Daemonize flag (`--daemonize` via `fork` crate) | Plan of record: skip. Foreground-only acceptable for MVP. | TC25 or explicitly skipped. |
| Per-user vs per-machine daemon | Plan of record: per-user. | TC26 |
| LICENSE file creation and README license-section update | Not in TC01 scope. README contradiction documented. | Dedicated license goal (not yet assigned). |
| README crate-count update (6 -> 7) | Not in TC01 scope. Seven-crate list is authoritative for implementation. | Dedicated doc goal (not yet assigned, post-TC04). |
| Encryption at rest (sqlcipher) | Off. | Post-MVP. |
| Kernel-enforced policy (Landlock, seccomp-bpf) | Advisory only. | Post-MVP hardening. |
| macOS / Windows-native port | Not built. PTY abstraction kept feature-flagged. | Post-MVP. |
| Journal probe (`journal_probe`) | Not built in MVP. Must not appear in `system_discover`. | Post-MVP. |

## 4. Stack / branch / privilege / destructive / empty-repo register

Required by TC01 acceptance criterion 9.

### 4.1 Stack assumption

- Rust workspace (edition 2024, MSRV 1.92), seven crates under
  `crates/<short>/`, rmcp 1.7.0, tokio, rusqlite 0.39 bundled +
  refinery 0.9 + FTS5 + WAL, notify 8.2 +
  notify-debouncer-full 0.7, pty-process 0.5.3 (`async`). All
  locked per `_USER_DECISIONS.md`. Any downstream divergence
  requires a user decision update.

### 4.2 Branch assumption

- All TC01-TC32 work targets branch `feature/terminal-commander-mvp`.
- `main` and `master` are prohibited working branches for every
  goal in this chain.
- Branch creation outside the locked target requires an explicit
  goal that scopes it.
- Pushing, force-pushing, and remote-branch deletion require user
  approval (global gstack rule).

### 4.3 Privilege assumption

- The MCP server (`terminal-commander-mcp`) runs unprivileged.
  Never as root.
- The daemon (`terminal-commanderd`) runs unprivileged by default.
  Privileged operation modes are post-MVP; they require an explicit
  goal that scopes the elevation.
- The CLI (`terminal-commander-cli`) is unprivileged and must not
  bypass daemon policy.
- Default-deny paths (private keys, password files, credential
  stores, token caches per `README.md:294-297`) are denied in every
  policy profile until an explicit allow rule is added.

### 4.4 Destructive-change assumption

- No destructive migrations, filesystem deletions, package
  installs, or system-level changes ship as default behavior.
- A destructive operation may be added only by a goal that explicitly
  permits it and demonstrates safe dry-run behavior first.
- TC22 audit log is append-only; any operation that could subvert
  it (truncate, replace, redirect) must itself be policy-gated and
  audited.

### 4.5 Empty-repo assumption

- The repository was empty except for the user-added README.md at
  the start of the chain.
- As of TC01 execution, the repo also contains the TC01 research
  output under `docs/research/` and the `.agent/goals/` chain. No
  Rust source, Cargo.toml, CI files, or LICENSE file exist yet.
- TC01 deliverables (this file, SPEC.md, ARCHITECTURE.md,
  ROADMAP.md, CONTRIBUTING.md, docs/research/README.md) are
  documentation only; no implementation code is created in TC01.
- Goals that assume the presence of a workspace (Cargo.toml,
  rust-toolchain.toml, etc.) must depend on TC04 directly or
  transitively.

## 5. Correction protocol

If any assumption in this file is wrong:

1. Stop the current goal.
2. Mark the goal `Blocked` and record the corrected fact in the
   goal-file frontmatter (`blocked_reason`).
3. Update this file with the corrected fact (in the appropriate
   section) in a separate edit. Cite the new source.
4. Do not guess. Do not paper over the conflict by editing a
   downstream document to match a fabricated belief.

If a research document is wrong, the correction lands in a new
research file under `docs/research/`; the original is left intact as
the historical record of what was believed at TC01 time.
