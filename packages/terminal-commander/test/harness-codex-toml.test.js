// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Codex TOML writer: per-server [.env] emission + force-rewrite idempotency.
// File-level behavior (real temp-file I/O), distinct from the dry-run stanza
// assertions in harness-write-session.test.js.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  writeCodexTomlConfig,
  isLikelyMalformedToml,
} = require("../lib/harness/io/toml_mcp.js");

function tmpCfg() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tc-codex-"));
  return path.join(dir, "config.toml");
}

test("codex writer emits [.env] with TC_SESSION + TC_SURFACE", () => {
  const target = tmpCfg();
  const r = writeCodexTomlConfig({
    path: target,
    sessionToken: "tc-aaa111",
    surface: "compact",
  });
  assert.equal(r.status, "config_created");
  const text = fs.readFileSync(target, "utf8");
  assert.match(text, /\[mcp_servers\.terminal_commander\.env\]/);
  assert.match(text, /TC_SESSION = "tc-aaa111"/);
  assert.match(text, /TC_SURFACE = "compact"/);
  assert.match(text, /CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"/);
});

test("codex force-rewrite replaces the env table cleanly (no orphaned/duplicate keys)", () => {
  const target = tmpCfg();
  writeCodexTomlConfig({ path: target, sessionToken: "tc-aaa111", surface: "compact" });
  const r = writeCodexTomlConfig({
    path: target,
    sessionToken: "tc-aaa111",
    surface: "full",
    force: true,
    clobber_backup: true,
  });
  assert.equal(r.status, "config_updated");
  const text = fs.readFileSync(target, "utf8");
  assert.equal(
    (text.match(/TC_SURFACE = /g) || []).length,
    1,
    `expected exactly one TC_SURFACE line:\n${text}`,
  );
  assert.equal(
    (text.match(/\[mcp_servers\.terminal_commander\.env\]/g) || []).length,
    1,
    "expected exactly one env sub-table header",
  );
  assert.match(text, /TC_SURFACE = "full"/);
  assert.equal((text.match(/CODEX_MCP_PROTOCOL_VERSION = /g) || []).length, 1);
  assert.equal(text.includes('"compact"'), false, "stale compact value must be gone");
});

test("codex force-rewrite is idempotent (no comment/section accumulation)", () => {
  const target = tmpCfg();
  const opts = {
    path: target,
    sessionToken: "tc-aaa111",
    surface: "compact",
    force: true,
    clobber_backup: true,
  };
  writeCodexTomlConfig({ ...opts, force: false }); // initial create
  writeCodexTomlConfig(opts); // force-rewrite #1
  const afterFirst = fs.readFileSync(target, "utf8");
  writeCodexTomlConfig(opts); // force-rewrite #2
  const afterSecond = fs.readFileSync(target, "utf8");
  assert.equal(afterFirst, afterSecond, "repeated force-rewrite must be byte-stable");
  const comments = (afterSecond.match(/# Terminal Commander MCP stdio adapter/g) || []).length;
  assert.equal(comments, 1, `expected exactly one marker comment, got ${comments}`);
  const sections = (afterSecond.match(/\[mcp_servers\.terminal_commander\]/g) || []).length;
  assert.equal(sections, 1, "expected exactly one terminal_commander section");
});

test("codex force-rewrite preserves OTHER mcp_servers sections", () => {
  const target = tmpCfg();
  fs.writeFileSync(
    target,
    '[mcp_servers.terminal_commander]\ncommand = "old"\nargs = []\n\n' +
      '[mcp_servers.terminal_commander.env]\nTC_SESSION = "tc-old"\n\n' +
      '[mcp_servers.other]\ncommand = "x"\nargs = []\n',
  );
  const r = writeCodexTomlConfig({
    path: target,
    sessionToken: "tc-new111",
    surface: "compact",
    force: true,
    clobber_backup: true,
  });
  assert.equal(r.status, "config_updated");
  const text = fs.readFileSync(target, "utf8");
  assert.match(text, /\[mcp_servers\.other\]/);
  assert.match(text, /command = "x"/);
  assert.match(text, /TC_SESSION = "tc-new111"/);
  assert.equal(text.includes("tc-old"), false, "old session token must be stripped");
  assert.equal((text.match(/TC_SESSION = /g) || []).length, 1);
});

// --- Fix 3: malformed-safe + BOM-strip ---

test("isLikelyMalformedToml flags a truncated section header, not valid TOML", () => {
  // Valid configs (incl. comments + the real stanza) are NOT flagged.
  assert.equal(isLikelyMalformedToml(""), false);
  assert.equal(isLikelyMalformedToml("# just a comment\n"), false);
  assert.equal(
    isLikelyMalformedToml('[mcp_servers.terminal_commander]\ncommand = "x"\nargs = []\n'),
    false,
  );
  // A header line that opens '[' but never closes ']' is clearly broken.
  assert.equal(isLikelyMalformedToml("[mcp_servers.terminal_commander\ncommand = "), true);
});

test("writeCodexTomlConfig refuses a malformed config.toml and leaves it untouched", () => {
  const target = tmpCfg();
  const bad = "[mcp_servers.terminal_commander\ncommand = ";
  fs.writeFileSync(target, bad);
  const r = writeCodexTomlConfig({
    path: target,
    sessionToken: "tc-aaa111",
    force: true,
    clobber_backup: true,
  });
  assert.equal(r.status, "invalid_toml");
  // The malformed file is NOT modified, and no backup was made.
  assert.equal(fs.readFileSync(target, "utf8"), bad);
  const baks = fs.readdirSync(path.dirname(target)).filter((n) => n.endsWith(".bak"));
  assert.equal(baks.length, 0, "a refused write must not create a backup");
});

test("writeCodexTomlConfig strips a leading UTF-8 BOM before the section check", () => {
  const target = tmpCfg();
  // BOM + an existing terminal_commander section. Without BOM-strip the section
  // check would miss it (BOM glued to '[') and the writer would append a
  // duplicate; with force-rewrite it must instead refresh the single section.
  fs.writeFileSync(
    target,
    "﻿[mcp_servers.terminal_commander]\ncommand = \"old\"\nargs = []\n",
  );
  const r = writeCodexTomlConfig({
    path: target,
    sessionToken: "tc-new222",
    force: true,
    clobber_backup: true,
  });
  assert.equal(r.status, "config_updated");
  const text = fs.readFileSync(target, "utf8");
  assert.equal(
    (text.match(/\[mcp_servers\.terminal_commander\]/g) || []).length,
    1,
    "BOM must not cause a duplicate section",
  );
  assert.equal(text.includes('command = "old"'), false, "stale command must be gone");
  // The rewritten file carries no BOM.
  assert.equal(text.charCodeAt(0) === 0xfeff, false, "rewritten config must not carry the BOM");
});

const APPROVAL = (tool) => new RegExp(`\\[mcp_servers\\.terminal_commander\\.tools\\.${tool}\\]\\napproval_mode = "approve"`);

test("codex writer approves the code-surface tools by default and a re-run is a no-op", () => {
  const target = tmpCfg();
  assert.equal(writeCodexTomlConfig({ path: target, sessionToken: "tc-aaa111" }).status, "config_created");
  const text = fs.readFileSync(target, "utf8");
  for (const tool of ["code_search", "code_execute", "status"]) assert.match(text, APPROVAL(tool));
  const again = writeCodexTomlConfig({ path: target, sessionToken: "tc-aaa111", force: true });
  assert.equal(again.status, "already_exists");
  assert.equal(fs.readFileSync(target, "utf8"), text);
});

test("codex force-rewrite adds missing code approvals from the stored surface and keeps user tables", () => {
  const target = tmpCfg();
  fs.writeFileSync(target, [
    "[mcp_servers.terminal_commander]",
    'command = "tc-mcp"',
    "args = []",
    "",
    "[mcp_servers.terminal_commander.env]",
    'TC_SURFACE = "code"',
    "",
    "[mcp_servers.terminal_commander.tools.run_and_watch]",
    'approval_mode = "approve"',
    "",
    "[mcp_servers.terminal_commander.tools.status]",
    'approval_mode = "prompt"',
    "",
  ].join("\n"));
  const r = writeCodexTomlConfig({ path: target, force: true, clobber_backup: true });
  assert.equal(r.status, "config_updated");
  const text = fs.readFileSync(target, "utf8");
  assert.match(text, APPROVAL("code_search"));
  assert.match(text, APPROVAL("code_execute"));
  assert.match(text, APPROVAL("run_and_watch"));
  assert.match(text, /tools\.status\]\napproval_mode = "prompt"/);
  assert.equal((text.match(/tools\.status\]/g) || []).length, 1);
});

test("codex writer adds no code approvals for an explicit full surface or for grok", () => {
  const full = tmpCfg();
  writeCodexTomlConfig({ path: full, surface: "full" });
  assert.doesNotMatch(fs.readFileSync(full, "utf8"), /\.tools\./);
  const grok = tmpCfg();
  writeCodexTomlConfig({ path: grok, providerId: "grok" });
  assert.doesNotMatch(fs.readFileSync(grok, "utf8"), /\.tools\./);
});
