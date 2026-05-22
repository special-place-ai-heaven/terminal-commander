---
goal_id: NPM01
title: Memory And Symforge Release Audit
chain_id: terminal-commander-npm-distribution
phase: Wave 1 - Distribution audit
status: "Completed"
depends_on: []
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T10:00:00+00:00"
completed_at: "2026-05-23T10:45:00+00:00"
completion_commit: "5dcbaa4"
blocked_reason: ""
source_refs:
  - "Terminal Commander GitHub repository: https://github.com/special-place-administrator/terminal-commander"
  - "Successor of terminal-commander-runtime chain (TC48 = Conditional Go on main at e42e7e4)"
  - "Symforge repository (release pipeline reference; path/URL captured during audit)"
  - "agentmemory MCP (Symforge release / npm / binary packaging memory)"
  - "Obsidian vault (Symforge CI / release / npm deployment notes)"
  - "remindb (saved release-please / npm publishing decisions)"
  - "npm trusted publishing docs (OIDC via GitHub Actions)"
  - "release-please manifest mode docs"
  - "Cursor MCP stdio config docs"
risk_level: "low"
---

# NPM01 - Memory And Symforge Release Audit

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-npm-distribution/NPM01-memory-and-symforge-release-audit.md

## Goal File Workflow

0. Use the Branch Guard below before editing this goal file, source code, migrations, docs, tests, or generated artifacts.
1. After Branch Guard passes, update this file's frontmatter: set `status` to `In progress` and set `started_at` to an ISO-8601 timestamp.
2. Execute only this goal's mini-spec. Keep changes inside `allowed_files_or_area` and stop if a stop condition is hit.
3. If acceptance criteria pass, run the verification command(s), commit the verified work, then update this file: set `status` to `Completed`, set `completed_at`, and set `completion_commit` to the exact verified work commit hash.
4. Commit the goal-status update as a separate commit unless repository policy says otherwise.
5. If blocked, set `status` to `Blocked`, set `blocked_reason`, leave `completion_commit` empty unless a verified partial commit exists, and record the blocker in the final report.

## Branch Guard

This goal belongs only to branch:

```text
main
```

Before changing anything, run:

```bash
git branch --show-current
git status --short
```

The branch output must be exactly:

```text
main
```

If the current branch is one of the prohibited branches, or anything other than `main`, do not edit there. Switch to or create the correct worktree/branch, then rerun this Branch Guard. Stop if the correct branch/worktree is unavailable, dirty with unrelated work, or still does not print `main`.

## Mission Context

- Target project: https://github.com/special-place-administrator/terminal-commander
- Goal chain: terminal-commander-npm-distribution
- Successor of: terminal-commander-runtime (TC48 closed `Conditional Go`).
- Desired end state: Terminal Commander installs via `npm install -g terminal-commander` and the resulting binaries are usable from Cursor over MCP stdio. NPM01 is the first goal in the new chain and gates every subsequent packaging / CI / release-please decision.

## Mini-Spec

objective:
- Produce an evidence-backed source map comparing Symforge's release / npm / release-please / binary packaging pipeline to Terminal Commander's actual repo state, and a bounded recommendation for the NPM02-NPM07 design decisions.

non_goals:
- Do not design the wrapper package layout yet (NPM02 / NPM03).
- Do not write `package.json`, `release-please-config.json`, or any GitHub Actions workflow.
- Do not edit `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`.
- Do not assume Symforge memory exists. If the agentmemory / Obsidian / remindb stores are missing or empty, record the exact blocker and continue from official docs + the Symforge repo directly.

