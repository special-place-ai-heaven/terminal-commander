---
goal_id: NPM03
title: Wrapper Package And Platform Package Layout
chain_id: terminal-commander-npm-distribution
phase: Wave 2 - Layout
status: "Completed"
depends_on: ["NPM02"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T12:00:00+00:00"
completed_at: "2026-05-23T12:50:00+00:00"
completion_commit: "00f3ddb"
blocked_reason: ""
source_refs:
  - "NPM02 contract: docs/release/npm-packaging-contract.md"
  - "npm package.json docs (bin / optionalDependencies / files)"
risk_level: "medium"
---

# NPM03 - Wrapper Package And Platform Package Layout

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-npm-distribution/NPM03-wrapper-package-and-platform-package-layout.md

## Goal File Workflow

0. Branch Guard.
1. Mark `In progress`, set `started_at`.
2. Execute only the mini-spec.
3. On pass: commit verified work, mark Completed, set `completed_at` + `completion_commit`.
4. Goal status as separate commit.
5. On block: mark Blocked with `blocked_reason`.

## Branch Guard

```text
main
```

```bash
git branch --show-current
git status --short
```

## Mission Context

NPM02 locked the contract. NPM03 lays the package directories down: root wrapper + per-platform binary packages, with Node shims, `optionalDependencies`, and a resolver that picks the right platform binary at runtime. No GitHub Actions yet, no `release-please` yet.

## Mini-Spec

objective:
- Create the npm package directory layout under `packages/` (or the chosen root per NPM02) so a local `npm pack` of each package succeeds and the root wrapper's Node shims resolve a real binary from the matching platform package. No publishing.

non_goals:
- Do not write GitHub Actions.
- Do not write release-please config.
- Do not publish.
- Do not run a `cargo build` cross-compile yet (NPM05 handles the matrix).
- Do not modify `crates/**` or `Cargo.toml`.

allowed_files_or_area:
- packages/terminal-commander/** (root wrapper)
- packages/terminal-commander-linux-x64/** (platform package, scoped namespace name per NPM02)
- packages/terminal-commander-linux-arm64/** (platform package)
- packages/.gitignore / packages/README.md
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM03-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- scripts/**
- .github/workflows/**
- release-please-config.json
- .release-please-manifest.json
- secrets / tokens / private paths anywhere

contracts_or_interfaces:
- Root wrapper package matches NPM02 name + version.
- Root `bin` field exposes the three commands (`terminal-commanderd`, `terminal-commander-mcp`, `terminal-commander`) as Node shims.
- Each shim resolves `${platform_package}/bin/${command}` via the matching `optionalDependencies` entry, picking by `process.platform` + `process.arch`.
- If no matching platform package is installed, the shim exits non-zero with a clear error citing the supported targets — never prints a stack trace.
- Platform packages contain ONLY a `bin/` directory with the three binaries (placeholders allowed at NPM03; real binaries come from NPM05). Each platform `package.json` declares `"os"` and `"cpu"` so npm refuses to install on the wrong host.
- `files` field set so only `bin/` ships.
- License + repository fields populated.
- `engines` field declares the Node minimum used to run the shims.

invariants:
- The runtime chain invariants carry over.
- No `postinstall` script.
- No download from GitHub Releases at install time.

scope_substitution_policy:
- If NPM01 recommended a flat (non-scoped) platform package name, use that and record the substitution here.

acceptance_criteria:
- `npm pack` succeeds in `packages/terminal-commander/`, `packages/terminal-commander-linux-x64/`, and `packages/terminal-commander-linux-arm64/`.
- `npm pack --dry-run` for the root shows the three `bin` shims in the tarball file list.
- Root wrapper's Node shim can be unit-tested for the resolver fallback (no platform package installed → clear non-zero exit).
- `crates/**`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/**` untouched.

evidence_required:
- Branch evidence.
- File paths created.
- `npm pack` output for each package.
- Resolver fallback evidence (test or scripted run).

stop_conditions:
- Branch is not `main`.
- Layout would require runtime code changes.
- A platform package is needed for an unsupported target.

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
( cd packages/terminal-commander && npm pack --dry-run )
( cd packages/terminal-commander-linux-x64 && npm pack --dry-run )
( cd packages/terminal-commander-linux-arm64 && npm pack --dry-run )
rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp
rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src
```

## Task Prompt

Run NPM03 only on branch `main`. Create the package layout per the NPM02 contract.

## Final Report

Objective:
- Implement the NPM02 contract: root wrapper + two platform packages under `packages/`, JS bin shims with bounded resolver, npm pack dry-runs green.

Changes (verified work commit `00f3ddb`):
- 20 new files under `packages/`:
  - `packages/.gitignore`, `packages/README.md`.
  - `packages/terminal-commander/` — root wrapper: `package.json`, `README.md`, `LICENSE`, three JS shims under `bin/`, `lib/resolve-binary.js`, `test/resolve-binary.test.js`.
  - `packages/terminal-commander-linux-x64/` — `package.json`, `LICENSE`, three `bin/*.placeholder` files.
  - `packages/terminal-commander-linux-arm64/` — `package.json`, `LICENSE`, three `bin/*.placeholder` files.

No product-code changes. No CI / release-please / publish workflow added. No `crates/**`, `Cargo.toml`, `Cargo.lock`, `rules/**`, `config/**`, `scripts/**`, `.github/**`, `release-please-config.json`, `.release-please-manifest.json` touched.

Files changed:
- `packages/.gitignore` (new)
- `packages/README.md` (new)
- `packages/terminal-commander/package.json` (new)
- `packages/terminal-commander/README.md` (new)
- `packages/terminal-commander/LICENSE` (new)
- `packages/terminal-commander/bin/terminal-commanderd.js` (new)
- `packages/terminal-commander/bin/terminal-commander-mcp.js` (new)
- `packages/terminal-commander/bin/terminal-commander.js` (new)
- `packages/terminal-commander/lib/resolve-binary.js` (new)
- `packages/terminal-commander/test/resolve-binary.test.js` (new)
- `packages/terminal-commander-linux-x64/package.json` (new)
- `packages/terminal-commander-linux-x64/LICENSE` (new)
- `packages/terminal-commander-linux-x64/bin/terminal-commanderd.placeholder` (new)
- `packages/terminal-commander-linux-x64/bin/terminal-commander-mcp.placeholder` (new)
- `packages/terminal-commander-linux-x64/bin/terminal-commander.placeholder` (new)
- `packages/terminal-commander-linux-arm64/package.json` (new)
- `packages/terminal-commander-linux-arm64/LICENSE` (new)
- `packages/terminal-commander-linux-arm64/bin/terminal-commanderd.placeholder` (new)
- `packages/terminal-commander-linux-arm64/bin/terminal-commander-mcp.placeholder` (new)
- `packages/terminal-commander-linux-arm64/bin/terminal-commander.placeholder` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM03-*.md` (this file)

Verification (Linux WSL2):
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
- PASS: `git diff HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ scripts/ .github/ release-please-config.json .release-please-manifest.json` — empty
- PASS: `npm pack --dry-run` for `packages/terminal-commander` — 7 files, 7.3 kB tarball; contents: `LICENSE`, `README.md`, `bin/*.js` (×3), `lib/resolve-binary.js`, `package.json`.
- PASS: `npm pack --dry-run` for `packages/terminal-commander-linux-x64` — 5 files, 4.6 kB tarball; contents: `LICENSE`, `bin/*.placeholder` (×3), `package.json`.
- PASS: `npm pack --dry-run` for `packages/terminal-commander-linux-arm64` — 5 files, 4.6 kB tarball; contents: `LICENSE`, `bin/*.placeholder` (×3), `package.json`.
- PASS: `cd packages/terminal-commander && npm test` — **12/12 resolver tests pass** (`node --test test/*.test.js`).

Resolver tests (12 cases):
1. `ALLOWED_BINARIES` is exactly the three TC commands.
2. `SUPPORTED_TARGETS` is exactly linux-x64 + linux-arm64.
3. Invalid binary name returns `invalid_binary`.
4. linux/x64 + platform package installed → `ok` + correct absolute `binaryPath`.
5. linux/arm64 + platform package installed → `ok` + correct absolute `binaryPath`.
6. darwin/x64 → `unsupported_platform`; `formatResolveError` cites both supported targets.
7. win32/x64 → `unsupported_platform`.
8. linux/mips → `unsupported_platform`.
9. linux/x64 without platform package → `platform_package_missing` with the expected scoped name.
10. `formatResolveError` returns `null` on `ok` result.
11. `formatResolveError` message is single-line and bounded (<200 chars).
12. `SUPPORTED_TARGETS` is frozen and immutable.

Evidence — explicit acceptance against the NPM03 mini-spec:
- **npm package layout exists.** All 20 files placed per the NPM02 contract layout (§5).
- **`npm pack --dry-run` passes for all three packages.** Tarball contents listed above.
- **Tarball contents include only intended files.** Each package's `files` whitelist limits the tarball to `bin/`, `lib/` (root only), `package.json`, `LICENSE`, `README.md` (root only).
- **Resolver tests cover linux-x64, linux-arm64, unsupported platform, and missing package.** Cases 4, 5, 6+7+8, 9 above. Plus invalid-binary rejection (case 3) and bounded-error formatting (case 11).
- **Real Rust binaries not yet copied; deferred to NPM04.** Platform packages ship `*.placeholder` files at NPM03; NPM05 GitHub Actions overwrites them with real `cargo build --release --target <triple>` artifacts at release time. The repository's `packages/.gitignore` excludes the real binary filenames so no host-built binary ever lands in git accidentally.
- **No CI / release / publish files added.** Confirmed by forbidden-paths `git diff` returning empty.
- **NPM04 not started.** Goal file remains `Pending`.

Shim safety properties confirmed (each shim, by inspection of `bin/*.js`):
- Resolves via `lib/resolve-binary.js` only.
- `child_process.spawn(binaryPath, process.argv.slice(2), { stdio: 'inherit', shell: false })`. No `exec`, no shell.
- Forwards `argv` verbatim. No command-building from user strings.
- No file reads beyond `require.resolve()` of the platform package's `package.json`.
- No socket access.
- No environment-variable echo (the platform-mismatch error names only `process.platform` + `process.arch` + the supported targets; no env vars are printed).
- On non-OK resolver result, writes ONE bounded stderr line and exits `64` (matches TC40 unsupported-platform exit code).
- Mirrors the child's exit code (or signal) on parent exit.

Beta-state mapping:
- TC48 `Conditional Go` preserved. The npm layout shipping at NPM03 carries placeholders only; it does not yet make `npm install -g terminal-commander` produce working binaries on the host. NPM04 closes that gap with the local-install smoke.
- Provider live smoke pending: still NPM08 scope.
- Linux/WSL2 = the real platform story: the resolver rejects every other `process.platform` / `process.arch` combination with `unsupported_platform` and a bounded stderr line.
- TC46 + TC47 regressions stay green: this commit changes nothing under `crates/`.

Source-status:
- `packages/terminal-commander/**`: **live (NPM03)** — JS shims + resolver + tests.
- `packages/terminal-commander-linux-x64/**`, `packages/terminal-commander-linux-arm64/**`: **partial (NPM03)** — `package.json` + `LICENSE` live; binaries are placeholders pending NPM05 GitHub Actions.
- Terminal Commander runtime + MCP surface: **unchanged**.
- Every `crates/` source file: **unchanged**.

Commits:
- Verified work commit: `00f3ddb`
- Goal status commit: this commit

Known gaps / blockers:
- Real Rust binaries not yet shipped inside platform packages. Tracked by NPM04 (local install + smoke) and NPM05 (GitHub Actions build matrix). No new risk recorded; this is the expected NPM03 state.
- Operator preconditions from NPM02 (npmjs.com `@terminal-commander` org claim + trusted-publisher config) remain pending; both gate NPM07, not NPM04.

Next goal:
- NPM04-local-pack-and-global-install-smoke.md
