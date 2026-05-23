---
goal_id: NPM09
title: Release Dry Run And Beta Publish Review
chain_id: terminal-commander-npm-distribution
phase: Wave 5 - Release review
status: "Pending"
depends_on: ["NPM08b"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "NPM07 publish workflow"
  - "NPM08 Cursor smoke evidence (or Not Run record)"
  - "RELEASE_CHECKLIST.md (TC48 baseline)"
  - "EVIDENCE_REPORT_RUNTIME.md (TC48 baseline)"
  - "RISK_REGISTER.md (TC48 baseline)"
  - "BACKLOG.md (TC48 baseline)"
risk_level: "medium"
---

# NPM09 - Release Dry Run And Beta Publish Review

## Branch Guard

```text
main
```

## Mission Context

NPM01-NPM08 build the npm distribution and document the Cursor install. NPM09 is the chain's gate: an end-to-end release dry-run, a refresh of the beta posture against the new provider evidence (or the lack of it), and the decision on whether to keep `Conditional Go` or promote to `Go`.

## Mini-Spec

objective:
- Run the full release dry-run path (`release-please` PR generation → publish workflow `--dry-run`), refresh `RELEASE_CHECKLIST.md` / `BACKLOG.md` / `RISK_REGISTER.md` / `EVIDENCE_REPORT_RUNTIME.md` with NPM01-NPM08 outcomes, and lock the post-chain beta recommendation (`Go` / `Conditional Go` / `No-Go`).

non_goals:
- Do not actually publish to the npm registry under this goal. NPM07's workflow handles real publishing when the release-please PR is merged; NPM09 only verifies the dry-run.
- Do not modify `crates/**` or runtime behavior.
- Do not silence open risks or unresolved `Not Run` items.

allowed_files_or_area:
- RELEASE_CHECKLIST.md
- BACKLOG.md
- RISK_REGISTER.md
- EVIDENCE_REPORT_RUNTIME.md
- README.md (beta status line only if needed)
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM09-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**, config/**
- packages/**
- .github/workflows/** (unless a narrow fix is documented)
- secrets / tokens anywhere

contracts_or_interfaces:
- `release-please` PR generation runs (or is locally validated) against `main`; the resulting release PR is captured (URL or local diff) without merging.
- `npm publish --dry-run --provenance` runs against each package; output captured.
- `RELEASE_CHECKLIST.md` updated with: pre-flight gates from the runtime chain (unchanged), the npm gates (`npm pack --dry-run`, `npm publish --dry-run --provenance`, trusted publisher configured on npmjs.com), and the provider gate (Codex / Claude Code / Cursor status — at most one of which may upgrade the beta posture if a live smoke transcript landed in NPM08 or in the operator beta phase).
- `BACKLOG.md` / `RISK_REGISTER.md` updated to remove items resolved by NPM01-NPM08 and to add any new items the chain discovered.
- The beta recommendation is the operator decision: `Go` / `Conditional Go` / `No-Go`. The recommendation MUST be evidence-backed. Provider live smokes that are `Not Run` keep the ceiling at `Conditional Go`.

invariants:
- No runtime / `crates/**` changes.
- No `Not Run` evidence promoted to PASS.
- No risk row deleted without traceable resolution.

acceptance_criteria:
- All four artifacts (`RELEASE_CHECKLIST.md`, `BACKLOG.md`, `RISK_REGISTER.md`, `EVIDENCE_REPORT_RUNTIME.md`) refreshed against NPM01-NPM08.
- Release dry-run output recorded (PR URL or local diff + `npm publish --dry-run` output).
- Beta recommendation locked with rationale.
- Chain is closed: this is the terminal goal of `terminal-commander-npm-distribution`.

evidence_required:
- Branch evidence.
- File paths changed.
- Release-please dry-run output reference.
- `npm publish --dry-run --provenance` output reference per package.
- Beta recommendation with rationale.
- Updated source-status snapshot.

stop_conditions:
- Branch is not `main`.
- A blocker discovered during the dry-run requires runtime code change beyond a tiny documented compatibility fix.
- The beta recommendation cannot be evidence-backed.

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
cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture
bash scripts/smoke/verify-runtime-smoke.sh
# npm-side dry-runs:
( cd packages/terminal-commander && npm publish --dry-run --provenance )
( cd packages/terminal-commander-linux-x64 && npm publish --dry-run --provenance )
( cd packages/terminal-commander-linux-arm64 && npm publish --dry-run --provenance )
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM09 only on branch `main`. Dry-run only. Lock the post-chain beta posture honestly.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal:
- none — NPM09 closes the `terminal-commander-npm-distribution` chain. Promotion to `Go` (if not yet locked) requires the operator-driven provider live smoke transcript, not a new code chain.
