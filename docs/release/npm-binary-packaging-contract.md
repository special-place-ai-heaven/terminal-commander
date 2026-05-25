# NPM02 — npm binary packaging contract for Terminal Commander

Status: NPM02 deliverable.
Branch: `main`.
Date: 2026-05-23.
Depends on: [`docs/release/npm-distribution-audit.md`](npm-distribution-audit.md) (NPM01).

This contract is the binding spec NPM03–NPM07 implement. Each
section locks (or explicitly rejects) one decision. The audit
recommendations in §16 of NPM01 are the upstream source; this
document records the final, machine-checkable contract.

Language: ASCII only.

## 0. Provenance

- NPM01 audit: commit `5dcbaa4`, document `docs/release/npm-distribution-audit.md`.
- Symforge precedent inspected at NPM01: single npm package +
  postinstall download + long-lived `NPM_TOKEN` + crates.io dual
  publish + macOS/Windows targets. Terminal Commander **diverges**
  on each of those — see §4 and §7.
- Terminal Commander beta posture: `Conditional Go` per
  `RELEASE_CHECKLIST.md` (TC48). Provider live smoke remains
  `Not Run` until NPM08 captures a Cursor transcript.

## 1. Package names

- **Root wrapper package:** `terminal-commander`.
- **Platform binary packages:**
  - `@terminal-commander/linux-x64`
  - `@terminal-commander/linux-arm64`

### 1.1 npm name-availability check

Run on 2026-05-23 from WSL2 (`npm 10.9.7`):

| Name | `npm view <name> name version` exit | Status |
|------|-------------------------------------|--------|
| `terminal-commander` | E404 (`'terminal-commander@*' is not in this registry.`) | **Available** |
| `@terminal-commander/linux-x64` | E404 | **Available** |
| `@terminal-commander/linux-arm64` | E404 | **Available** |
| `@special-place-administrator/terminal-commander` (fallback) | E404 | Available (recorded as fallback only) |

### 1.2 Operator preconditions

- The `@terminal-commander` scope on npmjs.com is **unclaimed**.
  An npmjs.com account with org-create permissions MUST register
  the `@terminal-commander` organization before NPM07 publishing.
- If org registration is denied or contested, switch the platform
  package scope to `@special-place-administrator/terminal-commander-linux-x64`
  / `@special-place-administrator/terminal-commander-linux-arm64`
  and amend this contract in a NPM02 prep amendment commit.

### 1.3 If a name later becomes unavailable

Stop NPM03 and write a prep amendment. Do NOT silently rename.

## 2. User-facing install contract

### 2.1 Install command

```sh
npm install -g terminal-commander
```

### 2.2 Installed commands

After install, the user's PATH carries:

- `terminal-commanderd`  — daemon binary
- `terminal-commander-mcp` — MCP stdio adapter
- `terminal-commander` — admin CLI

### 2.3 Confirmation against npm semantics

- `bin` field on the root `package.json` is an object map of
  command name → script path. npm's global install creates a
  shim/symlink per `bin` entry under `${prefix}/bin/` (Linux) so
  the commands are on PATH.
- Source: `package.json` `bin` semantics (npm docs).
- The three shim scripts (under `bin/`) are tiny Node launchers
  that resolve the platform package and exec its native binary
  (no postinstall, no network call).

## 3. Platform contract

### 3.1 Supported npm binary platforms (initial publish)

- Linux x64 (`x86_64-unknown-linux-gnu`)
- Linux arm64 (`aarch64-unknown-linux-gnu`)

### 3.2 Unsupported (rejected at NPM02)

- macOS arm64
- macOS x64
- Windows-native (any arch)
- musl / Alpine
- Any platform not proven by the TC44 runtime baseline (Unix-only
  PTY) + the daemon UDS (Unix-only transport).

### 3.3 WSL2 fallback for Windows operators

- Install Terminal Commander **inside WSL** (`npm install -g
  terminal-commander` from an Ubuntu-24.04 or compatible
  distribution); use the WSL `terminal-commander-mcp` from Cursor
  on Windows via `command: "wsl"`.
