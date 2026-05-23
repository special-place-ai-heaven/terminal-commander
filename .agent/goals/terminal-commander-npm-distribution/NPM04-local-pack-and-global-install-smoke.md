---
goal_id: NPM04
title: Local Pack And Global Install Smoke
chain_id: terminal-commander-npm-distribution
phase: Wave 2 - Layout
status: "Completed"
depends_on: ["NPM03"]
target_branch: "main"
prohibited_branches: ["master", "feature/terminal-commander-mvp", "feature/terminal-commander-runtime", "production", "release"]
worktree_hint: ""
created_at: "2026-05-23T00:00:00+00:00"
started_at: "2026-05-23T13:00:00+00:00"
completed_at: "2026-05-23T13:50:00+00:00"
completion_commit: "970db2c"
blocked_reason: ""
source_refs:
  - "NPM03 layout"
  - "scripts/smoke/verify-runtime-smoke.sh (TC46 regression)"
risk_level: "medium"
---

# NPM04 - Local Pack And Global Install Smoke

Use this file directly with `/goal`:

    /goal .agent/goals/terminal-commander-npm-distribution/NPM04-local-pack-and-global-install-smoke.md

## Goal File Workflow

0. Branch Guard.
1. Mark `In progress`.
2. Execute only the mini-spec.
3. On pass: commit + status commit.

## Branch Guard

```text
main
```

## Mission Context

NPM03 stood up the layout. NPM04 proves a clean `npm install -g <local-tarball>` on a Linux host produces working binaries on PATH and a working MCP stdio handshake. Local binaries (from `cargo build`) feed the platform package's `bin/` directory; no CI involvement yet.

## Mini-Spec

objective:
- Build the three Rust binaries for the host platform, drop them into the matching platform package, run `npm pack` for the root + platform packages, install the resulting tarballs globally into a sandboxed npm prefix, and verify the three commands work end-to-end including the TC46 local smoke run against the npm-installed binaries.

non_goals:
- Do not publish to the public npm registry.
- Do not modify `crates/**` source.
- Do not add macOS or Windows targets.
- Do not write GitHub Actions.

