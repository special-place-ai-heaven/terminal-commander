---
goal_id: WWS08
title: Readme And Release Contract Update
chain_id: terminal-commander-windows-wsl-bridge
phase: Wave 4 - Docs
status: "Completed"
depends_on: ["WWS07"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T12:00:00+00:00"
started_at: "2026-05-23T19:30:00+00:00"
completed_at: "2026-05-23T20:40:00+00:00"
completion_commit: "12b47ce"
blocked_reason: ""
source_refs:
  - "WWS01..WWS07 outcomes"
  - "README.md (NPM08b canonical public README baseline)"
  - "docs/release/npm-binary-packaging-contract.md (NPM02 + WWS02 §13b amendment)"
  - "docs/release/windows-wsl-bridge-contract.md (commit 6220eb2 + WWS02..WWS07 amendments)"
  - "docs/integrations/cursor.md (NPM08 + WWS04..WWS07 amendments)"
  - "scripts/smoke/verify-windows-bridge-smoke.ps1 (WWS07)"
risk_level: "low"
---

# WWS08 - Readme And Release Contract Update

## Branch Guard

```text
main
```

## Mission Context

WWS08 closes the documentation loop. The chain has implemented (WWS02-WWS06) and verified (WWS07) the Windows + WSL bridge. WWS08 updates the canonical public-facing docs so the README, package README, Cursor walk-through, examples, release contract, release checklist, and backlog all reflect the actual landed surface: bridge-required resolver branch, WSL discovery + doctor helpers, terminal-commander-mcp Windows bridge shim, Cursor config writer, setup/doctor/pair CLI, and the WWS07 PowerShell smoke. The beta posture stays `Conditional Go` because the Cursor provider GUI smoke remains Not Run (no operator transcript captured at WWS07).

This goal is documentation-only. NO `package.json` edit. NO workflow edit. NO `crates/**` edit. NO new MCP tool. NO publish. NO `lib/wsl/**` / `lib/cursor/**` / `lib/cli/**` / `bin/*` edit (the chain's source code stays byte-identical at the WWS07 baseline).

## Prep amendment (2026-05-23, before WWS08 implementation)

This goal file was prep-amended once before WWS08 implementation started. Three scope adjustments were locked:

1. **Allowed-files widening**. The original allowed set covered `README.md`, `docs/release/**`, `docs/install/**`, `docs/integrations/**` (cross-link only), `BACKLOG.md`, `RELEASE_CHECKLIST.md`, and the goal file. The directive widened to also touch:
   - `packages/terminal-commander/README.md` (package-level README WWS chain status; the package README is already mostly current through WWS06 / WWS07, so the WWS08 edit is light)
   - `examples/provider-harness/cursor/README.md` (already mostly current; one-line WWS state confirmation)
   - `RISK_REGISTER.md` (Windows/WSL bridge entry)
   - `ROADMAP.md` (acknowledge bridge chain landed; remaining items)
   - `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md` (status alignment)
   - `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md` (status alignment)

2. **No version bump. No workflow change.** WWS08 is strictly documentation. `packages/*/package.json` is byte-identical. `.github/**` is byte-identical. `release-please.yml`, `npm-binary-build.yml`, `npm-bootstrap-publish.yml`, `npm-trusted-publishing` workflow surfaces are all untouched. `npm-bootstrap-publish.yml` stays committed-but-undispatched. Token surface remains zero.

3. **No promotion of Not Run to PASS.** The Cursor provider GUI smoke stays `Not Run` in every doc that mentions it; the WWS07 PowerShell bridge MCP round-trip stays Not Run honestly because the WSL-side runtime is missing (npm package still E404). Beta posture remains `Conditional Go`.

4. **Doctrine carry-forward**. `CAP01-capability-registry-contract.md` remains a future goal. NOT started.

## Mini-Spec

objective:
- Update `README.md`:
  - Feature matrix: add 6 new rows for the WWS chain (root `os` widened; WSL discovery/doctor; bridge shim; Cursor config writer; setup/doctor/pair CLI; WWS07 PowerShell smoke). Mark each row `live (WWS0X)` with the verified-work commit.
  - Install section: keep the existing Linux/WSL2 + future-published-path + local-tarball + cargo-built paths AND add a new "Windows host (bridge / setup surface)" subsection explaining the WWS chain installs (terminal-commander setup cursor-wsl etc.) ONCE the npm package is published. Until publish, point operators at the local-tarball install + manual `wsl.exe` Cursor config in §6.
  - Cursor integration section: keep both existing JSON examples; add a one-line mention that `terminal-commander setup cursor-wsl` (WWS06) auto-generates the bridge-form stanza on Windows once the package is installed; add a one-line mention of `scripts/smoke/verify-windows-bridge-smoke.ps1` (WWS07).
  - Current beta status: keep `Conditional Go`. Add WWS state table (WWS01..WWS09 with status + completion commit OR Pending). Reaffirm Cursor provider GUI smoke = Not Run.
  - Repository layout: no change required (the chain already added `.agent/goals/terminal-commander-windows-wsl-bridge/`).
- Update `docs/release/windows-wsl-bridge-contract.md`:
  - §18 add WWS08 entry (landed; docs-only).
  - §15 reaffirm D-01..D-15 unchanged.
- Update `docs/release/npm-binary-packaging-contract.md`:
  - §13b (existing WWS02 amendment) cross-link the WWS chain landed status.
  - Reaffirm: platform packages byte-identical (Linux-only); root `os: ["linux", "win32"]`; optionalDependencies pinning unchanged.
- Update `docs/release/npm-distribution-final-report.md` (NPM10 final report) with a short "WWS chain follow-up" subsection acknowledging WWS02..WWS07 landed and `npm-bootstrap-publish.yml` remains undispatched.
- Update `docs/integrations/cursor.md`: minor; §11d WWS07 already complete from the previous commit. WWS08 only adjusts the §12 source-status table to reflect the WWS chain landed status.
- Update `examples/provider-harness/cursor/README.md`: one-line WWS state confirmation in source-status.
- Update `RELEASE_CHECKLIST.md`: new "Windows + WSL bridge chain (WWS01..WWS07)" subsection. Do NOT modify the publish gates locked at NPM09; only ADD the bridge-chain readiness rows.
- Update `BACKLOG.md`: add explicit follow-up entries for the chain's known gaps:
  - `terminal-commander setup cursor-wsl --uninstall` (D-14 rollback, partial)
  - Multi-distro interactive prompt (D-07 future enhancement)
  - Full WSL-side `pair accept` handshake (deferred at WWS06)
  - CAP01 capability registry (future doctrine carry-forward; NOT started)
  - `npm-bootstrap-publish.yml` disable / rotate follow-up after first publish (BACKLOG P1.5b inherited from NPM10)
  - Cursor provider GUI smoke transcript (operator-driven; gates `Go` promotion)
- Update `RISK_REGISTER.md`: add R-WWS-* entries to the table (mirror the WWS01 contract §15 R-WWS-01..R-WWS-10 risks for the public risk register).
- Update `ROADMAP.md`: acknowledge bridge chain landed; remaining items (NPM07 first publish + WWS09 readiness review + future enhancements).
- Update WWS08 frontmatter to Completed in the status commit.
- Update `GOAL_CHAIN_INDEX.md` + `RUN_ORDER.md`.

non_goals:
- NO `package.json` edit. NO version bump.
- NO workflow edit (`.github/**`).
- NO `crates/**` change.
- NO `packages/*/lib/**` / `packages/*/bin/**` / `packages/*/test/**` change (chain source byte-identical).
- NO `scripts/**` change (WWS07 smoke script byte-identical).
- NO new MCP tool. NO daemon change.
- NO publish. NO workflow dispatch.
- NO promotion of Not Run to PASS.
- NO secret / token / private path written into any doc.
- NO CAP01 implementation.

allowed_files_or_area:
- `README.md`
- `packages/terminal-commander/README.md`
- `docs/release/**`
- `docs/install/**`
- `docs/integrations/**`
- `examples/provider-harness/cursor/README.md`
- `BACKLOG.md`
- `RELEASE_CHECKLIST.md`
- `RISK_REGISTER.md`
- `ROADMAP.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/WWS08-*.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/GOAL_CHAIN_INDEX.md`
- `.agent/goals/terminal-commander-windows-wsl-bridge/RUN_ORDER.md`

forbidden_files:
- `crates/**`
- `Cargo.toml`
- `Cargo.lock`
- `rules/**`
- `config/**`
- `scripts/**`
- `.github/**`
- `packages/*/package.json`
- `packages/*/lib/**`
- `packages/*/bin/**`
- `packages/*/test/**`
- `packages/terminal-commander-linux-x64/**`
- `packages/terminal-commander-linux-arm64/**`
- `examples/provider-harness/cursor/*.json`
- `.cursor/mcp.json` anywhere in the repo
- secrets / tokens / private paths

contracts_or_interfaces:
- Every WWS state claim in the README is traceable to a goal completion commit (linked in the new WWS state table).
- The feature matrix's new WWS rows use the same `live (XYZ)` style as the existing NPM / TC rows.
- The Install section's Windows path is described as future-published only; the local-tarball pre-publish path remains the working path until the first npm publish occurs.
- The Current beta status section keeps `Conditional Go` verbatim AND honestly records the WWS07 Cursor provider GUI smoke as `Not Run` with the exact reason from the WWS07 final report.
- No `live` claim depends on a future event.

invariants:
- Diffstat ONLY touches the allowed-files set above.
- Forbidden-paths diff (`--ignore-cr-at-eol`) empty.
- MCP guard greps clean.
- Secret-leak grep clean.
- No `.cursor/mcp.json` anywhere in the repo.
- TC48 + NPM10 `Conditional Go` posture preserved.
- npm-bootstrap-publish workflow stays committed-but-undispatched.

acceptance_criteria:
- Every relative README link resolves at the verified-work commit.
- Beta status section explicitly records the WWS07 Cursor smoke status (Not Run, with reason).
- BACKLOG records the chain's deferred items.
- RISK_REGISTER records the R-WWS-* entries.
- ROADMAP acknowledges the chain landed AND lists the remaining gaps.
- `npm pack --dry-run` clean for all three packages; file counts unchanged from WWS07 (root 23 / linux-x64 5 / linux-arm64 5).
- `crates/**` untouched; nextest 347/347 PASS.
- runtime-smoke + npm-local-install PASS unchanged.
- MCP guard greps + secret-leak grep clean.
- `npm view` E404 for all three names.
- `.github/**` diff empty.
- `npm-bootstrap-publish` NOT dispatched.

evidence_required:
- Branch evidence.
- File paths.
- Link-resolution sweep result (every new relative link in README/docs resolves).
- Diff statistic.
- Verification gauntlet output.

stop_conditions:
- Branch is not `main`.
- A README claim would require a runtime / package / workflow / version change to be honest.
- Origin/main has moved during implementation.

verification_command:
```bash
git branch --show-current
git status --short
git diff --check
test ! -e .cursor/mcp.json

( cd packages/terminal-commander && npm test )

npm pack ./packages/terminal-commander --dry-run
npm pack ./packages/terminal-commander-linux-x64 --dry-run
npm pack ./packages/terminal-commander-linux-arm64 --dry-run

python3 - <<'PY'
import json
from pathlib import Path
root = json.loads(Path("packages/terminal-commander/package.json").read_text())
x64 = json.loads(Path("packages/terminal-commander-linux-x64/package.json").read_text())
arm64 = json.loads(Path("packages/terminal-commander-linux-arm64/package.json").read_text())
assert root["os"] == ["linux", "win32"], root["os"]
assert x64["os"] == ["linux"], x64["os"]
assert arm64["os"] == ["linux"], arm64["os"]
assert x64["cpu"] == ["x64"], x64["cpu"]
assert arm64["cpu"] == ["arm64"], arm64["cpu"]
versions = {root["version"], x64["version"], arm64["version"]}
assert len(versions) == 1, versions
deps = root.get("optionalDependencies", {})
assert deps.get("@terminal-commander/linux-x64") == root["version"], deps
assert deps.get("@terminal-commander/linux-arm64") == root["version"], deps
print("package-contract-ok", root["version"])
PY

CARGO_TARGET_DIR=target-wsl cargo fmt --all --check
CARGO_TARGET_DIR=target-wsl cargo clippy --workspace --all-targets -- -D warnings
CARGO_TARGET_DIR=target-wsl cargo nextest run --workspace
bash scripts/smoke/verify-runtime-smoke.sh
bash scripts/smoke/verify-npm-local-install.sh

rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
rg "NPM_TOKEN_TC|CARGO_REGISTRY_TOKEN_TC|RELEASE_PLEASE_TOKEN_TC|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}" README.md docs examples packages .agent/goals scripts || true
rg "npm install -g terminal-commander" README.md docs examples packages || true
rg "Cursor provider.*PASS|provider.*PASS|npm package.*published|Windows-native runtime|macOS support" README.md docs examples packages || true

npm view terminal-commander version || true
npm view @terminal-commander/linux-x64 version || true
npm view @terminal-commander/linux-arm64 version || true
```

## Task Prompt

Run WWS08 only on branch `main`. Documentation-only. Pull WWS state verbatim from the chain's status commits; do not invent. No version bump. No workflow change. No code change. The Cursor provider GUI smoke stays `Not Run`.

## Final Report Format

- Pushed WWS07 range (confirmation only)
- Pushed WWS08 prep-amendment range (this file)
- Files changed by WWS08 (verified-work commit)
- README / install wording summary
- Cursor docs wording summary
- Release-contract wording summary
- Current beta / publish posture (verbatim from WWS07)
- Explicit Not Run items preserved (Cursor provider GUI smoke; WSL-side runtime; first npm publish)
- Safety wording evidence (no credentials / no sudo / no publish / no workflow dispatch in any doc)
- Verification summary
- Confirmation `npm-bootstrap-publish` was not dispatched
- Confirmation no npm publish occurred
- Confirmation WWS09 not started
- Local git state (HEAD, ahead/behind, branch)
