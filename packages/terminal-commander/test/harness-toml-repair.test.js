// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { writeCodexTomlConfig } = require("../lib/harness/io/toml_mcp.js");

function config(t, content) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tc-toml-repair-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const target = path.join(dir, "config.toml");
  fs.writeFileSync(target, content);
  return target;
}

test("quoted TOML tables and multiline strings preserve unrelated policy", (t) => {
  const target = config(t, `[[agents]]
name = 'fixture'
[mcp_servers."terminal_commander"] # managed
command = 'wrapper'
args = ['old']
instructions = '''
[not_a_table]
command = 'preserve'
'''
enabled = false
[mcp_servers.'terminal_commander'.env] # environment
"CUSTOM" = 'fixture'
TC_SURFACE = 'compact'
`);
  const opts = { path: target, exePath: "/stable/tc", force: true, sessionToken: "fixture-session", providerId: "grok" };
  assert.equal(writeCodexTomlConfig(opts).status, "config_updated");
  const result = fs.readFileSync(target, "utf8");
  assert.match(result, /command = "\/stable\/tc"\nargs = \[\]/);
  assert.ok(result.includes("[not_a_table]\ncommand = 'preserve'"));
  assert.ok(result.includes('"CUSTOM" = \'fixture\''));
  assert.match(result, /enabled = false/);
  assert.match(result, /TC_SURFACE = 'compact'/);
  assert.doesNotMatch(result, /CODEX_MCP_PROTOCOL_VERSION|required = true|startup_timeout_sec/);
  writeCodexTomlConfig(opts);
  assert.equal(fs.readFileSync(target, "utf8"), result);
});

for (const content of [
  '[mcp_servers.terminal_commander]\nurl = "https://example.invalid/mcp"\n',
  '[mcp_servers.terminal_commander]\ntype = "http"\n',
  '[mcp_servers.terminal_commander]\nenv = { CUSTOM = "fixture" }\n',
  'mcp_servers.terminal_commander.command = "wrapper"\n',
  'mcp_servers."terminal_commander".command = "wrapper"\n',
  'mcp_servers = { terminal_commander = { command = "wrapper" } }\n',
  '[mcp_servers]\nterminal_commander = { command = "wrapper" }\n',
  '[[mcp_servers.terminal_commander]]\ncommand = "wrapper"\n',
]) {
  test(`unsupported TOML layout stays byte-identical (${content.split("\n")[0]})`, (t) => {
    const target = config(t, content);
    const result = writeCodexTomlConfig({ path: target, exePath: "/stable/tc", force: true });
    assert.equal(result.status, "unsupported");
    assert.equal(fs.readFileSync(target, "utf8"), content);
    assert.deepEqual(fs.readdirSync(path.dirname(target)), ["config.toml"]);
  });
}

test("missing launch keys at EOF gain a separate statement", (t) => {
  const target = config(t, "[mcp_servers.terminal_commander]\nenabled = false");
  const opts = { path: target, exePath: "/stable/tc", force: true, codexDefaults: false };
  assert.equal(writeCodexTomlConfig(opts).status, "config_updated");
  const result = fs.readFileSync(target, "utf8");
  assert.match(result, /enabled = false\ncommand = "\/stable\/tc"\nargs = \[\]\n/);
  writeCodexTomlConfig(opts);
  assert.equal(fs.readFileSync(target, "utf8"), result);
});

test("existing Codex registrations gain missing readiness defaults and repeated refresh is a no-op", (t) => {
  const target = config(t, "[mcp_servers.terminal_commander]\ncommand = 'old'\nargs = []\n");
  const opts = { path: target, exePath: "/stable/tc", force: true };
  assert.equal(writeCodexTomlConfig(opts).status, "config_updated");
  const content = fs.readFileSync(target, "utf8");
  assert.match(content, /required = true/);
  assert.match(content, /startup_timeout_sec = 60/);
  const files = fs.readdirSync(path.dirname(target));
  assert.equal(writeCodexTomlConfig(opts).status, "already_exists");
  assert.equal(fs.readFileSync(target, "utf8"), content);
  assert.deepEqual(fs.readdirSync(path.dirname(target)), files);
});
