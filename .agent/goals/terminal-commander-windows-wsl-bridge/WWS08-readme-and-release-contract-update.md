---
goal_id: WWS08
title: Readme And Release Contract Update
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 4 - Docs
status: "Pending"
depends_on: ["WWS07"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "WWS01..WWS07 outcomes"
  - "README.md (NPM08b canonical public README)"
  - "docs/release/npm-binary-packaging-contract.md (NPM02 baseline being amended)"
  - "docs/release/npm-bootstrap-first-publish.md (NPM10 policy exception)"
risk_level: "low"
---

# WWS08 - Readme And Release Contract Update

## Branch Guard

```text
main
```

## Mission Context

WWS08 closes the documentation loop: update the canonical public README + release contracts to reflect the Windows-bridge + WSL-runtime split locked at WWS01 and implemented in WWS02–WWS07. The README install section needs the Windows path; the feature matrix needs the new CLI subcommands + bridge shim row; the safety posture needs the bridge-shim invariants; the beta status needs the new Cursor live smoke status from WWS07.

This goal is documentation-only. No `package.json` edit. No workflow edit. No `crates/**` edit.

## Mini-Spec

objective:
- Update `README.md` with the Windows + WSL install paths, the `setup cursor-wsl` quickstart, the bridge architecture line, and the WWS07 smoke status. Update `docs/release/npm-binary-packaging-contract.md` (NPM02) with the WWS02 amendment (root `os: ["linux", "win32"]`; platform packages remain Linux-only) recorded as a clear "amended at WWS08 from NPM02 baseline" section. Update `docs/release/release-please-contract.md`, `docs/release/npm-trusted-publishing-contract.md`, and `docs/release/npm-distribution-final-report.md` as needed to acknowledge the bridge work without changing the OIDC contract.

non_goals:
- No `package.json` edit (versions stay at whatever release-please last bumped to).
- No workflow edit.
- No new MCP tool.
- No runtime behavior change.
- No publish dispatch.

allowed_files_or_area:
- `README.md`
- `docs/release/**`
- `docs/install/**`
- `docs/integrations/**` (cross-link only)
- `BACKLOG.md` (record any work deferred during the chain)
- `RELEASE_CHECKLIST.md` (acknowledge the bridge chain landed; do NOT modify the publish gates locked at NPM09)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS08-*.md`

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- `packages/*/package.json` (version edits forbidden)
- `packages/*/lib/**` (WWS04 / WWS05 owns the JS; WWS08 only documents it)
- secrets / tokens / private paths

contracts_or_interfaces:
- README install section: three paths still recorded honestly (future published, current local-tarball, cargo-built) + the new Windows-bridge path.
- Feature matrix: bridge shim + setup CLI subcommands + Cursor config writer rows.
- Beta status: Cursor live smoke status pulled verbatim from WWS07 final report (PASS only if a transcript landed; otherwise `Not Run` with exact blocker).
- All `live` claims traceable to a chain goal's completion commit.
- No promotion of `Not Run` to PASS.

invariants:
- Diffstat ONLY touches README + docs/ + BACKLOG + RELEASE_CHECKLIST + goal file.
- Forbidden-paths diff (`--ignore-cr-at-eol`) empty.
- MCP guard greps remain clean.

acceptance_criteria:
- Every relative README link resolves at the verified work commit.
- Beta status section explicitly states whether the WWS07 Cursor smoke transcript landed.
- BACKLOG records any WWS chain deferrals.

evidence_required:
- Branch evidence.
- File paths.
- Link-resolution sweep result.
- Diff statistic.

stop_conditions:
- Branch is not `main`.
- A README claim would require a runtime / package / workflow / version change to be honest.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh
( cd packages/terminal-commander && npm test )
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS08 only on branch `main`. Documentation-only. Pull beta status verbatim from WWS07; do not invent.

## Final Report Format

Objective / Changes / Files changed / Link-resolution sweep / Beta status verbatim / Verification / Evidence / Commit / Known gaps / Next goal (WWS09).
