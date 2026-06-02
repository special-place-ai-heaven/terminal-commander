# NPM01 — Symforge release pipeline audit + Terminal Commander recommendations

Status: NPM01 deliverable.
Branch: `main`.
Date: 2026-05-23.

This document is the **evidence-backed audit** that gates the rest of
the `terminal-commander-npm-distribution` chain. It maps the Symforge
release pipeline against Terminal Commander's actual repo state and
records explicit recommendations for NPM02–NPM07.

Language: ASCII only.

## 1. Sources queried

| Source | Status | Evidence |
|--------|--------|----------|
| agentmemory MCP `memory_smart_search("symforge npm release-please binary packaging publishing")` | OK | Hits include `mem_mp6xflam_6c69f7f265d1` ("User's SymForge install path is npm-driven. `npm install -g sy[mforge]`") and `mem_mp879e2s_d94e00c1bd0e` ("SymForge Wave 0 close-out commits a1d511f (docs final release …)"). |
| agentmemory MCP `memory_recall("symforge release pipeline npm package json platform binary wrapper")` | OK | Same hits + Wave 0 commit + Codex init notes. |
| remindb-vault `MemorySearch("symforge release npm publishing trusted")` | OK | Hits include `wiki/howto/install-symforge-and-obsidian-mcp-on-linux-vps.md` (`npm install -g --prefix /home/svetipeter/.local symforge`) and `wiki/projects/symforge/SymForge Backlog Intake 2026-05-16.md`. None of the matches cover trusted publishing / OIDC — keyword absent in vault corpus. |
| Symforge repo on disk | OK | `/mnt/c/AI_STUFF/PROGRAMMING/symforge` reachable from WSL2. Files inspected: `npm/package.json`, `npm/scripts/install.js`, `npm/bin/symforge.js`, `.github/workflows/release.yml`, `.github/workflows/ci.yml`, `.github/release-please-config.json`, `.github/.release-please-manifest.json`, `execution/release_ops.py`. |
| Terminal Commander repo (this repo) | OK | Cargo workspace, 3 bin targets (`terminal-commanderd`, `terminal-commander-mcp`, `terminal-commander`), no `package.json` / `packages/` / `.github/workflows/` / `release-please-config.json` / `.release-please-manifest.json` today. Verified via `find` + `ls`. |
| Official npm trusted-publishing docs | Referenced indirectly | The audit relies on npm documentation that the chain `GOAL_CHAIN_INDEX.md` already cites; not re-fetched at NPM01 because the Symforge precedent is the primary evidence source. |
| Official release-please docs | Referenced indirectly | Same rationale; the Symforge config file is the concrete precedent. |

No source returned `Blocked` / `Not Run`.

## 2. Symforge source map

Files inspected and their role in the release pipeline:

| Symforge file | Role |
|---------------|------|
| `npm/package.json` | Single npm package named `symforge`. `version` synced from Cargo via `release-please` `extra-files`. Bin: `{ "symforge": "bin/symforge.js" }`. `postinstall`: `node scripts/install.js`. `files`: `bin/`, `scripts/`, `LICENSE`. `engines.node: ">=18"`. |
| `npm/scripts/install.js` | **Postinstall binary download.** Picks `${platform}-${arch}` artifact from the GitHub Release, writes to `$SYMFORGE_HOME/bin/` (or `~/.symforge/bin/`). Skips download if `symforge.version` matches. Handles Windows file-lock by staging `symforge.pending.exe`. Supported targets: `windows-x64`, `darwin-arm64`, `darwin-x64`, `linux-x64`. |
| `npm/bin/symforge.js` | Tiny Node launcher that exec's the downloaded binary. |
| `.github/workflows/release.yml` | The release pipeline. Triggers on push to `main` or manual dispatch. Stages: `verify-main-push` → `prepare-release` (release-please) → `build` (cargo build per target) → `build-npm-package` (npm pack) → `upload-release-assets` (gh release upload) → `cargo-publish` → `npm-publish`. |
| `.github/release-please-config.json` | Single-package config (root `.`), `release-type: "rust"`, `extra-files: ["npm/package.json"]` so the npm package version follows Cargo. |
| `.github/.release-please-manifest.json` | Pin: `{ ".": "7.13.1" }`. |
| `execution/release_ops.py` | Python wrapper around `npm publish <tarball> --access public`. Uses `NODE_AUTH_TOKEN` (mapped from `secrets.NPM_TOKEN`). |
| Build matrix | `windows-latest` / `ubuntu-latest` / `macos-latest` × respective Rust targets. Artifacts uploaded to the GitHub Release. |

