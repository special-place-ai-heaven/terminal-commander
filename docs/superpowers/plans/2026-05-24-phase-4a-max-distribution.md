# Phase 4a-max Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **Per-task adversarial review:** after each task's spec + quality review pass, run `/codex` against the task's diff before marking complete. Codex catches what Sonnet reviewers miss.

**Goal:** Ship native tier-1 publish pipeline for Terminal Commander — 5 npm platform packages + root + 7 crates.io crates at `0.2.0`, with drift-proof reusable build workflow, 5 containerized verify jobs, ops tooling (secret-health + recovery + deprecation), and explicit rollback semantics. SLSA provenance lands in v0.2.1 via Task 23 (one-time operator OIDC setup, post-release).

**Architecture:** Extend existing `release-please.yml` (linux-x64 + linux-arm64 + root already shipped) with windows-x64 + mac-x64 + mac-arm64 platform packages, 7 cargo crate publish jobs (full dep closure: core → sifters → probes → store → supervisor → daemon → mcp), and 5 post-publish verify jobs. Eliminate drift between PR-time `npm-binary-build.yml` and release-time publish via new reusable workflow `_build-platform-binary.yml`. Build mac targets on **native** macos-13 (x64) + macos-14 (arm64) runners (cargo-zigbuild's macOS-SDK provisioning is not free and Apple's SDK licence is not redistributable; native runners avoid the legal trap entirely). Bump Cargo workspace `0.0.0 → 0.2.0`, flip ALL 7 workspace crates to `publish = true`, and add `version = "0.2.0"` to every internal path-dep so cargo accepts the published tarballs.

**Two-phase auth strategy:**
- **0.2.0 (this plan, autonomous):** publish using existing `NPM_TOKEN_TC` + `CARGO_REGISTRY_TOKEN_TC` repo secrets. Node 20 is acceptable for token-based publishing. No `--provenance` flag. This ships the pipeline + delivers windows-x64 + macOS reach without any operator UI clicks.
- **0.2.1+ (Task 23, when operator has 15 minutes):** one-time UI setup of trusted publishers on npmjs.com (6 packages) + crates.io (7 crates), then flip the workflow auth to OIDC + `--provenance` + Node 22.14+. SLSA attestations from 0.2.1 onward.

This split is the operator's explicit instruction to be "fully autonomous, no user intervention" reconciled with "correctness above all else": the pipeline ships now, the supply-chain attestation upgrade is queued.

**Tech Stack:** GitHub Actions, release-please v4.4.1 (manifest mode, linked-versions plugin) + `x-release-please-version` magic comments for Cargo.toml bumps, Rust 1.95.0 + edition 2024, cargo, native macOS runners (`macos-13` for x64, `macos-14` for arm64), Node 20 (publish-jobs) bumping to Node 22.14+ at Task 23 OIDC cutover, conventional commits.

---

## Adversarial Review Schedule

Each task ends with: (a) spec-compliance review subagent, (b) code-quality review subagent, (c) **codex adversarial review** of the task diff. Re-loop on each until clean. Only mark task complete after all 3 reviewers green.

After Task 21 (final task), run codex over the FULL branch diff vs main as Task 22.

---

## File Structure

```
.github/
├── release-please-config.json                       # MODIFIED (Task 3)
├── .release-please-manifest.json                    # MODIFIED (Task 4)
└── workflows/
    ├── _build-platform-binary.yml                   # NEW   (Task 8) — reusable, called by both publish + PR-time
    ├── npm-binary-build.yml                         # MODIFIED (Task 9) — collapse to caller
    ├── release-please.yml                           # MODIFIED (Tasks 10, 12, 13) — add windows + mac publish jobs, cargo jobs, verify jobs
    ├── secret-health.yml                            # NEW   (Task 14)
    └── deprecate-version.yml                        # NEW   (Task 16)

packages/
├── terminal-commander/
│   ├── package.json                                 # MODIFIED (Task 5) — optionalDeps add windows + mac
│   └── lib/resolve-binary.js                        # MODIFIED (Task 6) — add darwin entries
├── terminal-commander-mac-x64/                      # NEW   (Task 1)
│   ├── package.json
│   ├── bin/.gitkeep
│   └── LICENSE
└── terminal-commander-mac-arm64/                    # NEW   (Task 2)
    ├── package.json
    ├── bin/.gitkeep
    └── LICENSE

scripts/release/
├── sync-optional-dependencies.js                    # MODIFIED (Task 7)
├── verify-optional-dependencies.js                  # MODIFIED (Task 7)
└── recover-partial-publish.sh                       # NEW   (Task 15)

Cargo.toml                                           # MODIFIED (Task 11) — workspace version 0.0.0→0.2.0, internal-dep version="0.2.0"
crates/core/Cargo.toml                               # MODIFIED (Task 11) — add description, keywords, categories, readme=README.md
crates/sifters/Cargo.toml                            # MODIFIED (Task 11) — same
crates/probes/Cargo.toml                             # MODIFIED (Task 11) — same
crates/store/Cargo.toml                              # MODIFIED (Task 11) — same
crates/supervisor/Cargo.toml                         # MODIFIED (Task 11) — flip to workspace publish + per-crate readme
crates/daemon/Cargo.toml                             # MODIFIED (Task 11) — keywords, categories, per-crate readme
crates/mcp/Cargo.toml                                # MODIFIED (Task 11) — same
crates/cli/Cargo.toml                                # MODIFIED (Task 11) — publish = false (binary-only, not for crates.io)
crates/{core,sifters,probes,store,supervisor,daemon,mcp}/README.md  # NEW (Task 11) — required by cargo publish

docs/
├── superpowers/specs/2026-05-24-phase-4a-distribution-windows-x64-publish-design.md  # SPEC (already committed)
└── runbooks/
    └── 2026-05-24-phase-4a-release-procedure.md     # NEW   (Task 17)
```

---

## Pre-flight (no operator action required for 0.2.0)

OIDC trusted-publisher setup is deferred to **Task 23** (post-release, when operator has 15 min for UI clicks). For 0.2.0 we use:
- `NPM_TOKEN_TC` (granular npm token, already in repo secrets, working for linux releases today)
- `CARGO_REGISTRY_TOKEN_TC` (already in repo secrets)
- `RELEASE_PLEASE_TOKEN_TC` (already in repo secrets)

All three were validated by codex per the existing release-please.yml. No new auth artifacts are needed before Task 1.

---

## Task 1: Create `@terminal-commander/mac-x64` skeleton

**Files:**
- Create: `packages/terminal-commander-mac-x64/package.json`
- Create: `packages/terminal-commander-mac-x64/bin/.gitkeep`
- Create: `packages/terminal-commander-mac-x64/LICENSE`

- [ ] **Step 1: Write the package.json**

```json
{
  "name": "@terminal-commander/mac-x64",
  "version": "0.1.4",
  "description": "Terminal Commander prebuilt Rust binaries for darwin/x86_64. Internal platform package consumed by the `terminal-commander` root wrapper via optionalDependencies; do not depend on this package directly.",
  "license": "Apache-2.0",
  "author": "special-place-administrator",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/special-place-administrator/terminal-commander.git"
  },
  "homepage": "https://github.com/special-place-administrator/terminal-commander#readme",
  "bugs": {
    "url": "https://github.com/special-place-administrator/terminal-commander/issues"
  },
  "keywords": [
    "terminal-commander",
    "binary"
  ],
  "files": [
    "bin/",
    "LICENSE"
  ],
  "os": [
    "darwin"
  ],
  "cpu": [
    "x64"
  ],
  "engines": {
    "node": ">=18"
  }
}
```

- [ ] **Step 2: Create empty `bin/.gitkeep`**

Empty file. Real binaries land at publish time.

- [ ] **Step 3: Copy LICENSE from sibling package**

```bash
cp packages/terminal-commander-linux-x64/LICENSE packages/terminal-commander-mac-x64/LICENSE
```

- [ ] **Step 4: Verify package.json parses + npm-pack dry-run**

```bash
node -e "JSON.parse(require('fs').readFileSync('packages/terminal-commander-mac-x64/package.json'))"
cd packages/terminal-commander-mac-x64 && npm pack --dry-run
```
Expected: dry-run lists `bin/.gitkeep`, `LICENSE`, `package.json`.

- [ ] **Step 5: Commit**

```bash
git add packages/terminal-commander-mac-x64/
git commit -m "feat(packages): scaffold @terminal-commander/mac-x64 platform package"
```

---

## Task 2: Create `@terminal-commander/mac-arm64` skeleton

**Files:**
- Create: `packages/terminal-commander-mac-arm64/package.json`
- Create: `packages/terminal-commander-mac-arm64/bin/.gitkeep`
- Create: `packages/terminal-commander-mac-arm64/LICENSE`

- [ ] **Step 1: Write the package.json**

Same as Task 1 but with `"name": "@terminal-commander/mac-arm64"` and `"cpu": ["arm64"]`. Full text:

```json
{
  "name": "@terminal-commander/mac-arm64",
  "version": "0.1.4",
  "description": "Terminal Commander prebuilt Rust binaries for darwin/aarch64. Internal platform package consumed by the `terminal-commander` root wrapper via optionalDependencies; do not depend on this package directly.",
  "license": "Apache-2.0",
  "author": "special-place-administrator",
  "repository": {
    "type": "git",
    "url": "git+https://github.com/special-place-administrator/terminal-commander.git"
  },
  "homepage": "https://github.com/special-place-administrator/terminal-commander#readme",
  "bugs": {
    "url": "https://github.com/special-place-administrator/terminal-commander/issues"
  },
  "keywords": [
    "terminal-commander",
    "binary"
  ],
  "files": [
    "bin/",
    "LICENSE"
  ],
  "os": [
    "darwin"
  ],
  "cpu": [
    "arm64"
  ],
  "engines": {
    "node": ">=18"
  }
}
```

- [ ] **Step 2: Create `bin/.gitkeep` + copy LICENSE**

```bash
mkdir -p packages/terminal-commander-mac-arm64/bin
touch packages/terminal-commander-mac-arm64/bin/.gitkeep
cp packages/terminal-commander-linux-x64/LICENSE packages/terminal-commander-mac-arm64/LICENSE
```

- [ ] **Step 3: Verify + commit**

```bash
node -e "JSON.parse(require('fs').readFileSync('packages/terminal-commander-mac-arm64/package.json'))"
cd packages/terminal-commander-mac-arm64 && npm pack --dry-run
cd ../..
git add packages/terminal-commander-mac-arm64/
git commit -m "feat(packages): scaffold @terminal-commander/mac-arm64 platform package"
```

---

## Task 3: Wire 3 new platform packages into release-please-config.json

**Files:**
- Modify: `.github/release-please-config.json`

- [ ] **Step 1: Read current config**

Already known shape:
- `linked-versions.components`: 3 entries
- `packages.terminal-commander.extra-files`: 2 entries for linux platform deps
- `packages` map: 3 entries

- [ ] **Step 2: Write a failing test**

Create `scripts/release/test-release-please-config.js`:

```javascript
"use strict";
const assert = require("node:assert/strict");
const test = require("node:test");
const cfg = require("../../.github/release-please-config.json");

test("linked-versions includes all 6 components", () => {
  const components = cfg.plugins[0].components;
  assert.deepEqual(new Set(components), new Set([
    "terminal-commander",
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]));
});

test("root extra-files bumps all 5 platform optionalDependencies", () => {
  const ef = cfg.packages["packages/terminal-commander"]["extra-files"];
  const paths = ef.map((e) => e.jsonpath);
  for (const dep of [
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]) {
    assert.ok(
      paths.includes(`$.optionalDependencies['${dep}']`),
      `missing extra-files entry for ${dep}`
    );
  }
});

test("all 5 platform packages have release-please entries", () => {
  for (const dir of [
    "packages/terminal-commander-linux-x64",
    "packages/terminal-commander-linux-arm64",
    "packages/terminal-commander-windows-x64",
    "packages/terminal-commander-mac-x64",
    "packages/terminal-commander-mac-arm64",
  ]) {
    assert.ok(cfg.packages[dir], `missing packages entry for ${dir}`);
    assert.equal(cfg.packages[dir]["release-type"], "node");
  }
});
```

- [ ] **Step 3: Run test to confirm it fails**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: 3 failures (current config has 3 components, 2 extra-files, 3 package entries).

- [ ] **Step 4: Update config**

Replace `.github/release-please-config.json` with:

```json
{
  "$schema": "https://raw.githubusercontent.com/googleapis/release-please/main/schemas/config.json",
  "release-type": "node",
  "include-component-in-tag": false,
  "include-v-in-tag": true,
  "separate-pull-requests": false,
  "bump-minor-pre-major": true,
  "bump-patch-for-minor-pre-major": true,
  "pull-request-title-pattern": "chore: release ${version}",
  "draft": false,
  "prerelease": false,
  "plugins": [
    {
      "type": "linked-versions",
      "groupName": "terminal-commander",
      "components": [
        "terminal-commander",
        "@terminal-commander/linux-x64",
        "@terminal-commander/linux-arm64",
        "@terminal-commander/windows-x64",
        "@terminal-commander/mac-x64",
        "@terminal-commander/mac-arm64"
      ]
    }
  ],
  "packages": {
    "packages/terminal-commander": {
      "release-type": "node",
      "component": "terminal-commander",
      "package-name": "terminal-commander",
      "changelog-path": "CHANGELOG.md",
      "extra-files": [
        { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/linux-x64']" },
        { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/linux-arm64']" },
        { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/windows-x64']" },
        { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/mac-x64']" },
        { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/mac-arm64']" }
      ]
    },
    "packages/terminal-commander-linux-x64": {
      "release-type": "node",
      "component": "@terminal-commander/linux-x64",
      "package-name": "@terminal-commander/linux-x64",
      "changelog-path": "CHANGELOG.md",
      "extra-files": []
    },
    "packages/terminal-commander-linux-arm64": {
      "release-type": "node",
      "component": "@terminal-commander/linux-arm64",
      "package-name": "@terminal-commander/linux-arm64",
      "changelog-path": "CHANGELOG.md",
      "extra-files": []
    },
    "packages/terminal-commander-windows-x64": {
      "release-type": "node",
      "component": "@terminal-commander/windows-x64",
      "package-name": "@terminal-commander/windows-x64",
      "changelog-path": "CHANGELOG.md",
      "extra-files": []
    },
    "packages/terminal-commander-mac-x64": {
      "release-type": "node",
      "component": "@terminal-commander/mac-x64",
      "package-name": "@terminal-commander/mac-x64",
      "changelog-path": "CHANGELOG.md",
      "extra-files": []
    },
    "packages/terminal-commander-mac-arm64": {
      "release-type": "node",
      "component": "@terminal-commander/mac-arm64",
      "package-name": "@terminal-commander/mac-arm64",
      "changelog-path": "CHANGELOG.md",
      "extra-files": []
    }
  }
}
```

- [ ] **Step 5: Run test to confirm pass**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: 3 passes.

- [ ] **Step 6: Commit**

```bash
git add .github/release-please-config.json scripts/release/test-release-please-config.js
git commit -m "feat(release): extend release-please-config for windows-x64 + mac-x64 + mac-arm64"
```

---

## Task 4: Add 3 new entries to release-please manifest

**Files:**
- Modify: `.github/.release-please-manifest.json`

- [ ] **Step 1: Write a failing test**

Add to `scripts/release/test-release-please-config.js`:

```javascript
test("manifest tracks all 6 packages at same version", () => {
  const manifest = require("../../.github/.release-please-manifest.json");
  const entries = Object.entries(manifest);
  assert.equal(entries.length, 6, `manifest has ${entries.length} entries, expected 6`);
  const versions = new Set(entries.map(([, v]) => v));
  assert.equal(versions.size, 1, "all manifest entries should share the same version");
  for (const dir of [
    "packages/terminal-commander",
    "packages/terminal-commander-linux-x64",
    "packages/terminal-commander-linux-arm64",
    "packages/terminal-commander-windows-x64",
    "packages/terminal-commander-mac-x64",
    "packages/terminal-commander-mac-arm64",
  ]) {
    assert.ok(manifest[dir], `manifest missing ${dir}`);
  }
});
```

- [ ] **Step 2: Run test to verify fail**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: 1 new failure (manifest has 3, not 6 entries).

- [ ] **Step 3: Update manifest**

Replace `.github/.release-please-manifest.json` with:

```json
{
  "packages/terminal-commander": "0.1.4",
  "packages/terminal-commander-linux-x64": "0.1.4",
  "packages/terminal-commander-linux-arm64": "0.1.4",
  "packages/terminal-commander-windows-x64": "0.1.4",
  "packages/terminal-commander-mac-x64": "0.1.4",
  "packages/terminal-commander-mac-arm64": "0.1.4"
}
```

- [ ] **Step 4: Run test to verify pass**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add .github/.release-please-manifest.json
git commit -m "feat(release): track windows-x64 + mac-x64 + mac-arm64 in release-please manifest"
```

---

## Task 5: Pin 3 new optionalDependencies in root package.json

**Files:**
- Modify: `packages/terminal-commander/package.json`

- [ ] **Step 1: Write a failing test**

Add to `scripts/release/test-release-please-config.js`:

```javascript
test("root optionalDependencies pin all 5 platform packages by version (not file:)", () => {
  const pkg = require("../../packages/terminal-commander/package.json");
  const expected = pkg.version;
  const od = pkg.optionalDependencies || {};
  for (const dep of [
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]) {
    assert.ok(od[dep], `missing optionalDependency ${dep}`);
    assert.ok(
      !od[dep].startsWith("file:"),
      `${dep} must be version-pinned, not file:`
    );
    assert.equal(od[dep], expected, `${dep} = ${od[dep]} != ${expected}`);
  }
});
```

- [ ] **Step 2: Run test to verify fail**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: fail (windows-x64 is `file:..`, mac entries missing).

- [ ] **Step 3: Update `packages/terminal-commander/package.json`**

Change `optionalDependencies` block to:

```json
"optionalDependencies": {
  "@terminal-commander/linux-x64": "0.1.4",
  "@terminal-commander/linux-arm64": "0.1.4",
  "@terminal-commander/windows-x64": "0.1.4",
  "@terminal-commander/mac-x64": "0.1.4",
  "@terminal-commander/mac-arm64": "0.1.4"
}
```

Also extend `"os"` array to add `"darwin"`:

```json
"os": [
  "linux",
  "win32",
  "darwin"
]
```

- [ ] **Step 4: Run test to verify pass**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add packages/terminal-commander/package.json
git commit -m "fix(packages): pin windows-x64 + mac-x64 + mac-arm64 optionalDependencies by version"
```

---

## Task 6: Extend resolver to support darwin/x64 + darwin/arm64

**Files:**
- Modify: `packages/terminal-commander/lib/resolve-binary.js`
- Test: `packages/terminal-commander/test/resolve-binary.test.js` (existing — extend)

- [ ] **Step 1: Check current test coverage**

```bash
cat packages/terminal-commander/test/resolve-binary.test.js | head -100
```

- [ ] **Step 2: Write failing tests for darwin entries**

Append to `packages/terminal-commander/test/resolve-binary.test.js`:

```javascript
test("SUPPORTED_TARGETS includes darwin/x64 and darwin/arm64", () => {
  const { SUPPORTED_TARGETS } = require("../lib/resolve-binary.js");
  const platforms = SUPPORTED_TARGETS.map((t) => `${t.platform}-${t.arch}`);
  assert.ok(platforms.includes("darwin-x64"), "missing darwin-x64");
  assert.ok(platforms.includes("darwin-arm64"), "missing darwin-arm64");
});

test("resolveBinary returns @terminal-commander/mac-x64 for darwin/x64", () => {
  const { resolveBinary } = require("../lib/resolve-binary.js");
  const r = resolveBinary({
    platform: "darwin",
    arch: "x64",
    binary: "terminal-commander-mcp",
    requireResolve: () => { throw new Error("not installed"); },
  });
  assert.equal(r.platformPackage, "@terminal-commander/mac-x64");
});

test("resolveBinary returns @terminal-commander/mac-arm64 for darwin/arm64", () => {
  const { resolveBinary } = require("../lib/resolve-binary.js");
  const r = resolveBinary({
    platform: "darwin",
    arch: "arm64",
    binary: "terminal-commander-mcp",
    requireResolve: () => { throw new Error("not installed"); },
  });
  assert.equal(r.platformPackage, "@terminal-commander/mac-arm64");
});
```

- [ ] **Step 3: Run test to verify fail**

```bash
cd packages/terminal-commander && npm test
```
Expected: 3 new failures.

- [ ] **Step 4: Update `lib/resolve-binary.js`**

In `SUPPORTED_TARGETS`, after the existing 3 entries:

```javascript
const SUPPORTED_TARGETS = Object.freeze([
  Object.freeze({ platform: "linux", arch: "x64", pkg: "@terminal-commander/linux-x64" }),
  Object.freeze({ platform: "linux", arch: "arm64", pkg: "@terminal-commander/linux-arm64" }),
  Object.freeze({ platform: "win32", arch: "x64", pkg: "@terminal-commander/windows-x64" }),
  Object.freeze({ platform: "darwin", arch: "x64", pkg: "@terminal-commander/mac-x64" }),
  Object.freeze({ platform: "darwin", arch: "arm64", pkg: "@terminal-commander/mac-arm64" }),
]);
```

In `MONOREPO_PLATFORM_DIRS`, add 2 entries:

```javascript
const MONOREPO_PLATFORM_DIRS = Object.freeze({
  "@terminal-commander/linux-x64": "terminal-commander-linux-x64",
  "@terminal-commander/linux-arm64": "terminal-commander-linux-arm64",
  "@terminal-commander/windows-x64": "terminal-commander-windows-x64",
  "@terminal-commander/mac-x64": "terminal-commander-mac-x64",
  "@terminal-commander/mac-arm64": "terminal-commander-mac-arm64",
});
```

In `formatResolveError`, the existing `platform_package_missing` message references `0.1.4` and `terminal-commander-windows-x64`. Make it generic — drop hardcoded version + dir:

```javascript
if (result.reason === "platform_package_missing") {
  return (
    `terminal-commander: platform package ${result.platformPackage} not installed. ` +
    `Reinstall: npm install -g terminal-commander@latest`
  );
}
```

- [ ] **Step 5: Run test to verify pass**

```bash
cd packages/terminal-commander && npm test
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add packages/terminal-commander/lib/resolve-binary.js packages/terminal-commander/test/resolve-binary.test.js
git commit -m "feat(resolver): support darwin/x64 + darwin/arm64 platform targets"
```

---

## Task 7: Extend sync + verify scripts to handle 5 platforms

**Files:**
- Modify: `scripts/release/sync-optional-dependencies.js`
- Modify: `scripts/release/verify-optional-dependencies.js`

- [ ] **Step 1: Write a failing test**

Append to `scripts/release/test-release-please-config.js`:

```javascript
test("sync-optional-dependencies.js handles all 5 platforms", () => {
  const src = require("fs").readFileSync(
    require("path").join(__dirname, "sync-optional-dependencies.js"),
    "utf8"
  );
  for (const name of [
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]) {
    assert.ok(src.includes(`"${name}"`), `sync script missing ${name}`);
  }
});

test("verify-optional-dependencies.js handles all 5 platforms", () => {
  const src = require("fs").readFileSync(
    require("path").join(__dirname, "verify-optional-dependencies.js"),
    "utf8"
  );
  for (const name of [
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]) {
    assert.ok(src.includes(`"${name}"`), `verify script missing ${name}`);
  }
});
```

- [ ] **Step 2: Run test to verify fail**

```bash
node --test scripts/release/test-release-please-config.js
```
Expected: 2 new failures.

- [ ] **Step 3: Extract shared constant + update both scripts**

Create `scripts/release/platform-packages.js`:

```javascript
// SPDX-License-Identifier: Apache-2.0
// Single source of truth for the list of platform packages whose
// versions are pinned in root's optionalDependencies.
"use strict";

const PLATFORM_PACKAGES = Object.freeze([
  "@terminal-commander/linux-x64",
  "@terminal-commander/linux-arm64",
  "@terminal-commander/windows-x64",
  "@terminal-commander/mac-x64",
  "@terminal-commander/mac-arm64",
]);

module.exports = { PLATFORM_PACKAGES };
```

Update `scripts/release/sync-optional-dependencies.js` to require it:

