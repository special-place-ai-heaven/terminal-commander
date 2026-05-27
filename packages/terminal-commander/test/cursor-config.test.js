// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS05 Cursor config pure-helper tests. No file I/O.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");

const {
  getCursorGlobalConfigPath,
  getCursorProjectConfigPath,
  buildTerminalCommanderCommandConfig,
  buildTerminalCommanderServerConfig,
  parseExistingCursorConfig,
  validateCursorConfigShape,
  mergeCursorMcpConfig,
  serializeCursorMcpConfig,
  isPathInsideScope,
  SERVER_NAME,
  SERVER_COMMAND,
  SERVER_TYPE,
  CONFIG_FILENAME,
  CONFIG_DIRNAME,
  MAX_CONFIG_BYTES,
  CONFIG_STATUSES,
  UNSAFE_DISTRO_NAME,
} = require("../lib/cursor/config.js");

test("CONFIG_STATUSES enum covers the locked 12 statuses", () => {
  assert.deepEqual(
    new Set(Object.values(CONFIG_STATUSES)),
    new Set([
      "config_created",
      "config_updated",
      "already_exists",
      "invalid_json",
      "config_too_large",
      "path_not_allowed",
      "project_root_required",
      "unsafe_distro_name",
      "distro_not_found",
      "backup_failed",
      "write_failed",
      "unsupported_host",
    ]),
  );
});

test("SERVER_NAME / SERVER_COMMAND / SERVER_TYPE are locked", () => {
  assert.equal(SERVER_NAME, "terminal-commander");
  assert.equal(SERVER_COMMAND, "terminal-commander-mcp");
  assert.equal(SERVER_TYPE, "stdio");
});

test("MAX_CONFIG_BYTES is 256 KiB", () => {
  assert.equal(MAX_CONFIG_BYTES, 256 * 1024);
});

test("CONFIG_FILENAME + CONFIG_DIRNAME match Cursor conventions", () => {
  assert.equal(CONFIG_FILENAME, "mcp.json");
  assert.equal(CONFIG_DIRNAME, ".cursor");
});

test("getCursorGlobalConfigPath derives Linux path from HOME", () => {
  const p = getCursorGlobalConfigPath({
    platform: "linux",
    env: { HOME: "/home/op" },
  });
  assert.equal(p, path.join("/home/op", ".cursor", "mcp.json"));
});

test("getCursorGlobalConfigPath derives Windows path from USERPROFILE", () => {
  const p = getCursorGlobalConfigPath({
    platform: "win32",
    env: { USERPROFILE: "C:\\Users\\op" },
  });
  assert.equal(p, path.join("C:\\Users\\op", ".cursor", "mcp.json"));
});

test("getCursorGlobalConfigPath throws when env vars missing", () => {
  assert.throws(() =>
    getCursorGlobalConfigPath({ platform: "linux", env: {} }),
  );
  assert.throws(() =>
    getCursorGlobalConfigPath({ platform: "win32", env: {} }),
  );
});

test("getCursorProjectConfigPath requires explicit projectRoot", () => {
  assert.throws(() => getCursorProjectConfigPath(undefined), (err) => {
    assert.equal(err.code, "PROJECT_ROOT_REQUIRED");
    return true;
  });
  assert.throws(() => getCursorProjectConfigPath(""), (err) => {
    assert.equal(err.code, "PROJECT_ROOT_REQUIRED");
    return true;
  });
  assert.throws(() => getCursorProjectConfigPath(null));
  const p = getCursorProjectConfigPath("/repo/x");
  assert.equal(p, path.join("/repo/x", ".cursor", "mcp.json"));
});

test("buildTerminalCommanderServerConfig default stanza (Linux / native)", () => {
  const s = buildTerminalCommanderServerConfig({
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
  });
  assert.deepEqual(s, {
    type: "stdio",
    command: "C:/node/node.exe",
    args: ["C:/pkg/bin/terminal-commander-mcp.js"],
  });
  assert.equal("env" in s, false);
});

test("buildTerminalCommanderServerConfig uses executable command plus JS shim args", () => {
  const s = buildTerminalCommanderServerConfig({
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
  });
  assert.equal(s.command, "C:/node/node.exe");
  assert.deepEqual(s.args, ["C:/pkg/bin/terminal-commander-mcp.js"]);
  assert.equal(s.type, "stdio");
});

test("buildTerminalCommanderServerConfig with safe distro adds env.TC_WSL_DISTRO only", () => {
  const s = buildTerminalCommanderServerConfig({
    distro: "Ubuntu-24.04",
    nodePath: "C:/node/node.exe",
    scriptPath: "C:/pkg/bin/terminal-commander-mcp.js",
  });
  assert.deepEqual(s, {
    type: "stdio",
    command: "C:/node/node.exe",
    args: ["C:/pkg/bin/terminal-commander-mcp.js"],
    env: { TC_WSL_DISTRO: "Ubuntu-24.04" },
  });
  assert.equal(Object.keys(s.env).length, 1, "env must contain exactly TC_WSL_DISTRO");
});

