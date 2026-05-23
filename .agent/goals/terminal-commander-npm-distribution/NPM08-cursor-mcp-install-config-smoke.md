---
goal_id: NPM08
title: Cursor Mcp Install Config Smoke
chain_id: terminal-commander-npm-distribution
phase: Wave 5 - Provider smoke
status: "Completed"
depends_on: ["NPM07"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T05:00:00+00:00"
completed_at: "2026-05-23T06:00:00+00:00"
completion_commit: "6ab2343"
blocked_reason: ""
source_refs:
  - "Cursor MCP stdio config docs"
  - "docs/integrations/codex-cli.md (existing pattern from TC46)"
  - "docs/integrations/claude-code.md (existing pattern from TC46)"
  - "User directive 2026-05-23: docs/integrations/CURSOR_MCP-equivalent + examples/provider-harness/cursor/*.json (4 configs + README)"
risk_level: "low"
---

# NPM08 - Cursor Mcp Install Config Smoke

## Branch Guard

```text
main
```

## Mission Context

NPM07 produced a published npm package. NPM08 documents the Cursor MCP install path against the published package and runs the live Cursor smoke if the operator host has Cursor available. If not, this goal records `Not Run` with the exact blocker — never promoted to PASS.

### Prep amendment (NPM08, 2026-05-23)

User directive widened the deliverable set beyond the original goal-file mini-spec:

- Add `examples/provider-harness/cursor/` directory with concrete config examples (`mcp.global.linux-wsl.json`, `mcp.project.linux-wsl.json`, `mcp.global.native-linux.json`, `README.md`).
- Keep `docs/integrations/cursor.md` (per goal-file naming convention, matches sibling `claude-code.md`, `codex-cli.md`).
- Add `.cursor/mcp.json` to `forbidden_files` so no active repo-level Cursor config is committed.

No silent widening. Recorded here. Live Cursor smoke is still operator-driven; this amendment does not change that.

### NPM07 published-package state correction (NPM08, 2026-05-23)

The original NPM08 mission context said "NPM07 produced a published npm package." That is incomplete. As of NPM08 start:

- NPM07 added the publish workflow (output-gated, OIDC, no token).
- NPM07's live run successfully ran release-please with `releases_created='false'`; all three publish jobs were `skipped`.
- No `npm publish` has executed against the registry. The first live publish is **Pending operator npmjs.com setup + a release PR merge**, both recorded in `docs/release/npm-trusted-publishing-contract.md` §14.
- The Cursor docs in NPM08 must therefore distinguish (a) the future published-npm install path, (b) the current local-tarball smoke path, and (c) the Cursor manual config path.

## Mini-Spec

objective:
- Add `docs/integrations/cursor.md` documenting the Cursor MCP stdio install path against `npm install -g terminal-commander`. Cover both Linux/WSL2-native Cursor and Windows Cursor calling through `wsl`. Execute the live smoke if Cursor is available; otherwise record `Not Run` with the exact blocker.

non_goals:
- Do not modify `crates/**` or runtime behavior.
- Do not modify the published npm packages.
- Do not write a Cursor extension.
- Do not fake provider success.

allowed_files_or_area:
- docs/integrations/cursor.md (new)
- docs/integrations/README.md (status table refresh)
- docs/release/**
- examples/provider-harness/cursor/** (new — config examples + README per user directive 2026-05-23)
- scripts/smoke/** (only if a non-interactive Cursor harness exists; otherwise no script)
- .agent/goals/terminal-commander-npm-distribution/NPM08-*.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**, config/**
- packages/** (the npm package set is locked; NPM08 does not modify packages)
- .github/workflows/**
- .cursor/mcp.json (no active repo-level Cursor config committed)
- secrets / tokens / private paths anywhere

contracts_or_interfaces:
- The Cursor MCP stdio config uses `command: "terminal-commander-mcp"` (from npm global install) and env `TC_SOCKET=${TC_DATA}/terminal-commanderd.sock`.
- The Windows-Cursor + WSL path uses `command: "wsl"` + `args: ["-d", "<distro>", "bash", "-lc", "terminal-commander-mcp"]`.
- The Cursor smoke is defined as: operator opens a Cursor session, asks Cursor to list MCP tools, confirms the 29-tool TC45 catalogue, and asks Cursor to run `command_start_combed` → `bucket_wait` → `command_status` against `echo hello`. Transcript or screenshot evidence is attached.
- If Cursor is not installed on the verification host, the goal is `Completed` with the live smoke marked `Not Run` — exact blocker recorded. The doc still ships.

invariants:
- No secrets / tokens / private paths in the doc.
- No machine-specific absolute paths; use `${TC_DATA}` / `${XDG_STATE_HOME}` style placeholders.

acceptance_criteria:
- `docs/integrations/cursor.md` exists with both the native and WSL-bridge config examples.
- `docs/integrations/README.md` lists Cursor alongside Codex and Claude Code with honest status.
- Live Cursor smoke either: (a) executed with a captured transcript reference, or (b) marked `Not Run` with exact blocker.
- No runtime / npm package changes.

evidence_required:
- Branch evidence.
- File paths changed.
- Cursor smoke transcript reference OR `Not Run` blocker text.
- Beta posture in `RELEASE_CHECKLIST.md` updated only if at least one provider live smoke transcript exists.

stop_conditions:
- Branch is not `main`.
- The Cursor doc would require runtime / npm package changes.
- The host requires interactive auth / secrets to run the smoke and the operator did not pre-authorize the smoke run.

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
# Bounded grep — no secrets / private paths in the new doc:
rg "(/home/|/Users/|C:\\\\Users\\\\|sk-|ghp_|npm_)" docs/integrations/cursor.md || true
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM08 only on branch `main`. Document the Cursor MCP install path. If Cursor is unavailable, mark the live smoke `Not Run` with the exact blocker.

## Final Report Format

Objective / Changes / Files changed / Verification / Evidence / Commit / Known gaps / Next goal (NPM09).

## Final Report

Objective:
- Add Cursor MCP integration documentation + three copy-pasteable config examples (native Linux, project-scoped, Windows→WSL bridge). Run the Cursor live smoke if Cursor is available; otherwise honestly record `Not Run` with the exact blocker.

Changes (verified work commit `6ab2343`, prep amendment commit `3826535`):
- `docs/integrations/cursor.md` (new, 12 sections)
- `docs/integrations/README.md` (modified; Cursor stanza + source-status rows added)
- `examples/provider-harness/cursor/mcp.global.native-linux.json` (new)
- `examples/provider-harness/cursor/mcp.project.linux-wsl.json` (new; includes `TC_SOCKET` env-var placeholder)
- `examples/provider-harness/cursor/mcp.global.linux-wsl.json` (new; Windows Cursor → WSL bridge via `wsl.exe`)
- `examples/provider-harness/cursor/README.md` (new; boundary statement + per-file table + how-to-use)
- Goal file: prep amendment widened `allowed_files_or_area` for `examples/provider-harness/cursor/**`, added `.cursor/mcp.json` to `forbidden_files`, recorded NPM07 published-state correction.

No edits to: `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `.github/**`, `packages/**`.

Files changed:
- `docs/integrations/cursor.md` (new)
- `docs/integrations/README.md` (modified)
- `examples/provider-harness/cursor/README.md` (new)
- `examples/provider-harness/cursor/mcp.global.linux-wsl.json` (new)
- `examples/provider-harness/cursor/mcp.global.native-linux.json` (new)
- `examples/provider-harness/cursor/mcp.project.linux-wsl.json` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM08-*.md` (prep + status)

Docs / examples created:
- `docs/integrations/cursor.md` — 12 sections covering prerequisites, Cursor config locations, daemon start, three host topologies, tool discovery, minimal flow, smoke evidence rules, security, troubleshooting, source status.
- `examples/provider-harness/cursor/` directory:
  - `mcp.global.native-linux.json` — minimal `terminal-commander-mcp` stdio config.
  - `mcp.project.linux-wsl.json` — workspace-scoped variant with `TC_SOCKET=${TC_DATA}/terminal-commanderd.sock` env block (placeholder, no machine-specific path).
  - `mcp.global.linux-wsl.json` — Windows-Cursor → WSL bridge: `command: "wsl"` + `args: ["-d", "Ubuntu-24.04", "bash", "-lc", "terminal-commander-mcp"]`.
  - `README.md` — boundary statement (no shell bridge, no auto-run, no env secrets, no postinstall, no Mac/Win/musl claim) + per-file table + how-to-use.

Cursor provider smoke status: **Not Run**.

Exact blocker:
- Cursor 3.5.30 IS installed on the host (`cursor.exe` resolves from `C:\Users\<host>\AppData\Local\Programs\cursor\resources\app\bin\cursor`, equivalent path inside WSL via `/mnt/c/...`). The GUI app is reachable.
- However, Cursor's MCP discovery + tool invocation today has NO documented non-interactive entry point. There is no `cursor --list-mcp-tools` subcommand and no `cursor exec mcp ...` form. `cursor --help` lists only the VS Code-derived editor commands (`--diff`, `--merge`, `--goto`, etc.).
- The `cursor-agent` CLI (the headless variant that would enable scripted MCP smoke) is NOT installed on this host (`which cursor-agent` returns nothing on Windows and inside WSL).
- A live Cursor smoke therefore requires operator-driven steps that are not scriptable in this session:
  1. operator opens Cursor
  2. operator copies one of the example configs into `~/.cursor/mcp.json` (or `<workspace>/.cursor/mcp.json`)
  3. operator starts the daemon inside Linux/WSL
  4. operator asks the Cursor chat panel to list MCP tools (expect the 29 TC45 tools) and to call `health`
  5. operator captures transcript / screenshot evidence
- None of those steps complete inside this verified-work commit. NPM08 honestly records the smoke as **Not Run** — it is NOT promoted to PASS.

Direct local MCP / npm smoke (still GREEN, secondary evidence):
- `cargo nextest run --workspace`: 347/347 PASS, 0 skipped
- `cargo test --workspace`: 43 result lines, all OK
- `cargo fmt --all --check`: PASS
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS (clean)
- `bash scripts/smoke/verify-runtime-smoke.sh` (TC46): 8/8 PASS
- `bash scripts/smoke/verify-npm-local-install.sh` (NPM04): 12 PASS — end-to-end MCP stdio against npm-installed binaries
- `npm pack ./packages/terminal-commander --dry-run`: 7 files, 0.1.0-beta.1
- `npm pack ./packages/terminal-commander-linux-x64 --dry-run`: 5 files, 0.1.0-beta.1
- `npm pack ./packages/terminal-commander-linux-arm64 --dry-run`: 5 files, 0.1.0-beta.1

Confirmation — no active `.cursor/mcp.json` was committed:
- `git ls-files .cursor/` returns no files.
- `ls .cursor/` returns "No such file or directory".
- The example configs live at `examples/provider-harness/cursor/`. Operators copy intentionally; nothing in this repo is loaded by Cursor at clone time.
- `forbidden_files` in the NPM08 goal file now explicitly lists `.cursor/mcp.json`.

Confirmation — no secrets / private paths in examples or new docs:
- `rg '/home/[a-z]|/Users/[A-Za-z]|sk-[A-Za-z0-9]{10}|ghp_[A-Za-z0-9]{10}|npm_[A-Za-z0-9]{20}' docs/integrations/cursor.md docs/integrations/README.md examples/provider-harness/cursor/` → no matches.
- `grep -RE "C:\\\\Users\\\\<host>|<host>" docs/integrations/cursor.md docs/integrations/README.md examples/provider-harness/cursor/` → no matches.
- The examples use `${TC_DATA}/terminal-commanderd.sock` env-var placeholder + the generic distro name `Ubuntu-24.04`. No machine-specific absolute path, no GitHub-actor username, no email, no token.

Confirmation — no runtime / code / release / publish changes:
- `git diff --ignore-cr-at-eol --shortstat HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ packages/ .github/` → empty.
- No `package.json` `version` edits (all three remain `0.1.0-beta.1`).
- No `.github/workflows/release-please.yml` edits.
- No `.github/workflows/npm-binary-build.yml` edits.
- No new MCP tools, no new IPC surface, no shell expansion in any example.
- No `npm publish`, no `cargo publish`, no `_TC` token reference introduced.
- MCP guard greps unchanged: guard 1 doc/negative-assertion only; guard 2 no matches.

Beta-state mapping:
- TC48 `Conditional Go` preserved. The Cursor live transcript needed to lift to `Go` is still pending operator-driven smoke. NPM08 honestly does NOT advance the posture.

Commits:
- Prep amendment commit: `3826535`
- Verified work commit: `6ab2343`
- Goal status commit: this commit

Known gaps / blockers:
- **Cursor live provider smoke pending operator-driven steps.** Documented above. The same blocker pattern that TC46 / TC48 recorded for Codex CLI / Claude Code remains the dominant constraint for promoting beta to `Go`.
- **No `cursor-agent` headless CLI on this host.** If acquired in a future amendment, a non-interactive Cursor smoke script could land under `scripts/smoke/` (allowed by the goal-file mini-spec). NPM08 deliberately does NOT add a partial script; that would be future-proofing for a tool not present.
- **First live `npm install -g terminal-commander` install path still pending NPM07 live publish.** Operators using the local-tarball NPM04 path today get the same binaries; the docs distinguish these clearly.

Confirmation — NPM09 not started.

Next goal (per updated chain plan after the user's 2026-05-23 directive):
- **NPM08b-readme-project-normative-overhaul.md** (new goal inserted between NPM08 and NPM09; the chain-plan insertion commit follows this status commit).
- After NPM08b: NPM09-release-dry-run-and-beta-publish-review.md.