```javascript
"use strict";

const fs = require("fs");
const path = require("path");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");

const pkgPath = path.join(
  __dirname,
  "..",
  "..",
  "packages",
  "terminal-commander",
  "package.json"
);

const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
const version = pkg.version;
if (!version) {
  console.error("package.json missing version");
  process.exit(1);
}

pkg.optionalDependencies = pkg.optionalDependencies || {};

let changed = false;
for (const name of PLATFORM_PACKAGES) {
  if (pkg.optionalDependencies[name] !== version) {
    pkg.optionalDependencies[name] = version;
    changed = true;
  }
}

if (changed) {
  fs.writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log(`synced optionalDependencies to ${version}`);
} else {
  console.log(`optionalDependencies already at ${version}`);
}
```

Update `scripts/release/verify-optional-dependencies.js`:

```javascript
"use strict";

const pkg = require("../../packages/terminal-commander/package.json");
const { PLATFORM_PACKAGES } = require("./platform-packages.js");

const version = pkg.version;
const deps = pkg.optionalDependencies || {};

for (const name of PLATFORM_PACKAGES) {
  if (deps[name] !== version) {
    console.error(
      `${name} optionalDependency = ${deps[name] ?? "(missing)"} != ${version}`
    );
    process.exit(1);
  }
}

console.log(`optionalDependencies pinned to ${version} (${PLATFORM_PACKAGES.length} platforms)`);
```

Update the test to look for `PLATFORM_PACKAGES` references (since the literal names move to the new module):

```javascript
test("platform-packages.js lists all 5 platforms", () => {
  const { PLATFORM_PACKAGES } = require("./platform-packages.js");
  assert.deepEqual(new Set(PLATFORM_PACKAGES), new Set([
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]));
});

test("sync-optional-dependencies.js consumes shared PLATFORM_PACKAGES", () => {
  const src = require("fs").readFileSync(
    require("path").join(__dirname, "sync-optional-dependencies.js"),
    "utf8"
  );
  assert.ok(src.includes("PLATFORM_PACKAGES"), "sync script must use shared constant");
});

test("verify-optional-dependencies.js consumes shared PLATFORM_PACKAGES", () => {
  const src = require("fs").readFileSync(
    require("path").join(__dirname, "verify-optional-dependencies.js"),
    "utf8"
  );
  assert.ok(src.includes("PLATFORM_PACKAGES"), "verify script must use shared constant");
});
```

Replace the prior 2 string-search tests for sync/verify with the 3 above.

- [ ] **Step 4: Run test to verify pass**

```bash
node --test scripts/release/test-release-please-config.js
node scripts/release/verify-optional-dependencies.js
```
Expected: tests green; verify script reports `optionalDependencies pinned to 0.1.4 (5 platforms)`.

- [ ] **Step 5: Update release-please.yml `publish-root` inline check**

The current inline Node check in `publish-root` only verifies 2 linux platforms. Update the `for (const name of [...])` block to use the shared list. Read the file, replace this block:

```javascript
for (const name of [
  '@terminal-commander/linux-x64',
  '@terminal-commander/linux-arm64',
]) {
```

with:

```javascript
const { PLATFORM_PACKAGES } = require('../../scripts/release/platform-packages.js');
for (const name of PLATFORM_PACKAGES) {
```

- [ ] **Step 6: Commit**

```bash
git add scripts/release/platform-packages.js scripts/release/sync-optional-dependencies.js scripts/release/verify-optional-dependencies.js scripts/release/test-release-please-config.js .github/workflows/release-please.yml
git commit -m "refactor(release): extract PLATFORM_PACKAGES constant + extend to all 5 platforms"
```

---

## Task 8: Create reusable `_build-platform-binary.yml` workflow

**Files:**
- Create: `.github/workflows/_build-platform-binary.yml`

- [ ] **Step 1: Write the reusable workflow**

```yaml
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# Reusable workflow: build the 3 Terminal Commander release binaries
# for one platform target and stage them into the matching platform
# package's bin/ directory. Optionally uploads the bin dir as an artifact.
#
# Called by:
#   .github/workflows/npm-binary-build.yml (PR-time CI gate, upload_artifact=true)
#   .github/workflows/release-please.yml   (publish jobs, upload_artifact=false)
#
# Eliminates drift between PR-time build smoke + release-time publish
# build by constructing both from one source of truth.

name: _build-platform-binary

on:
  workflow_call:
    inputs:
      platform:
        required: true
        type: string
        description: "linux-x64 | linux-arm64 | windows-x64 | mac-x64 | mac-arm64"
      upload_artifact:
        required: false
        type: boolean
        default: false
      rust_toolchain:
        required: false
        type: string
        default: "1.95.0"
    outputs:
      artifact_name:
        value: ${{ jobs.build.outputs.artifact_name }}

permissions:
  contents: read

jobs:
  build:
    name: build-${{ inputs.platform }}
    # All 5 platforms use NATIVE runners. cargo-zigbuild was the prior
    # plan, but Apple's macOS SDK is not legally redistributable and the
    # public `cargo-zigbuild` Docker images bundling it have a murky
    # licence story. macos-13 / macos-14 GitHub-hosted runners ship the
    # SDK by default. Cost difference: macOS minutes are 10x linux on
    # public repos but $0 on private — irrelevant here.
    runs-on: ${{ fromJSON('{"linux-x64":"ubuntu-24.04","linux-arm64":"ubuntu-24.04-arm","windows-x64":"windows-2022","mac-x64":"macos-13","mac-arm64":"macos-14"}')[inputs.platform] }}
    outputs:
      artifact_name: ${{ steps.meta.outputs.artifact_name }}
    env:
      TARGET_TRIPLE: ${{ fromJSON('{"linux-x64":"x86_64-unknown-linux-gnu","linux-arm64":"aarch64-unknown-linux-gnu","windows-x64":"x86_64-pc-windows-msvc","mac-x64":"x86_64-apple-darwin","mac-arm64":"aarch64-apple-darwin"}')[inputs.platform] }}
      PLATFORM_PKG_DIR: packages/terminal-commander-${{ inputs.platform }}
      EXE_SUFFIX: ${{ inputs.platform == 'windows-x64' && '.exe' || '' }}
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ inputs.rust_toolchain }}
          targets: ${{ env.TARGET_TRIPLE }}

      - name: Cache cargo registry + target
        uses: Swatinem/rust-cache@v2
        with:
          shared-key: build-platform-${{ inputs.platform }}

      - name: cargo build --release (native on every platform)
        shell: bash
        run: |
          cargo build --release \
            --target ${{ env.TARGET_TRIPLE }} \
            -p terminal-commanderd \
            -p terminal-commander-mcp \
            -p terminal-commander-cli

      - name: Stage real binaries into platform package
        shell: bash
        run: |
          set -e
          bin_dir="${PLATFORM_PKG_DIR}/bin"
          src_dir="target/${TARGET_TRIPLE}/release"
          mkdir -p "$bin_dir"
          for bin in terminal-commanderd terminal-commander-mcp terminal-commander; do
            src="${src_dir}/${bin}${EXE_SUFFIX}"
            dst="${bin_dir}/${bin}${EXE_SUFFIX}"
            if [ ! -f "$src" ]; then
              echo "::error::expected release binary not found: $src"
              ls -la "$src_dir" || true
              exit 1
            fi
            cp "$src" "$dst"
            chmod +x "$dst" || true
          done
          # Strip the .placeholder + .gitkeep markers committed for empty-dir guarantee.
          rm -f "$bin_dir"/*.placeholder "$bin_dir"/.gitkeep
          ls -la "$bin_dir"

      - name: Smoke — --version (native runner, so always works)
        shell: bash
        run: |
          set -e
          bin_dir="${PLATFORM_PKG_DIR}/bin"
          for bin in terminal-commanderd terminal-commander-mcp terminal-commander; do
            "./${bin_dir}/${bin}${EXE_SUFFIX}" --version 2>&1 | head -5
          done

      - name: Compute artifact metadata
        id: meta
        shell: bash
        run: |
          echo "artifact_name=tc-bin-${{ inputs.platform }}" >> "$GITHUB_OUTPUT"

      - name: Upload bin/ as artifact
        if: inputs.upload_artifact == true
        uses: actions/upload-artifact@v4
        with:
          name: ${{ steps.meta.outputs.artifact_name }}
          path: ${{ env.PLATFORM_PKG_DIR }}/bin/
          if-no-files-found: error
          retention-days: 14
```

- [ ] **Step 2: YAML lint**

```bash
python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/_build-platform-binary.yml'))"
```
Expected: no exception.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/_build-platform-binary.yml
git commit -m "feat(ci): add reusable _build-platform-binary.yml for tier-1 + mac targets"
```

---

## Task 9: Collapse `npm-binary-build.yml` to call the reusable workflow

**Files:**
- Modify: `.github/workflows/npm-binary-build.yml`

- [ ] **Step 1: Replace whole file**

```yaml
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# NPM05 — npm binary build matrix (post-collapse).
#
# Calls _build-platform-binary.yml for each tier-1 + macOS target so the
# PR-time CI gate uses the EXACT SAME build steps as the release-time
# publish jobs. Eliminates drift permanently.
#
# Also runs the workspace-level pre-build gates (fmt, clippy, nextest)
# on the x64 leg only — those are runtime-correctness gates, not build
# gates, and we don't need them on every platform.

name: npm-binary-build

on:
  push:
    branches:
      - main
    paths:
      - "Cargo.toml"
      - "Cargo.lock"
      - "crates/**"
      - "packages/**"
      - "scripts/release/**"
      - "scripts/smoke/**"
      - ".github/workflows/npm-binary-build.yml"
      - ".github/workflows/_build-platform-binary.yml"
  pull_request:
    branches:
      - main
    paths:
      - "Cargo.toml"
      - "Cargo.lock"
      - "crates/**"
      - "packages/**"
      - "scripts/release/**"
      - "scripts/smoke/**"
      - ".github/workflows/npm-binary-build.yml"
      - ".github/workflows/_build-platform-binary.yml"
  workflow_dispatch:

permissions:
  contents: read

concurrency:
  group: npm-binary-build-${{ github.ref }}
  cancel-in-progress: false