- Documented in NPM08 + `docs/integrations/cursor.md`.

### 3.4 Capability claim rule

A platform package MUST NOT claim runtime support beyond what is
proven on `main`. If musl support is added later, it requires a
dedicated TC-level runtime goal first (not an NPM01-style audit).

## 4. Package architecture (locked)

### 4.1 Preferred (LOCKED)

- Root wrapper package `terminal-commander` with JS bin shims under
  `packages/terminal-commander/bin/`.
- Two platform packages (`@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`) each carrying the three Rust
  binaries under `bin/`.
- Root depends on platform packages through `optionalDependencies`:

  ```json
  {
    "optionalDependencies": {
      "@terminal-commander/linux-x64": "<exact-version>",
      "@terminal-commander/linux-arm64": "<exact-version>"
    }
  }
  ```

- A JS resolver (`packages/terminal-commander/lib/resolve-platform.js`)
  picks the matching platform package by `process.platform` +
  `process.arch` and returns the absolute path to the requested
  Rust binary.
- The shims `require()` the resolver and `child_process.spawn` the
  resolved binary, forwarding `argv` + `stdio: 'inherit'` (or
  `stdio: ['inherit', 'inherit', 'inherit']`).
- NO postinstall script.
- NO network access at install time.

### 4.2 Rejected by default (LOCKED)

| Option | Why rejected |
|--------|--------------|
| Postinstall binary download from GitHub Releases | Hides a network call inside `npm install`; breaks offline / restricted-network installs; opaque failure mode; conflicts with Terminal Commander's "bounded surface, no implicit network" posture. NPM01 risk R-NPM-01 recorded the divergence from Symforge. |
| Compiling Rust from `npm install` | Adds Rust toolchain as a hidden install dependency; massive surface; unacceptable for any LLM-harness target. |
| Shipping all platform binaries in one package | Bloats every install (~2x-4x); npm's `os` / `cpu` filtering only applies to dependencies, not files inside one package. |
| Long-lived `NPM_TOKEN` publish | Stored credential, no provenance, no OIDC binding. Trusted publishing is preferred per §7. |

### 4.3 `optionalDependencies` semantics

- Root `package.json` MUST declare `engines.npm: ">=8"` so npm
  honors the `os` / `cpu` fields on the platform package
  `package.json` and skips non-matching installs.
- If the user's platform matches NO platform package (e.g. macOS
  today), the root shim MUST exit non-zero with a clear error
  citing the supported targets — no stack trace, no silent hang.

### 4.4 Shim behavior contract

Each shim (`terminal-commanderd.js`, `terminal-commander-mcp.js`,
`terminal-commander.js`):

1. `require('../lib/resolve-platform.js')` → returns
   `{ platformPackage, binaryPath, supportedTargets }`.
2. If `binaryPath` is `null`:
   - Print one line to stderr naming the user's `process.platform`
     + `process.arch`, the supported targets, and the fact that
     `optionalDependencies` may not have been installed (often a
     `--no-optional` flag or an older npm).
   - Exit with code `64` (matches the existing TC40 unsupported-
     platform exit code on the MCP binary).
3. Otherwise `child_process.spawn(binaryPath, process.argv.slice(2),
   { stdio: 'inherit' })`.
4. Mirror the child's exit code on parent exit.

## 5. Binary layout (locked)

```
packages/
├─ terminal-commander/
│  ├─ package.json
│  ├─ README.md                                 # quickstart + boundary statement
│  ├─ LICENSE                                   # Apache-2.0 copy
│  ├─ bin/
│  │  ├─ terminal-commanderd.js                 # Node shim → daemon
│  │  ├─ terminal-commander-mcp.js              # Node shim → MCP adapter
│  │  └─ terminal-commander.js                  # Node shim → admin CLI
│  └─ lib/
│     └─ resolve-platform.js                    # process.{platform,arch} resolver
├─ terminal-commander-linux-x64/
│  ├─ package.json                              # "os":["linux"], "cpu":["x64"]
│  ├─ LICENSE
│  └─ bin/
│     ├─ terminal-commanderd
│     ├─ terminal-commander-mcp
│     └─ terminal-commander
└─ terminal-commander-linux-arm64/
   ├─ package.json                              # "os":["linux"], "cpu":["arm64"]
   ├─ LICENSE
   └─ bin/
      ├─ terminal-commanderd
      ├─ terminal-commander-mcp
      └─ terminal-commander
```

