# Citadel Project Spec

Version: 1

## Project

Name: Terminal Commander
Summary: Terminal Commander is a local-only MCP control plane for coding agents: it runs commands, PTYs, and file watches and returns structured signals (rule matches plus exit state), never raw output, and a quiet command returns a bounded receipt, never silence. It ships three binaries: `terminal-commanderd` (daemon), `terminal-commander-mcp` (stdio MCP adapter that forwards every tool call over local IPC), and `terminal-commander` (admin CLI).

## Conventions

- Crates (workspace members in `Cargo.toml`): `crates/core` (ids, buckets, context rings, events), `crates/sifters` (rule evaluation, dedupe), `crates/probes` (process, file, PTY runtimes), `crates/store` (SQLite events, registry, audit), `crates/supervisor` (ensure_daemon, replace_if_stale, session tokens), `crates/ipc` (wire protocol, UDS and named-pipe clients), `crates/daemon` (`terminal-commanderd`: IPC, policy, router, runtimes), `crates/mcp` (adapter), `crates/cli` (admin CLI). Package names are `terminal-commander-<crate>` except the daemon, which is `terminal-commanderd`; pass these to `cargo -p`. npm wrappers live in `packages/`.
- Output contract: tool responses are bounded structured signals. Never add a path that returns unbounded raw stdout/stderr, and a quiet command must still return a receipt (exit state, suppressed-line count, short tail). The code and its tests are the only source of truth: every document here, `.specify/memory/constitution.md` included, is a description that may be stale or superseded. When a doc and the code disagree, verify against the code and fix the doc.
- Single execution chokepoint: `crates/mcp` must not spawn processes, open sockets, or touch the filesystem; the daemon owns probes, policy, audit, and storage. `scripts/linux-gate.sh` greps `crates/mcp/src` for these and fails the gate. The daemon binds a local endpoint only (UDS or named pipe), no public TCP listener.
- Policy before spawn: every gated start is policy-checked and audited. The default profile is `full_access` (all `[policy.caps]` on); `developer_local`, `repo_only`, `read_only_observer`, `admin_debug` are opt-in hardening. Caps are daemon config, never MCP-flippable.
- The one failsafe: deleting OS-critical infrastructure is refused with `OsCriticalPathProtected` in every profile, with no knob (`POLICY.md` section 2). Do not weaken or add a bypass.
- No mocks on production paths: test doubles stay in `tests/`, `fixtures/`, or `#[cfg(test)]`. "It compiled" is not verification. An unimplemented feature must surface as deferred or an explicit error, never as live (`CONTRIBUTING.md` section 9, `TESTING.md` sections 4-5).
- Honest degradation: an IPC blip returns `degraded: true` with `recover_hint` and the known job/bucket ids, never a bare error. Rule suggestion never auto-activates a rule (suggest, `registry_test`, then explicit upsert and activate).
- MCP protocol: the adapter prefers 2026-07-28 and also accepts legacy `initialize` with 2025-11-25 or 2025-06-18, and serves a client that sends no handshake as if it had initialized (`docs/integrations/README.md`). Raw-wire drivers and smoke scripts (guarded by `crates/mcp/tests/protocol_honesty.rs`) must open with `server/discover`, never `initialize`, and send `_meta` (`io.modelcontextprotocol/protocolVersion`, `clientCapabilities`, `clientInfo`) on every request; copy `scripts/smoke/verify-runtime-smoke.sh`.
- LLM-facing text is pinned by tests: the server `instructions` string must stay at or under 850 characters and name structured signal, argv, receipt, `run_and_watch`, `bucket_wait`, `command_output_tail` (`crates/mcp/tests/instructions_contract.rs`). The facade description consts in `crates/mcp/src/surface_list.rs` must stay identical to the `#[tool]` attributes in `crates/mcp/src/tools.rs`, and `tool_catalogue()` rows are hand-maintained separately and must match the live `#[tool]` descriptions.
- Adding or removing an MCP tool updates every count anchor and the `system_discover` fixture in the same change (`tests/fixtures/contracts/`, checked by `crates/mcp/tests/fixture_catalogue_contract.rs`).
- Surfaces: `TC_SURFACE=compact` advertises six facades (`command`, `files`, `recipe`, `registry`, `session`, `status`); unset or unrecognized selects `full` (60 granular tools). Both reach the same daemon operations under the same policy.
- Style: Rust edition 2024; workspace lints forbid `unsafe_code` and warn on clippy pedantic and nursery (`Cargo.toml`); every source file carries the two-line SPDX header from `CONTRIBUTING.md` section 2; docs and agent output are ASCII only; commits follow Conventional Commits.

