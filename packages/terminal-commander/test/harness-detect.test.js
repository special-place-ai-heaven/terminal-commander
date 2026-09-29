// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { detectCodex, detectCursor, detectOmp } = require("../lib/harness/detect.js");
const { buildJsonMcpStanza, writeProvider } = require("../lib/harness/write_all.js");
const { entryConfigured } = require("../lib/harness/needs.js");
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

test("OMP setup refreshes its server and enables it without changing other entries", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-omp-"));
  const configPath = path.join(root, ".omp", "agent", "mcp.json");
  fs.mkdirSync(path.dirname(configPath), { recursive: true });
  fs.writeFileSync(configPath, JSON.stringify({
    mcpServers: { "terminal-commander": { command: "old" }, keep: { command: "keep" } },
    enabledServers: ["keep"],
    disabledServers: ["terminal-commander"],
    otherSetting: true,
  }));
  const env = { ...process.env, HOME: root, USERPROFILE: root };
  assert.equal(detectOmp({ platform: process.platform, env }).detected, true);
  const result = writeProvider("omp", {
    detection: detectOmp({ platform: process.platform, env }),
    exePath: "C:/tc/terminal-commander-mcp.exe",
    machineKey: "test-machine",
    force: true,
  });
  assert.equal(result.status, "ok");
  const data = JSON.parse(fs.readFileSync(configPath, "utf8"));
  assert.equal(data.mcpServers["terminal-commander"].command, "C:/tc/terminal-commander-mcp.exe");
  assert.deepEqual(data.mcpServers["terminal-commander"].args, []);
  assert.ok(data.mcpServers["terminal-commander"].env.TC_SESSION);
  assert.deepEqual(data.enabledServers, ["keep", "terminal-commander"]);
  assert.deepEqual(data.disabledServers, []);
  assert.deepEqual(data.mcpServers.keep, { command: "keep" });
  assert.equal(data.otherSetting, true);
  assert.equal(entryConfigured("omp", { platform: process.platform, env }), true);
  data.disabledServers.push("terminal-commander");
  fs.writeFileSync(configPath, JSON.stringify(data));
  assert.equal(entryConfigured("omp", { platform: process.platform, env }), false);
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