Notes:

- Each platform package's `name` is the scoped form
  `@terminal-commander/linux-x64` / `@terminal-commander/linux-arm64`.
  The DIRECTORY name under `packages/` can be the unscoped form
  (NPM03 chooses the on-disk layout that keeps `release-please`
  happy; the published name is what matters).
- Binaries inside `packages/<platform>/bin/` are produced by `cargo
  build --release --target <triple>` and copied in. NPM03 stubs the
  directories; NPM05 GitHub Actions populates the real binaries.
- Each package's `files` field whitelists `bin/`, `lib/` (root only),
  `package.json`, `LICENSE`, `README.md` (root only). Nothing else
  is tar-balled.

## 6. Versioning contract (locked)

- **One shared semver** across the root + the two platform packages.
- **Initial version:** `0.1.0-beta.1` (matches
  `RELEASE_CHECKLIST.md`).
- Cargo workspace version stays at `0.0.0` for now; the npm
  packages have their own version line driven by release-please.
  No `Cargo.toml` in the release-please `extra-files` list.
- The root wrapper's `optionalDependencies` MUST pin the exact
  platform-package version (no `^` / `~` ranges) so a publish race
  cannot resolve to a newer platform package that the shim was
  not built against.
- **NO crates.io publish** in this chain (TC31 / TC48 baseline).
- If a future chain wants crates.io publish, it's a separate goal
  with its own audit (the Symforge dual-publish pattern is one
  option but is OUT of NPM02 scope).

## 7. Release contract (locked)

- `release-please` manifest mode at
  `.github/release-please-config.json` +
  `.github/.release-please-manifest.json` (Symforge path layout).
- Three packages registered in the manifest, each `release-type:
  "node"`, single shared version (one line in the manifest).
- Release-please creates the version-bump + CHANGELOG + release PR
  on Conventional Commits.
- Release PR is **review-gated** (no auto-merge through the first
  beta cuts; revisit post-`Go`).
- Publishing is a **separate workflow job** that runs only when
  release-please outputs `release_created == true`.
- **npm trusted publishing via GitHub Actions OIDC** is the
  required publish mechanism.
  - Job sets `permissions: id-token: write`.
  - `npm publish --provenance` on every package.
  - Publish order: platform packages first, root wrapper last.
  - First publish dist-tag is `--tag beta`.
- **NO long-lived `NPM_TOKEN`** committed or referenced in YAML
  unless trusted publishing is technically impossible AND the
  NPM07 final report explicitly approves a fine-grained
  automation-token fallback with the exact reason.

## 8. Cursor contract (locked, NPM08 implements)

- `docs/integrations/cursor.md` (NPM08 deliverable) ships two
  config blocks:

  **Linux / WSL2-native Cursor:**
  ```json
  {
    "mcpServers": {
      "terminal-commander": {
        "type": "stdio",
        "command": "terminal-commander-mcp"
      }
    }
  }
  ```

  **Windows Cursor → WSL:**
  ```json
  {
    "mcpServers": {
      "terminal-commander": {
        "type": "stdio",
        "command": "wsl",
        "args": ["-d", "Ubuntu-24.04", "bash", "-lc", "terminal-commander-mcp"]
      }
    }
  }
  ```

- The Cursor smoke is **operator-driven**. NPM08 marks it `Not Run`
  with the exact blocker if Cursor is unavailable on the
  verification host. This contract does NOT claim Cursor smoke
  success at NPM02; the claim only becomes true when NPM08 attaches
  a transcript.