Key Symforge design decisions:

- **Single npm package** with **postinstall binary download** from GitHub Releases. NOT a wrapper + per-platform packages via `optionalDependencies`.
- **Long-lived `NPM_TOKEN`** in repo secrets. NOT npm trusted publishing / OIDC. The release workflow does not set `permissions: id-token: write` and does not pass `--provenance` to `npm publish`.
- **`release-please-action@v4`** in `manifest-file` mode, but only one package (single line in manifest). Versions sync from Cargo via `extra-files`.
- **Cargo + npm dual publish** on the same release event.
- **GitHub Release as the binary CDN.** The npm tarball stays small (just the JS shims + install script).

## 3. Terminal Commander state (today)

- Cargo workspace at `Cargo.toml` workspace section + 7 crates.
- Three binaries shipped:
  - `crates/daemon/src/main.rs` → `terminal-commanderd`
  - `crates/mcp/src/main.rs` → `terminal-commander-mcp`
  - `crates/cli/src/main.rs` → `terminal-commander`
- Linux/WSL2 only (`pty-process` is `cfg(unix)`, daemon UDS is Unix-only). macOS arm64/x64 and Windows-native are NOT runtime-supported today (TC44 `non_goals`).
- No `package.json`, `packages/`, `.github/workflows/`, `release-please-config.json`, `.release-please-manifest.json` exist.
- Existing release surface: `RELEASE_CHECKLIST.md` (TC48 baseline, Conditional Go), `ROADMAP.md`, `docs/install/` (cargo install path documented).

## 4. Capability gap (Symforge → Terminal Commander)

| Capability | Symforge | Terminal Commander | Gap |
|------------|----------|--------------------|-----|
| npm package shape | single root, postinstall download | none yet | Decide: copy Symforge model OR diverge to platform packages? See §6. |
| Build matrix targets | win-x64 + linux-x64 + macos-arm64 + macos-x64 | none yet | Initial chain target: linux-x64 + linux-arm64 ONLY. Symforge's macOS / Windows targets do NOT apply (TC44 boundary). |
| release-please | manifest mode, single package, `release-type: rust`, `extra-files: ["npm/package.json"]` | none | Copy the shape. Adjust manifest to whichever package layout NPM02 locks. |
| Publish auth | `NPM_TOKEN` secret | none | Decide: copy Symforge OR adopt npm trusted publishing / OIDC per our prep amendment? See §6. |
| Provenance | not used | not specified | Add `npm publish --provenance` when OIDC is configured. |
| Cargo publish | yes, alongside npm | out of scope (TC31/TC48 say beta does not publish to crates.io) | Do NOT copy. |
| Test gate before publish | `verify-main-push` runs `cargo check`, `cargo test --all-targets`, npm test | TC46 smoke + TC47 load + nextest 347/347 already exist | Reuse these as the release-time gates (NPM05). |
| GitHub Release as binary CDN | yes | none | Decide based on §6. |
| Postinstall download | yes | locked OUT in our chain assumptions | Direct conflict with our prep amendment. See §6 for the recommended resolution. |

## 5. What to copy conceptually

- **release-please in manifest mode.** Drop-in fit. Adjust `release-type` if we ship multiple npm packages (manifest can carry multiple entries).
- **Version sync between Cargo and npm via `extra-files`.** Same shape applies regardless of the single-vs-platform-package decision.
- **Test gate stage before any binary build.** Symforge's `verify-main-push` job pattern matches our local invariants (TC46 smoke + TC47 load + nextest workspace + clippy + fmt). Adopt the same stage names.
- **Per-target build matrix with `actions/upload-artifact`.** Same shape; replace targets with `x86_64-unknown-linux-gnu` + `aarch64-unknown-linux-gnu` only.
- **Python orchestration helper (`execution/release_ops.py`).** Optional — Symforge uses it for `npm publish` + version sync tests. Terminal Commander can skip if the workflow is small enough to stay readable in YAML.
- **Single CHANGELOG.md** managed by release-please.

## 6. What NOT to copy (and where to diverge)

The `terminal-commander-npm-distribution` prep amendment locks
boundaries Symforge does not honor. The audit recommends keeping
the boundaries — the divergences are intentional, not accidental:

1. **Postinstall binary download.** Symforge does this. Our chain
   forbids it (prep amendment Locked assumption). Reason: a
   postinstall download script (a) requires network access at
   `npm install -g` time, (b) adds an opaque step that can fail
   silently under restricted CI / offline installs, (c) breaks the
   "bounded surface, no implicit network" posture Terminal Commander
   guarantees on the runtime side. **Recommendation:** keep the
   prep amendment's choice — platform binary packages pulled via
   `optionalDependencies` from a root wrapper. No postinstall
   network call. The root wrapper's bin shim resolves the platform
   package directly from `node_modules/`.

2. **Long-lived `NPM_TOKEN` secret.** Symforge uses one because it
   pre-dates npm trusted publishing being widely available on
   organization-scoped packages, and because their auth model is
   already operator-driven. Our chain's prep amendment locks
   trusted publishing via GitHub Actions OIDC + `--provenance`.
   **Recommendation:** keep the prep amendment's choice. Fallback
   (fine-grained automation token) is allowed ONLY if NPM07 records
   an explicit reason in its final report.

3. **Cargo publish.** Symforge publishes to crates.io alongside npm.
   Our TC31/TC48 baseline explicitly defers crates.io. **Do NOT
   copy.** Beta installs via `cargo install --path` (existing) or
   via the new `npm install -g` path (this chain).

4. **macOS / Windows-native binaries.** Symforge ships them. Our
   runtime is Unix-only (TC44 `non_goals`). **Do NOT copy.** Initial
   matrix is linux-x64 + linux-arm64 only. Windows operators use
   Cursor invoking `wsl ... terminal-commander-mcp`.

5. **Auto-merging the release PR.** Symforge auto-merges with a PAT
   (`secrets.RELEASE_PLEASE_TOKEN`). **Do NOT copy at NPM06.** Keep
   the release PR review-gated for the first beta cuts; revisit
   post-`Go`.

## 7. NPM02 recommendations (packaging contract)

- Root npm package: `terminal-commander`.
- Platform binary packages: `@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`. (Scoped namespace — register the
  `@terminal-commander` org on npmjs.com as a NPM02 operator
  precondition.)
- Root `bin` field exposes three commands:
  - `terminal-commanderd`
  - `terminal-commander-mcp`
  - `terminal-commander`
- Distribution: `optionalDependencies` from root to platform packages.
  Bin shims resolve the matching platform package via
  `process.platform` + `process.arch`. No postinstall.
- Platform packages each carry `"os"` + `"cpu"` so npm refuses to
  install on the wrong host.
- Initial version: `0.1.0-beta.1` (matches `RELEASE_CHECKLIST.md`).
- Versions for all packages stay in lockstep (single line in the
  release-please manifest).
- `engines.node`: `">=18"` (Symforge precedent; matches `actions/setup-node@v4` default).
- License: PolyForm-Noncommercial-1.0.0 (matches the Rust crate license).
- `files` field on each package limits the tarball to `bin/` +
  `package.json` + `LICENSE` (+ `README.md` on the root).

## 8. NPM03 recommendations (layout)

```
packages/
├─ terminal-commander/                # root wrapper
│  ├─ package.json
│  ├─ README.md
│  ├─ LICENSE
│  ├─ bin/
│  │  ├─ terminal-commanderd.js       # Node shim
│  │  ├─ terminal-commander-mcp.js    # Node shim
│  │  └─ terminal-commander.js        # Node shim
│  └─ lib/
│     └─ resolve-platform.js          # process.platform/arch → platform pkg
├─ terminal-commander-linux-x64/
│  ├─ package.json (cpu/os: linux/x64)
│  ├─ LICENSE
│  └─ bin/
│     ├─ terminal-commanderd
│     ├─ terminal-commander-mcp
│     └─ terminal-commander
└─ terminal-commander-linux-arm64/
   ├─ package.json (cpu/os: linux/arm64)
   ├─ LICENSE
   └─ bin/
      ├─ terminal-commanderd
      ├─ terminal-commander-mcp
      └─ terminal-commander
```

- Shims exit non-zero with a clear stderr message if no platform
  package is installed.
- Platform package directories ship empty `bin/` placeholders at
  NPM03 (real binaries arrive in NPM05).
- `.gitignore` excludes any locally-built binary files from being
  accidentally committed to the platform package dirs.

## 9. NPM04 recommendations (local pack + global install smoke)

