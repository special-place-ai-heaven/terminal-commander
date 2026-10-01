// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Harness config path resolution from env + platform.

"use strict";

const path = require("node:path");
const os = require("node:os");
const fs = require("node:fs");

function isProjectScope(opts) {
  const o = opts || {};
  return o.scope === "project" || o.cursor_scope === "project" || o.project_scope === "project";
}

function projectPath(opts, relativePath) {
  if (!opts.projectRoot) throw new Error("project scope requires projectRoot");
  return path.join(opts.projectRoot, relativePath);
}

function scopedPath(opts, projectRelativePath, globalPath) {
  return isProjectScope(opts) ? projectPath(opts, projectRelativePath) : globalPath();
}

function homeDir(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  if (platform === "win32") {
    const h = env.USERPROFILE;
    if (!h) throw new Error("USERPROFILE not set");
    return h;
  }
  const h = env.HOME || (o.env ? undefined : os.homedir());
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
  const o = opts || {};
  return scopedPath(o, ".codex/config.toml", () => {
    const env = o.env || process.env;
    return env.CODEX_HOME ? path.join(env.CODEX_HOME, "config.toml") : expandHome("~/.codex/config.toml", o);
  });
}

function ompConfigPath(opts) {
  return scopedPath(opts || {}, ".omp/mcp.json", () => expandHome("~/.omp/agent/mcp.json", opts));
}

function cursorConfigPath(opts) {
  return scopedPath(opts || {}, ".cursor/mcp.json", () => expandHome("~/.cursor/mcp.json", opts));
}

function geminiConfigPath(opts) {
  return scopedPath(opts || {}, ".gemini/settings.json", () => expandHome("~/.gemini/settings.json", opts));
}

function grokConfigPath(opts) {
  return scopedPath(opts || {}, ".grok/config.toml", () => expandHome("~/.grok/config.toml", opts));
}

function kiloConfigPath(opts) {
  const o = opts || {};
  let candidates;
  if (isProjectScope(o)) {
    candidates = ["kilo.jsonc", "kilo.json", ".kilo/kilo.jsonc", ".kilo/kilo.json", ".kilocode/mcp.json"].map((p) => projectPath(o, p));
  } else {
    const env = o.env || process.env;
    const base = env.XDG_CONFIG_HOME || expandHome("~/.config", o);
    candidates = [path.join(base, "kilo", "kilo.jsonc"), path.join(base, "kilo", "kilo.json")];
  }
  return candidates.find((p) => fs.existsSync(p)) || (isProjectScope(o) ? projectPath(o, ".kilo/kilo.jsonc") : candidates[0]);
}

/** General Claude Code settings (permissions, hooks) — not MCP. */
function claudeCodeSettingsPath(opts) {
  return expandHome("~/.claude/settings.json", opts);
}

/** User-scope MCP servers for Claude Code (official: ~/.claude.json). */
function claudeCodeMcpConfigPath(opts) {
  return scopedPath(opts || {}, ".mcp.json", () => expandHome("~/.claude.json", opts));
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
  isProjectScope,
  cursorConfigPath,
  geminiConfigPath,
  grokConfigPath,
  kiloConfigPath,
  codexConfigPath,
  ompConfigPath,
  claudeCodeSettingsPath,
  claudeCodeMcpConfigPath,
  claudeDesktopConfigPath,
};