test("buildTerminalCommanderServerConfig with sessionToken adds env.TC_SESSION", () => {
  const s = buildTerminalCommanderServerConfig({ sessionToken: "tc-0a1b2c3d4e5f" });
  assert.equal(s.env.TC_SESSION, "tc-0a1b2c3d4e5f");
  assert.equal(Object.keys(s.env).length, 1, "env must contain exactly TC_SESSION");
});

test("buildTerminalCommanderServerConfig merges TC_SESSION and TC_WSL_DISTRO", () => {
  const s = buildTerminalCommanderServerConfig({
    sessionToken: "tc-0a1b2c3d4e5f",
    distro: "Ubuntu-24.04",
  });
  assert.deepEqual(s.env, {
    TC_SESSION: "tc-0a1b2c3d4e5f",
    TC_WSL_DISTRO: "Ubuntu-24.04",
  });
});

test("buildTerminalCommanderServerConfig emits no env when neither session nor distro set", () => {
  const s = buildTerminalCommanderServerConfig({});
  assert.equal(s.env, undefined, "no env key when nothing to set (backward-compat)");
});

test("buildTerminalCommanderServerConfig rejects a malformed sessionToken (no pipe-squat)", () => {
  assert.throws(
    () => buildTerminalCommanderServerConfig({ sessionToken: "../evil" }),
    /TC_SESSION|session token/i,
    "a malformed token must be refused, never written into a kernel-object name",
  );
});

test("buildTerminalCommanderCommandConfig defaults to this package's JS shim", () => {
  const s = buildTerminalCommanderCommandConfig({ nodePath: "C:/node/node.exe" });
  assert.equal(s.command, "C:/node/node.exe");
  assert.equal(path.basename(s.args[0]), "terminal-commander-mcp.js");
  assert.match(s.args[0].replace(/\\/g, "/"), /\/bin\/terminal-commander-mcp\.js$/);
});

test("buildTerminalCommanderServerConfig rejects unsafe distro before emitting any stanza", () => {
  assert.throws(
    () => buildTerminalCommanderServerConfig({ distro: "Ubuntu; rm -rf /" }),
    (err) => {
      assert.equal(err.code, UNSAFE_DISTRO_NAME);
      return true;
    },
  );
});

test("buildTerminalCommanderServerConfig requireKnownDistro rejects unknown distros", () => {
  assert.throws(
    () =>
      buildTerminalCommanderServerConfig({
        distro: "Fedora",
        requireKnownDistro: true,
        knownDistros: [{ name: "Ubuntu" }, { name: "Debian" }],
      }),
    (err) => {
      assert.equal(err.code, "DISTRO_NOT_FOUND");
      return true;
    },
  );
});

test("buildTerminalCommanderServerConfig requireKnownDistro accepts known distros", () => {
  const s = buildTerminalCommanderServerConfig({
    distro: "Ubuntu",
    requireKnownDistro: true,
    knownDistros: [{ name: "Ubuntu" }, { name: "Debian" }],
  });
  assert.equal(s.env.TC_WSL_DISTRO, "Ubuntu");
});

test("parseExistingCursorConfig accepts empty / null buffer as new", () => {
  assert.deepEqual(parseExistingCursorConfig(null), {
    ok: true,
    value: { mcpServers: {} },
  });
  assert.deepEqual(parseExistingCursorConfig(Buffer.alloc(0)), {
    ok: true,
    value: { mcpServers: {} },
  });
  assert.deepEqual(parseExistingCursorConfig(""), {
    ok: true,
    value: { mcpServers: {} },
  });
});

test("parseExistingCursorConfig parses valid JSON + injects mcpServers when missing", () => {
  const r = parseExistingCursorConfig('{"other":1}');
  assert.equal(r.ok, true);
  assert.deepEqual(r.value, { other: 1, mcpServers: {} });
});

test("parseExistingCursorConfig rejects invalid JSON with invalid_json", () => {
  const r = parseExistingCursorConfig("{not json");
  assert.deepEqual(r, { ok: false, reason: "invalid_json" });
});

test("parseExistingCursorConfig rejects over-size buffer with config_too_large", () => {
  const huge = Buffer.alloc(MAX_CONFIG_BYTES + 1, "x");
  const r = parseExistingCursorConfig(huge);
  assert.deepEqual(r, { ok: false, reason: "config_too_large" });
});

test("parseExistingCursorConfig rejects array / non-object root with bad_shape", () => {
  assert.equal(parseExistingCursorConfig("[]").ok, false);
  assert.equal(parseExistingCursorConfig('"x"').ok, false);
});

test("parseExistingCursorConfig rejects non-object mcpServers", () => {
  assert.equal(parseExistingCursorConfig('{"mcpServers":[]}').ok, false);
  assert.equal(parseExistingCursorConfig('{"mcpServers":1}').ok, false);
});