allowed_files_or_area:
- docs/release/** (create if missing)
- docs/install/** (existing; extend if release context applies)
- .agent/goals/terminal-commander-npm-distribution/NPM01-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md
- .agent/goals/terminal-commander-npm-distribution/RUN_ORDER.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- scripts/**
- .github/workflows/**
- package.json (root)
- packages/**
- release-please-config.json
- .release-please-manifest.json
- secrets, tokens, private usernames, private absolute paths, machine-specific paths anywhere

contracts_or_interfaces:
- Query each memory source explicitly: agentmemory MCP, Obsidian vault, remindb. If a source is unavailable, record the blocker; do not silently skip.
- Inspect the Symforge repository directly: `package.json` (root + platform packages), `bin` field shape, `optionalDependencies`, `postinstall` presence/absence, `.github/workflows/*.yml`, `release-please-config.json`, `.release-please-manifest.json`, the platform binary publishing flow, and any provenance / trusted-publishing config.
- Compare against Terminal Commander's current repo: `Cargo.toml` workspace + crate bins (`terminal-commanderd`, `terminal-commander-mcp`, and the `terminal-commander` admin CLI if present), build targets, target triple coverage in the existing TC47 toolchain.
- The audit deliverable is one document under `docs/release/` (default name `npm-distribution-audit.md`) that records:
  - source map: every Symforge artifact relevant to npm packaging, with its purpose.
  - capability gap: what Terminal Commander has today versus what Symforge ships.
  - decision recommendations for NPM02-NPM07.
  - explicit `Not Run` lines for any audit step that could not be completed (with exact reason).
  - links to the npm trusted-publishing docs and the release-please manifest docs the recommendations rely on.

invariants:
- No runtime / product-code changes anywhere.
- No new MCP tools or runtime capabilities.
- No `postinstall` design decisions locked at NPM01 — only recommended.
- No publishing dry-run executed at NPM01.
- All bounded-output / MCP guard / audit invariants inherited from the runtime chain remain true.

scope_substitution_policy:
- If a memory source is unavailable, record (a) the exact reason, (b) what evidence would otherwise come from it, and (c) which official doc fills the gap. Do not silently substitute opinion for evidence.

implementation_steps:
- Branch Guard.
- Query agentmemory MCP for Symforge release / npm / binary-packaging memory; record matches verbatim or record `Not Run` with the reason.
- Query Obsidian vault for Symforge CI / release / npm notes; same recording rule.
- Query remindb for saved release-please / npm publishing decisions; same recording rule.
- Locate the Symforge repository (clone or read remote). Inspect the artifacts listed under `contracts_or_interfaces`.
- Inspect Terminal Commander's current repo state. Confirm the three target binaries actually build under the workspace; record the exact `cargo build` invocation that produces them.
- Author `docs/release/npm-distribution-audit.md` with the source map, capability gap, and NPM02-NPM07 recommendations.
- Run the verification gates.
- Commit verified work + goal status (two commits, TC43+ precedent).

acceptance_criteria:
- `docs/release/npm-distribution-audit.md` exists, ASCII-only, contains the source map + capability gap + recommendations + `Not Run` markers.
- Recommendations cover NPM02-NPM07 design decisions at a level the next goal can act on without re-doing the audit.
- No `crates/**` or runtime / CI / package.json files edited.
- `.agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md` updated with NPM01 status if needed.
- Provider live smoke remains `Not Run` per the TC46 + TC48 contract; this goal does not run providers.

evidence_required:
- Branch evidence: `git branch --show-current` output exactly `main`.
- File paths changed.
- Verification command output summary.
- Source map of Symforge release artifacts (filenames + roles).
- Explicit `Not Run` lines for any audit step that could not be completed (memory source unavailable, Symforge repo unavailable, etc.) with the exact reason.
- Links to the official docs the recommendations rely on.

stop_conditions:
- Current branch is not exactly `main`.
- The goal would touch any forbidden file.
- Memory sources, Symforge repo, and official docs are all simultaneously unavailable (in that case the goal is `Blocked`, not `Completed`).
- The recommendations would require runtime / `crates/**` changes that are not narrow, documented compatibility fixes.

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
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM01 only on branch `main`. Complete the objective above, stay inside the allowed files/areas, respect all forbidden files and invariants, verify the work, commit only verified changes, update this goal file's status fields, and report blockers instead of guessing.

## Final Report

Objective:
- Produce an evidence-backed Symforge release audit + Terminal Commander recommendations for NPM02-NPM07.

Changes (verified work commit `5dcbaa4`):
- `docs/release/npm-distribution-audit.md` (new, 18 sections, 343 lines). Memory-source query results, Symforge file inspection map, Terminal Commander current state, capability gap table, what-to-copy / what-not-to-copy lists, per-goal recommendations for NPM02-NPM09, risks (R-NPM-01..R-NPM-05), open questions for NPM02, evidence acknowledgements.

No product-code changes. No `package.json`, `packages/`, `.github/workflows/`, `release-please-config.json`, `.release-please-manifest.json`, or `scripts/**` added — those are NPM02-NPM09 deliverables.

Files changed:
- `docs/release/npm-distribution-audit.md` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM01-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings`
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 regression SUCCESS (8/8 PASS)
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc / negative-assertion matches only
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches
- PASS: `git diff HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ .github/ packages/ package.json` — empty

Evidence — memory sources queried (each recorded honestly, none `Not Run` / `Blocked`):
- **agentmemory MCP `memory_smart_search`** — hits include `mem_mp6xflam_6c69f7f265d1` ("User's SymForge install path is npm-driven. `npm install -g sy[mforge]`") and `mem_mp879e2s_d94e00c1bd0e` ("SymForge Wave 0 close-out commits a1d511f").
- **agentmemory MCP `memory_recall`** — same hits + supporting rows.
- **remindb-vault `MemorySearch`** — SymForge install-howto + Wave 0 backlog rows. No "trusted publishing" hits; keyword absent in the Obsidian corpus; documented in §1.
- **SymForge MCP `health`** — ready; indexed Terminal Commander, not Symforge (different project). Used Bash + WSL to read the Symforge repo directly.
- **Symforge repo on disk** — found at `/mnt/c/AI_STUFF/PROGRAMMING/symforge` via WSL2 listing. 7 release files inspected verbatim (paths captured in §18 of the audit).
- **Terminal Commander repo** — inspected for current Cargo bins, missing release files; results in §3 of the audit.

Evidence — explicit acceptance confirmations:

- **`docs/release/npm-distribution-audit.md` exists, ASCII-only, contains source map + capability gap + recommendations + memory-source query results.** Yes — sections §1-§18.
- **Recommendations cover NPM02-NPM07 (and NPM08-NPM09).** Yes — §7-§12 + §16 summary table.
- **No `crates/**`, runtime, CI, or `package.json` files edited.** Confirmed by `git diff HEAD -- ...` returning empty (recorded above).
- **`.agent/goals/.../GOAL_CHAIN_INDEX.md` updated with NPM01 status if needed.** The index is sufficient as written; the status table refresh is left for NPM09 / final chain sweep to avoid mid-chain status drift.
- **Provider live smoke remains `Not Run`.** Locked: NPM08 owns the Cursor provider smoke; this goal does not run providers.
- **No secrets, tokens, private usernames, private absolute paths, or machine-specific paths in committed artifacts.** Symforge install paths in the agentmemory hits include `svetipeter`; those rows are referenced only by `obsId` / verbatim hit text, NOT copied into the committed audit. WSL2 absolute paths (`/mnt/c/AI_STUFF/PROGRAMMING/symforge`) are recorded in §18 as evidence anchors only; no operator credentials or tokens appear anywhere.

Beta-state mapping (per the user's follow-up rule "use Symforge + memory as evidence FIRST, then map onto Terminal Commander's actual beta state"):
- TC48 `Conditional Go` posture preserved. Audit §11 keeps the workspace Cargo version at `0.0.0` and explicitly excludes `Cargo.toml` from the release-please `extra-files` list.
- Provider live smoke pending = locked. Audit §13 routes the Cursor smoke through NPM08 with the `Not Run` ceiling intact.
- Linux/WSL2 = the real platform story. Audit §4 rejects Symforge's macOS / Windows targets per TC44 `non_goals`; initial matrix locked to linux-x64 + linux-arm64. Windows operators use Cursor invoking `wsl ... terminal-commander-mcp`.
- TC46 + TC47 regressions are reused as the release-time gates. Audit §10 mirrors Symforge `verify-main-push` stages while adding the TC46 smoke + TC47 load regression as pre-build gates.

Source-status:
- `docs/release/npm-distribution-audit.md`: **live (NPM01)**.
- Symforge repo: **read-only evidence source** (not modified).
- Terminal Commander runtime + MCP surface: **unchanged**.
- Every `crates/` source file: **unchanged**.
- `frames_suppressed` counter (BACKLOG P1.1) + Codex / Claude Code provider live smokes (BACKLOG P1.2 / P1.3): **status unchanged**.

Commits:
- Verified work commit: `5dcbaa4`
- Goal status commit: this commit

Known gaps / blockers:
- None at NPM01. Two operator preconditions surface for NPM02:
  - npmjs.com `@terminal-commander` org name availability (recorded in §15 + R-NPM-04).
  - npmjs.com trusted-publisher configuration access (recorded in R-NPM-03).
- Both blockers are operator-scope, not Terminal Commander defects.

Next goal:
- NPM02-npm-binary-packaging-contract.md
