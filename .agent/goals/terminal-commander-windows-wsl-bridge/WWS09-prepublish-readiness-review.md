---
goal_id: WWS09
title: Prepublish Readiness Review
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 4 - Chain close
status: "Completed"
depends_on: ["WWS08"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: "2026-05-23T21:00:00+00:00"
completed_at: "2026-05-23T22:00:00+00:00"
completion_commit: "b750f8b"
blocked_reason: ""
source_refs:
  - "WWS01..WWS08 outcomes"
  - "docs/release/npm-distribution-final-report.md (NPM09 closure baseline)"
  - "RELEASE_CHECKLIST.md (NPM09 npm-distribution gate)"
  - "BACKLOG.md (P1.5 + P1.5b operator follow-ups)"
risk_level: "medium"
---

# WWS09 - Prepublish Readiness Review

## Branch Guard

```text
main
```

## Mission Context

WWS09 is the chain-closing review. It consolidates evidence from WWS01–WWS08, runs the full Rust + smoke + pack verification gate one more time, probes the npm registry to confirm the package names are still unpublished (or, if NPM10 was dispatched in the meantime, confirms what was published), and produces `docs/release/windows-wsl-bridge-final-report.md`. The recommendation is binary: ready for first live publish (via NPM10 bootstrap workflow OR the OIDC release-please path, depending on whether trusted publisher has been configured) OR not ready, with the exact blocker.

WWS09 does NOT publish. WWS09 does NOT dispatch any workflow. WWS09 does NOT merge a release PR. WWS09 does NOT create a tag or GitHub release manually.

## Mini-Spec

objective:
- Produce `docs/release/windows-wsl-bridge-final-report.md` (chain close evidence consolidation, mirroring the shape of `npm-distribution-final-report.md`). Refresh `RELEASE_CHECKLIST.md` + `BACKLOG.md` to acknowledge the new Windows-bridge path. Lock the recommendation: "Ready for first publish" or "Conditional Go preserved; pending <exact blocker>".

non_goals:
- No publish.
- No release PR merge.
- No tag / release creation.
- No workflow dispatch.
- No `crates/**` change.
- No `packages/*/package.json` version edit unless release-please has produced an open release PR the operator merged.

allowed_files_or_area:
- `docs/release/**`
- `BACKLOG.md`
- `RELEASE_CHECKLIST.md`
- `ROADMAP.md` (only if a roadmap row needs updating)
- `README.md` (only for tiny beta-status correction if needed)
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS09-*.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md` (final status flip)
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md` (final status row only if needed)

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- `packages/*/package.json` (no version edit)
- `packages/*/lib/**`
- `packages/*/bin/**`
- secrets / tokens / private paths

contracts_or_interfaces:
- Final report covers: per-goal chain summary (WWS01..WWS08 + WWS09) + npm registry probe (E404 expected unless NPM10 dispatched) + version sync + tarball dry-runs + OIDC posture + bridge shim invariants + Cursor live smoke status + GitHub Actions latest status + active blockers / operator preconditions + negative-surface confirmations + chain terminal state + acceptance.
- Recommendation: choose `Ready for first publish` (only if Cursor live smoke transcript landed AND npmjs.com operator preconditions are confirmed) OR keep `Conditional Go` with the exact open blocker.

invariants:
- No `Not Run` evidence promoted to PASS.
- No publish.
- TC48 + NPM09 + NPM10 token-policy invariants preserved.
- The NPM10 bootstrap workflow MAY still be present; WWS09 documents whether it should be dispatched + the disable + rotate follow-up status.

acceptance_criteria:
- Final report exists.
- All NPM01..NPM10 + WWS01..WWS08 acceptance evidence cross-linked.
- Recommendation locked with rationale.

evidence_required:
- Branch evidence.
- File paths.
- npm registry probe result.
- Rust + smoke + pack gate snapshot.
- Active blocker list.

stop_conditions:
- Branch is not `main`.
- A blocker discovered during review requires a runtime code change beyond a tiny documented compatibility fix.
- The recommendation cannot be evidence-backed.

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
( cd packages/terminal-commander && npm test )
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run
npm view terminal-commander version
npm view @terminal-commander/linux-x64 version
npm view @terminal-commander/linux-arm64 version
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|secrets\.NPM|secrets\.CARGO|secrets\.RELEASE" .github docs BACKLOG.md RELEASE_CHECKLIST.md
rg "cargo publish|crates\.io" .github docs BACKLOG.md RELEASE_CHECKLIST.md
rg "postinstall" packages .github docs README.md
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run WWS09 only on branch `main`. Review + evidence consolidation only. No publish, no dispatch, no merge.

## Final Report Format

Objective / Changes / Files changed / Per-goal recap / npm registry probe / Rust + smoke gate snapshot / Bridge invariants confirmed / Cursor live smoke status / Active blockers / Recommendation / Verification / Evidence / Commit / Chain terminal state.
