---
goal_id: NPM08b
title: Readme Project Normative Overhaul
chain_id: terminal-commander-npm-distribution
phase: Wave 5 - Provider smoke
status: "Completed"
depends_on: ["NPM08"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T06:30:00+00:00"
started_at: "2026-05-23T07:00:00+00:00"
completed_at: "2026-05-23T08:00:00+00:00"
completion_commit: "cacfef5"
blocked_reason: ""
source_refs:
  - "Current root README.md"
  - "TC48 RELEASE_CHECKLIST.md / EVIDENCE_REPORT_RUNTIME.md (beta posture: Conditional Go)"
  - "NPM02 docs/release/npm-binary-packaging-contract.md"
  - "NPM06 docs/release/release-please-contract.md"
  - "NPM07 docs/release/npm-trusted-publishing-contract.md"
  - "NPM08 docs/integrations/cursor.md + examples/provider-harness/cursor/"
  - "User directive 2026-05-23: insert NPM08b between NPM08 and NPM09; rewrite root README only after npm + Cursor + smoke details are known"
risk_level: "low"
---

# NPM08b - Readme Project Normative Overhaul

## Branch Guard

```text
main
```

## Mission Context

NPM01–NPM08 stood up the npm distribution + Cursor MCP integration. The root `README.md` predates that work and reads as the runtime-chain (TC01–TC48) README. NPM08b rewrites the root README into the canonical public project README for Terminal Commander — product definition, architecture, feature matrix, install paths, Cursor configuration, quickstart, settings, safety posture, and the current beta status.

This goal is documentation-only. No runtime / package / workflow changes. No new MCP tools. No version bumps.

## Mini-Spec

objective:
- Rewrite `README.md` (root) into the canonical public project README. Reflect the actual current state of the project after NPM01–NPM08: npm packaging is implemented, Cursor MCP config is documented, npm-binary-build + release-please + trusted-publishing workflows exist, live publish + Cursor live smoke are pending operator-driven steps. Reuse and link existing canonical docs (`docs/runtime/`, `docs/mcp/`, `docs/install/`, `docs/integrations/`, `docs/release/`, `docs/security/`, `docs/testing/`, `examples/provider-harness/`, `examples/mcp/`) rather than duplicating content.

non_goals:
- Do not modify `crates/**` or runtime behavior.
- Do not change any `package.json` version.
- Do not change `.github/**` workflows or release-please config.
- Do not add or rename MCP tools.
- Do not promote `Not Run` evidence to PASS.
- Do not invent claims beyond what `EVIDENCE_REPORT_RUNTIME.md`, the TC48 baseline, or the NPM chain artifacts already record.

allowed_files_or_area:
- `README.md` (root)
- `docs/runtime/**`
- `docs/mcp/**`
- `docs/install/**`
- `docs/integrations/**`
- `docs/release/**`
- `docs/security/**`
- `docs/testing/**`
- `examples/provider-harness/**`
- `examples/mcp/**`
- `.agent/goals/terminal-commander-npm-distribution/NPM08b-*.md`
- `.agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md`
- `.agent/goals/terminal-commander-npm-distribution/RUN_ORDER.md`

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- any `package.json` version field
- any `.github/release-please-config.json` / `.release-please-manifest.json` edit
- any npm publish workflow change
- runtime behavior changes
- new MCP tools
- secrets / tokens / private usernames / private absolute paths / GitHub-actor literals

contracts_or_interfaces:
- The README MUST distinguish three install paths honestly:
  1. **Current pre-publish path:** local tarball smoke via `scripts/smoke/verify-npm-local-install.sh` (NPM04 evidence).
  2. **Future published path:** `npm install -g terminal-commander` once the npmjs.com trusted-publisher preconditions complete and the first release PR merges (per `docs/release/npm-trusted-publishing-contract.md` §8 + §14).
  3. **Cargo-built path:** `cargo build -p terminal-commanderd -p terminal-commander-mcp -p terminal-commander-cli` (always available).
- The README MUST link to `docs/integrations/cursor.md` for Cursor config and to `examples/provider-harness/cursor/` for copy-pasteable JSON.
- The README MUST honor the cross-chain invariants (no shell bridge, no raw stream lane, no network listener, MCP guard greps green, no postinstall downloader, no macOS / Windows-native or musl claim).
- Every claim labeled `live` MUST be traceable to TC48 / NPM-chain evidence; every claim labeled `pending` / `Not Run` / `Blocked` MUST cite the blocker.

invariants:
- No secrets, tokens, private usernames, private absolute paths, or machine-specific paths.
- No promotion of `Not Run` to PASS.
- Diagrams are plain Markdown / ASCII or Mermaid (only if GitHub renders it safely).

acceptance_criteria:
- `README.md` includes: product definition, value statement, architecture diagram, feature matrix, install section (three paths), quickstart, Cursor MCP examples (native Linux/WSL + Windows→WSL), settings/config section, safety posture, current beta status.
- Every cross-link to existing docs resolves (no broken relative paths).
- Forbidden paths diff (`crates/`, `Cargo.toml`, `Cargo.lock`, `rules/`, `config/`, `scripts/`, `.github/`, any `package.json` version) remains empty under `--ignore-cr-at-eol`.
- Direct local smoke (TC46 + NPM04) and Rust gates remain green.
- MCP guard greps remain doc / negative-assertion only.

evidence_required:
- Branch evidence.
- File paths changed.
- Diff summary (lines added / removed in `README.md`).
- Confirmation that every external reference in the README points at a file present in this repo at the goal's `completion_commit`.

stop_conditions:
- Branch is not `main`.
- A README claim would require a runtime / package / workflow / version change to be honest.
- A required cross-link points at a file that does not yet exist (raise as a follow-up goal instead of fabricating the file inline).

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
# Bounded README-side checks (run at NPM08b implementation time):
# - link-resolution sweep: every relative link from README.md must
#   resolve under the goal's completion_commit
# - secret / private-path grep over README.md and any updated docs
```

## Task Prompt

Run NPM08b only on branch `main`. Documentation-only. Reuse existing canonical docs via links instead of duplicating their content. Lock the README's "current state" claims against TC48 + NPM-chain evidence; do not invent claims beyond what evidence records.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM09).

## Final Report

Objective:
- Rewrite root `README.md` into the canonical public project README for Terminal Commander, reflecting the actual state after NPM01-NPM08. Documentation-only; no runtime / package / workflow / version change.

Changes (verified work commit `cacfef5`):
- `README.md` rewritten end-to-end. Diff stats: **382 insertions, 404 deletions** (net -22 lines; the previous README's TC48-era sections were replaced with NPM-chain-aware equivalents, not pruned).

No other file modified. No docs/runtime/, docs/mcp/, docs/install/, docs/integrations/, docs/release/, docs/security/, docs/testing/, examples/provider-harness/, or examples/mcp/ file changed — the README links to existing canonical docs instead of duplicating their content. Verified by `git status --short` post-commit (only the NPM08b goal file shows pending modification at status-commit time).

README files changed:
- `README.md` (1 file)

Whether any docs besides README changed: **no**. The other allowed paths under the NPM08b goal-file `allowed_files_or_area` (docs/runtime/, docs/mcp/, docs/install/, docs/integrations/, docs/release/, docs/security/, docs/testing/, examples/provider-harness/, examples/mcp/) were inspected but not edited. The README links to them in their current state.

Confirmation — no forbidden paths changed:
- `git diff --ignore-cr-at-eol --shortstat HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ packages/ .github/` → empty at verified work commit.
- No `package.json` version field touched (all three still `0.1.0-beta.1`).
- No release-please config / manifest edit.
- No new MCP tools, no IPC surface change, no runtime behavior change.
- MCP guard greps unchanged: guard 1 doc/negative-assertion only; guard 2 no matches.

Verification summary (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, npm 10.9.7, node 22.22.2):
- PASS `git branch --show-current` → `main`
- PASS `git status --short` post-stage (only README.md modified)
- PASS `git diff --check`
- PASS `cargo metadata --no-deps`
- PASS `cargo fmt --all --check`
- PASS `cargo clippy --workspace --all-targets -- -D warnings` (clean)
- PASS `cargo test --workspace` (43 test-result lines, all `ok`)
- PASS `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 8/8 PASS
- PASS `bash scripts/smoke/verify-npm-local-install.sh` — NPM04 SUCCESS (12 PASS, end-to-end MCP stdio against npm-installed binaries)
- PASS `npm pack ./packages/terminal-commander --dry-run` — 7 files, 0.1.0-beta.1
- PASS `npm pack ./packages/terminal-commander-linux-x64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc/negative-assertion matches only
- PASS `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches
- PASS `rg "sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}|NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC" README.md` — only the explicit negative-documentation sentence under "Current beta status" mentions the `_TC` token names to record that they are NOT referenced by any workflow. No API key / GitHub PAT / npm token pattern.
- PASS README link-resolution sweep (python regex over `\[text\](relative-path)` patterns) → every relative link resolves to a file present in the repo at the verified work commit. Specifically verified: `docs/runtime/REALTIME_SIGNAL_CHANNEL.md`, `docs/runtime/UDS_IPC.md`, `docs/runtime/COMMAND_RUNTIME.md`, `docs/mcp/TOOL_CONTROL_SURFACE.md`, `docs/security/PRIVILEGE_MODEL.md`, `docs/release/npm-trusted-publishing-contract.md`, `docs/release/npm-binary-packaging-contract.md`, `docs/integrations/cursor.md`, `examples/provider-harness/cursor/mcp.global.native-linux.json`, `examples/provider-harness/cursor/mcp.global.linux-wsl.json`, `scripts/smoke/verify-runtime-smoke.sh`, `scripts/smoke/verify-npm-local-install.sh`, `config/terminal-commanderd.example.toml`, `TESTING.md`, `SECURITY.md`, `RELEASE_CHECKLIST.md`, `EVIDENCE_REPORT_RUNTIME.md`, `RISK_REGISTER.md`, `BACKLOG.md`, `LICENSE`, `CONTRIBUTING.md`, `docs/research/license-decision.md`.

Explicit beta status wording used (verbatim from README §"Current beta status"):
> **Conditional Go** (TC48 baseline, preserved through NPM01-NPM08).
> ...
> Beta cannot promote to `Go` until at least one provider live smoke transcript is attached. `Not Run` is **not** PASS.

Explicit provider-smoke wording used (verbatim from README §"Cursor MCP integration"):
> **Not Run.** Cursor 3.5.30 is installed on the verification host, but Cursor today has no documented non-interactive MCP discovery / tool-call entry point — there is no `cursor --list-mcp-tools` subcommand, and the `cursor-agent` headless CLI is not installed. Live smoke requires operator-driven GUI steps (open Cursor → place config → start daemon → ask Cursor chat to list MCP tools and call `health` → capture transcript). Not scriptable in this session; not promoted to PASS.

Codex CLI + Claude Code provider smokes are also explicitly recorded as **Not Run** (TC46 / TC48 baseline) in the feature matrix and in the beta-status section.

Confirmation — no `Not Run` evidence promoted to PASS:
- Cursor provider live smoke: `Not Run`.
- Codex CLI provider live smoke: `Not Run`.
- Claude Code provider live smoke: `Not Run`.
- First live npm publish: pending (operator preconditions + release PR merge) — recorded as pending, not as success.
- Beta posture: `Conditional Go`, NOT `Go`.
- Every `live` claim in the feature matrix is traceable to a TC chain (TC33-TC48) or NPM chain (NPM02-NPM07) goal already accepted; every `Not Run` / `pending` claim cites the exact blocker.

Confirmation — README forbidden-content rules honored:
- No secrets, no API keys, no GitHub PATs, no npm tokens.
- No `/home/<user>/` literal, no `/Users/<user>/` literal, no Windows USERPROFILE literal.
- No machine-specific absolute paths. The README uses `${TC_DATA}` / `${XDG_STATE_HOME}` placeholders for daemon data dir; the WSL example uses the generic distro name `Ubuntu-24.04`.
- No active `.cursor/mcp.json` referenced as committed in this repo (the directory does not exist; configs live under `examples/provider-harness/cursor/`).
- No claim that npm package is already published. The README explicitly distinguishes the future published path from the current local-tarball path.
- No unsupported PASS claim. Every `live` row in the feature matrix is evidence-backed.

Beta-state mapping:
- TC48 `Conditional Go` preserved. NPM08b documentation-only commit does not advance or retract the beta posture.
- NPM07 + NPM08 outcomes accurately summarized in §"Current beta status".

Commits:
- Verified work commit: `cacfef5`
- Goal status commit: this commit

Known gaps / blockers:
- **Cursor provider live smoke is still Not Run.** Documented in README + `docs/integrations/cursor.md` §9. Cursor has no headless MCP entry on the verification host. Lifting requires operator GUI smoke.
- **First live npm publish still pending.** Documented in README + `docs/release/npm-trusted-publishing-contract.md` §14. Requires npmjs.com trusted-publisher setup + a release PR merge.
- **Node 20 actions deprecation** annotation on release-please-action v4.4.1 (live in NPM06 + NPM07 runs) is unchanged. Re-pin to v5 is a follow-up amendment outside NPM08b scope.

Confirmation — NPM09 not started.

Next goal:
- NPM09-release-dry-run-and-beta-publish-review.md (depends_on NPM08b; updated at chain-plan insertion `0d0e530`).