## 9. Safety contract (locked, mirrors runtime invariants)

The npm packages MUST NOT widen the runtime contract documented in
`docs/security/`, `RISK_REGISTER.md`, and `EVIDENCE_REPORT_RUNTIME.md`.
Specifically:

- The MCP package (`crates/mcp`) MUST NOT add command-spawn
  behavior. The two existing guard greps remain green:
  - `rg "Command::new|Command::spawn|TcpListener|UdpSocket"
    crates/mcp` → doc / negative-assertion matches only.
  - `rg "tokio::fs|std::fs|File::open|read_to_string|read_to_end"
    crates/mcp/src` → no matches.
- The Node shims MAY only:
  - Read `process.platform` / `process.arch`.
  - Resolve the platform package via `require.resolve()` /
    relative paths.
  - `child_process.spawn` the resolved Rust binary with `stdio:
    'inherit'`.
  - Forward `argv` and the child's exit code.
- The shims MUST NOT:
  - Open network sockets.
  - Read or write arbitrary files outside the resolved binary
    path.
  - Spawn anything other than the resolved Rust binary.
  - Exec a shell or interpret shell-syntax in arguments.
  - Cache, log, or echo any environment variables to stdout /
    stderr beyond the platform-mismatch error.
- No secrets, tokens, private usernames, private absolute paths,
  or machine-specific paths in any committed npm artifact.
- No raw stream lane is added. The MCP surface remains the 29-tool
  TC45 catalogue with bounded JSON envelopes.

## 10. Per-goal recommendations (binding for NPM03–NPM07)

- **NPM03 (layout):** Implement §5 verbatim. Add a `packages/`
  root-level `.gitignore` that excludes any host-built binaries
  from being accidentally committed. Stub the platform package
  `bin/` directories with placeholder files (e.g. a 1-byte file
  per binary) so `npm pack` works locally before NPM05 produces
  real binaries.
- **NPM04 (local pack + global install smoke):** Build the three
  binaries via `cargo build --release` for the host triple; copy
  into the host-matching platform package `bin/`; `npm pack` each
  package; install all three tarballs into a sandboxed `--prefix`
  via `npm install -g`; re-run `scripts/smoke/verify-runtime-smoke.sh`
  with `PATH=${prefix}/bin:$PATH` so the npm-installed binaries
  are exercised end-to-end.
- **NPM05 (CI build matrix):** GitHub Actions workflow at
  `.github/workflows/release.yml`. Stages: `verify-main-push`
  (regression gates) → `build` (linux-x64 + linux-arm64) →
  `build-npm-package` → `upload-release-assets`. Toolchain pinned
  to `1.95.0`. No `cargo publish`. No auto-merge.
- **NPM06 (release-please):** Manifest mode; three packages
  registered; `release-type: "node"` on all three; shared version;
  `include-v-in-tag: true`; release-please-action pinned to a SHA
  (NOT a floating `@v4` tag).
- **NPM07 (publishing):** OIDC trusted publishing as locked in §7.
  Publish order: platform packages first, root last. First publish
  `--tag beta`. Operator pre-config on npmjs.com side documented
  in `docs/release/`.

## 11. Risks and unresolved questions

| ID | Risk | Mitigation |
|----|------|-----------|
| R-NPM-04 | `@terminal-commander` org name availability on npmjs.com | npm view confirmed the scoped names are free; org-create permission required (operator precondition recorded in §1.2). |
| R-NPM-05 | `optionalDependencies` semantics on npm <8 | `engines.npm: ">=8"` on root wrapper. |
| R-NPM-06 | Linux arm64 GitHub-hosted runner availability | NPM05 records exact runner label; falls back to QEMU `cross` if hosted arm64 unavailable. (Recorded in NPM01 §14 as R-NPM-02.) |
| R-NPM-07 | Trusted publishing requires npmjs.com org-owner config | Operator precondition; documented in NPM07. No `NPM_TOKEN` fallback without explicit NPM07 approval. (Recorded in NPM01 §14 as R-NPM-03.) |
| R-NPM-08 | Shim resolver false-negative on rare arch combinations | Test matrix in NPM04 covers Linux x64; Linux arm64 host coverage may require a separate runner. NPM04 records the exact verification host. |
| R-NPM-09 | Cursor MCP config may drift as Cursor evolves the schema | Recorded in NPM01 §14 as R-NPM-05; NPM08 doc links the current Cursor MCP docs at the time of capture. |

