// SPDX-License-Identifier: Apache-2.0

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const PKG_ROOT = path.resolve(__dirname, "..");

test("npm install is passive: no lifecycle script starts bootstrap work", () => {
  const pkg = JSON.parse(fs.readFileSync(path.join(PKG_ROOT, "package.json"), "utf8"));
  const scripts = pkg.scripts || {};

  assert.equal(scripts.install, undefined);
  assert.equal(scripts.postinstall, undefined);
  assert.equal(scripts.preinstall, undefined);
  assert.equal((pkg.files || []).includes("scripts/"), false);
  assert.equal(fs.existsSync(path.join(PKG_ROOT, "scripts", "install.js")), false);
  assert.equal(
    fs.existsSync(path.join(PKG_ROOT, "lib", "daemon", "session_supervisor.js")),
    false,
  );
});

test("Cursor-facing MCP shim directly spawns the native MCP binary", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander-mcp.js"),
    "utf8",
  );

  assert.match(src, /require\(\s*["']child_process["']\s*\)/);
  assert.match(src, /\bspawn\s*\(\s*result\.binaryPath/);
  assert.doesNotMatch(src, /session_supervisor/);
  assert.doesNotMatch(src, /runHarnessMcpSession/);
  assert.doesNotMatch(src, /windowsHide/);
});

test("admin CLI update is explicit npm update with no shell wrapper", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander.js"),
    "utf8",
  );

  assert.match(src, /terminal-commander@latest/);
  assert.match(src, /update-locks/);
  assert.match(src, /npm-cli\.js/);
  assert.match(src, /process\.execPath/);
  assert.match(src, /shell:\s*false/);
  assert.doesNotMatch(src, /npm\.cmd|taskkill|cmd\.exe|cmd \/c|powershell|ExecutionPolicy|windowsHide/);
});

test("admin CLI version advisory checks npm registry without spawning npm", () => {
  const src = fs.readFileSync(
    path.join(PKG_ROOT, "bin", "terminal-commander.js"),
    "utf8",
  );

  assert.match(src, /registry\.npmjs\.org\/terminal-commander\/latest/);
  assert.match(src, /Update available/);
  assert.doesNotMatch(src, /npm view/);
});

test("JS-only control-plane commands route before native binary spawn", () => {
  const shim = path.join(PKG_ROOT, "bin", "terminal-commander.js");
  for (const args of [
    ["setup", "--help"],
    ["pair", "--help"],
    ["doctor", "harness", "--help"],
  ]) {
    const r = spawnSync(process.execPath, [shim, ...args], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      shell: false,
    });
    assert.equal(r.status, 0, `${args.join(" ")} failed: ${r.stderr}`);
    assert.equal(r.stderr, "");
    assert.match(r.stdout, /terminal-commander/);
  }
});