test("validateCursorConfigShape accepts well-formed configs", () => {
  assert.equal(
    validateCursorConfigShape({
      mcpServers: {
        "terminal-commander": { command: "terminal-commander-mcp", type: "stdio" },
      },
    }),
    true,
  );
  assert.equal(validateCursorConfigShape({ mcpServers: {} }), true);
});

test("validateCursorConfigShape rejects malformed configs", () => {
  assert.equal(validateCursorConfigShape(null), false);
  assert.equal(validateCursorConfigShape([]), false);
  assert.equal(validateCursorConfigShape({}), false);
  assert.equal(validateCursorConfigShape({ mcpServers: [] }), false);
  assert.equal(
    validateCursorConfigShape({ mcpServers: { foo: { command: 1 } } }),
    false,
  );
  assert.equal(
    validateCursorConfigShape({ mcpServers: { foo: { command: "" } } }),
    false,
  );
});

test("mergeCursorMcpConfig refuses existing terminal-commander entry without force", () => {
  const existing = {
    mcpServers: {
      "terminal-commander": { type: "stdio", command: "terminal-commander-mcp" },
    },
  };
  const r = mergeCursorMcpConfig(existing, { type: "stdio", command: "terminal-commander-mcp" });
  assert.deepEqual(r, { ok: false, reason: "already_exists" });
});

test("mergeCursorMcpConfig overwrites with force:true and reports was_present:true", () => {
  const existing = {
    mcpServers: {
      "terminal-commander": { type: "stdio", command: "old" },
    },
  };
  const r = mergeCursorMcpConfig(
    existing,
    { type: "stdio", command: "terminal-commander-mcp" },
    { force: true },
  );
  assert.equal(r.ok, true);
  assert.equal(r.was_present, true);
  assert.equal(r.value.mcpServers["terminal-commander"].command, "terminal-commander-mcp");
});

test("mergeCursorMcpConfig preserves unrelated mcpServers entries", () => {
  const existing = {
    mcpServers: {
      "some-other-server": { type: "stdio", command: "other-cmd", env: { K: "v" } },
      "yet-another": { type: "stdio", command: "yet-another-cmd" },
    },
    otherTopLevelKey: { keep: true },
  };
  const r = mergeCursorMcpConfig(existing, {
    type: "stdio",
    command: "terminal-commander-mcp",
  });
  assert.equal(r.ok, true);
  assert.equal(r.was_present, false);
  assert.deepEqual(r.value.mcpServers["some-other-server"], existing.mcpServers["some-other-server"]);
  assert.deepEqual(r.value.mcpServers["yet-another"], existing.mcpServers["yet-another"]);
  assert.equal(r.value.mcpServers["terminal-commander"].command, "terminal-commander-mcp");
  assert.deepEqual(r.value.otherTopLevelKey, { keep: true });
  // Existing object NOT mutated.
  assert.equal("terminal-commander" in existing.mcpServers, false);
});

test("serializeCursorMcpConfig pretty-prints with trailing newline", () => {
  const s = serializeCursorMcpConfig({ mcpServers: {} });
  assert.equal(s.endsWith("\n"), true);
  assert.match(s, /\{\n  /);
});

test("isPathInsideScope is true for child paths and false for siblings/parents", () => {
  assert.equal(isPathInsideScope("/a/b", "/a/b"), true);
  assert.equal(isPathInsideScope("/a/b", "/a/b/c"), true);
  assert.equal(isPathInsideScope("/a/b", "/a/b/c/d"), true);
  assert.equal(isPathInsideScope("/a/b", "/a/c"), false);
  assert.equal(isPathInsideScope("/a/b", "/a"), false);
  assert.equal(isPathInsideScope("/a/b", "/a/bb"), false);
});

test("generated default stanza does NOT contain wsl.exe / wsl-direct args", () => {
  const s = buildTerminalCommanderServerConfig();
  const json = serializeCursorMcpConfig({ mcpServers: { "terminal-commander": s } });
  assert.equal(/wsl\.exe/i.test(json), false);
  assert.equal(/"command"\s*:\s*"wsl"/.test(json), false);
  assert.equal(/"-d"/.test(json), false);
  assert.equal(/bash\s+-lc/.test(json), false);
});

test("generated stanza contains no secret / token / private path text", () => {
  const s = buildTerminalCommanderServerConfig({ distro: "Ubuntu-24.04" });
  const json = serializeCursorMcpConfig({ mcpServers: { "terminal-commander": s } });
  for (const forbidden of [
    /NPM_TOKEN/i,
    /GITHUB_TOKEN/i,
    /OPENAI_API_KEY/i,
    /ANTHROPIC_API_KEY/i,
    /SLACK_TOKEN/i,
    /password/i,
    /credential/i,
    /sk-[A-Za-z0-9]/,
    /ghp_[A-Za-z0-9]/,
    /npm_[A-Za-z0-9]/,
    /USERPROFILE/i,
    /Users\\/i,
  ]) {
    assert.equal(forbidden.test(json), false, `generated stanza must not match ${forbidden}; got: ${json}`);
  }
});
