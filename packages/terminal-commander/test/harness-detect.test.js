// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { detectCodex, detectCursor } = require("../lib/harness/detect.js");
const { buildJsonMcpStanza } = require("../lib/harness/write_all.js");
const { buildCodexTomlBlock, writeCodexTomlConfig } = require("../lib/harness/io/toml_mcp.js");

test("detectCodex finds config.toml in temp home", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-harness-"));
  const codexDir = path.join(root, ".codex");
  fs.mkdirSync(codexDir, { recursive: true });
  fs.writeFileSync(path.join(codexDir, "config.toml"), "# empty\n");
  const r = detectCodex({
    platform: process.platform,
    env: { ...process.env, HOME: root, USERPROFILE: root },
  });
  assert.equal(r.detected, true);
  assert.match(r.config_path, /config\.toml$/);
});

test("writeCodexTomlConfig creates section in fresh file", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-codex-"));
  const target = path.join(root, "config.toml");
  const r = writeCodexTomlConfig({
    path: target,
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
  });
  assert.equal(r.status, "config_created");
  const text = fs.readFileSync(target, "utf8");
  assert.match(text, /\[mcp_servers\.terminal_commander\]/);
  assert.match(text, /command = "C:\/node\/node\.exe"/);
  assert.match(text, /args = \["C:\/pkg\/bin\/terminal-commander-mcp\.js"\]/);
});

test("buildCodexTomlBlock uses executable command plus JS shim args", () => {
  const text = buildCodexTomlBlock({
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
  });
  assert.match(text, /command = "C:\/node\/node\.exe"/);
  assert.match(text, /args = \["C:\/pkg\/bin\/terminal-commander-mcp\.js"\]/);
});

test("buildJsonMcpStanza uses executable command plus JS shim args", () => {
  const stanza = buildJsonMcpStanza({
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
    platform: "win32",
    distro: "Ubuntu-24.04",
  });
  assert.deepEqual(stanza, {
    command: "C:/node/node.exe",
    args: ["C:/pkg/bin/terminal-commander-mcp.js"],
    env: { TC_WSL_DISTRO: "Ubuntu-24.04" },
  });
});