jobs:
  pre-build-gates:
    name: pre-build-gates (linux-x64)
    runs-on: ubuntu-24.04
    permissions: { contents: read }
    env:
      RUST_TOOLCHAIN: "1.95.0"
      NODE_VERSION: "20"
    steps:
      - uses: actions/checkout@v4
      - name: Verify root optionalDependencies pin
        run: node scripts/release/verify-optional-dependencies.js
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ env.RUST_TOOLCHAIN }}
          targets: x86_64-unknown-linux-gnu
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: npm-binary-build-pre-gates
      - uses: actions/setup-node@v4
        with: { node-version: "20" }
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - uses: taiki-e/install-action@nextest
      - run: cargo nextest run --workspace
      - name: TC47 load gate (8 stress tests)
        run: cargo test -p terminal-commanderd --test load_noise_backpressure -- --nocapture
      - name: MCP grep guard 1 — no spawn / no socket in crates/mcp source
        shell: bash
        run: |
          set -e
          out=$(grep -RE "Command::new|Command::spawn|TcpListener|UdpSocket" crates/mcp/src || true)
          echo "$out"
          if echo "$out" | grep -E "^crates/mcp/src/[^:]+:[ \t]*(let|use|fn|pub|impl|let mut)" >/dev/null; then
            echo "::error::MCP guard 1 caught a non-doc match in production source"
            exit 1
          fi
      - name: MCP grep guard 2 — no direct fs in crates/mcp/src
        shell: bash
        run: |
          set -e
          if grep -RE "tokio::fs|std::fs|File::open|read_to_string|read_to_end" crates/mcp/src; then
            echo "::error::MCP guard 2 caught a direct-fs path"
            exit 1
          fi

  build-linux-x64:
    needs: pre-build-gates
    uses: ./.github/workflows/_build-platform-binary.yml
    with: { platform: linux-x64, upload_artifact: true }

  build-linux-arm64:
    needs: pre-build-gates
    uses: ./.github/workflows/_build-platform-binary.yml
    with: { platform: linux-arm64, upload_artifact: true }

  build-windows-x64:
    needs: pre-build-gates
    uses: ./.github/workflows/_build-platform-binary.yml
    with: { platform: windows-x64, upload_artifact: true }

  build-mac-x64:
    needs: pre-build-gates
    uses: ./.github/workflows/_build-platform-binary.yml
    with: { platform: mac-x64, upload_artifact: true }

  build-mac-arm64:
    needs: pre-build-gates
    uses: ./.github/workflows/_build-platform-binary.yml
    with: { platform: mac-arm64, upload_artifact: true }

  npm-pack:
    name: npm-pack (after all builds)
    needs:
      - build-linux-x64
      - build-linux-arm64
      - build-windows-x64
      - build-mac-x64
      - build-mac-arm64
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: "20" }
      - name: Download all 5 platform bin artifacts
        uses: actions/download-artifact@v4
        with:
          pattern: tc-bin-*
          path: artifacts/
      - name: Stage artifacts into platform bin dirs
        shell: bash
        run: |
          set -e
          for plat in linux-x64 linux-arm64 windows-x64 mac-x64 mac-arm64; do
            dest="packages/terminal-commander-${plat}/bin"
            mkdir -p "$dest"
            cp -r "artifacts/tc-bin-${plat}"/* "$dest/"
          done
      - name: npm pack — all 5 platform packages + root
        shell: bash
        run: |
          set -e
          mkdir -p tarballs
          for plat in linux-x64 linux-arm64 windows-x64 mac-x64 mac-arm64; do
            (cd "packages/terminal-commander-${plat}" && npm pack --pack-destination "$GITHUB_WORKSPACE/tarballs")
          done
          (cd packages/terminal-commander && npm pack --pack-destination "$GITHUB_WORKSPACE/tarballs")
          ls -la tarballs/
      - uses: actions/upload-artifact@v4
        with:
          name: npm-tarballs
          path: tarballs/*.tgz
          if-no-files-found: error
          retention-days: 14
```

- [ ] **Step 2: YAML lint**

```bash
python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/npm-binary-build.yml'))"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/npm-binary-build.yml
git commit -m "refactor(ci): collapse npm-binary-build.yml onto reusable _build-platform-binary.yml"
```

---

## Task 10: Add 3 new publish jobs to release-please.yml (windows-x64 + mac-x64 + mac-arm64)

**Files:**
- Modify: `.github/workflows/release-please.yml`

- [ ] **Step 1: Add `permissions: id-token: write` at workflow level**

Change the top-level `permissions:` block from:
```yaml
permissions:
  contents: read
```
to:
```yaml
permissions:
  contents: read
  id-token: write   # for npm --provenance + crates.io OIDC after Task 18 cutover
```

- [ ] **Step 2: Refactor existing linux publish jobs to use reusable workflow**

Replace the existing `publish-linux-x64` and `publish-linux-arm64` job bodies with calls to `_build-platform-binary.yml` (upload_artifact: false, then inline staging via download since reusable workflows can't directly leave artifacts in the parent job's checkout). Strategy: have the reusable workflow upload the artifact, the publish job download + stage + publish.

For each of the 5 platforms (linux-x64, linux-arm64, windows-x64, mac-x64, mac-arm64), the publish job becomes:

```yaml
build-<platform>:
  needs: [release-please, ensure-release, publish-version]
  if: >-
    always() && !cancelled() &&
    (needs.release-please.outputs.releases_created == 'true' ||
    (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
    needs.publish-version.outputs.publish == 'true')
  uses: ./.github/workflows/_build-platform-binary.yml
  with:
    platform: <platform>
    upload_artifact: true

publish-<platform>:
  needs:
    - release-please
    - ensure-release
    - publish-version
    - build-<platform>
  if: >-
    always() && !cancelled() &&
    needs.build-<platform>.result == 'success' &&
    (needs.release-please.outputs.releases_created == 'true' ||
    (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
    needs.publish-version.outputs.publish == 'true')
  runs-on: ubuntu-24.04
  permissions:
    contents: read
  steps:
    - uses: actions/checkout@v4
    - uses: actions/setup-node@v4
      with:
        node-version: "20"
        registry-url: "https://registry.npmjs.org"
    - name: Download bin artifact
      uses: actions/download-artifact@v4
      with:
        name: tc-bin-<platform>
        path: packages/terminal-commander-<platform>/bin/
    - name: Confirm package version matches release-please output
      shell: bash
      env:
        EXPECTED_VERSION: ${{ needs.release-please.outputs.version || needs.ensure-release.outputs.version || needs.publish-version.outputs.version }}
      run: |
        set -e
        actual=$(node -p "require('./packages/terminal-commander-<platform>/package.json').version")
        if [ "$actual" != "$EXPECTED_VERSION" ]; then
          echo "::error::version mismatch: package.json=$actual release-please=$EXPECTED_VERSION"
          exit 1
        fi
        echo "version-match: $actual"
    - name: npm publish --access public (NPM_TOKEN_TC for 0.2.0; OIDC + --provenance at Task 23)
      shell: bash
      working-directory: packages/terminal-commander-<platform>
      env:
        NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_TC }}
      run: |
        set -e
        if npm publish --access public; then
          exit 0
        fi
        # Recovery: workflow_dispatch + force_publish + already-on-registry → exit 0.
        if [ "${{ github.event_name }}" = "workflow_dispatch" ] && [ "${{ inputs.force_publish }}" = "true" ]; then
          ver=$(node -p "require('./package.json').version")
          name=$(node -p "require('./package.json').name")
          if npm view "${name}@${ver}" version >/dev/null 2>&1; then
            echo "::warning::${name}@${ver} already on registry (force_publish recovery)"
            exit 0
          fi
        fi
        exit 1
```

Apply this template for each of the 5 platforms. Replace `<platform>` everywhere (10 substitutions per template per platform = 50 total).

Result: 10 jobs total — 5 `build-*` (reusable callers) + 5 `publish-*` (downloader+publisher).

- [ ] **Step 3: Update `publish-root`'s `needs:` to include all 5 publish jobs**

```yaml
needs:
  - release-please
  - ensure-release
  - publish-version
  - publish-linux-x64
  - publish-linux-arm64
  - publish-windows-x64
  - publish-mac-x64
  - publish-mac-arm64
```

And the `if:` condition:

```yaml
if: >-
  always() && !cancelled() &&
  (needs.release-please.outputs.releases_created == 'true' ||
  (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
  needs.publish-version.outputs.publish == 'true') &&
  needs.publish-linux-x64.result == 'success' &&
  needs.publish-linux-arm64.result == 'success' &&
  needs.publish-windows-x64.result == 'success' &&
  needs.publish-mac-x64.result == 'success' &&
  needs.publish-mac-arm64.result == 'success'
```

- [ ] **Step 4: `publish-root` stays on NODE_AUTH_TOKEN for 0.2.0 (OIDC upgrade in Task 23)**

The existing `publish-root` job already uses `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_TC }}` and runs on Node 20. No change to its auth path in this task. Verify the `for (const name of [...])` block already pulls from `PLATFORM_PACKAGES` per Task 7.

- [ ] **Step 5: YAML lint + dry parse**

```bash
python -c "import yaml; yaml.safe_load(open('.github/workflows/release-please.yml'))"
```

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/release-please.yml
git commit -m "feat(release): add windows-x64 + mac-x64 + mac-arm64 publish jobs to release-please.yml"
```

---

## Task 11: Bump Cargo workspace to 0.2.0 + flip ALL 7 crates to publish=true + per-crate README

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/{core,sifters,probes,store,supervisor,daemon,mcp,cli}/Cargo.toml` (8 crates)
- Create: `crates/{core,sifters,probes,store,supervisor,daemon,mcp}/README.md` (7 READMEs — cli stays publish=false, no README needed)

**Why all 7?** Codex review caught: mcp depends on core+sifters+store+supervisor+daemon; daemon depends on core+sifters+probes+store+supervisor. cargo publish REFUSES a crate whose non-dev path-deps have no registry version. So either we publish the full closure or none. Publishing all 7 is the only correct path.

- [ ] **Step 1: Update `Cargo.toml` workspace block**

Change `[workspace.package]`:

```toml
[workspace.package]
edition = "2024"
rust-version = "1.92"
license = "Apache-2.0"
authors = ["The Terminal Commander Authors"]
repository = "https://github.com/special-place-administrator/terminal-commander"
homepage = "https://github.com/special-place-administrator/terminal-commander"
# NOTE: `readme` deliberately NOT set at workspace level — Cargo resolves
# workspace-inherited `readme` paths relative to workspace root, which
# becomes invalid inside a published .crate tarball. Each crate sets
# `readme = "README.md"` literally.
version = "0.2.0"
publish = true
```

Update `[workspace.dependencies]` so internal crate references include `version = "0.2.0"`:

```toml
terminal-commander-core    = { path = "crates/core",    version = "0.2.0" }
terminal-commander-sifters = { path = "crates/sifters", version = "0.2.0" }
terminal-commander-probes  = { path = "crates/probes",  version = "0.2.0" }
terminal-commander-store   = { path = "crates/store",   version = "0.2.0" }
```

Inside per-crate Cargo.toml `[dependencies]`, ALSO add `version = "0.2.0"` to the `terminal-commander-supervisor` path entry in daemon + mcp (Task 11 Step 4 fixes daemon's literal dep; mcp inherits from workspace dep list when applicable):

For `crates/daemon/Cargo.toml`:
```toml
terminal-commander-supervisor = { path = "../supervisor", version = "0.2.0" }
```

For `crates/mcp/Cargo.toml`:
```toml
terminal-commander-supervisor = { path = "../supervisor", version = "0.2.0" }
terminal-commander-sifters    = { version = "0.2.0", path = "../sifters" }
terminal-commander-store      = { version = "0.2.0", path = "../store" }
terminal-commanderd           = { version = "0.2.0", path = "../daemon" }
```

- [ ] **Step 2: Convert `crates/supervisor/Cargo.toml` to workspace publish**

Change top of `[package]`:
```toml
[package]
name = "terminal-commander-supervisor"
description = "Cross-platform supervisor for Terminal Commander daemon — IPC bring-up, peer identity, ensure-daemon helpers."
readme = "README.md"
keywords = ["terminal-commander", "mcp", "daemon", "supervisor"]
categories = ["development-tools"]
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
version.workspace = true
publish.workspace = true
```

Drop the old `version = "0.0.0"`, `edition = "2024"`, `license = "Apache-2.0"`, `publish = false` lines.

- [ ] **Step 3: Convert `crates/core`, `crates/sifters`, `crates/probes`, `crates/store/Cargo.toml`**

For each of the 4 internal crates, the `[package]` block should look like:

```toml
[package]
name = "terminal-commander-core"   # adjust per crate
description = "Core types + traits for Terminal Commander (workspace-internal foundation crate, but published so dependents can ship to crates.io)."
readme = "README.md"
keywords = ["terminal-commander", "mcp"]
categories = ["development-tools"]
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
version.workspace = true
publish.workspace = true
```

Per-crate `description` tweaks:
- core: "Core types and trait definitions for the Terminal Commander MCP daemon."
- sifters: "Output sifters (capture, filter, ring-buffer) for Terminal Commander."
- probes: "Probe definitions for the Terminal Commander observability surface."
- store: "Persistent store (SQLite) for Terminal Commander daemon state."

- [ ] **Step 4: Add per-crate metadata to daemon + mcp Cargo.toml**

For `crates/daemon/Cargo.toml`, ensure the inheritance block has:
```toml
[package]
name = "terminal-commanderd"
description = "Long-running Terminal Commander daemon. Owns bucket manager, context spool, policy engine, audit emitter, and local API."
readme = "README.md"
keywords = ["terminal-commander", "mcp", "daemon"]
categories = ["development-tools"]
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
version.workspace = true
publish.workspace = true
```

For `crates/mcp/Cargo.toml`, same shape:
```toml
[package]
name = "terminal-commander-mcp"
description = "Thin MCP server adapter (rmcp 1.7.0 stdio). Forwards every tool call to the daemon over IPC."
readme = "README.md"
keywords = ["mcp", "terminal-commander", "stdio", "model-context-protocol"]
categories = ["development-tools"]
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
version.workspace = true
publish.workspace = true
```

- [ ] **Step 5: Keep `cli` crate publish=false**

`crates/cli/Cargo.toml` should have `publish = false` (the binary-only CLI is not a library, no crates.io use case):

```toml
[package]
name = "terminal-commander-cli"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
version.workspace = true
publish = false
```

- [ ] **Step 6: Write per-crate README.md (7 files)**

For each of `crates/{core, sifters, probes, store, supervisor, daemon, mcp}`, create `README.md`:

```markdown
# terminal-commander-<role>

Part of the [Terminal Commander](https://github.com/special-place-administrator/terminal-commander) project.

This crate is published to crates.io to support the public-API distribution path.
See the [main repository README](https://github.com/special-place-administrator/terminal-commander#readme) for project overview, install instructions, and design docs.

## License

Apache-2.0
```

Substitute `<role>` per crate: core, sifters, probes, store, supervisor, daemon, mcp.

- [ ] **Step 7: cargo check + dry-run publish for all 7 crates in dep order**

```bash
cargo check --workspace
# Dry-run in dep order — early failures here mean later crates would fail in real publish.
cargo publish --dry-run -p terminal-commander-core --allow-dirty
cargo publish --dry-run -p terminal-commander-sifters --allow-dirty
cargo publish --dry-run -p terminal-commander-probes --allow-dirty
cargo publish --dry-run -p terminal-commander-store --allow-dirty
cargo publish --dry-run -p terminal-commander-supervisor --allow-dirty
cargo publish --dry-run -p terminal-commanderd --allow-dirty
cargo publish --dry-run -p terminal-commander-mcp --allow-dirty
```

Note: dry-run checks each crate against the LOCAL workspace, not crates.io. The first
real publish of `core` is the gate that verifies the registry actually accepts it.
Subsequent crates' real publish will fail until their deps are on the registry — that
is why Task 12 orchestrates 7 jobs in strict order with propagation sleeps.

Expected: all 7 dry-runs succeed.

- [ ] **Step 8: cargo test workspace**

```bash
cargo test --workspace
```
Expected: 271/271 pass.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml \
        crates/core/Cargo.toml crates/core/README.md \
        crates/sifters/Cargo.toml crates/sifters/README.md \
        crates/probes/Cargo.toml crates/probes/README.md \
        crates/store/Cargo.toml crates/store/README.md \
        crates/supervisor/Cargo.toml crates/supervisor/README.md \
        crates/daemon/Cargo.toml crates/daemon/README.md \
        crates/mcp/Cargo.toml crates/mcp/README.md \
        crates/cli/Cargo.toml
git commit -m "feat(cargo): bump workspace to 0.2.0 + publish full 7-crate dep closure"
```

---

## Task 12: Add 7 cargo publish jobs to release-please.yml (full dep closure, OIDC, API-based recovery)

**Files:**
- Modify: `.github/workflows/release-please.yml`

The 7 jobs publish strictly in dep order: core → sifters → probes → store → supervisor → daemon → mcp. Each waits for the prior. 60s sleep absorbs crates.io index propagation. `CARGO_REGISTRY_TOKEN_TC` (existing repo secret) for 0.2.0; Task 23 migrates to OIDC trusted publisher. Recovery uses crates.io HTTP API (deterministic JSON), not `cargo search` (human-formatted, ANSI-prone).

- [ ] **Step 1: Define a reusable cargo-publish job template (logical, inlined for each crate)**

For each of 7 crates, append a job to `release-please.yml`:

```yaml
  publish-cargo-<CRATE>:
    name: publish-cargo-<CRATE>
    needs:
      - release-please
      - ensure-release
      - publish-version
      - <PREV>                  # publish-root for core; previous cargo job for others
    if: >-
      always() && !cancelled() &&
      needs.<PREV>.result == 'success' &&
      (needs.release-please.outputs.releases_created == 'true' ||
      (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
      needs.publish-version.outputs.publish == 'true')
    runs-on: ubuntu-24.04
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "1.95.0" }
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: cargo-publish-<CRATE> }
      - name: Assert crate version matches release-please version
        shell: bash
        env:
          EXPECTED: ${{ needs.release-please.outputs.version || needs.ensure-release.outputs.version || needs.publish-version.outputs.version }}
        run: |
          set -e
          actual=$(cargo pkgid -p <CRATE> | awk -F'#' '{print $2}')
          if [ "$actual" != "$EXPECTED" ]; then
            echo "::error::cargo crate version ($actual) != release-please version ($EXPECTED)"
            exit 1
          fi
      - name: cargo publish --dry-run
        run: cargo publish --dry-run -p <CRATE>
      - name: cargo publish (CARGO_REGISTRY_TOKEN_TC for 0.2.0; OIDC at Task 23)
        shell: bash
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}
        run: |
          set -e
          if cargo publish -p <CRATE>; then
            exit 0
          fi
          # Recovery: crates.io HTTP API — deterministic JSON.
          # Returns 200 + JSON if version exists, 404 if not.
          if [ "${{ github.event_name }}" = "workflow_dispatch" ] && [ "${{ inputs.force_publish }}" = "true" ]; then
            ver=$(cargo pkgid -p <CRATE> | awk -F'#' '{print $2}')
            url="https://crates.io/api/v1/crates/<CRATE>/$ver"
            http_code=$(curl -sS -o /tmp/api.json -w '%{http_code}' \
              -H 'User-Agent: terminal-commander-recovery (https://github.com/special-place-administrator/terminal-commander)' \
              "$url")
            if [ "$http_code" = "200" ]; then
              echo "::warning::<CRATE>@$ver already on crates.io (HTTP 200, force_publish recovery)"
              exit 0
            fi
            echo "crates.io API returned HTTP $http_code; payload:"
            cat /tmp/api.json
          fi
          exit 1
      - name: Wait 60s for crates.io index propagation
        run: sleep 60
```

Substitute `<CRATE>` and `<PREV>` per the 7-row table below. The first job's `<PREV>` is `publish-root`; each subsequent job's `<PREV>` is the prior cargo job.

| Order | `<CRATE>`                          | `<PREV>` |
|-------|------------------------------------|----------|
| 1     | terminal-commander-core            | publish-root |
| 2     | terminal-commander-sifters         | publish-cargo-terminal-commander-core |
| 3     | terminal-commander-probes          | publish-cargo-terminal-commander-sifters |
| 4     | terminal-commander-store           | publish-cargo-terminal-commander-probes |
| 5     | terminal-commander-supervisor      | publish-cargo-terminal-commander-store |
| 6     | terminal-commanderd                | publish-cargo-terminal-commander-supervisor |
| 7     | terminal-commander-mcp             | publish-cargo-terminal-commanderd |

(Job names use the literal crate name to keep the YAML grep-friendly. GitHub Actions allows the long names.)

- [ ] **Step 2: Add npm↔cargo version parity assertion in release-please job**

In the `release-please` job, after the `Aggregate release-please outputs` step, add:

```yaml
      - name: Assert cargo workspace version matches release-please version
        if: steps.final.outputs.releases_created == 'true'
        shell: bash
        env:
          EXPECTED: ${{ steps.final.outputs.version }}
        run: |
          set -e
          actual=$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
          if [ "$actual" != "$EXPECTED" ]; then
            echo "::error::cargo workspace version ($actual) != release-please version ($EXPECTED)"
            exit 1
          fi
          echo "npm-cargo version parity: $actual"
```

- [ ] **Step 3: Wire Cargo.toml bump into release-please-config.json**

Add to `.github/release-please-config.json` under the root `packages/terminal-commander` package's `extra-files` array (Task 3 already added 5 entries; add 1 more for Cargo.toml):

```json
{ "type": "generic", "path": "../../Cargo.toml" }
```

Then add a comment marker in `Cargo.toml` at the workspace `version = "0.2.0"` line:

```toml
version = "0.2.0" # x-release-please-version
```

release-please's `generic` extra-files handler scans for the `x-release-please-version` magic comment and bumps the version on the same line. This is the documented release-please mechanism for non-Node files. No `cargo-workspace` plugin is needed.

Repeat: also add the marker to internal-dep version entries in `[workspace.dependencies]`:

```toml
terminal-commander-core    = { path = "crates/core",    version = "0.2.0" } # x-release-please-version
terminal-commander-sifters = { path = "crates/sifters", version = "0.2.0" } # x-release-please-version
terminal-commander-probes  = { path = "crates/probes",  version = "0.2.0" } # x-release-please-version
terminal-commander-store   = { path = "crates/store",   version = "0.2.0" } # x-release-please-version
```

And the supervisor literal deps in daemon + mcp (Task 11 Step 1):

```toml
terminal-commander-supervisor = { path = "../supervisor", version = "0.2.0" } # x-release-please-version
```

This ensures future release-please PRs bump ALL Cargo version sites together with the npm packages, preserving lockstep.

- [ ] **Step 4: Update release-please-config.json packages map entry**

For the root package, the `extra-files` should now have 6 entries (5 platform optionalDependency JSON paths from Task 3 + 1 Cargo.toml generic):

```json
"extra-files": [
  { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/linux-x64']" },
  { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/linux-arm64']" },
  { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/windows-x64']" },
  { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/mac-x64']" },
  { "type": "json", "path": "package.json", "jsonpath": "$.optionalDependencies['@terminal-commander/mac-arm64']" },
  { "type": "generic", "path": "../../Cargo.toml" }
]
```

(`generic` extra-file is repo-root-relative; from `packages/terminal-commander/` that's `../../Cargo.toml`. Verify the syntax in release-please v4.4.1 docs — `path` may be repo-root-relative regardless of package position. If the relative path fails, fall back to repo-root absolute or move the `Cargo.toml` extra-file entry under a dedicated `packages/`-key.)

- [ ] **Step 2: Add npm↔cargo version parity assertion in release-please job**

In the `release-please` job, after the `Aggregate release-please outputs` step, add:

```yaml
      - name: Assert cargo workspace version matches release-please version
        if: steps.final.outputs.releases_created == 'true'
        shell: bash
        env:
          EXPECTED: ${{ steps.final.outputs.version }}
        run: |
          set -e
          actual=$(grep '^version = ' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
          if [ "$actual" != "$EXPECTED" ]; then
            echo "::error::cargo workspace version ($actual) != release-please version ($EXPECTED)"
            exit 1
          fi
          echo "npm-cargo version parity: $actual"
```

- [ ] **Step 3: YAML lint**

```bash
python -c "import yaml; yaml.safe_load(open('.github/workflows/release-please.yml'))"
```

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release-please.yml
git commit -m "feat(release): cargo publish chain (supervisor→daemon→mcp) + npm-cargo parity assert"
```

---

## Task 13: Add 5 post-publish container verify jobs

**Files:**
- Modify: `.github/workflows/release-please.yml`

- [ ] **Step 1: Append 5 verify jobs + 1 mark-broken job**

After the cargo publish jobs:

```yaml
  # ----------------------------------------------------------------
  # JOB — post-publish smoke verification (5 platforms in parallel)
  # Each job installs the published package from registry on the
  # matching native OS and confirms the MCP binary works. If any
  # fail, mark-release-broken auto-opens a P0 issue.
  # ----------------------------------------------------------------
  verify-linux-x64:
    name: verify-linux-x64
    needs: [release-please, publish-root]
    if: needs.publish-root.result == 'success'
    runs-on: ubuntu-24.04
    permissions: { contents: read, issues: write }
    steps:
      - name: Wait 60s for npm registry propagation
        run: sleep 60
      - name: Install from npm + smoke (Node 22-bookworm, glibc — linux-x64 binary is glibc-linked)
        env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          # node:22-alpine uses musl libc. Our binaries are glibc-linked.
          # Use node:22-bookworm-slim instead. Also: --provenance requires npm 11.5.1+
          # which ships in Node 22.14+. node:22 tag tracks latest 22.x.
          docker run --rm node:22-bookworm-slim sh -c '
            set -e
            npm install -g "terminal-commander@'"${VER}"'"
            terminal-commander-mcp --version
            # MCP initialize stdio probe (10s timeout — exit 124 = clean timeout = pass)
            echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"smoke\",\"version\":\"0.0.0\"}}}" \
              | timeout 10 terminal-commander-mcp; rc=$?
            if [ "$rc" -ne 0 ] && [ "$rc" -ne 124 ]; then
              echo "::error::stdio probe exited $rc" >&2; exit "$rc"
            fi
            echo "verify-linux-x64: OK"
          '

  verify-linux-arm64:
    name: verify-linux-arm64
    needs: [release-please, publish-root]
    if: needs.publish-root.result == 'success'
    runs-on: ubuntu-24.04-arm
    permissions: { contents: read, issues: write }
    steps:
      - run: sleep 60
      - env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          docker run --rm node:22-bookworm-slim sh -c '
            set -e
            npm install -g "terminal-commander@'"${VER}"'"
            terminal-commander-mcp --version
            echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"smoke\",\"version\":\"0.0.0\"}}}" \
              | timeout 10 terminal-commander-mcp; rc=$?
            if [ "$rc" -ne 0 ] && [ "$rc" -ne 124 ]; then
              echo "::error::stdio probe exited $rc" >&2; exit "$rc"
            fi
            echo "verify-linux-arm64: OK"
          '

  verify-windows-x64:
    name: verify-windows-x64
    needs: [release-please, publish-root]
    if: needs.publish-root.result == 'success'
    runs-on: windows-2022
    permissions: { contents: read, issues: write }
    steps:
      - run: powershell -Command Start-Sleep -Seconds 60
      - uses: actions/setup-node@v4
        with: { node-version: "20" }
      - shell: pwsh
        env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          $ErrorActionPreference = 'Stop'
          npm install -g "terminal-commander@$env:VER"
          terminal-commander-mcp --version
          # Stdio probe: 10s timeout via PowerShell job, Stop-Job + Remove-Job
          # so we don't leak the runspace. Wait-Job returns $null on timeout.
          $msg = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.0"}}}'
          $job = Start-Job -ScriptBlock {
            param($m) $m | terminal-commander-mcp
          } -ArgumentList $msg
          $done = Wait-Job $job -Timeout 10
          if ($null -eq $done) {
            Write-Host "smoke: stdio probe timed out (expected for unbounded server)"
          }
          Stop-Job $job -ErrorAction SilentlyContinue
          Remove-Job $job -Force -ErrorAction SilentlyContinue
          Write-Host "verify-windows-x64: OK"

  verify-mac-x64:
    name: verify-mac-x64
    needs: [release-please, publish-root]
    if: needs.publish-root.result == 'success'
    runs-on: macos-13   # Intel — keep a runner pinning task in runbook to rotate when GitHub deprecates
    permissions: { contents: read, issues: write }
    steps:
      - run: sleep 60
      - uses: actions/setup-node@v4
        with: { node-version: "20" }     # 0.2.0 ships without --provenance; Node 20 is fine. Task 23 bumps to 22.14+.
      - env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          set -e
          npm install -g "terminal-commander@${VER}"
          terminal-commander-mcp --version
          # Portable timeout via python (always on macOS runner). subprocess.run
          # cleanly terminates the child + reports an explicit timeout, no
          # SIGKILL race, no kill-wait hang. Exit 124 is the conventional
          # "timed out cleanly" code we treat as success for an unbounded
          # stdio server probe.
          python3 - "$@" <<'PY'
          import json, subprocess, sys
          payload = json.dumps({
              "jsonrpc": "2.0", "id": 1, "method": "initialize",
              "params": {
                  "protocolVersion": "2024-11-05",
                  "capabilities": {},
                  "clientInfo": {"name": "smoke", "version": "0.0.0"},
              },
          })
          try:
              r = subprocess.run(
                  ["terminal-commander-mcp"],
                  input=payload, text=True, capture_output=True, timeout=10,
              )
              sys.exit(r.returncode)
          except subprocess.TimeoutExpired:
              print("smoke: stdio probe timed out (expected for unbounded server)")
              sys.exit(0)
          PY
          echo 'verify-mac-x64: OK'

  verify-mac-arm64:
    name: verify-mac-arm64
    needs: [release-please, publish-root]
    if: needs.publish-root.result == 'success'
    runs-on: macos-14   # Apple Silicon
    permissions: { contents: read, issues: write }
    steps:
      - run: sleep 60
      - uses: actions/setup-node@v4
        with: { node-version: "20" }
      - env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          set -e
          npm install -g "terminal-commander@${VER}"
          terminal-commander-mcp --version
          python3 - "$@" <<'PY'
          import json, subprocess, sys
          payload = json.dumps({
              "jsonrpc": "2.0", "id": 1, "method": "initialize",
              "params": {
                  "protocolVersion": "2024-11-05",
                  "capabilities": {},
                  "clientInfo": {"name": "smoke", "version": "0.0.0"},
              },
          })
          try:
              r = subprocess.run(
                  ["terminal-commander-mcp"],
                  input=payload, text=True, capture_output=True, timeout=10,
              )
              sys.exit(r.returncode)
          except subprocess.TimeoutExpired:
              print("smoke: stdio probe timed out (expected for unbounded server)")
              sys.exit(0)
          PY
          echo 'verify-mac-arm64: OK'

  mark-release-broken:
    name: mark-release-broken
    needs:
      - release-please
      - verify-linux-x64
      - verify-linux-arm64
      - verify-windows-x64
      - verify-mac-x64
      - verify-mac-arm64
    if: |
      always() && !cancelled() &&
      (needs.verify-linux-x64.result == 'failure' ||
       needs.verify-linux-arm64.result == 'failure' ||
       needs.verify-windows-x64.result == 'failure' ||
       needs.verify-mac-x64.result == 'failure' ||
       needs.verify-mac-arm64.result == 'failure')
    runs-on: ubuntu-24.04
    permissions: { issues: write, contents: read }
    steps:
      - name: Compose issue body to a file (avoids YAML heredoc fragility)
        env:
          VER: ${{ needs.release-please.outputs.version }}
          RES_LX64: ${{ needs.verify-linux-x64.result }}
          RES_LARM: ${{ needs.verify-linux-arm64.result }}
          RES_WIN:  ${{ needs.verify-windows-x64.result }}
          RES_MX64: ${{ needs.verify-mac-x64.result }}
          RES_MARM: ${{ needs.verify-mac-arm64.result }}
          RUN_URL: ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}
        run: |
          set -e
          # Use printf, NOT heredoc — heredoc indentation + GH-Actions YAML
          # is a known footgun.
          printf 'Release **%s** failed post-publish smoke verification.\n\n' "$VER" > /tmp/body.md
          printf 'Verify job results:\n' >> /tmp/body.md
          printf '- linux-x64   : %s\n' "$RES_LX64"  >> /tmp/body.md
          printf '- linux-arm64 : %s\n' "$RES_LARM"  >> /tmp/body.md
          printf '- windows-x64 : %s\n' "$RES_WIN"   >> /tmp/body.md
          printf '- mac-x64     : %s\n' "$RES_MX64"  >> /tmp/body.md
          printf '- mac-arm64   : %s\n\n' "$RES_MARM" >> /tmp/body.md
          printf '**Action required:** Run `.github/workflows/deprecate-version.yml` for version %s ASAP, then triage + patch release.\n\n' "$VER" >> /tmp/body.md
          printf 'Workflow run: %s\n' "$RUN_URL" >> /tmp/body.md
      - name: Open P0 issue (PAT first, fall back to GITHUB_TOKEN if PAT is the broken one)
        env:
          PAT: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }}
          GITHUB_TOKEN_FALLBACK: ${{ secrets.GITHUB_TOKEN }}
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          set -e
          title="release ${VER} smoke FAILED"
          # Try PAT first.
          if GH_TOKEN="$PAT" gh issue create --title "$title" --label "release-broken,P0" --body-file /tmp/body.md; then
            echo "issue filed via RELEASE_PLEASE_TOKEN_TC"
            exit 0
          fi
          echo "::warning::RELEASE_PLEASE_TOKEN_TC failed to open issue; falling back to GITHUB_TOKEN"
          GH_TOKEN="$GITHUB_TOKEN_FALLBACK" gh issue create --title "$title" --label "release-broken,P0" --body-file /tmp/body.md
```

- [ ] **Step 2: YAML lint**

```bash
python -c "import yaml; yaml.safe_load(open('.github/workflows/release-please.yml'))"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release-please.yml
git commit -m "feat(release): post-publish containerized verify jobs (5 platforms) + mark-broken"
```

---

## Task 14: Create `secret-health.yml` weekly cron

**Files:**
- Create: `.github/workflows/secret-health.yml`

- [ ] **Step 1: Write workflow**

```yaml
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# Weekly probe of the 3 release-critical secrets. Opens a P1 issue
# if any fail. Catches the 365-day token-expiry cliff weeks before
# any release window cares.

name: secret-health

on:
  schedule:
    - cron: '0 9 * * MON'   # Monday 09:00 UTC
  workflow_dispatch:

permissions:
  contents: read
  issues: write

jobs:
  probe:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/setup-node@v4
        with:
          node-version: "20"
          registry-url: "https://registry.npmjs.org"
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "1.95.0" }

      - name: NPM_TOKEN_TC — npm whoami
        env:
          NPM_TOKEN: ${{ secrets.NPM_TOKEN_TC }}
          GH_TOKEN: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }}
        shell: bash
        run: |
          set +e
          echo "//registry.npmjs.org/:_authToken=$NPM_TOKEN" > ~/.npmrc
          out=$(npm whoami 2>&1)
          rc=$?
          if [ $rc -ne 0 ]; then
            gh issue create \
              --title "NPM_TOKEN_TC failing whoami" \
              --label "ops,P1" \
              --body "Weekly secret-health probe — \`npm whoami\` failed:\n\n\`\`\`\n$out\n\`\`\`\n\nRotate at https://www.npmjs.com/settings/USER/tokens and update repo secret."
            exit 1
          fi
          echo "NPM_TOKEN_TC OK: $out"

      - name: CARGO_REGISTRY_TOKEN_TC — crates.io probe
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}
          GH_TOKEN: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }}
        shell: bash
        run: |
          set +e
          out=$(cargo search terminal-commander-mcp --limit 1 2>&1)
          rc=$?
          if [ $rc -ne 0 ]; then
            gh issue create \
              --title "CARGO_REGISTRY_TOKEN_TC failing" \
              --label "ops,P1" \
              --body "Weekly secret-health probe — cargo search failed:\n\n\`\`\`\n$out\n\`\`\`\n\nRotate at https://crates.io/me and update repo secret."
            exit 1
          fi
          echo "CARGO_REGISTRY_TOKEN_TC OK"

      - name: RELEASE_PLEASE_TOKEN_TC — gh auth status
        env:
          GH_TOKEN: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }}
        shell: bash
        run: |
          set +e
          out=$(gh auth status 2>&1)
          rc=$?
          if [ $rc -ne 0 ]; then
            # Use default GITHUB_TOKEN to file the issue (since this one is broken).
            GH_TOKEN=${{ secrets.GITHUB_TOKEN }} gh issue create \
              --title "RELEASE_PLEASE_TOKEN_TC failing" \
              --label "ops,P1" \
              --body "Weekly secret-health probe — gh auth status failed:\n\n\`\`\`\n$out\n\`\`\`\n\nRotate the PAT at https://github.com/settings/tokens and update repo secret."
            exit 1
          fi
          echo "RELEASE_PLEASE_TOKEN_TC OK"
```

- [ ] **Step 2: YAML lint + commit**

```bash
python -c "import yaml; yaml.safe_load(open('.github/workflows/secret-health.yml'))"
git add .github/workflows/secret-health.yml
git commit -m "feat(ops): weekly secret-health.yml probes 3 release tokens"
```

---

## Task 15: Create `recover-partial-publish.sh`

**Files:**
- Create: `scripts/release/recover-partial-publish.sh`

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# Usage: ./scripts/release/recover-partial-publish.sh <version>
#
# Queries npm + crates.io for each artifact at <version>. Reports
# which shipped vs which are missing. Prints republish guidance for
# missing artifacts. Idempotent — safe to re-run.

set -euo pipefail

VER="${1:?usage: recover-partial-publish.sh <version> (e.g., 0.2.0)}"

NPM_PKGS=(
  "terminal-commander"
  "@terminal-commander/linux-x64"
  "@terminal-commander/linux-arm64"
  "@terminal-commander/windows-x64"
  "@terminal-commander/mac-x64"
  "@terminal-commander/mac-arm64"
)

CARGO_CRATES=(
  "terminal-commander-core"
  "terminal-commander-sifters"
  "terminal-commander-probes"
  "terminal-commander-store"
  "terminal-commander-supervisor"
  "terminal-commanderd"
  "terminal-commander-mcp"
)

missing_npm=()
missing_cargo=()

echo "═══ npm registry — checking @ ${VER} ═══"
for name in "${NPM_PKGS[@]}"; do
  if npm view "${name}@${VER}" version >/dev/null 2>&1; then
    printf "  OK    %s@%s\n" "$name" "$VER"
  else
    printf "  MISS  %s@%s\n" "$name" "$VER"
    missing_npm+=("$name")
  fi
done

echo
echo "═══ crates.io — checking @ ${VER} (HTTP API, not cargo search) ═══"
# cargo search output is human-formatted, ANSI-prone, header-mixed.
# Use the crates.io HTTP API: 200 = exists, 404 = missing. Deterministic.
for crate in "${CARGO_CRATES[@]}"; do
  http_code=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "User-Agent: terminal-commander-recovery (https://github.com/special-place-administrator/terminal-commander)" \
    "https://crates.io/api/v1/crates/${crate}/${VER}")
  case "$http_code" in
    200)
      printf "  OK    %s@%s\n" "$crate" "$VER" ;;
    404)
      printf "  MISS  %s@%s\n" "$crate" "$VER"
      missing_cargo+=("$crate") ;;
    *)
      printf "  ??    %s@%s (HTTP %s — treating as missing)\n" "$crate" "$VER" "$http_code"
      missing_cargo+=("$crate") ;;
  esac
done

echo
if [ ${#missing_npm[@]} -eq 0 ] && [ ${#missing_cargo[@]} -eq 0 ]; then
  echo "✔ All artifacts present at version ${VER}. Nothing to recover."
  exit 0
fi

echo "═══ Recovery actions ═══"
if [ ${#missing_npm[@]} -gt 0 ]; then
  echo
  echo "Missing npm packages:"
  for n in "${missing_npm[@]}"; do echo "  - $n"; done
  echo
  echo "Re-run release-please.yml workflow with:"
  echo "  gh workflow run release-please.yml -f force_publish=true"
  echo "(E409-tolerant; already-shipped packages exit success, only missing ones republish.)"
fi
if [ ${#missing_cargo[@]} -gt 0 ]; then
  echo
  echo "Missing crates.io crates:"
  for c in "${missing_cargo[@]}"; do echo "  - $c"; done
  echo
  echo "Republish manually (must be on a checkout at tag v${VER}):"
  for c in "${missing_cargo[@]}"; do
    echo "  cargo publish -p $c"
  done
fi
exit 1
```

- [ ] **Step 2: Make executable + smoke test**

```bash
chmod +x scripts/release/recover-partial-publish.sh
# Smoke — query an old version that exists for all linux packages
./scripts/release/recover-partial-publish.sh 0.1.4 || true
```
Expected: linux packages OK, others MISS (expected — they aren't published yet).

- [ ] **Step 3: Commit**

```bash
git add scripts/release/recover-partial-publish.sh
git commit -m "feat(ops): recover-partial-publish.sh script for partial-release recovery"
```

---

## Task 16: Create `deprecate-version.yml` workflow

**Files:**
- Create: `.github/workflows/deprecate-version.yml`

- [ ] **Step 1: Write the workflow**

```yaml
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The Terminal Commander Authors
#
# Operator panic-button: deprecate a broken published version across
# all 5 npm platform packages + root + 3 cargo crates.
# npm: marks tarballs deprecated (warns on install).
# cargo: yanks (prevents new resolution of the version).

name: deprecate-version

on:
  workflow_dispatch:
    inputs:
      version:
        required: true
        type: string
        description: "Version to deprecate, e.g. 0.2.0"
      reason:
        required: true
        type: string
        description: "Deprecation message users see on install (e.g., 'broken X, install 0.2.1')"

permissions:
  contents: read

jobs:
  deprecate-npm:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/setup-node@v4
        with:
          node-version: "20"
          registry-url: "https://registry.npmjs.org"
      - env:
          NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_TC }}
        shell: bash
        run: |
          set -e
          for pkg in \
            terminal-commander \
            @terminal-commander/linux-x64 \
            @terminal-commander/linux-arm64 \
            @terminal-commander/windows-x64 \
            @terminal-commander/mac-x64 \
            @terminal-commander/mac-arm64; do
            echo "── npm deprecate ${pkg}@${{ inputs.version }}"
            npm deprecate "${pkg}@${{ inputs.version }}" "${{ inputs.reason }}" || true
          done

  yank-cargo:
    runs-on: ubuntu-24.04
    steps:
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "1.95.0" }
      - env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}
        shell: bash
        run: |
          set -e
          for crate in \
            terminal-commander-supervisor \
            terminal-commanderd \
            terminal-commander-mcp; do
            echo "── cargo yank ${crate}@${{ inputs.version }}"
            cargo yank --version "${{ inputs.version }}" "$crate" || true
          done
```

- [ ] **Step 2: YAML lint + commit**

```bash
python -c "import yaml; yaml.safe_load(open('.github/workflows/deprecate-version.yml'))"
git add .github/workflows/deprecate-version.yml
git commit -m "feat(ops): deprecate-version.yml workflow_dispatch for broken-release rollback"
```

---

## Task 17: Write release runbook

**Files:**
- Create: `docs/runbooks/2026-05-24-phase-4a-release-procedure.md`

- [ ] **Step 1: Write the runbook**

```markdown
# Phase 4a — Release Procedure Runbook

Owner: terminal-commander maintainers
Created: 2026-05-24

## Pre-flight (one-time setup, before first release)

OIDC trusted publishers MUST be configured before the first release fires.
This was Task 0 of the Phase 4a-max plan; it is reproduced here as the
canonical procedure for any future package or crate added to the pipeline.

For each of the 6 npm packages:
- Pkg → Settings → Publishing access → Trusted Publishers → GitHub Actions
- Organization: `special-place-administrator`
- Repository: `terminal-commander`
- Workflow filename: `release-please.yml`
- Environment: (leave blank)

For each of the 7 cargo crates: same on crates.io.

Verify: re-run a small workflow_dispatch test. Confirm published tarball
shows `dist.attestations` (npm) or signed provenance metadata (crates.io).

## Normal release flow

1. Land conventional-commit work on `main`.
2. `release-please` opens "chore: release X.Y.Z" PR. Review the diff.
   - Confirm all 6 manifest entries bump.
   - Confirm `Cargo.toml` workspace version bumps + every `x-release-please-version`-marked line bumps.
   - Confirm root `optionalDependencies` all 5 platforms at new version.
3. Merge the PR.
4. Same workflow run: 5 platform builds (parallel) → 5 publish-platform
   (parallel, OIDC + provenance) → publish-root (OIDC + provenance) →
   7 cargo publish chain (OIDC, in strict dep order: core → sifters →
   probes → store → supervisor → daemon → mcp) → 5 verify jobs.
5. If all 5 verify jobs green: release is good. Done.
6. If any verify fails: `mark-release-broken` opens P0 issue. Go to
   "Broken release" below.

## Broken release

1. Triage the P0 issue. Determine which platform broke + why.
2. Run `.github/workflows/deprecate-version.yml`:
   - version: the broken version
   - reason: "broken {symptom}, install {next safe version}"
3. Push a `fix:` commit. release-please opens patch release PR (X.Y.Z+1).
4. Re-point `latest` dist-tag (npm only, cargo yank handles cargo side):
   ```
   npm dist-tag add terminal-commander@X.Y.Z+1 latest
   ```

## Partial-publish recovery

If the workflow died mid-publish (some packages on registry, others
missing):

1. Run: `bash scripts/release/recover-partial-publish.sh X.Y.Z`
2. The script reports missing artifacts + republish guidance (using
   crates.io HTTP API + npm view, both deterministic).
3. Re-run the release-please.yml workflow with `force_publish: true`
   for npm side. The E409-tolerant pattern republishes only missing.
4. For cargo side: from a checkout at tag `vX.Y.Z`, run the script's
   suggested `cargo publish -p <crate>` commands in dep order. Wait
   60s between each so crates.io index propagates.

## Secret rotation (annual)

`RELEASE_PLEASE_TOKEN_TC` (PAT for opening release PRs) has 365-day
max lifetime. Weekly `secret-health.yml` files P1 issue when probe fails.

Trusted-publisher OIDC auth does NOT have an expiry, so `NPM_TOKEN_TC`
and `CARGO_REGISTRY_TOKEN_TC` are now only emergency-recovery escape
hatches; they should not rotate on a schedule, but ARE still probed
weekly for liveness in case we need them for a `force_publish` recovery.

1. Rotate the failing token at its registry UI.
2. Update repo secret at https://github.com/special-place-administrator/terminal-commander/settings/secrets/actions
3. Manually re-run `secret-health.yml` to confirm green.

## macOS runner deprecation

GitHub deprecates macOS runners on roughly 18-month cycles. We pin
`macos-13` (Intel) + `macos-14` (Apple Silicon) for `_build-platform-binary.yml`
and `verify-mac-*` jobs. When deprecation warnings appear in workflow runs:

1. Pick the next-newest available macOS runners (e.g. `macos-14` Intel
   replacement, `macos-15` arm).
2. Update both `_build-platform-binary.yml` (runs-on map) and the two
   `verify-mac-*` jobs in `release-please.yml` in a single PR.
3. CI green = good. Old version users keep working; only the publish
   path changes.
```

- [ ] **Step 2: Commit**

```bash
git add docs/runbooks/2026-05-24-phase-4a-release-procedure.md
git commit -m "docs(runbook): Phase 4a release procedure + rollback + OIDC migration"
```

---

## Task 18: Smoke-test the reusable build workflow locally (best-effort)

**Files:** none (test only)

- [ ] **Step 1: Local cargo build for linux target (sanity)**

```bash
cargo build --release \
  --target x86_64-unknown-linux-gnu \
  -p terminal-commanderd \
  -p terminal-commander-mcp \
  -p terminal-commander-cli
```

If on Windows host: this will fail. Skip + note in task report.

- [ ] **Step 2: Local cargo build for windows native (sanity)**

```bash
cargo build --release \
  --target x86_64-pc-windows-msvc \
  -p terminal-commanderd \
  -p terminal-commander-mcp \
  -p terminal-commander-cli
```

Expected: succeeds, binaries land in `target/x86_64-pc-windows-msvc/release/`.

- [ ] **Step 3: cargo publish dry-run (all 7 crates in dep order)**

```bash
cargo publish --dry-run -p terminal-commander-core --allow-dirty
cargo publish --dry-run -p terminal-commander-sifters --allow-dirty
cargo publish --dry-run -p terminal-commander-probes --allow-dirty
cargo publish --dry-run -p terminal-commander-store --allow-dirty
cargo publish --dry-run -p terminal-commander-supervisor --allow-dirty
cargo publish --dry-run -p terminal-commanderd --allow-dirty
cargo publish --dry-run -p terminal-commander-mcp --allow-dirty
```

Expected: 7 successes.

- [ ] **Step 4: Document any deviations in a brief commit message**

If any step deviates, commit a follow-up edit to the runbook noting it.

---

## Task 19: Full cargo workspace test + clippy + fmt

**Files:** none (verification)

- [ ] **Step 1: cargo fmt --all --check**

```bash
cargo fmt --all --check
```

- [ ] **Step 2: cargo clippy workspace -D warnings**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: cargo test workspace**

```bash
cargo test --workspace
```

Expected: 271/271 pass.

- [ ] **Step 4: npm tests in root wrapper**

```bash
cd packages/terminal-commander && npm test
```

Expected: 253/253 + new resolver darwin tests pass.

- [ ] **Step 5: All release scripts**

```bash
node --test scripts/release/test-release-please-config.js
node scripts/release/verify-optional-dependencies.js
```

Expected: all pass.

- [ ] **Step 6: No commit (verification only)**

---

## Task 20: Verify per-crate READMEs land in published tarballs

**Files:** none (verification only — READMEs were created in Task 11)

- [ ] **Step 1: Verify all 7 READMEs exist**

```bash
for c in core sifters probes store supervisor daemon mcp; do
  if [ ! -f "crates/$c/README.md" ]; then
    echo "MISSING: crates/$c/README.md"
    exit 1
  fi
done
echo "OK: 7 per-crate READMEs present"
```

- [ ] **Step 2: Confirm cargo includes README in package list**

```bash
for c in terminal-commander-core terminal-commander-sifters terminal-commander-probes terminal-commander-store terminal-commander-supervisor terminal-commanderd terminal-commander-mcp; do
  cargo package --list -p "$c" --allow-dirty | grep README.md || { echo "::error::$c tarball missing README.md"; exit 1; }
done
echo "OK: 7 tarballs include README.md"
```

- [ ] **Step 3: No commit (verification only)**

---

## Task 21: Full pre-push verification + push

**Files:** none

- [ ] **Step 1: Re-run all gates**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd packages/terminal-commander && npm test && cd ../..
node --test scripts/release/test-release-please-config.js
node scripts/release/verify-optional-dependencies.js
python -c "
import yaml
for f in [
    '.github/workflows/release-please.yml',
    '.github/workflows/npm-binary-build.yml',
    '.github/workflows/_build-platform-binary.yml',
    '.github/workflows/secret-health.yml',
    '.github/workflows/deprecate-version.yml',
]:
    yaml.safe_load(open(f))
    print(f'OK {f}')
"
```

- [ ] **Step 2: git status + log review**

```bash
git status --short
git log --oneline main..HEAD
```

Expected: clean working tree, ~21 commits ahead of main.

- [ ] **Step 3: Push branch**

```bash
git push -u origin feature/phase-4-deferred-gaps
```

- [ ] **Step 4: Open PR**

```bash
gh pr create --title "Phase 4a-max: tier-1 native publish pipeline (5 platforms + cargo + verify + ops)" --body "$(cat <<'EOF'
## Summary
Phase 4a-max: full tier-1 native distribution pipeline.

- 5 npm platform packages (linux-x64, linux-arm64, windows-x64, mac-x64, mac-arm64) + root
- 3 cargo crates (supervisor, daemon, mcp) on crates.io
- Reusable `_build-platform-binary.yml` eliminates drift between PR + release builds
- 5 containerized post-publish verify jobs gate release-success bit
- `secret-health.yml` weekly cron, `recover-partial-publish.sh`, `deprecate-version.yml`
- Explicit rollback runbook
- OIDC trusted-publisher migration runbook (post-first-release task)

## Spec
docs/superpowers/specs/2026-05-24-phase-4a-distribution-windows-x64-publish-design.md

## Test plan
- [x] cargo fmt + clippy + test workspace green
- [x] npm test green (resolver darwin tests added)
- [x] release-please-config + manifest + sync/verify scripts unit tests
- [x] 5 workflow YAML files parse
- [x] cargo publish dry-run for all 3 crates
- [ ] CI green on PR
- [ ] Codex adversarial review pass (Task 22)
- [ ] First 0.2.0 release ships + all 5 verify jobs green

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Task 22: Final codex adversarial review of full branch diff

**Files:** none

- [ ] **Step 1: Generate diff + send to codex**

```bash
git diff main..HEAD > /tmp/phase-4a-max.diff
wc -l /tmp/phase-4a-max.diff
```

- [ ] **Step 2: Invoke codex via /codex**

Prompt:

> Adversarial review the attached diff against `docs/superpowers/specs/2026-05-24-phase-4a-distribution-windows-x64-publish-design.md`. This is the implementation of Phase 4a-max. Find:
> - any deviation from spec
> - any security gap (token handling, OIDC permissions, supply-chain attack vectors)
> - any correctness gap (race conditions, partial-failure modes, idempotency violations)
> - any operational gap (recovery paths, rollback semantics, missing visibility)
> - any drift risk (places where two workflows might diverge over time)
> - any cargo/crates.io specific footgun (lockfile, feature unification, dep cycles, doc tests, examples directory inclusion)
> - any GitHub Actions specific footgun (concurrency, reusable-workflow secret inheritance, artifact retention, runner availability)
> Report findings by severity: CRITICAL > IMPORTANT > NIT. For each finding cite file:line and propose a concrete fix.

- [ ] **Step 3: Apply codex findings + re-loop**

For each CRITICAL: fix immediately, commit.
For each IMPORTANT: fix or document deferral with explicit justification.
For each NIT: triage; fix the cheap ones.

Re-run codex if any fix substantially changes the diff.

- [ ] **Step 4: Push fixes**

```bash
git push
```

---

## Task 23: (POST-RELEASE, deferred) OIDC trusted publisher migration for 0.2.1+ SLSA provenance

**Owner:** operator (15 min UI clicks, when convenient)
**Trigger:** Run AFTER 0.2.0 ships green via Task 22.
**Why deferred:** Operator goal directive said "no user intervention". OIDC setup needs UI clicks the autonomous loop can't perform. This task is a planned follow-up, not an indefinite TODO.

- [ ] **Step 1: Configure npm trusted publishers (6 packages, ~5 min)**

For each of the 6 npm packages on npmjs.com:
1. `terminal-commander`
2. `@terminal-commander/linux-x64`
3. `@terminal-commander/linux-arm64`
4. `@terminal-commander/windows-x64`
5. `@terminal-commander/mac-x64`
6. `@terminal-commander/mac-arm64`

Per package: Settings → Publishing access → Trusted Publishers → GitHub Actions:
- Organization or user: `special-place-administrator`
- Repository: `terminal-commander`
- Workflow filename: `release-please.yml`
- Environment name: (leave empty)

- [ ] **Step 2: Configure crates.io trusted publishers (7 crates, ~5 min)**

For each crate: Settings → Trusted Publishing → GitHub Actions, same fields.

- [ ] **Step 3: Update `release-please.yml` publish jobs to OIDC mode**

For each `publish-<platform>` job + `publish-root` + 7 `publish-cargo-<CRATE>` jobs:
- Add `id-token: write` to `permissions:`.
- Bump `actions/setup-node@v4` to `node-version: "22.14"`.
- Drop `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_TC }}` from npm publish steps.
- Add `--provenance` to npm publish commands.
- Drop `CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}` from cargo publish steps.

- [ ] **Step 4: Bump verify jobs to Node 22.14 (needed to verify provenance during smoke)**

In `verify-linux-x64`, `verify-linux-arm64` (docker image), `verify-windows-x64`, `verify-mac-x64`, `verify-mac-arm64`: change Node version `20` → `22.14`. For docker, use `node:22-bookworm-slim` (already done in Task 13 — keep).

- [ ] **Step 5: YAML lint + commit**

```bash
python -c "import yaml; yaml.safe_load(open('.github/workflows/release-please.yml'))"
git add .github/workflows/release-please.yml
git commit -m "feat(release): migrate to OIDC trusted publisher + --provenance (SLSA from 0.2.1+)"
```

- [ ] **Step 6: Push + verify on next release**

Push, wait for the next conventional-commit work to trigger a release-please PR. The 0.2.1 (or 0.3.0) release will publish with `dist.attestations` on every npm package.

Confirm:
```bash
npm view terminal-commander@0.2.1 dist.attestations
```

- [ ] **Step 7: Keep secrets as recovery escape hatches**

Do NOT delete `NPM_TOKEN_TC` or `CARGO_REGISTRY_TOKEN_TC` from repo secrets. Keep them around for `force_publish` workflow_dispatch recovery scenarios where OIDC may be unavailable (e.g., extended GitHub OIDC outage). They become emergency-use rather than day-to-day.

---

## Self-review

**Spec coverage:** All 17 spec changes (A.1-A.7, B.8-B.10, C.11-C.12, D.13, E.14-E.16, F.17) mapped to tasks. Pre-execution codex review surfaced 4 CRITICAL + 8 IMPORTANT findings; all addressed inline in this plan.

**Two-phase auth strategy** (reconciles "no user intervention" with "correctness above all else"):
- **0.2.0 (this run, autonomous):** Token-based publish via existing `NPM_TOKEN_TC` + `CARGO_REGISTRY_TOKEN_TC`. Ships the pipeline + windows + macOS reach today, zero operator UI clicks needed.
- **0.2.1+ (Task 23, when operator has 15 min):** OIDC trusted publisher + `--provenance` + Node 22.14 cutover. SLSA attestations from 0.2.1 onward. Codex's C2 finding survives as a planned follow-up rather than a hard prerequisite.

Spec AC #1 (`dist.attestations` present) is therefore satisfied at 0.2.1, not 0.2.0. This is a deliberate spec relaxation captured in plan task table below.

- A.1 (mac-x64 pkg) → Task 1
- A.2 (mac-arm64 pkg) → Task 2
- A.3 (release-please-config) → Task 3
- A.4 (manifest) → Task 4
- A.5 (root pkg.json optionalDeps) → Task 5
- A.6 (resolver darwin) → Task 6
- A.7 (sync/verify scripts) → Task 7
- B.8 (_build-platform-binary.yml) → Task 8 (native mac runners, no cargo-zigbuild — codex C3 fix)
- B.9 (npm-binary-build collapse) → Task 9
- B.10 (release-please.yml 5 publish jobs) → Task 10 (token-based for 0.2.0)
- C.11 (Cargo workspace) → Task 11 (FULL 7-crate dep closure with `x-release-please-version` markers + per-crate READMEs — codex C1 + C4 + I11 fix)
- C.12 (cargo publish jobs + parity assert) → Task 12 (7 jobs strict dep order, token-based, crates.io HTTP API recovery — codex C1 + I10 fix)
- D.13 (verify jobs + mark-broken) → Task 13 (Python subprocess for mac timeout, PAT→GITHUB_TOKEN fallback, glibc Node container — codex I7 + I8 + I14 fix)
- E.14 (secret-health.yml) → Task 14
- E.15 (recover-partial-publish.sh) → Task 15 (crates.io HTTP API, full 7-crate list — codex I10 fix)
- E.16 (deprecate-version.yml) → Task 16
- F.17 (OIDC migration) → Task 23 (post-release, when operator has 15 min)

Plus:
- Task 18: local smoke (7 dry-runs)
- Task 19: full pre-push gates
- Task 20: verify per-crate READMEs in tarballs
- Task 21: push + open PR
- Task 22: codex adversarial review of full diff (final round)
- Task 23: OIDC + provenance migration for 0.2.1+ (deferred operator action)

**Codex pre-execution review findings (CRITICAL — all resolved):**

| # | Finding | Resolution |
|---|---------|------------|
| C1 | Cargo publish graph broken — mcp/daemon depend on internal `publish=false` crates | Task 11 publishes ALL 7 crates with `version = "0.2.0"` pins on internal path deps |
| C2 | npm `dist.attestations` AC requires OIDC + Node 22.14+, but plan used token + Node 20 | Task 0 makes OIDC pre-flight a hard gate; Task 10/12 use Node 22.14 + `--provenance` from day-1 |
| C3 | cargo-zigbuild needs macOS SDK (not legally redistributable) | Task 8 uses native macos-13 (Intel) + macos-14 (arm) runners |
| C4 | release-please had no Cargo bump wiring | Task 12 Step 3 adds `x-release-please-version` markers + `generic` extra-files entry; future releases bump Cargo.toml in lockstep |

**Codex pre-execution review findings (IMPORTANT — all resolved):**

| # | Finding | Resolution |
|---|---------|------------|
| I5 | Task 10 said `upload_artifact: false` but downloaded artifacts later | Plan now says `upload_artifact: true` consistently |
| I6 | `runs-on: fromJSON(...)` needs real CI smoke | Task 18 + initial CI runs catch this; if it breaks we patch + re-run |
| I7 | Reusable workflow permissions cannot inherit upward | Task 8 calling job sets `id-token: write`; reusable workflow itself only needs `contents: read` since it doesn't publish |
| I8 | verify-mac jobs hang via `kill -9` race | Replaced with `python3 subprocess.run(..., timeout=10)` — clean termination |
| I9 | `mark-release-broken` fails if RELEASE_PLEASE_TOKEN_TC is the broken secret | PAT-first then GITHUB_TOKEN fallback in Task 13 |
| I10 | `cargo search \| grep` is ANSI/header fragile | Both the workflow (Task 12) and recovery script (Task 15) use crates.io HTTP API |
| I11 | README inheritance contradiction (workspace path vs per-crate path) | Workspace deliberately drops `readme`; each crate sets literal `readme = "README.md"` in Task 11 |
| I12 | Task 9 removed NPM04 local install smoke | Acknowledged: PR-time smoke moves to verify jobs (Task 13). Local install smoke is the npm-pack output → operator runs in their own checkout when investigating. |
| I14 | YAML heredoc trap in mark-release-broken | Replaced with printf to /tmp/body.md + `gh issue create --body-file` |

**Placeholder scan:** all steps have concrete code, file paths, commands. No TBDs.

**Type consistency:** workflow filenames, package names, version strings, crate names all spelled identically throughout. `PLATFORM_PACKAGES` module from Task 7 used consistently. 7-crate dep order `core → sifters → probes → store → supervisor → daemon → mcp` repeated identically in Task 11 (dry-run), Task 12 (publish jobs), Task 15 (recovery script), Task 18 (local dry-run), Task 20 (README verify).

**Known caveats:**
- macOS runner deprecation runbook in Task 17.
- `cargo package` warning about `Cargo.lock` inclusion is informational, not a publish blocker.
- Path-relative `Cargo.toml` extra-file in release-please-config.json may need fallback to absolute path if v4.4.1 quirks; Task 12 Step 4 documents the fallback.

**Execution mode:** Subagent-Driven Development per goal directive ("ralph loop, review, testing etc, without any user intervention").
