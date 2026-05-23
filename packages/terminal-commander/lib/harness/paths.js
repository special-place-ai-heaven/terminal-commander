// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Harness config path resolution from env + platform.

"use strict";

const path = require("node:path");
const os = require("node:os");

function homeDir(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  if (platform === "win32") {
    const h = env.USERPROFILE;
    if (!h) throw new Error("USERPROFILE not set");
    return h;
  }
  const h = env.HOME || os.homedir();
  if (!h) throw new Error("HOME not set");
  return h;
}

function expandHome(p, opts) {
  if (typeof p !== "string") return p;
  if (p.startsWith("~/")) {
    return path.join(homeDir(opts), p.slice(2));
  }
  return p;
}

function codexConfigPath(opts) {
  return expandHome("~/.codex/config.toml", opts);
}

/** General Claude Code settings (permissions, hooks) — not MCP. */
function claudeCodeSettingsPath(opts) {
  return expandHome("~/.claude/settings.json", opts);
}

/** User-scope MCP servers for Claude Code (official: ~/.claude.json). */
function claudeCodeMcpConfigPath(opts) {
  return expandHome("~/.claude.json", opts);
}

function claudeDesktopConfigPath(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  if (platform === "win32") {
    const appData = env.APPDATA;
    if (appData) return path.join(appData, "Claude", "claude_desktop_config.json");
    return path.join(homeDir(o), "AppData", "Roaming", "Claude", "claude_desktop_config.json");
  }
  if (platform === "darwin") {
    return path.join(
      homeDir(o),
      "Library",
      "Application Support",
      "Claude",
      "claude_desktop_config.json",
    );
  }
  return expandHome("~/.config/Claude/claude_desktop_config.json", o);
}

module.exports = {
  homeDir,
  expandHome,
  codexConfigPath,
  claudeCodeSettingsPath,
  claudeCodeMcpConfigPath,
  claudeDesktopConfigPath,
};
