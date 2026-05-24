# Phase 4a-max Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **Per-task adversarial review:** after each task's spec + quality review pass, run `/codex` against the task's diff before marking complete. Codex catches what Sonnet reviewers miss.

**Goal:** Ship native tier-1 publish pipeline for Terminal Commander — 5 npm platform packages + root + 3 crates.io crates at `0.2.0`, with SLSA provenance, drift-proof reusable build workflow, 5 containerized verify jobs, ops tooling (secret-health + recovery + deprecation), and explicit rollback semantics.

**Architecture:** Extend existing `release-please.yml` (linux-x64 + linux-arm64 + root already shipped) with windows-x64 + mac-x64 + mac-arm64 platform packages, 3 cargo crate publish jobs, and 5 post-publish verify jobs. Eliminate drift between PR-time `npm-binary-build.yml` and release-time publish via new reusable workflow `_build-platform-binary.yml`. Cross-compile mac targets from Linux runners using `cargo-zigbuild` (free, well-tested). Bump Cargo workspace `0.0.0 → 0.2.0` and flip `publish` on 3 public-API crates. After first 0.2.0 ships via `NPM_TOKEN_TC`, migrate to OIDC trusted publisher per package for `--provenance` attestations.

**Tech Stack:** GitHub Actions, release-please v4.4.1 (manifest mode, linked-versions plugin), Rust 1.95.0 + edition 2024, cargo, cargo-zigbuild for mac cross-compile, npm 10, Node 20, conventional commits, OIDC trusted publishers (npmjs.com + crates.io).

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

Cargo.toml                                           # MODIFIED (Task 11) — workspace version 0.0.0→0.2.0, publish=true
crates/supervisor/Cargo.toml                         # MODIFIED (Task 11) — flip to workspace publish
crates/daemon/Cargo.toml                             # MODIFIED (Task 11) — add readme, keywords, categories
crates/mcp/Cargo.toml                                # MODIFIED (Task 11) — add readme, keywords, categories

docs/
├── superpowers/specs/2026-05-24-phase-4a-distribution-windows-x64-publish-design.md  # SPEC (already committed)
└── runbooks/
    └── 2026-05-24-phase-4a-release-procedure.md     # NEW   (Task 17)