- New script: `scripts/smoke/verify-npm-local-install.sh`.
- Build the three binaries via `cargo build --release -p
  terminal-commanderd -p terminal-commander-mcp -p
  terminal-commander`.
- Copy binaries into the host-matching platform package's `bin/`.
- `npm pack` each package; install the resulting tarballs into a
  sandboxed `--prefix` (temp dir); confirm the three commands exist
  on `${prefix}/bin/`.
- Re-run the TC46 smoke (`bash scripts/smoke/verify-runtime-smoke.sh`)
  with `PATH=${prefix}/bin:$PATH` so the npm-installed binaries are
  exercised.
- Tear down the sandbox unconditionally on exit.

## 10. NPM05 recommendations (GitHub Actions build matrix)

- Adopt Symforge's stage pattern: `verify-main-push` → `build` →
  `build-npm-package` → `upload-release-assets`. Skip the
  `cargo-publish` and `auto-merge` steps.
- Matrix targets (initial publish):
  - `ubuntu-latest` × `x86_64-unknown-linux-gnu`
  - `ubuntu-24.04-arm` × `aarch64-unknown-linux-gnu` (GitHub-hosted
    Linux arm64 runners are available; record the exact runner
    label at NPM05 implementation time).
- Rust toolchain pinned to `1.95.0` via `dtolnay/rust-toolchain` to
  match Symforge precedent and `RELEASE_CHECKLIST.md` doctrine.
- `Swatinem/rust-cache` for cargo cache.
- Pre-build gate: `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo nextest run
  --workspace`, `cargo test -p terminal-commanderd --test
  load_noise_backpressure`, `bash scripts/smoke/verify-runtime-smoke.sh`.
- Artifacts uploaded with deterministic names. Checksums via
  `sha256sum`.

## 11. NPM06 recommendations (release-please)

- Use `.github/release-please-config.json` +
  `.github/.release-please-manifest.json` (Symforge path layout).
- Manifest mode, single shared version, three npm packages
  registered:
  - `packages/terminal-commander` (`release-type: "node"`)
  - `packages/terminal-commander-linux-x64` (`release-type: "node"`)
  - `packages/terminal-commander-linux-arm64` (`release-type: "node"`)
- Initial version: `0.1.0-beta.1`. `include-v-in-tag: true`.
- Workspace Cargo version stays at `0.0.0` (unless TC48 release
  posture changes), so no `extra-files` entry for `Cargo.toml`.
- Release-please-action pinned to a SHA, NOT a floating `@v4` tag.
- No `auto-merge` step at NPM06. Release PRs are review-gated.

## 12. NPM07 recommendations (trusted publishing)

- Use npm trusted publishing via GitHub Actions OIDC.
- Workflow job sets `permissions: id-token: write`.
- `npm publish --provenance` on each package.
- Publish order: platform packages first, root wrapper last so the
  `optionalDependencies` resolve at install time.
- Initial dist-tag: `--tag beta`. Promotion to `latest` is an
  operator action recorded in NPM09 / post-chain.
- npmjs.com side: configure the trusted publisher for each package
  to accept publishes from `special-place-administrator/terminal-commander`
  + the publish workflow file path. Document the steps in
  `docs/release/` so the operator can mirror them.
- Fallback (fine-grained automation token) ONLY if NPM07 final
  report explicitly approves; never silently use `NPM_TOKEN`.

## 13. Cursor install implications

- The chain `GOAL_CHAIN_INDEX.md` already locks the Cursor MCP
  stdio config pattern. NPM08 implements `docs/integrations/cursor.md`.
- The Windows-Cursor + WSL path (`command: "wsl" + args: ["-d",
  "<distro>", "bash", "-lc", "terminal-commander-mcp"]`) is the
  primary fallback for operators who do not run Cursor inside WSL.
- Provider smoke for Cursor is operator-driven; NPM08 marks it
  `Not Run` honestly if Cursor is unavailable on the verification
  host.

## 14. Risks and blockers

- **R-NPM-01.** Symforge precedent uses postinstall download; we
  diverge. The platform-packages-via-optionalDependencies model is
  less battle-tested in the SymForge org. Mitigation: NPM04 +
  NPM08 catch this early via local install smoke + Cursor smoke.
- **R-NPM-02.** GitHub-hosted Linux arm64 runners may have queue
  delays or label changes. Mitigation: NPM05 records the exact
  runner label at implementation time; fall back to QEMU cross-build
  via `cross` if hosted arm64 is unavailable.