## Workflows

- Toolchain: Rust 1.97.1 from `rust-toolchain.toml` (rustfmt, clippy). The documented MSRV 1.92 is not CI-enforced.
- Inner loop and minimum gate for any Rust change: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`. Narrow with `cargo nextest run -p terminal-commander-mcp --tests` or `-p terminal-commanderd --tests`. Each new MCP tool needs an integration test that goes through the daemon IPC.
- If `session_reap_token_shuts_down_the_daemon` hangs on a cloud VM, exclude it: `cargo nextest run --workspace -E 'not test(session_reap_token_shuts_down_the_daemon)'`. `.config/nextest.toml` already kills tests at 5 minutes and retries that one twice.
- PR gate (what CI runs): `bash scripts/linux-gate.sh` (linux/mac; `pwsh scripts/linux-gate.ps1` runs it in WSL from Windows) and `pwsh scripts/windows-gate.ps1`. Required before pushing when a change touches `cfg(unix)`, `cfg(windows)`, `target_os` code, or any test (`CONTRIBUTING.md` section 6.1). The linux gate needs cargo-nextest, node, and python3.
- Platform-asymmetric fixes ship three pieces together: the production fix, a Windows or Unix regression test, and for Windows cfg sentinels registration in `scripts/windows-gate.ps1`. Live dogfood on the affected OS needs evidence; otherwise mark it `UNVERIFIED` with a stated reason.
- Quickest daemon plus MCP end-to-end check: `bash scripts/smoke/verify-runtime-smoke.sh` (builds into `target-wsl/` by default, uses a private temp data dir).
- npm wrapper tests: `npm --prefix packages/terminal-commander test` (no install needed). Do not run plain `npm install` there; the committed `package-lock.json` is stale relative to `package.json` and gets dirtied.
- Manual or live testing: start the daemon with an explicit data directory and point CLI or MCP clients at its socket with `TC_SOCKET`. Set `TC_IDLE_TTL_SECS=0` only for long-lived manual sessions; the default self-reaps idle daemons.
- Report evidence per `TESTING.md` section 10: branch, files changed, PASS/FAIL per command, and a source-status label (`live`, `partial`, `degraded`, `disabled`, `test-only`, `mock`, `blocked`) for each behavior touched. `unknown` is not allowed at commit.
- Document every public type, MCP tool, and CLI subcommand before treating it as live (`CONTRIBUTING.md` section 10).

## Constraints

- Versions and changelogs belong to release-please (`.github/release-please-config.json`, `.github/.release-please-manifest.json`, `x-release-please-version` markers in `Cargo.toml`, `scripts/release/sync-cargo-versions.py`). Do not hand-edit version numbers or `CHANGELOG.md`.
- Commit locally at every verified checkpoint. Push, PR merge, remote-branch deletion and publish wait for the owner's go-ahead. Merging a feature PR to main leads straight to a published release (the release PR auto-merges), so fix every known or suspected problem before proposing a merge.
- Because `full_access` is the default, a test that pins a deny must select a hardened profile (and `allow_shell = false` where relevant) or the command really spawns. Destructive-looking tests must target non-existent paths (see `crates/daemon/tests/command_runtime.rs`).
- No document is a rulebook. `ARCHITECTURE.md`, `SPEC.md`, `ROADMAP.md`, `CONTRIBUTING.md` and the files under `docs/` can be stale; when a change makes one wrong, or you find one wrong, correct it against the code in the same change. Dated records (`docs/audits`, `docs/dogfood`, `docs/research`, `docs/superpowers`, `specs/`) are history: leave them as written.
- Fixtures must be deterministic, small, and free of real secrets, hostnames, or tokens (`TESTING.md` section 6).
- Platform limits: shell sessions are unix-only; live ConPTY child-output e2e is gated by `TC_CONPTY_E2E=1`; macOS is not live-verified (`README.md`).