```

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
    runs-on: ${{ fromJSON('{"linux-x64":"ubuntu-24.04","linux-arm64":"ubuntu-24.04-arm","windows-x64":"windows-2022","mac-x64":"ubuntu-24.04","mac-arm64":"ubuntu-24.04"}')[inputs.platform] }}
    outputs:
      artifact_name: ${{ steps.meta.outputs.artifact_name }}
    env:
      TARGET_TRIPLE: ${{ fromJSON('{"linux-x64":"x86_64-unknown-linux-gnu","linux-arm64":"aarch64-unknown-linux-gnu","windows-x64":"x86_64-pc-windows-msvc","mac-x64":"x86_64-apple-darwin","mac-arm64":"aarch64-apple-darwin"}')[inputs.platform] }}
      PLATFORM_PKG_DIR: packages/terminal-commander-${{ inputs.platform }}
      USES_ZIGBUILD: ${{ startsWith(inputs.platform, 'mac-') }}
      EXE_SUFFIX: ${{ inputs.platform == 'windows-x64' && '.exe' || '' }}
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ inputs.rust_toolchain }}
          targets: ${{ env.TARGET_TRIPLE }}

      - name: Install cargo-zigbuild + zig (mac cross only)
        if: env.USES_ZIGBUILD == 'true'
        uses: mlugg/setup-zig@v1
        with:
          version: 0.13.0

      - name: Install cargo-zigbuild binary (mac cross only)
        if: env.USES_ZIGBUILD == 'true'
        run: cargo install --locked cargo-zigbuild@0.19.7

      - name: Cache cargo registry + target
        uses: Swatinem/rust-cache@v2
        with:
          shared-key: build-platform-${{ inputs.platform }}

      - name: cargo build --release (native)
        if: env.USES_ZIGBUILD != 'true'
        shell: bash
        run: |
          cargo build --release \
            --target ${{ env.TARGET_TRIPLE }} \
            -p terminal-commanderd \
            -p terminal-commander-mcp \
            -p terminal-commander-cli

      - name: cargo zigbuild --release (mac cross)
        if: env.USES_ZIGBUILD == 'true'
        shell: bash
        run: |
          cargo zigbuild --release \
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

      - name: Smoke — --version (skipped for non-native targets)
        if: inputs.platform == 'linux-x64' || inputs.platform == 'linux-arm64' || inputs.platform == 'windows-x64'
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
    id-token: write
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
    - name: npm publish --access public
      shell: bash
      working-directory: packages/terminal-commander-<platform>
      env:
        NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN_TC }}
      run: |
        set -e
        if npm publish --access public; then
          exit 0
        fi
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

- [ ] **Step 4: Update `publish-root` inline node check (was done in Task 7 already if Task 7 also touched this file; if not, do it now)**

The inline `for (const name of [...])` block in `publish-root` already pulls from `PLATFORM_PACKAGES` per Task 7. Verify and skip if done.

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

## Task 11: Bump Cargo workspace + flip 3 crates to publish=true

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/supervisor/Cargo.toml`
- Modify: `crates/daemon/Cargo.toml`
- Modify: `crates/mcp/Cargo.toml`

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
readme = "README.md"
version = "0.2.0"
publish = true
```

(version `0.0.0 → 0.2.0`, publish `false → true`, readme `../../README.md → README.md`.)

Update `[workspace.dependencies]` to keep internal crate versions at `0.2.0`:

```toml
terminal-commander-core    = { path = "crates/core",    version = "0.2.0" }
terminal-commander-sifters = { path = "crates/sifters", version = "0.2.0" }
terminal-commander-probes  = { path = "crates/probes",  version = "0.2.0" }
terminal-commander-store   = { path = "crates/store",   version = "0.2.0" }
```

- [ ] **Step 2: Flip `crates/supervisor/Cargo.toml` to workspace publish**

Change:
```toml
[package]
name = "terminal-commander-supervisor"
version = "0.0.0"
edition = "2024"
license = "Apache-2.0"
publish = false
```
to:
```toml
[package]
name = "terminal-commander-supervisor"
description = "Cross-platform supervisor for Terminal Commander daemon — IPC bring-up, peer identity, ensure-daemon helpers."
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
homepage.workspace = true
readme.workspace = true
version.workspace = true
publish.workspace = true
keywords = ["terminal-commander", "mcp", "daemon", "supervisor"]
categories = ["development-tools"]
```

- [ ] **Step 3: Add description + keywords + categories to daemon + mcp Cargo.toml**

In `crates/daemon/Cargo.toml`, after `description = "..."`, add:
```toml
keywords = ["terminal-commander", "mcp", "daemon"]
categories = ["development-tools"]
```
Also add `readme.workspace = true` to the inheritance block.

In `crates/mcp/Cargo.toml`, after `description = "..."`, add:
```toml
keywords = ["mcp", "terminal-commander", "stdio", "model-context-protocol"]
categories = ["development-tools"]
```
Also add `readme.workspace = true`.

- [ ] **Step 4: Keep internal-only crates pinned to `publish = false`**

Edit `crates/core/Cargo.toml`, `crates/sifters/Cargo.toml`, `crates/probes/Cargo.toml`, `crates/store/Cargo.toml`, `crates/cli/Cargo.toml`: ensure each has `publish = false` in its `[package]` block (override workspace inheritance). Otherwise bumping workspace `publish = true` would publish them too.

If `publish.workspace = true` was set, change to `publish = false`. If no publish field, add `publish = false`.

- [ ] **Step 5: cargo check + cargo publish dry-run for the 3 crates**

```bash
cargo check --workspace
cargo publish --dry-run -p terminal-commander-supervisor --allow-dirty
cargo publish --dry-run -p terminal-commanderd --allow-dirty
cargo publish --dry-run -p terminal-commander-mcp --allow-dirty
```
Expected: all 3 dry-runs succeed (or fail with the well-known "uncommitted changes" message if no `--allow-dirty`).

- [ ] **Step 6: cargo test workspace**

```bash
cargo test --workspace
```
Expected: 271/271 pass (matches the baseline from PR #8).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/supervisor/Cargo.toml crates/daemon/Cargo.toml crates/mcp/Cargo.toml crates/core/Cargo.toml crates/sifters/Cargo.toml crates/probes/Cargo.toml crates/store/Cargo.toml crates/cli/Cargo.toml
git commit -m "feat(cargo): bump workspace to 0.2.0 + flip 3 public crates to publish=true"
```

---

## Task 12: Add 3 cargo publish jobs to release-please.yml

**Files:**
- Modify: `.github/workflows/release-please.yml`

- [ ] **Step 1: Append after `publish-root`**