## 12. Alternatives considered (and rejected)

- **Single npm package + postinstall download (Symforge pattern).**
  Rejected per §4.2 (network call hidden in `npm install`).
- **Single npm package with all binaries inside.** Rejected per
  §4.2 (bloat, no `os` / `cpu` skipping for files).
- **Per-binary npm packages (one package per Rust binary).**
  Rejected: would force users to install three separate top-level
  packages and breaks the "one install command" UX in §2.
- **musl / Alpine-targeted publish at NPM02.** Rejected: musl
  + cap-std + pty-process combinations are not validated on the
  runtime side. Revisit when a runtime musl goal lands.
- **release-please monorepo mode with independent versions per
  package.** Rejected: the three npm packages MUST stay in
  lockstep so the root wrapper's pinned `optionalDependencies`
  always resolve.

## 13. Acceptance against NPM02 mini-spec

- [x] Package names locked (§1) with npm name-availability check
      recorded.
- [x] User-facing install contract locked (§2) with bin semantics
      cited.
- [x] Platform contract locked (§3): linux-x64 + linux-arm64 only;
      WSL2 fallback; no macOS / Windows / musl until proven.
- [x] Package architecture locked (§4): platform packages via
      `optionalDependencies`; no postinstall download; no
      RT-compile.
- [x] Binary layout locked (§5).
- [x] Versioning contract locked (§6): single shared semver,
      `0.1.0-beta.1` initial, no crates.io publish.
- [x] Release contract locked (§7): release-please manifest mode,
      review-gated PR, OIDC trusted publishing + provenance, no
      long-lived `NPM_TOKEN`.
- [x] Cursor contract locked (§8): both config blocks; smoke
      operator-driven; no claim of success until NPM08.
- [x] Safety contract locked (§9): MCP guards + shim behavior +
      no-secrets rule.
- [x] Per-goal recommendations for NPM03–NPM07 (§10).

## 13b. WWS02 amendment (2026-05-23)

The `terminal-commander-windows-wsl-bridge` chain (WWS01..WWS09)
landed a single binding change to this contract at WWS02. Every
other lock above is preserved verbatim.

| Field | NPM02 lock | WWS02 amendment | Rationale |
|-------|-----------|-----------------|-----------|
| Root `packages/terminal-commander/package.json` `os` | `["linux"]` | `["linux", "win32"]` | The WWS chain reframes the project as an **MCP control plane with environment runners**: Linux / WSL is the real runtime host; Windows is a **bridge / setup surface only**. Widening root `os` lets `npm install -g terminal-commander` succeed on Windows while npm's `os` / `cpu` filter on `optionalDependencies` (NPM02 §4.3) keeps both Linux platform packages out of the Windows install. No Rust binary ships on Windows. No native daemon claim. See `docs/release/windows-wsl-bridge-contract.md` (WWS01, commit `6220eb2`) for the full design + 15 binding decisions D-01..D-15. |

Preserved (UNCHANGED at WWS02):

- §1 Package names (`terminal-commander`,
  `@terminal-commander/linux-x64`,
  `@terminal-commander/linux-arm64`).
- §2 Install command (`npm install -g terminal-commander`).
- §3.1 Supported npm binary platforms (linux-x64 +
  linux-arm64). Windows is NOT a binary platform; it is a
  bridge-only host.
- §3.2 Unsupported (Windows-native runtime still rejected;
  macOS / musl / Alpine still rejected).
- §4.1 Package architecture (`optionalDependencies`-via-npm
  filtering, no postinstall, no RT-compile).
