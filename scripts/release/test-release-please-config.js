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