```yaml
  # ----------------------------------------------------------------
  # JOB — cargo publish chain: supervisor → daemon → mcp
  # crates.io requires dep crates to be already published before
  # dependent crates. Hence the strict `needs:` chain + 60s
  # propagation sleep between steps (index lag).
  # ----------------------------------------------------------------
  publish-cargo-supervisor:
    name: publish-cargo-supervisor
    needs:
      - release-please
      - ensure-release
      - publish-version
      - publish-root
    if: >-
      always() && !cancelled() &&
      needs.publish-root.result == 'success' &&
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
        with: { shared-key: cargo-publish-supervisor }
      - name: cargo publish --dry-run
        run: cargo publish --dry-run -p terminal-commander-supervisor
      - name: cargo publish
        shell: bash
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}
        run: |
          set -e
          if cargo publish -p terminal-commander-supervisor; then
            exit 0
          fi
          if [ "${{ github.event_name }}" = "workflow_dispatch" ] && [ "${{ inputs.force_publish }}" = "true" ]; then
            ver=$(cargo pkgid -p terminal-commander-supervisor | sed 's/.*#//')
            if cargo search terminal-commander-supervisor --limit 1 | grep -q "= \"$ver\""; then
              echo "::warning::terminal-commander-supervisor@$ver already on crates.io (force_publish recovery)"
              exit 0
            fi
          fi
          exit 1
      - name: Wait 60s for crates.io index propagation
        run: sleep 60

  publish-cargo-daemon:
    name: publish-cargo-daemon
    needs:
      - release-please
      - ensure-release
      - publish-version
      - publish-cargo-supervisor
    if: >-
      always() && !cancelled() &&
      needs.publish-cargo-supervisor.result == 'success'
    runs-on: ubuntu-24.04
    permissions: { contents: read }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "1.95.0" }
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: cargo-publish-daemon }
      - name: cargo publish --dry-run
        run: cargo publish --dry-run -p terminal-commanderd
      - name: cargo publish
        shell: bash
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}
        run: |
          set -e
          if cargo publish -p terminal-commanderd; then exit 0; fi
          if [ "${{ github.event_name }}" = "workflow_dispatch" ] && [ "${{ inputs.force_publish }}" = "true" ]; then
            ver=$(cargo pkgid -p terminal-commanderd | sed 's/.*#//')
            if cargo search terminal-commanderd --limit 1 | grep -q "= \"$ver\""; then
              echo "::warning::terminal-commanderd@$ver already on crates.io"
              exit 0
            fi
          fi
          exit 1
      - run: sleep 60

  publish-cargo-mcp:
    name: publish-cargo-mcp
    needs:
      - release-please
      - ensure-release
      - publish-version
      - publish-cargo-daemon
    if: >-
      always() && !cancelled() &&
      needs.publish-cargo-daemon.result == 'success'
    runs-on: ubuntu-24.04
    permissions: { contents: read }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: "1.95.0" }
      - uses: Swatinem/rust-cache@v2
        with: { shared-key: cargo-publish-mcp }
      - name: cargo publish --dry-run
        run: cargo publish --dry-run -p terminal-commander-mcp
      - name: cargo publish
        shell: bash
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN_TC }}
        run: |
          set -e
          if cargo publish -p terminal-commander-mcp; then exit 0; fi
          if [ "${{ github.event_name }}" = "workflow_dispatch" ] && [ "${{ inputs.force_publish }}" = "true" ]; then
            ver=$(cargo pkgid -p terminal-commander-mcp | sed 's/.*#//')
            if cargo search terminal-commander-mcp --limit 1 | grep -q "= \"$ver\""; then
              echo "::warning::terminal-commander-mcp@$ver already on crates.io"
              exit 0
            fi
          fi
          exit 1
```

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
      - name: Install from npm + smoke
        env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          docker run --rm node:20-alpine sh -c "
            set -e
            npm install -g terminal-commander@${VER}
            terminal-commander-mcp --version
            # MCP initialize stdio probe (10s timeout)
            echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"smoke\",\"version\":\"0.0.0\"}}}' \
              | timeout 10 terminal-commander-mcp || [ \$? -eq 124 ]
            echo 'verify-linux-x64: OK'
          "

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
          docker run --rm node:20-alpine sh -c "
            set -e
            npm install -g terminal-commander@${VER}
            terminal-commander-mcp --version
            echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"smoke\",\"version\":\"0.0.0\"}}}' \
              | timeout 10 terminal-commander-mcp || [ \$? -eq 124 ]
            echo 'verify-linux-arm64: OK'
          "

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
      - shell: powershell
        env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          $ErrorActionPreference = 'Stop'
          npm install -g terminal-commander@${env:VER}
          terminal-commander-mcp --version
          # Stdio probe: 10s timeout via PowerShell job
          $job = Start-Job -ScriptBlock {
            $msg = '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.0"}}}'
            $msg | terminal-commander-mcp
          }
          Wait-Job $job -Timeout 10 | Out-Null
          Stop-Job $job -ErrorAction SilentlyContinue
          Write-Host "verify-windows-x64: OK"

  verify-mac-x64:
    name: verify-mac-x64
    needs: [release-please, publish-root]
    if: needs.publish-root.result == 'success'
    runs-on: macos-13   # Intel
    permissions: { contents: read, issues: write }
    steps:
      - run: sleep 60
      - uses: actions/setup-node@v4
        with: { node-version: "20" }
      - env:
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          set -e
          npm install -g terminal-commander@${VER}
          terminal-commander-mcp --version
          echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.0"}}}' \
            | gtimeout 10 terminal-commander-mcp || [ $? -eq 124 ]
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
          npm install -g terminal-commander@${VER}
          terminal-commander-mcp --version
          # macOS lacks `timeout`; install via brew not allowed in 10s budget — skip timeout
          echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.0"}}}' \
            | terminal-commander-mcp &
          PID=$!
          sleep 10
          kill -9 $PID 2>/dev/null || true
          wait $PID 2>/dev/null || true
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
      - name: Open P0 issue
        env:
          GH_TOKEN: ${{ secrets.RELEASE_PLEASE_TOKEN_TC }}
          VER: ${{ needs.release-please.outputs.version }}
        run: |
          set -e
          body=$(cat <<EOF
          Release **${VER}** failed post-publish smoke verification.

          Verify job results:
          - linux-x64   : ${{ needs.verify-linux-x64.result }}
          - linux-arm64 : ${{ needs.verify-linux-arm64.result }}
          - windows-x64 : ${{ needs.verify-windows-x64.result }}
          - mac-x64     : ${{ needs.verify-mac-x64.result }}
          - mac-arm64   : ${{ needs.verify-mac-arm64.result }}

          **Action required:** Run \`.github/workflows/deprecate-version.yml\` for version ${VER} ASAP, then triage + patch release.
          Workflow run: ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}
          EOF
          )
          gh issue create \
            --title "release ${VER} smoke FAILED" \
            --label "release-broken,P0" \
            --body "$body"
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
echo "═══ crates.io — checking @ ${VER} ═══"
for crate in "${CARGO_CRATES[@]}"; do
  if cargo search "$crate" --limit 1 2>/dev/null | grep -q "^${crate} = \"${VER}\""; then
    printf "  OK    %s@%s\n" "$crate" "$VER"
  else
    printf "  MISS  %s@%s\n" "$crate" "$VER"
    missing_cargo+=("$crate")
  fi
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

## Normal release flow

1. Land conventional-commit work on `main`.
2. `release-please` opens "chore: release X.Y.Z" PR. Review the diff.
   - Confirm all 6 manifest entries bump.
   - Confirm Cargo.toml workspace version bumps.
   - Confirm root `optionalDependencies` all 5 platforms at new version.
3. Merge the PR.
4. Same workflow run: 5 platform builds (parallel) → 5 publish-platform
   (parallel) → publish-root → 3 cargo publish chain → 5 verify jobs.
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
2. The script reports missing artifacts + republish commands.
3. Re-run the release-please.yml workflow with `force_publish: true`
   for npm side. The E409-tolerant pattern republishes only missing.
4. For cargo side: from a checkout at tag `vX.Y.Z`, run the script's
   suggested `cargo publish -p <crate>` commands in order.

## Secret rotation (annual)

NPM_TOKEN_TC, CARGO_REGISTRY_TOKEN_TC, RELEASE_PLEASE_TOKEN_TC have
365-day max lifetimes. The weekly `secret-health.yml` cron files a P1
issue the first Monday after a token starts failing.

1. Rotate the failing token at its registry UI.
2. Update repo secret at https://github.com/special-place-administrator/terminal-commander/settings/secrets/actions
3. Manually re-run `secret-health.yml` to confirm green.

## OIDC migration (one-time)

After the first 0.2.0 release ships using `NPM_TOKEN_TC`:

1. For each of the 6 npm packages, configure trusted publisher on
   npmjs.com:
   - Pkg → Settings → Publishing → Trusted Publishers → GitHub Actions
   - Org: special-place-administrator
   - Repo: terminal-commander
   - Workflow file: .github/workflows/release-please.yml
   - Environment: (leave blank)
2. For each of the 3 cargo crates, same on crates.io.
3. Edit `release-please.yml`: remove `NODE_AUTH_TOKEN` env from publish
   steps, add `--provenance` flag to `npm publish`. Remove
   `CARGO_REGISTRY_TOKEN` env from cargo publish steps.
4. Confirm next release's published tarball shows `dist.attestations`.
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

- [ ] **Step 3: cargo publish dry-run**

```bash
cargo publish --dry-run -p terminal-commander-supervisor --allow-dirty
cargo publish --dry-run -p terminal-commanderd --allow-dirty
cargo publish --dry-run -p terminal-commander-mcp --allow-dirty
```

Expected: 3 successes.

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

## Task 20: Wire daemon README into published-crate tarballs

**Files:**
- Verify: `crates/daemon/README.md` exists, `crates/mcp/README.md` exists, `crates/supervisor/README.md` exists

- [ ] **Step 1: cargo publishing requires a README per crate**

For each of `crates/supervisor`, `crates/daemon`, `crates/mcp`, check if a README.md exists in that crate directory. If not, create one with minimal content:

```markdown
# terminal-commander-<role>

Part of [Terminal Commander](https://github.com/special-place-administrator/terminal-commander).

See the [main README](https://github.com/special-place-administrator/terminal-commander#readme) for project overview.
```

(`<role>` = `supervisor` | `daemon` | `mcp`.)

- [ ] **Step 2: Update each crate's Cargo.toml `readme` field**

Each `[package]` block should have:
```toml
readme = "README.md"
```
(NOT `readme.workspace = true` — the workspace path `../../README.md` is wrong for a published tarball, where the crate dir is the root.)

Revisit Task 11 Step 3: change `readme.workspace = true` to literal `readme = "README.md"` in the 3 published crates.

- [ ] **Step 3: Re-run dry publish**

```bash
cargo publish --dry-run -p terminal-commander-supervisor --allow-dirty
cargo publish --dry-run -p terminal-commanderd --allow-dirty
cargo publish --dry-run -p terminal-commander-mcp --allow-dirty
```

Expected: 3 successes (README warnings gone).

- [ ] **Step 4: Commit**

```bash
git add crates/supervisor/README.md crates/daemon/README.md crates/mcp/README.md crates/supervisor/Cargo.toml crates/daemon/Cargo.toml crates/mcp/Cargo.toml
git commit -m "feat(cargo): per-crate README.md + fix readme path for published-tarball context"
```

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

## Self-review

**Spec coverage:** All 17 spec changes (A.1-A.7, B.8-B.10, C.11-C.12, D.13, E.14-E.16, F.17) mapped to tasks:

- A.1 (mac-x64 pkg) → Task 1
- A.2 (mac-arm64 pkg) → Task 2
- A.3 (release-please-config) → Task 3
- A.4 (manifest) → Task 4
- A.5 (root pkg.json optionalDeps) → Task 5
- A.6 (resolver darwin) → Task 6
- A.7 (sync/verify scripts) → Task 7
- B.8 (_build-platform-binary.yml) → Task 8
- B.9 (npm-binary-build collapse) → Task 9
- B.10 (release-please.yml 5 publish jobs) → Task 10
- C.11 (Cargo.toml workspace bump) → Task 11
- C.12 (cargo publish jobs + parity assert) → Task 12
- D.13 (verify jobs + mark-broken) → Task 13
- E.14 (secret-health.yml) → Task 14
- E.15 (recover-partial-publish.sh) → Task 15
- E.16 (deprecate-version.yml) → Task 16
- F.17 (OIDC migration) → Task 17 (runbook only; live migration is a post-first-release operator action, not code)

Plus:
- Task 18: local smoke
- Task 19: full pre-push gates
- Task 20: README.md per crate (cargo publish requirement) — discovered during planning
- Task 21: push + open PR
- Task 22: codex adversarial review

**Placeholder scan:** all steps have concrete code, file paths, commands. No TBDs.

**Type consistency:** workflow filenames, package names, version strings all spelled identically throughout. `PLATFORM_PACKAGES` module created in Task 7 referenced consistently in Task 10 Step 4.

**Known caveats:**
- macOS verify jobs on `macos-13` + `macos-14` runners — GitHub deprecates macOS runners on ~18-month cycle. Runbook task 17 §"Secret rotation" doesn't cover runner rotation. Add a NIT to the runbook in Task 17 if codex flags it.
- `cargo-zigbuild` cross-compile to macOS targets unsigned Mach-O binaries. They run locally fine but Apple Gatekeeper will quarantine. Acceptable for tier-3 build-only target; users on locked-down macs need a workaround we don't document yet. Spec explicitly says we ship artifacts but don't QA them.
- `RELEASE_PLEASE_TOKEN_TC` used to open issues in verify-fail / secret-health flows. If this PAT itself is the failing token, the failsafe uses `GITHUB_TOKEN` (Task 14 step 3).

**Execution mode:** Subagent-Driven Development per goal directive ("ralph loop, review, testing etc, without any user intervention").
