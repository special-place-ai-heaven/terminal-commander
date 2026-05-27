# Fully Automated Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a `feat:`/`fix:` push touching `crates/` (or anywhere) auto-cut a release and publish npm binaries with no human step, and gate publish on a pre-publish bookworm load test so full-auto cannot ship a glibc-broken binary.

**Architecture:** Two parts in one workflow + config change. Part 1 adds a `"."` root version-driver component (release-type `simple`) to the existing linked-versions group, so release-please attributes root/`crates/` commits and bumps the shared version (the existing auto-merge in `release-pr-sync.yml` then publishes with no human). Part 2 adds linux-only pre-publish jobs that load the BUILT binary in `node:22-bookworm-slim` and gate `publish-linux-*` on them. The config-test fixture is updated to expect the new component + manifest entry.

**Tech Stack:** release-please (manifest mode, linked-versions plugin, `simple` release-type), GitHub Actions, docker (`node:22-bookworm-slim`), Node test runner, conventional commits.

---

## Background: verified facts (do not re-derive)

- `.github/release-please-config.json`: `plugins[0]` is `linked-versions` with `groupName: terminal-commander` and a `components` array of the 6 npm component names. `packages` maps 6 `packages/terminal-commander*` dirs, each `release-type: node`. The 6-component linked group is already load-bearing (v0.1.13 released through it), so adding a 7th linked member is the same shape -- the action's `version` output stays the single shared version.
- `.github/.release-please-manifest.json`: 6 entries, all `"0.1.13"`.
- `release-please.yml` outputs `releases_created` / `version` / `tag_name` at the RELEASE level (not per-component); publish jobs gate on `releases_created == 'true'` and read `version` -- a root-triggered release sets these identically.
- `release-pr-sync.yml`: on the release PR, syncs versions, reads version from `packages/terminal-commander/package.json` (line 42/51), runs `gh pr merge --auto` (no human). linked-versions keeps that package.json equal to the root bump.
- `publish-linux-x64` (release-please.yml:317-391): `needs: [release-please, ensure-release, publish-version, build-linux-x64]`, `if:` gates on `build-linux-x64.result == 'success'` + releases_created. Downloads artifact `tc-bin-linux-x64` into `packages/terminal-commander-linux-x64/`, untars `bin.tar`, version-match, `npm publish`.
- `publish-linux-arm64` (409-482): same shape, artifact `tc-bin-linux-arm64`, dir `packages/terminal-commander-linux-arm64/`.
- `build-linux-x64`/`-arm64` (302-315, 394-407): call `_build-platform-binary.yml` with `upload_artifact: true`; the artifact is `bin.tar` (tar of `bin/`, preserves +x). Build already runs the objdump glibc guard (commit 538bba0).
- Post-publish `verify-linux-x64`/`-arm64` (1532-1582): `needs: [release-please, publish-root]`, `docker run node:22-bookworm-slim`, `npm install -g terminal-commander@VER`, `terminal-commander-mcp --version` + MCP initialize stdio probe (rc 0 or 124 = pass). These STAY as post-publish backstop.
- Platform package `package.json`: `files: ["bin/","LICENSE"]`, `os`/`cpu` gated, NO `bin` field (the root wrapper exposes the CLI). So a pre-publish smoke execs the staged binary DIRECTLY (not via npm install).
- `scripts/release/test-release-please-config.js`: hardcodes TWO brittle assertions that this change breaks:
  - "linked-versions includes all 6 components" (exact `Set` of 6) -- line ~8.
  - "manifest tracks all 6 packages at same version" (`entries.length === 6`) -- line ~48.
  Both must be updated or the npm-binary-build PR gate (which runs this test) fails.
- `release-type: simple` natively updates a root `version.txt` + CHANGELOG; no `x-release-please-*` markers needed (markers are only for the `generic` updater on arbitrary files).

## File Structure

