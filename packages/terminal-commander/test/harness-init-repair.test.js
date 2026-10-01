// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { writeJsonMcpConfig } = require("../lib/harness/io/json_mcp.js");
const { writeCodexTomlConfig } = require("../lib/harness/io/toml_mcp.js");
const { writeProvider } = require("../lib/harness/write_all.js");
const { detectProvider } = require("../lib/harness/detect.js");
const { listProviders } = require("../lib/harness/registry.js");
const { entryConfigured } = require("../lib/harness/needs.js");

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tc-init-repair-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return { root, env: { HOME: root, USERPROFILE: root, APPDATA: path.join(root, "roaming") } };
}

test("Kilo JSONC refresh preserves policy, strings and other servers", (t) => {
  const { root, env } = fixture(t);
  const target = path.join(root, ".config/kilo/kilo.jsonc");
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `{
    // user settings
    "theme": "https://example.invalid/a//b/*c*/",
    "mcp": {
      "terminal_commander": {"type":"local", "command":["wrapper","old"], "enabled":false,
        "timeout":90000, "environment":{"CUSTOM_SETTING":"fixture", "TC_SURFACE":"compact"},},
      "symforge": {"type":"local", "command":["/stable/symforge"],},
    },
  }`);
  const options = { platform: "linux", env, exePath: "/stable path/terminal-commander-mcp", machineKey: "fixture", force: true };
  assert.equal(writeProvider("kilo-code", options).status, "ok");
  const data = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.equal(data.theme, "https://example.invalid/a//b/*c*/");
  assert.deepEqual(data.mcp.symforge, { type: "local", command: ["/stable/symforge"] });
  assert.equal(data.mcp.terminal_commander.enabled, false);
  assert.equal(data.mcp.terminal_commander.timeout, 90000);
  assert.equal(data.mcp.terminal_commander.environment.CUSTOM_SETTING, "fixture");
  assert.equal(data.mcp.terminal_commander.environment.TC_SURFACE, "compact");
  assert.deepEqual(data.mcp.terminal_commander.command, [options.exePath]);
  assert.equal(entryConfigured("kilo-code", options), true);
  const before = fs.readFileSync(target, "utf8");
  writeProvider("kilo-code", options);
  assert.equal(fs.readFileSync(target, "utf8"), before);
});

test("Cursor refresh leaves an unrelated remote MCP intact", (t) => {
  const { root, env } = fixture(t);
  const target = path.join(root, ".cursor/mcp.json");
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const remote = { url: "https://example.invalid/mcp", type: "http" };
  fs.writeFileSync(target, JSON.stringify({ mcpServers: { remote } }));
  assert.equal(writeProvider("cursor", { env, platform: "linux", exePath: "/stable/mcp", force: true }).status, "ok");
  assert.deepEqual(JSON.parse(fs.readFileSync(target, "utf8")).mcpServers.remote, remote);
});

test("explicit first-time setup creates config without harness markers", (t) => {
  const { root, env } = fixture(t);
  const options = { env, platform: "linux", providerFilter: "cursor", exePath: "/stable/mcp", force: true };
  assert.equal(writeProvider("cursor", options).status, "ok");
  assert.ok(fs.existsSync(path.join(root, ".cursor/mcp.json")));
  assert.equal(writeProvider("gemini", { ...options, providerFilter: "gemini", scope: "project", projectRoot: root }).status, "ok");
  assert.ok(fs.existsSync(path.join(root, ".gemini/settings.json")));
  assert.equal(writeProvider("claude-desktop", { ...options, providerFilter: "claude-desktop", scope: "project", projectRoot: root }).status, "skipped");
});

test("automatic OMP refresh preserves disabled lists; explicit setup activates", (t) => {
  const { root, env } = fixture(t);
  const target = path.join(root, ".omp/agent/mcp.json");
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, JSON.stringify({ mcpServers: { "terminal-commander": { command: "old", args: [] } },
    enabledServers: ["symforge"], disabledServers: ["terminal-commander"] }));
  const options = { env, platform: "linux", exePath: "/stable/mcp", force: true, activate: false };
  assert.equal(writeProvider("omp", options).status, "ok");
  let data = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.deepEqual(data.enabledServers, ["symforge"]);
  assert.deepEqual(data.disabledServers, ["terminal-commander"]);
  writeProvider("omp", { ...options, activate: true });
  data = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.deepEqual(data.enabledServers, ["symforge", "terminal-commander"]);
  assert.deepEqual(data.disabledServers, []);
});

