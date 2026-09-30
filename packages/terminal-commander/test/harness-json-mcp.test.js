// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// JSON mcpServers writer (shared by Claude Code / Desktop / Gemini / Kilo /
// Cursor): single-key overwrite under force (idempotent refresh), malformed-safe
// (never clobbers unparseable JSON), BOM-strip on read, timestamped backup.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  writeJsonMcpConfig,
  parseJsonMcp,
  mergeJsonMcpServers,
} = require("../lib/harness/io/json_mcp.js");

function tmpCfg(name) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tc-json-"));
  return path.join(dir, name || "claude.json");
}

const STANZA = Object.freeze({
  command:
    "C:\\Users\\op\\AppData\\Local\\terminal-commander\\bin\\terminal-commander-mcp.exe",
  args: [],
});

test("writeJsonMcpConfig creates a fresh config with the terminal_commander entry", () => {
  const target = tmpCfg();
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
  });
  assert.equal(r.status, "config_created");
  const data = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.deepEqual(data.mcpServers.terminal_commander, STANZA);
});

test("Fix 1: force overwrites the single named key, preserves all other servers (idempotent)", () => {
  const target = tmpCfg();
  fs.writeFileSync(
    target,
    JSON.stringify({
      mcpServers: {
        terminal_commander: { command: "stale", args: [] },
        "other-server": { command: "keep-me", args: ["--x"] },
      },
      otherTopLevel: { user: "data" },
    }),
  );
  const r1 = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
    force: true,
  });
  assert.equal(r1.status, "config_updated");
  const data = JSON.parse(fs.readFileSync(target, "utf8"));
  // Our key refreshed.
  assert.deepEqual(data.mcpServers.terminal_commander, STANZA);
  // Other server + other top-level keys untouched.
  assert.deepEqual(data.mcpServers["other-server"], { command: "keep-me", args: ["--x"] });
  assert.deepEqual(data.otherTopLevel, { user: "data" });
  // Exactly one terminal_commander key (no duplicate).
  assert.equal(
    Object.keys(data.mcpServers).filter((k) => k === "terminal_commander").length,
    1,
  );
  // Re-run is byte-stable except for a fresh timestamped backup -> idempotent content.
  const before = fs.readFileSync(target, "utf8");
  writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
    force: true,
  });
  assert.equal(fs.readFileSync(target, "utf8"), before, "second force-refresh must be content-stable");
});

test("without force, an existing entry is left as already_exists (not clobbered)", () => {
  const target = tmpCfg();
  fs.writeFileSync(
    target,
    JSON.stringify({ mcpServers: { terminal_commander: { command: "stale", args: [] } } }),
  );
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
  });
  assert.equal(r.status, "already_exists");
  const data = JSON.parse(fs.readFileSync(target, "utf8"));
  assert.equal(data.mcpServers.terminal_commander.command, "stale");
});

test("Fix 3: malformed JSON is reported and the file is NEVER overwritten", () => {
  const target = tmpCfg();
  const bad = "{ this is : not json";
  fs.writeFileSync(target, bad);
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
    force: true,
  });
  assert.equal(r.status, "invalid_json");
  assert.equal(fs.readFileSync(target, "utf8"), bad, "malformed file must be left untouched");
  const baks = fs.readdirSync(path.dirname(target)).filter((n) => n.endsWith(".bak"));
  assert.equal(baks.length, 0, "a refused write must not create a backup");
});

test("OMP setup refuses a malformed enabledServers list without touching the config", () => {
  const target = tmpCfg("omp.json");
  const prior = JSON.stringify({ mcpServers: {}, enabledServers: "wrong-type" });
  fs.writeFileSync(target, prior);
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal-commander",
    serverConfig: STANZA,
    enableServer: true,
  });
  assert.equal(r.status, "invalid_json");
  assert.equal(fs.readFileSync(target, "utf8"), prior);
});

test("Fix 3: a leading UTF-8 BOM is stripped on read (parses + merges cleanly)", () => {
  const target = tmpCfg();
  fs.writeFileSync(
    target,
    "﻿" + JSON.stringify({ mcpServers: { "other-server": { command: "x", args: [] } } }),
  );
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
    force: true,
  });
  assert.equal(r.status, "config_updated", "BOM must not break the JSON parse");
  const text = fs.readFileSync(target, "utf8");
  assert.equal(text.charCodeAt(0) === 0xfeff, false, "rewritten config must not carry the BOM");
  const data = JSON.parse(text);
  assert.deepEqual(data.mcpServers.terminal_commander, STANZA);
  assert.deepEqual(data.mcpServers["other-server"], { command: "x", args: [] });
});