- **Modify:** `.github/release-please-config.json` (root component + linked-versions member).
- **Modify:** `.github/.release-please-manifest.json` (`"."` entry).
- **Create:** `version.txt` (repo root, version anchor for `simple`).
- **Modify:** `scripts/release/test-release-please-config.js` (update the 2 brittle assertions for the 7th component + 7th manifest entry).
- **Modify:** `.github/workflows/release-please.yml` (2 new presmoke jobs; add `needs: presmoke-*` to the 2 publish-linux jobs).

---

## Task 1: Root version-driver component (config + manifest + version.txt)

**Files:**
- Modify: `.github/release-please-config.json`
- Modify: `.github/.release-please-manifest.json`
- Create: `version.txt`

- [ ] **Step 1: Add the `"."` root component to the packages map**

In `.github/release-please-config.json`, add a `"."` entry to the `packages` object (alongside the 6 existing). Insert it as the FIRST key in `packages` for readability:

```json
  "packages": {
    ".": {
      "release-type": "simple",
      "component": "terminal-commander-root",
      "package-name": "terminal-commander-root",
      "changelog-path": "CHANGELOG.md"
    },
    "packages/terminal-commander": {
```
(leave the 6 existing package entries exactly as-is after this).

- [ ] **Step 2: Add the root component to linked-versions**

In the same file, add `"terminal-commander-root"` to `plugins[0].components`:

```json
  "plugins": [
    {
      "type": "linked-versions",
      "groupName": "terminal-commander",
      "components": [
        "terminal-commander-root",
        "terminal-commander",
        "@terminal-commander/linux-x64",
        "@terminal-commander/linux-arm64",
        "@terminal-commander/windows-x64",
        "@terminal-commander/mac-x64",
        "@terminal-commander/mac-arm64"
      ]
    }
  ],
```

- [ ] **Step 3: Add the manifest entry**

In `.github/.release-please-manifest.json`, add the `"."` key (keep the 6 existing):

```json
{
  ".": "0.1.13",
  "packages/terminal-commander": "0.1.13",
  "packages/terminal-commander-linux-x64": "0.1.13",
  "packages/terminal-commander-linux-arm64": "0.1.13",
  "packages/terminal-commander-windows-x64": "0.1.13",
  "packages/terminal-commander-mac-x64": "0.1.13",
  "packages/terminal-commander-mac-arm64": "0.1.13"
}
```

- [ ] **Step 4: Create the root version anchor**

Create `version.txt` at the repo root with exactly:

```
0.1.13
```
(single line, trailing newline. `simple` rewrites this on each release.)

- [ ] **Step 5: Validate JSON**

```
cd "C:/Users/poslj/terminal-commander"
node -e "require('./.github/release-please-config.json'); require('./.github/.release-please-manifest.json'); console.log('json ok')"
```
Expected: `json ok`.

- [ ] **Step 6: Commit**

```
git add .github/release-please-config.json .github/.release-please-manifest.json version.txt
git commit -F <msg-file>
```
Subject: `feat(ci): add root version-driver component so crates/ changes release`
Body: a "." simple component in the linked-versions group; feat:/fix: anywhere now bumps the shared version. (This is a feat: ON PURPOSE -- it is the commit that proves the pipeline fires.)

---

## Task 2: Update the config-test fixture (unbreak the PR gate)

**Files:**
- Modify: `scripts/release/test-release-please-config.js`

- [ ] **Step 1: Update the linked-versions assertion to expect 7 components**

Replace the `Set` of 6 in the "linked-versions includes all 6 components" test with 7 (add the root + rename the test):

```javascript
test("linked-versions includes all 7 components (6 npm + root driver)", () => {
  const components = cfg.plugins[0].components;
  assert.deepEqual(new Set(components), new Set([
    "terminal-commander-root",
    "terminal-commander",
    "@terminal-commander/linux-x64",
    "@terminal-commander/linux-arm64",
    "@terminal-commander/windows-x64",
    "@terminal-commander/mac-x64",
    "@terminal-commander/mac-arm64",
  ]));
});
```

