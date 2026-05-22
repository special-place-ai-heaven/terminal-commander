---
goal_id: NPM07
title: Npm Trusted Publishing Workflow
chain_id: terminal-commander-npm-distribution
phase: Wave 4 - Publishing
status: "Pending"
depends_on: ["NPM06"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: ""
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "npm trusted publishing docs (GitHub Actions OIDC)"
  - "npm provenance docs"
  - "release-please-action README (release publishing section)"
risk_level: "high"
---

# NPM07 - Npm Trusted Publishing Workflow

## Branch Guard

```text
main
```

## Mission Context

NPM06 produces release PRs and GitHub Releases. NPM07 publishes the root + platform packages to npm via npm trusted publishing (GitHub Actions OIDC) — no long-lived `NPM_TOKEN`. Provenance is enabled.

## Mini-Spec

objective:
- Add a GitHub Actions publish workflow that triggers on the release-please `release_created` output and uses npm trusted publishing (OIDC) to push the root wrapper + each platform package to the npm registry with provenance enabled. The platform packages publish first; the root wrapper publishes last so its `optionalDependencies` resolve.

non_goals:
- Do not store a long-lived `NPM_TOKEN`. If trusted publishing is impossible AND explicitly approved in this goal's final report, a fine-grained automation token is acceptable as a fallback; the fallback decision must name the reason.
- Do not publish to the `latest` dist-tag for the first beta tag. Use `--tag beta` (or `next`) until the operator promotes.
- Do not modify `crates/**`.

allowed_files_or_area:
- .github/workflows/** (publish workflow + reusable steps)
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM07-*.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**, config/**
- packages/*/package.json edits beyond release-please bumps
- any secret literal in YAML

contracts_or_interfaces:
- The npm packages are configured on npmjs.com to accept trusted publishing from this GitHub repo + workflow path. This is operator-side; the goal records the steps.
- Workflow runs only on `release_created == true` from release-please.
- `permissions: id-token: write` set at the job level (required for OIDC).
- `npm publish --provenance` for each package.
- Publish order: platform packages first (`@terminal-commander/linux-x64`, `@terminal-commander/linux-arm64`), then root wrapper.
- First publish uses `--tag beta`. Promotion to `latest` is an operator action recorded in a follow-up.
- No `secrets.NPM_TOKEN` referenced in the workflow unless the fallback exception is recorded.

invariants:
- No raw stream / no network listener / no shell expansion / no runtime change.
- The publish workflow does not run the test suite — it publishes artifacts already validated by NPM05 (regression gates run pre-publish).

acceptance_criteria:
- Publish workflow file exists with OIDC permissions + provenance flags.
- Operator instructions for configuring the trusted publisher on npmjs.com live in `docs/release/`.
- Dry-run path documented (`npm publish --dry-run --provenance`).
- No long-lived token committed; fallback path explicitly recorded if used.

evidence_required:
- Branch evidence.
- Workflow YAML path.
- Operator setup notes (npmjs.com side) — bounded, no secrets.
- Dry-run output captured at goal completion.

stop_conditions:
- Branch is not `main`.
- npm trusted publishing is technically unavailable for the chosen packages AND the operator does not approve a token fallback in this goal's final report.
- The publish workflow would require any `crates/**` change.

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
yamllint .github/workflows || true
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
# Bounded grep — no committed npm tokens:
rg "NPM_TOKEN|npm_[a-zA-Z0-9]{30,}" .github docs/release packages
```

## Task Prompt

Run NPM07 only on branch `main`. Trusted publishing only; no token fallback unless explicitly approved here.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM08).