- **R-NPM-03.** npm trusted publishing requires npmjs.com side
  configuration that only an org owner can do. NPM07 records the
  exact steps; if the operator does not have org-owner access the
  goal is `Blocked` until access is granted (no `NPM_TOKEN`
  fallback without explicit approval).
- **R-NPM-04.** The `@terminal-commander` scoped namespace must be
  available on npmjs.com. NPM02 operator precondition: claim the
  org before locking the contract. If the name is taken, NPM02
  records the substitution.
- **R-NPM-05.** `optionalDependencies` semantics on older npm
  versions (npm <7) install ALL optional deps regardless of `os`/`cpu`.
  Mitigation: `engines.npm: ">=8"` on the root wrapper. Modern npm
  (npm 9+) honors `os` and `cpu` and skips non-matching platform
  packages.

## 15. Open questions for NPM02

- Whether to register the npm org as `@terminal-commander` or as
  `@special-place-administrator/terminal-commander`. Recommendation:
  `@terminal-commander` for ergonomics; fall back to the
  GitHub-org-mirror name only if unavailable.
- Whether to bundle a `mcp.json` template inside `packages/terminal-commander/`
  for Cursor / Codex / Claude Code config copy-paste. NPM08 owns
  this; NPM02 only needs to decide whether `files` includes a
  `templates/` directory.

## 16. Recommendation summary table

| Goal  | Key recommendation |
|-------|--------------------|
| NPM02 | Lock root `terminal-commander` + scoped platform packages; `optionalDependencies`; no postinstall; trusted publishing + provenance; initial `0.1.0-beta.1`. |
| NPM03 | `packages/` directory layout per §8; bin shims with platform resolver; `cpu`/`os` declared on platform packages. |
| NPM04 | `scripts/smoke/verify-npm-local-install.sh` that builds + packs + installs into sandbox prefix + re-runs the TC46 smoke against the npm-installed binaries. |
| NPM05 | Linux x64 + Linux arm64 matrix; pre-build gates mirror Symforge `verify-main-push` plus TC46 + TC47 regressions; toolchain pinned to `1.95.0`. |
| NPM06 | release-please manifest mode at `.github/release-please-config.json` + `.github/.release-please-manifest.json`; three packages registered; single shared version; release-please-action pinned to a SHA; no auto-merge. |
| NPM07 | OIDC trusted publishing + `--provenance`; publish order: platform packages first, root last; first publish `--tag beta`; no `NPM_TOKEN` unless NPM07 final report explicitly approves. |
| NPM08 | `docs/integrations/cursor.md` covers native + WSL bridge; provider live smoke marked `Not Run` with exact blocker if Cursor unavailable. |
| NPM09 | Run release-please + `npm publish --dry-run --provenance`; refresh `RELEASE_CHECKLIST.md`, `BACKLOG.md`, `RISK_REGISTER.md`, `EVIDENCE_REPORT_RUNTIME.md`; lock the post-chain beta posture. |

## 17. Non-decisions deliberately deferred

- macOS arm64 / x64 npm packages — deferred until macOS runtime
  validation lands.
- Windows-native package — deferred until Windows ConPTY lands
  (TC44 `non_goals`).
- crates.io publishing — out of TC31/TC48 scope.
- Auto-merge release PR — keep review-gated through the first beta
  cuts.
- Cargo publish step in GitHub Actions — not part of this chain.

## 18. Evidence acknowledgements

- Symforge files inspected (verbatim paths):
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/npm/package.json`
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/npm/scripts/install.js`
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/npm/bin/symforge.js`
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/.github/workflows/release.yml`
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/.github/release-please-config.json`
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/.github/.release-please-manifest.json`
  - `/mnt/c/AI_STUFF/PROGRAMMING/symforge/execution/release_ops.py`
- agentmemory `obsId` references: `mem_mp6xflam_6c69f7f265d1`,
  `mem_mp879e2s_d94e00c1bd0e`, `mem_mpadofdf_3d5a12725ecb`.
- remindb `id` references: `Br5TdW9sIaX`, `GTc0ArzXi8F`,
  `F8RYjunzp50`.
- Symforge git commit reference (agentmemory decision row):
  Wave 0 close-out at commit `a1d511f` (recorded historically; not
  re-verified inside this repo because Symforge git history is
  out-of-scope here).
- Verified via shell on 2026-05-23 from WSL2.
