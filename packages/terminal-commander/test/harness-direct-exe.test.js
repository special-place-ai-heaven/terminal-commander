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

test("without exePath on non-Windows the fallback is the portable bare-name command", () => {
  // On Unix the MCP client's PATH + shell resolution finds the installed
  // `terminal-commander-mcp` shim, so a bare command is portable and acceptable.
  const c = buildTerminalCommanderCommandConfig({ platform: "linux" });
  assert.equal(c.command, "terminal-commander-mcp");
  assert.deepEqual(c.args, []);
});

test("without exePath on Windows the fallback must NOT be the bare command when a resolver yields an absolute exe", () => {
  // CONTRACT CHANGE (fix/harness-windows-shell-false): the previous test
  // asserted an UNCONDITIONAL bare-name fallback. That is ENOENT-fatal on
  // Windows when the MCP client spawns with shell:false (Node does no PATHEXT
  // resolution and cannot exec a .cmd/.ps1 shim => the server never starts =>
  // "0 tools"). When a caller injects a resolver that returns an absolute exe,
  // the win32 fallback MUST use it, never the bare name.
  const ABS = "C:\\Users\\op\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe";
  const c = buildTerminalCommanderCommandConfig({
    platform: "win32",
    resolveExePath: ({ platform }) => (platform === "win32" ? ABS : "/x/tc-mcp"),
  });
  assert.equal(c.command, ABS);
  assert.deepEqual(c.args, []);
  assert.notEqual(c.command, "terminal-commander-mcp");
});

test("win32 fallback: a resolver that throws or returns null degrades safely (last-resort bare, no crash)", () => {
  // Defense in depth: a resolver hiccup must never throw out of the pure builder.
  // When NO absolute path can be recovered on win32, the builder returns the bare
  // name as a caller-prevented last resort (the orchestrator's resolver chain is
  // expected to have produced o.exePath or warned). We assert it does not crash.
  const throwing = buildTerminalCommanderCommandConfig({
    platform: "win32",
    resolveExePath: () => {
      throw new Error("resolver boom");
    },
  });
  assert.equal(throwing.command, "terminal-commander-mcp");
  const nullish = buildTerminalCommanderCommandConfig({
    platform: "win32",
    resolveExePath: () => null,
  });
  assert.equal(nullish.command, "terminal-commander-mcp");
});

test("exePath still wins over an injected resolver (precedence preserved)", () => {
  const ABS_STABLE = "C:\\stable\\terminal-commander-mcp.exe";
  const c = buildTerminalCommanderCommandConfig({
    platform: "win32",
    exePath: ABS_STABLE,
    resolveExePath: () => "C:\\other\\terminal-commander-mcp.exe",
  });
  assert.equal(c.command, ABS_STABLE);
});

test("buildJsonMcpStanza on win32 without exePath recovers an absolute exe via the wired resolver (no bare ENOENT)", () => {
  // The JSON harness (Claude Code / Desktop) is the exact harness that shipped a
  // bare command and showed "0 tools". With the resolver wired in write_all.js,
  // a win32 stanza built without a pre-resolved exePath must not be the bare name.
  const ABS = "C:\\node_modules\\@tc\\windows-x64\\bin\\terminal-commander-mcp.exe";
  const stanza = buildJsonMcpStanza({
    platform: "win32",
    resolveExePath: ({ platform }) => (platform === "win32" ? ABS : null),
  });
  assert.equal(stanza.command, ABS);
  assert.notEqual(stanza.command, "terminal-commander-mcp");
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