- §4.3 `optionalDependencies` semantics (`engines.npm: ">=8"`
  preserved on root so Windows installs cleanly skip the
  Linux platform packages).
- §4.4 Shim behavior contract — extended with a new
  `bridge_required` resolver branch. WWS02 wires the three bin
  shims to refuse with a single bounded stderr line + exit `64`
  on Windows; the actual `wsl.exe` invocation belongs to WWS04.
- §6 Versioning contract (single shared `0.1.0-beta.1`; no
  bump at WWS02).
- §7 Release contract (release-please manifest mode + OIDC
  trusted publishing). NPM10 bootstrap workflow remains
  committed-but-not-dispatched. WWS01 §14.1 RECOMMENDS keeping
  it paused until at minimum `WWS02 + WWS04 + WWS05 + WWS06 +
  WWS08` land — publishing a known-broken `beta.1` would burn a
  beta number.
- §8 Cursor contract (stanza shape unchanged; WWS05 ships the
  config writer; WWS06 wires the setup CLI).
- §9 Safety contract — extended with the bridge invariants in
  WWS01 §4.4 (whitelist-validated distro, argv-array spawn,
  `shell: false`, no hidden-window option). No new MCP tools. MCP
  guard greps remain clean.
- §10 NPM03–NPM07 per-goal recommendations.
- §11 Risks — extended at WWS01 §16 with R-WWS-01..R-WWS-10.
- §12 Alternatives considered.

Behavioral evidence (WWS02 verification commit):
- Resolver tests: 20 cases pass (12 NPM03 baseline + 8 WWS02
  cases including `win32/x64` `bridge_required`, `win32/arm64`
  `bridge_required`, `freebsd` regression guard,
  formatResolveError bounded ASCII single-line bridge case,
  and three shim spawn-no-`wsl.exe` static guards).
- `npm pack` dry-runs clean on all three packages with no file
  count or version change.
- Platform-package `package.json` files diff empty after WWS02
  (`packages/terminal-commander-linux-x64/package.json` and
  `-linux-arm64/package.json` unchanged).

WWS chain landing follow-up (recorded at WWS08, docs-only): WWS03
(`lib/wsl/{distro-name,detect,doctor}.js`, commit `ec8441e`),
WWS04 (`lib/wsl/spawn.js`, commit `d86e73f`), WWS05
(`lib/cursor/{config,write,index}.js`, commit `ae37878`), WWS06
(`lib/cli/**`, commit `4936904`), WWS07 (`scripts/smoke/verify-windows-bridge-smoke.ps1`,
commit `785d410`), WWS08 (this commit) ALL landed AFTER the WWS02
package contract change above. NONE of them modified any other
field in this contract: root `os` stayed at `["linux", "win32"]`;
platform packages stayed at `os: ["linux"]`; `optionalDependencies`
exact-pin preserved; `0.1.0-beta.1` preserved across all three
packages; root `optionalDependencies` shape unchanged. The pack
file counts grew at WWS03/WWS04/WWS05/WWS06 only because the
chain added JS-only modules to the root package's `lib/` (root
tarball: 7 files at NPM03 → 23 at WWS06; both platform packages
remain 5 files unchanged). WWS07 added one file under
`scripts/` (NOT in any npm pack). WWS08 is docs-only (NOT in any
npm pack).

## 14. Evidence

- NPM01 audit: `docs/release/npm-distribution-audit.md` (commit
  `5dcbaa4`).
- npm name-availability check: WSL2 `npm 10.9.7`, 2026-05-23. Four
  names probed; all E404 (free).
- Symforge precedent (read-only at NPM01): single package +
  postinstall + `NPM_TOKEN` + crates.io dual publish — all
  rejected per §4.2 + §6 + §7.
- Terminal Commander runtime evidence (preserved from
  `EVIDENCE_REPORT_RUNTIME.md` + TC48): 29-tool MCP surface, two
  MCP guard greps clean, TC46 local smoke passing, TC47 load
  regression 8/8.
- No live provider transcript captured at NPM02 (Cursor smoke is
  NPM08 scope).
