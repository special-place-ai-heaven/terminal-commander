// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Part A: harness configs launch the native exe DIRECTLY at a stable per-user
// path (command: <stable>\terminal-commander-mcp.exe, args: []) instead of the
// bare npm name, removing the npm script-launcher -> node -> JS-shim chain that
// heuristic AV reads as a loader. Dry-run is used so no filesystem is touched.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  buildTerminalCommanderCommandConfig,
  buildTerminalCommanderServerConfig,
} = require("../lib/cursor/config.js");
const { buildCodexTomlBlock } = require("../lib/harness/io/toml_mcp.js");
const { writeProvider, buildJsonMcpStanza } = require("../lib/harness/write_all.js");

const STABLE_EXE =
  "C:\\Users\\op\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe";

test("buildTerminalCommanderCommandConfig emits direct-exe command with empty args", () => {
  const c = buildTerminalCommanderCommandConfig({ exePath: STABLE_EXE });
  assert.equal(c.command, STABLE_EXE);
  assert.deepEqual(c.args, []);
});

test("exePath takes precedence over nodePath/scriptPath", () => {
  const c = buildTerminalCommanderCommandConfig({
    exePath: STABLE_EXE,
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
  });
  assert.equal(c.command, STABLE_EXE);
  assert.deepEqual(c.args, []);
});

test("without exePath the default stays the portable bare-name command (fallback)", () => {
  const c = buildTerminalCommanderCommandConfig({});
  assert.equal(c.command, "terminal-commander-mcp");
  assert.deepEqual(c.args, []);
});

test("Cursor stanza points command at the stable exe and keeps args empty", () => {
  const s = buildTerminalCommanderServerConfig({ exePath: STABLE_EXE });
  assert.equal(s.type, "stdio");
  assert.equal(s.command, STABLE_EXE);
  assert.deepEqual(s.args, []);
});

test("Codex TOML block points command at the stable exe", () => {
  const block = buildCodexTomlBlock({ exePath: STABLE_EXE });
  assert.match(block, /\[mcp_servers\.terminal_commander\]/);
  assert.match(block, /command = "C:\\\\Users\\\\op\\\\AppData\\\\Local\\\\terminal-commander\\\\bin\\\\terminal-commander-mcp\.exe"/);
  assert.match(block, /args = \[\]/);
});

test("Claude JSON stanza points command at the stable exe", () => {
  const stanza = buildJsonMcpStanza({ exePath: STABLE_EXE });
  assert.equal(stanza.command, STABLE_EXE);
  assert.deepEqual(stanza.args, []);
});

test("writeProvider cursor dry-run stanza carries the direct-exe command", () => {
  const r = writeProvider("cursor", {
    dry_run: true,
    detection: { detected: true },
    machineKey: "test-machine",
    exePath: STABLE_EXE,
  });
  assert.equal(r.status, "ok");
  assert.equal(r.stanza.command, STABLE_EXE);
  assert.deepEqual(r.stanza.args, []);
  // Session isolation is preserved alongside the direct-exe command.
  assert.ok(r.stanza.env && r.stanza.env.TC_SESSION);
});

test("writeProvider claude-code dry-run stanza carries the direct-exe command", () => {
  const r = writeProvider("claude-code", {
    dry_run: true,
    detection: { detected: true },
    machineKey: "test-machine",
    exePath: STABLE_EXE,
  });
  assert.equal(r.status, "ok");
  assert.equal(r.stanza.command, STABLE_EXE);
  assert.deepEqual(r.stanza.args, []);
});

test("the direct-exe stanza embeds no bare npm name and no JS-shim node hop", () => {
  const s = buildTerminalCommanderServerConfig({ exePath: STABLE_EXE });
  const json = JSON.stringify(s);
  // The command IS the exe; there must be no node.exe + .js shim form.
  assert.equal(/terminal-commander-mcp\.js/.test(json), false);
  assert.equal(/"command":"terminal-commander-mcp"/.test(json), false);
  assert.equal(/node\.exe/.test(json), false);
});