test("Fix 3: overwriting an existing config writes a timestamped .bak of the prior bytes", () => {
  const target = tmpCfg();
  const prior = JSON.stringify({ mcpServers: { terminal_commander: { command: "old", args: [] } } });
  fs.writeFileSync(target, prior);
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
    force: true,
  });
  assert.equal(r.status, "config_updated");
  const baks = fs.readdirSync(path.dirname(target)).filter((n) => /\.\d{8}T\d{9}Z\.bak$/.test(n));
  assert.equal(baks.length, 1, "exactly one timestamped backup");
  assert.equal(fs.readFileSync(path.join(path.dirname(target), baks[0]), "utf8"), prior);
});

// --- pure-helper coverage for BOM + single-key merge ---

test("parseJsonMcp strips a BOM before JSON.parse", () => {
  const r = parseJsonMcp(Buffer.from("﻿{\"mcpServers\":{}}", "utf8"));
  assert.equal(r.ok, true);
  assert.deepEqual(r.value.mcpServers, {});
});

test("mergeJsonMcpServers replaces only the named key under force", () => {
  const existing = { mcpServers: { terminal_commander: { command: "old" }, keep: { command: "k" } } };
  const r = mergeJsonMcpServers(existing, "terminal_commander", STANZA, { force: true });
  assert.equal(r.ok, true);
  assert.deepEqual(r.value.mcpServers.terminal_commander, STANZA);
  assert.deepEqual(r.value.mcpServers.keep, { command: "k" });
});

// One Terminal Commander server per config: an entry that already launches the
// adapter under ANOTHER name (hand-added, older installer) must not survive
// next to the canonical one, or the harness shows two TC servers and every
// session runs two adapters and two daemons.
const HAND_ENTRY = Object.freeze({
  command: "C:\\nvm4w\\nodejs\\node.exe",
  args: ["C:\\Users\\op\\.npm-global\\node_modules\\terminal-commander\\bin\\terminal-commander-mcp.js"],
  env: { TC_SESSION: "tc-claude-code", TC_SURFACE: "full" },
});

test("force replaces a Terminal Commander entry registered under another name instead of adding a second", () => {
  const target = tmpCfg();
  fs.writeFileSync(
    target,
    JSON.stringify({
      mcpServers: {
        "terminal-commander": HAND_ENTRY,
        "terminal-notes": { command: "notes-server", args: [] },
      },
    }),
  );
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
    force: true,
  });
  assert.equal(r.status, "config_updated");
  const servers = JSON.parse(fs.readFileSync(target, "utf8")).mcpServers;
  assert.deepEqual(Object.keys(servers).sort(), ["terminal-notes", "terminal_commander"]);
  assert.deepEqual(servers.terminal_commander, STANZA);
  assert.match(r.hint, /terminal-commander/, "the hint names the entry it replaced");
});

test("without force, an existing Terminal Commander entry under another name blocks a duplicate", () => {
  const target = tmpCfg();
  const before = JSON.stringify({ mcpServers: { "terminal-commander": HAND_ENTRY } });
  fs.writeFileSync(target, before);
  const r = writeJsonMcpConfig({
    path: target,
    serverName: "terminal_commander",
    serverConfig: STANZA,
  });
  assert.equal(r.status, "already_exists");
  assert.match(r.hint, /terminal-commander/);
  assert.equal(fs.readFileSync(target, "utf8"), before, "config is left untouched");
});

test("merge keeps servers that do not launch the Terminal Commander adapter", () => {
  const existing = {
    mcpServers: {
      "terminal-notes": { command: "notes-server", args: ["--terminal-commander-style"] },
      symforge: { command: "symforge", args: [] },
    },
  };
  const m = mergeJsonMcpServers(existing, "terminal_commander", STANZA, { force: true });
  assert.equal(m.ok, true);
  assert.deepEqual(Object.keys(m.value.mcpServers).sort(), ["symforge", "terminal-notes", "terminal_commander"]);
});
