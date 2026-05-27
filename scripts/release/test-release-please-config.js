"use strict";
const assert = require("node:assert/strict");
const test = require("node:test");
const cfg = require("../../.github/release-please-config.json");

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

test("root '.' component is a simple release-type version driver", () => {
  const root = cfg.packages["."];
  assert.ok(root, "missing '.' root component");
  assert.equal(root["release-type"], "simple");
  assert.equal(root.component, "terminal-commander-root");
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