test("OMP alias migration keeps the renamed server disabled during auto refresh", (t) => {
  const { root, env } = fixture(t);
  const target = path.join(root, ".omp/agent/mcp.json");
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, JSON.stringify({ mcpServers: { legacy: { command: "/old/terminal-commander-mcp", args: [] } },
    enabledServers: ["symforge"], disabledServers: ["legacy"] }));
  writeProvider("omp", { env, platform: "linux", exePath: "/stable/mcp", force: true, activate: false });
  const data = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.deepEqual(data.disabledServers, ["terminal-commander"]);
  assert.deepEqual(data.enabledServers, ["symforge"]);
  assert.equal(data.mcpServers.legacy, undefined);
  assert.deepEqual(data.mcpServers["terminal-commander"].args, []);
});

test("malformed JSONC is refused without backup or data loss", (t) => {
  const { root, env } = fixture(t);
  const target = path.join(root, ".config/kilo/kilo.jsonc");
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const before = '{"mcp": {} /* unclosed';
  fs.writeFileSync(target, before);
  assert.equal(writeProvider("kilo-code", { env, platform: "linux", exePath: "/stable/mcp", force: true }).harness_status, "invalid_json");
  assert.equal(fs.readFileSync(target, "utf8"), before);
  assert.deepEqual(fs.readdirSync(path.dirname(target)), ["kilo.jsonc"]);
});

test("bootstrap launch resolution failure never writes a transient shim registration", (t) => {
  const { root, env } = fixture(t);
  const result = writeProvider("cursor", { env, platform: "linux", providerFilter: "cursor", force: true,
    launchFailureReason: "transient_path" });
  assert.equal(result.status, "failed");
  assert.equal(result.harness_status, "binary_unavailable");
  assert.equal(fs.existsSync(path.join(root, ".cursor/mcp.json")), false);
});

test("JSON refresh migrates the launch pair while preserving policy and custom env", (t) => {
  const { root } = fixture(t);
  const target = path.join(root, "mcp.json");
  fs.writeFileSync(target, JSON.stringify({ mcpServers: {
    terminal_commander: {
      command: "wrapper", args: ["old-terminal-commander-mcp"],
      disabled: true, timeout: 90000, alwaysAllow: ["health"],
      env: { CUSTOM_SETTING: "fixture", TC_SURFACE: "compact", TC_WSL_DISTRO: "OldDistro" },
    },
    symforge: { command: "/stable/symforge", args: [] },
  }}));
  const result = writeJsonMcpConfig({ path: target, serverName: "terminal_commander", force: true,
    serverConfig: { command: "/stable/terminal-commander-mcp", args: [], env: { TC_SESSION: "fixture-session" } } });
  assert.equal(result.status, "config_updated");
  const config = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.deepEqual(config.mcpServers.terminal_commander, {
    command: "/stable/terminal-commander-mcp", args: [], disabled: true, timeout: 90000,
    alwaysAllow: ["health"], env: { CUSTOM_SETTING: "fixture", TC_SURFACE: "compact", TC_SESSION: "fixture-session" },
  });
  assert.deepEqual(config.mcpServers.symforge, { command: "/stable/symforge", args: [] });
});

test("registration never replaces a configured remote transport with a native launch", (t) => {
  const { root } = fixture(t);
  const target = path.join(root, "mcp.json");
  const before = JSON.stringify({ mcpServers: { terminal_commander: { url: "https://example.invalid/mcp" } } });
  fs.writeFileSync(target, before);
  const result = writeJsonMcpConfig({ path: target, serverName: "terminal_commander", force: true,
    serverConfig: { command: "/stable/terminal-commander-mcp", args: [] } });
  assert.equal(result.status, "unsupported");
  assert.equal(fs.readFileSync(target, "utf8"), before);
});

test("Codex refresh preserves main policy, env, tool subtables and multiline args", (t) => {
  const { root } = fixture(t);
  const target = path.join(root, "config.toml");
  fs.writeFileSync(target, `[mcp_servers.terminal_commander]
command = 'old-wrapper'
args = [
  'old-terminal-commander-mcp',
]
required = false
enabled = false
startup_timeout_sec = 90
tool_timeout_sec = 900
enabled_tools = ['health']

[mcp_servers.terminal_commander.env]
CUSTOM_SETTING = 'fixture'
TC_SURFACE = 'compact'
TC_WSL_DISTRO = 'OldDistro'

[mcp_servers.terminal_commander.tools.health]
approval_mode = 'auto'

[mcp_servers.symforge]
command = '/stable/symforge'
`);
  const opts = { path: target, exePath: "/stable/terminal-commander-mcp", platform: "linux", force: true,
    sessionToken: "fixture-session" };
  assert.equal(writeCodexTomlConfig(opts).status, "config_updated");
  const after = fs.readFileSync(target, "utf8");
  for (const line of ["required = false", "enabled = false", "startup_timeout_sec = 90", "tool_timeout_sec = 900",
    "enabled_tools = ['health']", "CUSTOM_SETTING = 'fixture'", "TC_SURFACE = 'compact'", "approval_mode = 'auto'"]) {
    assert.ok(after.includes(line), `missing preserved field: ${line}`);
  }
  assert.match(after, /command = "\/stable\/terminal-commander-mcp"/);
  assert.match(after, /args = \[\]/);
  assert.doesNotMatch(after, /old-wrapper|old-terminal-commander-mcp|TC_WSL_DISTRO/);
  assert.match(after, /\[mcp_servers.symforge\]/);
  writeCodexTomlConfig(opts);
  assert.equal(fs.readFileSync(target, "utf8"), after);
});