allowed_files_or_area:
- scripts/smoke/** (new local-install smoke script)
- packages/**
- docs/release/**
- .agent/goals/terminal-commander-npm-distribution/NPM04-*.md
- .agent/goals/terminal-commander-npm-distribution/GOAL_CHAIN_INDEX.md

forbidden_files:
- crates/**
- Cargo.toml
- Cargo.lock
- rules/**
- config/**
- .github/workflows/**

contracts_or_interfaces:
- The smoke script installs into a sandboxed `--prefix` (temp dir) — never the user's global npm prefix.
- After install, `${prefix}/bin/terminal-commanderd`, `${prefix}/bin/terminal-commander-mcp`, `${prefix}/bin/terminal-commander` exist and are executable.
- The script then runs the TC46 `verify-runtime-smoke.sh` flow but using the npm-installed binaries (via `PATH=${prefix}/bin:$PATH`).
- Script tears down the sandbox on exit, even on failure.
- No secrets / tokens written.

invariants:
- Runtime / MCP / audit invariants unchanged.
- Smoke script must not require root.

acceptance_criteria:
- `scripts/smoke/verify-npm-local-install.sh` exits 0 on a clean Linux x64 host.
- The three commands run from the sandboxed prefix.
- The bundled TC46 smoke (or its npm-installed equivalent) passes 8/8 assertions.
- `crates/**` and `.github/workflows/**` untouched.

evidence_required:
- Branch evidence.
- Smoke script output.
- File paths changed.

stop_conditions:
- Branch is not `main`.
- The host architecture is not one of the NPM02-locked targets.
- The smoke requires running as root or modifying the user's global npm prefix.

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
```

## Task Prompt

Run NPM04 only on branch `main`. Prove local install works before any CI work begins.

## Final Report

Objective:
- Prove the NPM03 npm wrapper / platform-package layout produces working binaries on a clean Linux host via local `npm pack` + `npm install -g <tarball>` into a sandbox prefix. No publishing.

Changes (verified work commit `970db2c`):
- `scripts/smoke/verify-npm-local-install.sh` (new, executable). 347 lines. Uses only `cargo`, `npm`, `node`, `python3` — no `jq` dependency.

No product-code changes. No CI / release-please / publish workflow. No committed Rust binaries. Committed `packages/.../bin/*.placeholder` files untouched.

Files changed:
- `scripts/smoke/verify-npm-local-install.sh` (new)
- `.agent/goals/terminal-commander-npm-distribution/NPM04-*.md` (this file)

Verification (Linux WSL2, `CARGO_TARGET_DIR=target-wsl`, `npm 10.9.7`, `node 22.22.2`):
- PASS: `git branch --show-current` — `main`
- PASS: `git status --short` — clean after work + status commits
- PASS: `git diff --check`
- PASS: `cargo metadata --no-deps`
- PASS: `cargo fmt --all --check`
- PASS: `cargo clippy --workspace --all-targets -- -D warnings`
- PASS: `cargo test --workspace` — every suite green
- PASS: `cargo nextest run --workspace` — **347/347, 0 skipped**
- PASS: `bash scripts/smoke/verify-runtime-smoke.sh` — TC46 regression SUCCESS (8/8 PASS)
- PASS: `bash scripts/smoke/verify-npm-local-install.sh` — **SUCCESS on linux-x64**:
  - PASS  cargo build produced three binaries
  - PASS  staged platform package with real binaries
  - PASS  platform tarball produced
  - PASS  root tarball produced
  - PASS  npm install -g into sandbox prefix
  - PASS  three commands present under sandbox bin
  - PASS  --help responses bounded for all three commands
  - PASS  terminal-commanderd self-check (0 failures)
  - PASS  initialize protocol version (MCP stdio)
  - PASS  tools/list reports 29 tools (MCP stdio)
  - PASS  system_discover payload bounded (MCP stdio)
  - PASS  health reports ok (MCP stdio)
- PASS: `npm pack ./packages/terminal-commander --dry-run` — 7 files, 0.1.0-beta.1
- PASS: `npm pack ./packages/terminal-commander-linux-x64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS: `npm pack ./packages/terminal-commander-linux-arm64 --dry-run` — 5 files, 0.1.0-beta.1
- PASS: `rg "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp` — doc / negative-assertion matches only
- PASS: `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src` — no matches
- PASS: `git diff HEAD -- crates/ Cargo.toml Cargo.lock rules/ config/ .github/ release-please-config.json .release-please-manifest.json` — empty

Smoke design (key boundary properties enforced by the script):
- `mktemp -d` for the temp root; `trap cleanup EXIT INT TERM` removes it unconditionally.
- Real binaries land ONLY under the temp staging tree. The committed `packages/<plat>/bin/*.placeholder` files are never overwritten.
- `npm install -g --prefix $TMP/prefix --no-audit --no-fund --no-save` — the user's global npm prefix is never touched.
- Platform tarball installs first; root wrapper second (matches NPM02 publish order).
- All shim invocations under `PATH=$TMP/prefix/bin:$PATH`, so any false-resolve would surface.
- MCP stdio JSON-RPC pump in python3; no `jq` dependency.
- Daemon spawn waits up to 5s for the UDS socket to appear before failing.
- Daemon torn down BEFORE the JSON shape assertions so a failure still cleans up.

Compatibility fix recorded (Cargo workspace inspection during NPM04):
- The admin CLI's Cargo PACKAGE name is `terminal-commander-cli`; the BINARY name it produces is `terminal-commander` per the `[[bin]]` section in `crates/cli/Cargo.toml`. The smoke script invokes `cargo build --release -p terminal-commander-cli` accordingly. This is a script-side correction, not a Cargo edit. No `crates/**` files were modified.

Evidence — explicit acceptance against the NPM04 mini-spec:

- **Local x64 npm global install smoke passes from tarballs.** Confirmed by 12 PASS lines in the smoke run on the verification host (linux-x64).
- **Installed commands are available from temp prefix.** `$TMP/prefix/bin/terminal-commanderd`, `$TMP/prefix/bin/terminal-commander-mcp`, `$TMP/prefix/bin/terminal-commander` present and executable; `<cmd> --help` returns 0.
- **Wrapper resolver finds the local platform package.** The MCP stdio pump runs through `$TMP/prefix/bin/terminal-commander-mcp`, which delegates to `lib/resolve-binary.js` and `child_process.spawn`s the staged platform binary. `tools/list` returning 29 tools is end-to-end proof.
- **Unsupported or missing platform behavior remains bounded.** The 12 NPM03 resolver tests already cover these; the runtime path here exercised the OK branch only. Resolver-fallback paths are unit-tested in `packages/terminal-commander/test/resolve-binary.test.js` (NPM03).
- **No raw stream endpoint introduced.** The bin shims `spawn` with `shell: false` + `stdio: 'inherit'`; the only new code is the JS shim layer NPM03 already shipped. No new MCP tools. No new IPC paths.
- **Existing TC46 runtime smoke still passes.** Recorded above.
- **`npm pack --dry-run` still passes for root + both platform packages.** Tarball file counts unchanged.
- **linux-arm64 package shape verified without pretending local arm64 execution occurred.** The smoke script names this explicitly in its final SUCCESS lines: `arm64 cross-arch execution NOT covered; only linux-x64 was actually run.` Cross-arch execution is deferred to NPM05's GitHub Actions matrix.
- **No publishing or CI / release files added.** Confirmed by forbidden-paths `git diff` returning empty.

Beta-state mapping:
- TC48 `Conditional Go` preserved. The npm install path now produces real working binaries on a linux-x64 host. The provider live smoke ceiling remains `Conditional Go` until NPM08 + the operator-driven Cursor transcript.
- Provider live smoke pending: still NPM08 scope. Cursor smoke is operator-driven.
- Linux/WSL2 = the real platform story: smoke explicitly refuses to run on non-Linux hosts and explicitly does NOT claim arm64 execution on x64.
- TC46 + TC47 regressions stay green.

Source-status:
- `scripts/smoke/verify-npm-local-install.sh`: **live (NPM04)** on linux-x64. Linux-arm64 path is implemented but not verified; will be verified on the NPM05 arm64 runner.
- `packages/terminal-commander/**` resolver + shims (NPM03): **unchanged**; the smoke proves they work end-to-end with real binaries.
- `packages/terminal-commander-linux-x64/**`: still **partial** — committed `bin/` carries placeholders; the smoke proves the layout works when real binaries are staged.
- `packages/terminal-commander-linux-arm64/**`: same as x64 — committed placeholders only, binary execution deferred.
- Terminal Commander runtime + MCP surface: **unchanged**.
- Every `crates/` source file: **unchanged**.

Commits:
- Verified work commit: `970db2c`
- Goal status commit: this commit

Known gaps / blockers:
- linux-arm64 binary execution not run locally (host arch is x86_64); shape verified via `npm pack --dry-run`. Cross-arch execution lands in NPM05 GitHub Actions.
- Operator preconditions from NPM02 (npmjs.com `@terminal-commander` org claim + trusted-publisher config) remain pending; both gate NPM07, not NPM05.

Next goal:
- NPM05-github-actions-build-matrix.md
