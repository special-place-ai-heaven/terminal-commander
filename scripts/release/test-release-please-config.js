"use strict";
const assert = require("node:assert/strict");
const test = require("node:test");
const cfg = require("../../.github/release-please-config.json");

const RELEASED_COMPONENTS = [
  "terminal-commander",
  "@terminal-commander/linux-x64",
  "@terminal-commander/linux-arm64",
  "@terminal-commander/windows-x64",
];

const PLATFORM_DIRS = [
  "packages/terminal-commander-linux-x64",
  "packages/terminal-commander-linux-arm64",
  "packages/terminal-commander-windows-x64",
];

test("linked-versions includes the root and Linux/Windows components", () => {
  const components = cfg.plugins[0].components;
  assert.deepEqual(new Set(components), new Set(RELEASED_COMPONENTS));
  assert.equal(components.some((name) => name.includes("mac")), false);
});

test("root extra-files bumps Linux and Windows optionalDependencies only", () => {
  const ef = cfg.packages["packages/terminal-commander"]["extra-files"];
  const paths = ef.map((e) => e.jsonpath);
  assert.deepEqual(paths, RELEASED_COMPONENTS.slice(1).map(
    (dep) => `$.optionalDependencies['${dep}']`,
  ));
});

test("Linux and Windows platform packages have release-please entries", () => {
  const configured = Object.keys(cfg.packages).filter((dir) => dir !== "packages/terminal-commander");
  assert.deepEqual(configured, PLATFORM_DIRS);
  for (const dir of PLATFORM_DIRS) {
    assert.equal(cfg.packages[dir]["release-type"], "node");
  }
});

test("manifest tracks root and Linux/Windows packages at the same version", () => {
  const manifest = require("../../.github/.release-please-manifest.json");
  const entries = Object.entries(manifest);
  assert.deepEqual(Object.keys(manifest), [
    "packages/terminal-commander",
    ...PLATFORM_DIRS,
  ]);
  const versions = new Set(entries.map(([, v]) => v));
  assert.equal(versions.size, 1, "all manifest entries should share the same version");
});

test("root optionalDependencies pin Linux and Windows packages by version (not file:)", () => {
  const pkg = require("../../packages/terminal-commander/package.json");
  const expected = pkg.version;
  const od = pkg.optionalDependencies || {};
  assert.deepEqual(Object.keys(od), RELEASED_COMPONENTS.slice(1));
  assert.deepEqual(pkg.os, ["linux", "win32"]);
  for (const dep of Object.keys(od)) {
    assert.equal(od[dep].startsWith("file:"), false, `${dep} must be version-pinned, not file:`);
    assert.equal(od[dep], expected, `${dep} = ${od[dep]} != ${expected}`);
  }
});

test("platform-packages.js lists Linux and Windows only", () => {
  const { PLATFORM_PACKAGES } = require("./platform-packages.js");
  assert.deepEqual(PLATFORM_PACKAGES, RELEASED_COMPONENTS.slice(1));
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