test("fresh Codex registrations wait for their managed MCP to become ready", (t) => {
  const { root } = fixture(t);
  const target = path.join(root, "config.toml");
  writeCodexTomlConfig({ path: target, exePath: "/stable/terminal-commander-mcp", sessionToken: "fixture-session" });
  const config = fs.readFileSync(target, "utf8");
  assert.match(config, /^required = true$/m);
  assert.match(config, /^startup_timeout_sec = 60$/m);
});

test("headless Linux Cursor/Grok box writes global config, keeps SymForge and user policy", (t) => {
  const { root, env } = fixture(t);
  const target = path.join(root, ".cursor", "mcp.json");
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, JSON.stringify({ mcpServers: {
    symforge: { command: "/stable/symforge", args: [], env: { SYMFORGE_SURFACE: "full" } },
    "terminal-commander": { command: "wrapper", args: ["old-terminal-commander-mcp"], disabled: true,
      env: { CUSTOM_SETTING: "fixture", TC_SURFACE: "compact" } },
  }}));
  const options = { platform: "linux", env, exePath: "/stable/terminal-commander-mcp", machineKey: "fixture", force: true };
  assert.equal(writeProvider("cursor", options).status, "ok");
  const config = JSON.parse(fs.readFileSync(target, "utf8"));
  const entry = config.mcpServers["terminal-commander"];
  assert.equal(entry.command, options.exePath);
  assert.deepEqual(entry.args, []);
  assert.equal(entry.disabled, true);
  assert.equal(entry.env.CUSTOM_SETTING, "fixture");
  assert.equal(entry.env.TC_SURFACE, "compact");
  assert.equal(entry.cwd, undefined);
  assert.deepEqual(config.mcpServers.symforge, { command: "/stable/symforge", args: [], env: { SYMFORGE_SURFACE: "full" } });
  const before = fs.readFileSync(target, "utf8");
  writeProvider("cursor", options);
  assert.equal(fs.readFileSync(target, "utf8"), before);
  assert.ok(fs.readdirSync(path.dirname(target)).some((name) => name.endsWith(".bak")));
});

test("every SymForge init target plus OMP has a functional provider", () => {
  const providers = listProviders({ includeStubs: false }).map((provider) => provider.id);
  for (const id of ["claude-code", "claude-desktop", "codex-cli", "cursor", "gemini", "grok", "kilo-code", "omp"]) {
    assert.ok(providers.includes(id), `${id} must have a functional writer`);
  }
});

for (const [id, location, format] of [
  ["gemini", ".gemini/settings.json", "json"],
  ["grok", ".grok/config.toml", "toml"],
  ["kilo-code", ".config/kilo/kilo.json", "kilo"],
]) {
  test(`${id} detects and registers in an isolated Linux home`, (t) => {
    const { root, env } = fixture(t);
    const target = path.join(root, location);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, format === "toml" ? "# fixture\n" : "{}");
    const options = { platform: "linux", env, machineKey: "fixture", exePath: "/stable/terminal-commander-mcp", force: true };
    assert.equal(detectProvider(id, options).detected, true);
    assert.equal(writeProvider(id, options).status, "ok");
    const content = fs.readFileSync(target, "utf8");
    if (format === "toml") {
      assert.match(content, /\[mcp_servers.terminal_commander\]/);
      assert.match(content, /command = "\/stable\/terminal-commander-mcp"/);
      assert.doesNotMatch(content, /CODEX_MCP_PROTOCOL_VERSION/);
    } else {
      const data = JSON.parse(content);
      const entry = format === "kilo" ? data.mcp.terminal_commander : data.mcpServers.terminal_commander;
      assert.deepEqual(format === "kilo" ? entry.command : [entry.command, ...entry.args], [options.exePath]);
      assert.ok((entry.environment || entry.env).TC_SESSION);
    }
  });
}