- [ ] **Step 2: Update the manifest-count assertion to expect 7 entries**

Replace the manifest test's `entries.length === 6` and add the `"."` key check:

```javascript
test("manifest tracks all 7 entries (6 packages + root) at same version", () => {
  const manifest = require("../../.github/.release-please-manifest.json");
  const entries = Object.entries(manifest);
  assert.equal(entries.length, 7, `manifest has ${entries.length} entries, expected 7`);
  const versions = new Set(entries.map(([, v]) => v));
  assert.equal(versions.size, 1, "all manifest entries should share the same version");
  for (const dir of [
    ".",
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
(Leave all other tests in the file unchanged -- the platform-package, optionalDependencies, and sync-script tests are unaffected.)

- [ ] **Step 2b: Add a test pinning the root component shape**

Append a new test so the root driver can't be silently dropped:

```javascript
test("root '.' component is a simple release-type version driver", () => {
  const root = cfg.packages["."];
  assert.ok(root, "missing '.' root component");
  assert.equal(root["release-type"], "simple");
  assert.equal(root.component, "terminal-commander-root");
});
```

- [ ] **Step 3: Run the config test**

```
cd "C:/Users/poslj/terminal-commander"
node scripts/release/test-release-please-config.js
```
Expected: all tests pass (the 2 edited + the new one + the untouched rest). If `node --test` discovery is needed, run `node --test scripts/release/test-release-please-config.js`.

- [ ] **Step 4: Commit**

```
git add scripts/release/test-release-please-config.js
git commit -F <msg-file>
```
Subject: `test(ci): config fixture expects the 7th (root) linked component`

---

## Task 3: Pre-publish bookworm load smoke (linux-x64)

**Files:**
- Modify: `.github/workflows/release-please.yml` (new `presmoke-linux-x64` job; add `needs` to `publish-linux-x64`)

- [ ] **Step 1: Add the presmoke-linux-x64 job**

Insert immediately AFTER the `build-linux-x64` job (ends line ~315, before `publish-linux-x64` at ~317):

```yaml
  presmoke-linux-x64:
    name: presmoke-linux-x64
    needs:
      - release-please
      - ensure-release
      - publish-version
      - build-linux-x64
    if: >-
      always() && !cancelled() &&
      needs.build-linux-x64.result == 'success' &&
      (needs.release-please.outputs.releases_created == 'true' ||
      (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
      needs.publish-version.outputs.publish == 'true')
    runs-on: ubuntu-24.04
    permissions:
      contents: read
    steps:
      - name: Download bin.tar artifact
        uses: actions/download-artifact@v4
        with:
          name: tc-bin-linux-x64
          path: pkg/
      - name: Extract bin.tar (preserves +x mode)
        shell: bash
        run: |
          set -euo pipefail
          cd pkg
          tar -xf bin.tar
          rm bin.tar
          ls -la bin/
      - name: Load the BUILT binary on the glibc floor (bookworm, pre-publish)
        shell: bash
        run: |
          # The built artifact must LOAD + run on the floor distro
          # (Debian 12 bookworm, glibc 2.36) BEFORE we publish it. This
          # is the gate that v0.1.13 lacked (its verify ran post-publish).
          # Exec the staged binary directly (the platform package has no
          # `bin` field; the CLI is exposed by the root wrapper, not
          # needed for a load test). rc 0 or 124 (clean timeout) = pass.
          set -euo pipefail
          docker run --rm -v "$PWD/pkg/bin:/tcbin:ro" node:22-bookworm-slim sh -c '
            set -e
            /tcbin/terminal-commander-mcp --version
            echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"presmoke\",\"version\":\"0.0.0\"}}}" \
              | timeout 10 /tcbin/terminal-commander-mcp; rc=$?
            if [ "$rc" -ne 0 ] && [ "$rc" -ne 124 ]; then
              echo "::error::pre-publish stdio probe exited $rc on bookworm" >&2; exit "$rc"
            fi
            echo "presmoke-linux-x64: OK (loads + runs on glibc 2.36)"
          '
```

- [ ] **Step 2: Gate publish-linux-x64 on the presmoke**

In the `publish-linux-x64` job's `needs:` list, add `presmoke-linux-x64`, and add a `result == 'success'` clause to its `if:`. The `needs:` becomes:

```yaml
    needs:
      - release-please
      - ensure-release
      - publish-version
      - build-linux-x64
      - presmoke-linux-x64
```
and the `if:` gains `needs.presmoke-linux-x64.result == 'success' &&` immediately after the `needs.build-linux-x64.result == 'success' &&` line:

```yaml
    if: >-
      always() && !cancelled() &&
      needs.build-linux-x64.result == 'success' &&
      needs.presmoke-linux-x64.result == 'success' &&
      (needs.release-please.outputs.releases_created == 'true' ||
      (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
      needs.publish-version.outputs.publish == 'true')
```

- [ ] **Step 3: Validate workflow YAML**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import yaml; yaml.safe_load(open(\".github/workflows/release-please.yml\")); print(\"yaml ok\")'"
```
Expected: `yaml ok`.

- [ ] **Step 4: Commit**

Subject: `feat(ci): pre-publish bookworm load smoke gates linux-x64 publish`

---

## Task 4: Pre-publish bookworm load smoke (linux-arm64)

**Files:**
- Modify: `.github/workflows/release-please.yml` (new `presmoke-linux-arm64` job; add `needs` to `publish-linux-arm64`)

- [ ] **Step 1: Add the presmoke-linux-arm64 job**

Insert immediately AFTER the `build-linux-arm64` job (ends ~407, before `publish-linux-arm64` at ~409). Identical to presmoke-linux-x64 EXCEPT artifact name, runner, and labels:

```yaml
  presmoke-linux-arm64:
    name: presmoke-linux-arm64
    needs:
      - release-please
      - ensure-release
      - publish-version
      - build-linux-arm64
    if: >-
      always() && !cancelled() &&
      needs.build-linux-arm64.result == 'success' &&
      (needs.release-please.outputs.releases_created == 'true' ||
      (needs.ensure-release.result == 'success' && needs.ensure-release.outputs.publish == 'true') ||
      needs.publish-version.outputs.publish == 'true')
    runs-on: ubuntu-24.04-arm
    permissions:
      contents: read
    steps:
      - name: Download bin.tar artifact
        uses: actions/download-artifact@v4
        with:
          name: tc-bin-linux-arm64
          path: pkg/
      - name: Extract bin.tar (preserves +x mode)
        shell: bash
        run: |
          set -euo pipefail
          cd pkg
          tar -xf bin.tar
          rm bin.tar
          ls -la bin/
      - name: Load the BUILT binary on the glibc floor (bookworm, pre-publish)
        shell: bash
        run: |
          set -euo pipefail
          docker run --rm -v "$PWD/pkg/bin:/tcbin:ro" node:22-bookworm-slim sh -c '
            set -e
            /tcbin/terminal-commander-mcp --version
            echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"presmoke\",\"version\":\"0.0.0\"}}}" \
              | timeout 10 /tcbin/terminal-commander-mcp; rc=$?
            if [ "$rc" -ne 0 ] && [ "$rc" -ne 124 ]; then
              echo "::error::pre-publish stdio probe exited $rc on bookworm" >&2; exit "$rc"
            fi
            echo "presmoke-linux-arm64: OK (loads + runs on glibc 2.36)"
          '
```

- [ ] **Step 2: Gate publish-linux-arm64 on the presmoke**

Add `presmoke-linux-arm64` to the `publish-linux-arm64` `needs:` and `needs.presmoke-linux-arm64.result == 'success' &&` to its `if:` (after the `build-linux-arm64.result` clause), mirroring Task 3 Step 2.

- [ ] **Step 3: Validate workflow YAML**

```
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import yaml; yaml.safe_load(open(\".github/workflows/release-please.yml\")); print(\"yaml ok\")'"
```
Expected: `yaml ok`.

- [ ] **Step 4: Commit**

Subject: `feat(ci): pre-publish bookworm load smoke gates linux-arm64 publish`

---

## Task 5: Push + let the pipeline prove itself (live 0.1.14)

- [ ] **Step 1: Final static checks**

```
cd "C:/Users/poslj/terminal-commander"
node scripts/release/test-release-please-config.js
node -e "require('./.github/release-please-config.json'); require('./.github/.release-please-manifest.json'); console.log('json ok')"
wsl.exe bash -lc "cd /mnt/c/Users/poslj/terminal-commander && python3 -c 'import yaml; yaml.safe_load(open(\".github/workflows/release-please.yml\")); print(\"yaml ok\")'"
```
Expected: config tests pass, `json ok`, `yaml ok`.

- [ ] **Step 2: Push to heaven**

```
git fetch heaven
git rev-list --left-right --count heaven/main...main   # expect 0  N (no divergence)
git push heaven main
```

- [ ] **Step 3: Watch the pipeline fire (this is the proof)**

The Task 1 `feat:` commit gives release-please a root-attributed `feat:` -> it opens a release PR bumping `.` + all 6 npm packages to 0.1.14. Watch:

```
gh run list --repo special-place-ai-heaven/terminal-commander --limit 6
gh pr list --repo special-place-ai-heaven/terminal-commander --state open
```
Expect: a `chore: release 0.1.14` PR opens; `release-pr-sync` auto-merges it; the merge re-triggers release-please -> build (glibc guard) -> presmoke (bookworm load) -> publish -> post-publish verify. Track to completion with `gh run watch <id>`.

- [ ] **Step 4: Confirm the binary is installable**

After publish completes:
```
wsl.exe bash -lc 'docker run --rm node:22-bookworm-slim sh -c "npm install -g terminal-commander@0.1.14 && terminal-commander-mcp --version"'
```
Expected: installs + prints a version, on a clean bookworm box (glibc 2.36). This is the end-to-end "push -> npm binary" proof.

- [ ] **Step 5: Report (DSPIVR)**

Objective, changes (config + manifest + version.txt + config-test + 2 presmoke jobs + 2 publish gates), verification (config tests local; pipeline fired + auto-merged + presmoke passed + published 0.1.14 + clean-box install works), evidence (the release PR number, the run id, the install output), known gaps (v0.1.14 supersedes broken v0.1.13 -> add the known-issue note; mac/windows have no pre-publish floor smoke).

---

## Spec coverage check

- Part 1 root component + linked-versions + manifest + version.txt -> Task 1.
- Config-test fixture unbroken (the brittle 6-component/6-entry assertions) -> Task 2.
- Part 2 presmoke linux-x64 + publish gate -> Task 3.
- Part 2 presmoke linux-arm64 + publish gate -> Task 4.
- CI-only verification + the live 0.1.14 proof (implementation lands as feat:) -> Task 5.
- linux-only presmoke (mac/windows unaffected) -> Tasks 3-4 touch only linux jobs.

## Notes carried from reasoning

- The 7th linked component is safe: the linked group is already multi-component in production; `version` output stays the single shared version the publish jobs consume.
- presmoke execs the staged binary DIRECTLY (platform package has no `bin` field); it tests the glibc LOAD class, which is the v0.1.13 failure, without npm-packaging entanglement.
- `simple` updates version.txt natively; no x-release-please markers needed.
- Task 1's commit MUST be `feat:` (not chore/docs) or release-please won't bump -- it is the spark that proves the whole chain.
