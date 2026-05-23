---
goal_id: NPM08b
title: Readme Project Normative Overhaul
chain_id: terminal-commander-npm-distribution
phase: Wave 5 - Provider smoke
status: "Pending"
depends_on: ["NPM08"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T06:30:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
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
