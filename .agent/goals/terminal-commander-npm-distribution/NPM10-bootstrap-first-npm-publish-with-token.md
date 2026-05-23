---
goal_id: NPM10
title: Bootstrap First Npm Publish With Token
chain_id: terminal-commander-npm-distribution
phase: Wave 6 - Bootstrap exception
status: "In progress"
depends_on: ["NPM09"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T10:30:00+00:00"
started_at: "2026-05-23T11:00:00+00:00"
completed_at: ""
completion_commit: ""
blocked_reason: ""
source_refs:
  - "NPM07 docs/release/npm-trusted-publishing-contract.md (OIDC-only contract this goal explicitly exempts)"
  - "NPM09 docs/release/npm-distribution-final-report.md (E404 / unpublished state for all three names)"
  - "User directive 2026-05-23: one-time NPM_TOKEN_TC bootstrap because trusted-publisher pages may not be configurable until the package pages exist on npmjs.com"
risk_level: "high"
---

# NPM10 - Bootstrap First Npm Publish With Token

## Branch Guard

```text
main
```

## Mission Context

NPM07 locked the publish path to npm trusted publishing (OIDC + provenance) and explicitly rejected the long-lived `NPM_TOKEN_TC` fallback. NPM09 closed the chain with all three packages at E404 / unpublished, gated by two operator preconditions on npmjs.com: claim `@terminal-commander` org + configure trusted publisher for each package page. The operator has identified that npmjs.com may not allow trusted-publisher configuration until the package PAGE exists, i.e. until at least one publish has already landed against the name — the classic bootstrap chicken-and-egg.

NPM10 is the **one-time, explicit policy exception** that uses the existing `NPM_TOKEN_TC` repository secret to perform the very first beta publish for the three packages so that the npmjs.com pages exist and trusted-publisher can subsequently be configured. After NPM10 succeeds, the operator MUST configure trusted publishing and the bootstrap workflow MUST be disabled (or removed in a follow-up goal). The OIDC-only contract resumes immediately for every subsequent publish.

NPM10 does NOT relax any other invariant:
- No crates.io / cargo publish.
- No `CARGO_REGISTRY_TOKEN_TC` use.
- No `RELEASE_PLEASE_TOKEN_TC` use.
- No runtime / MCP / package-architecture change.
- No postinstall downloader.
- No macOS / Windows-native or musl package.

## Mini-Spec

objective:
- Add `.github/workflows/npm-bootstrap-publish.yml` — a `workflow_dispatch`-only guarded workflow that uses `secrets.NPM_TOKEN_TC` to publish the three Terminal Commander npm packages for the FIRST time at `0.1.0-beta.1`. Two operator-supplied workflow inputs gate the real publish: `dry_run` (default `true`) and `confirm_publish` (exact string `publish-terminal-commander-beta` required). The default execution is a `npm publish --dry-run`; the real publish requires both gates flipped. Publish order: platform packages first, root last.

non_goals:
- Do not modify `crates/**` or runtime behavior.
- Do not modify `.github/workflows/release-please.yml` (NPM06 + NPM07).
- Do not modify `.github/workflows/npm-binary-build.yml` (NPM05).
- Do not modify any `package.json` version field.
- Do not publish to crates.io.
- Do not use `CARGO_REGISTRY_TOKEN_TC` or `RELEASE_PLEASE_TOKEN_TC`.
- Do not RUN the workflow at NPM10 implementation time — only create + commit. The dispatch is an explicit operator action.

allowed_files_or_area:
- `.github/workflows/npm-bootstrap-publish.yml` (new)
- `docs/release/npm-bootstrap-first-publish.md` (new)
- `BACKLOG.md` (note the bootstrap exception + the follow-up to disable the workflow + the still-pending Cursor/Codex/Claude provider smokes)
- `RELEASE_CHECKLIST.md` (optional: amend the "npm distribution gate" section to acknowledge the bootstrap path exists as a one-time fallback)
- `.agent/goals/terminal-commander-npm-distribution/NPM10-*.md`
- `.agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md` (add NPM10 row)
- `.agent/goals/terminal-commander-npm-distribution/RUN_ORDER.md` (add NPM10 row)

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `packages/*/package.json` (no version edits)
- `.github/release-please-config.json` / `.github/.release-please-manifest.json`
- `.github/workflows/release-please.yml`
- `.github/workflows/npm-binary-build.yml`
- secrets / tokens / private usernames / private absolute paths in any committed artifact

contracts_or_interfaces:
- Trigger: `workflow_dispatch` only. **No** `push`, `pull_request`, `release`, `schedule`, or `workflow_run` triggers.
- Inputs:
  - `dry_run` (boolean, default `true`)
  - `confirm_publish` (string, default empty; required exact value `publish-terminal-commander-beta` for a real publish)
- Default execution (`dry_run=true` OR `confirm_publish != "publish-terminal-commander-beta"`): `npm publish --dry-run` for every package. No real network publish.
- Real publish (`dry_run=false` AND `confirm_publish == "publish-terminal-commander-beta"`): `npm publish --tag beta` (with `--access public` on the two scoped platform packages).
- Auth: `NODE_AUTH_TOKEN` env mapped from `secrets.NPM_TOKEN_TC` via `actions/setup-node@v4` + `registry-url: "https://registry.npmjs.org"`. **No other secret referenced.**
- Permissions: `contents: read` only. **No** `id-token: write` (no OIDC claim made on this workflow). **No** `packages: write`.
- Provenance: NOT enabled at NPM10 (token publish does not satisfy npm provenance requirements; faking the flag with a token would produce a misleading attestation). Provenance returns at NPM07 + post-trusted-publisher-config publishes.
- Publish order (locked):
  1. `@terminal-commander/linux-x64`
  2. `@terminal-commander/linux-arm64` (parallel job with #1; both must succeed)
  3. `terminal-commander` (root, `needs:` both platform jobs)
- Pre-publish guards (every job, before any `npm publish`):
  - Toolchain pinned `1.95.0`.
  - Native arm64 build on `ubuntu-24.04-arm` (no QEMU).
  - Real binary `--help` returns 0.
  - `*.placeholder` files removed from `bin/` before pack.
  - `package.json` `version` equals the manifest-declared `0.1.0-beta.1`.
  - `optionalDependencies` exact-pin (root job only).
  - Resolver unit tests (`npm test`, 12 cases; root job only).
  - **Pre-publish E404 check**: `npm view <name> version` MUST return `E404` for every package name being published. If any name already exists, the job fails LOUDLY before any `npm publish`; this guards against accidental re-publish + against silent supply-chain takeover.
- Print package names + versions before each publish step. **Never print `NPM_TOKEN_TC`** or any other secret. **Never echo `npm config` auth output.**
- Workflow header carries a permanent warning: "ONE-TIME BOOTSTRAP. Disable or remove after trusted-publisher setup completes."

invariants:
- The trusted-publishing workflow at `.github/workflows/release-please.yml` remains **unchanged** at NPM10. NPM10 does not edit NPM07's workflow file. NPM10 adds a sibling bootstrap workflow that is `workflow_dispatch`-only.
- The bootstrap workflow does NOT run on push. It cannot accidentally fire on the NPM10 commit landing.
- No `cargo publish`. No crates.io reference.
- No postinstall downloader added to any `package.json`.
- No edit to `crates/`, `Cargo.toml`, `Cargo.lock`, `rules/`, `config/`, `scripts/`, `packages/*/package.json`.
- TC48 beta posture: still `Conditional Go`. NPM10's existence does not promote the posture; only the real publish + Cursor live smoke do, and even after a successful first publish promotion requires NPM07's trusted-publisher path replacing the token path.

acceptance_criteria:
- `.github/workflows/npm-bootstrap-publish.yml` exists with the exact shape above (workflow_dispatch only; two-gate confirm; default dry-run; OIDC permission scope `contents: read` only; no `id-token: write`; no `packages: write`; no other secret references).
- `docs/release/npm-bootstrap-first-publish.md` exists and documents the policy exception, the two-gate dispatch, the publish order, the post-publish required operator steps (configure trusted publisher + disable the bootstrap workflow), and the rollback story.
- `BACKLOG.md` notes:
  - the one-time NPM_TOKEN_TC exception
  - the follow-up to disable / remove the bootstrap workflow after trusted publishing is configured
- All Rust + smoke gates remain green.
- `npm pack --dry-run` clean for all three packages.
- Version sync intact: `0.1.0-beta.1` across three `package.json` + the three manifest entries.
- MCP guard greps unchanged.
- Forbidden-paths diff (`--ignore-cr-at-eol`) empty over the forbidden_files list.
- `rg "CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|cargo publish|crates\.io" .github docs BACKLOG.md RELEASE_CHECKLIST.md` returns negative-documentation matches only (no active workflow surface).

evidence_required:
- Branch evidence (`main`, clean tree pre + post status commit).
- File paths created / modified.
- Workflow YAML structure inspection (jobs, triggers, permissions, secrets referenced).
- Per-gate behavior described in the goal file.
- Local verification table from the verification commands below.

stop_conditions:
- Branch is not `main`.
- A package name has been claimed on npmjs.com between NPM09 verification and NPM10 implementation (the bootstrap is no longer needed; revert to NPM07's OIDC path).
- NPM07's workflow file or any of the three `package.json` files would have to be modified.
- The bootstrap workflow would require any non-`workflow_dispatch` trigger to operate as designed.

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
npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run
python3 - <<'PY'
import json
from pathlib import Path
pkgs = [
    Path("packages/terminal-commander/package.json"),
    Path("packages/terminal-commander-linux-x64/package.json"),
    Path("packages/terminal-commander-linux-arm64/package.json"),
]
data = [json.loads(p.read_text()) for p in pkgs]
versions = {d["version"] for d in data}
assert len(versions) == 1, versions
root = data[0]
deps = root.get("optionalDependencies", {})
assert deps.get("@terminal-commander/linux-x64") == root["version"], deps
assert deps.get("@terminal-commander/linux-arm64") == root["version"], deps
print("versions-ok", root["version"])
PY
python3 - <<'PY'
from pathlib import Path
import yaml
for path in [
    ".github/workflows/npm-bootstrap-publish.yml",
    ".github/workflows/release-please.yml",
    ".github/workflows/npm-binary-build.yml",
]:
    yaml.safe_load(Path(path).read_text())
print("yaml-ok")
PY
rg "CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|cargo publish|crates\.io" .github docs BACKLOG.md RELEASE_CHECKLIST.md || true
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM10 only on branch `main`. Create the guarded `workflow_dispatch`-only bootstrap workflow + the policy-exception doc + the BACKLOG follow-up. **Do not run the workflow.** Do not publish. Do not enable provenance with a token. Wait for explicit operator dispatch approval before the bootstrap publish is fired.

## Final Report Format

Objective / Changes / Files created / Workflow shape / Two-gate behavior / Verification / Evidence / Commit / Operator next steps (dispatch + follow-up disable) / Known gaps / Next goal (NPM11 follow-up to disable the token workflow + verify trusted publisher).
