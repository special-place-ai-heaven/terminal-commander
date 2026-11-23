// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Doctor check: a Claude Code agent whose `tools:` frontmatter names
// terminal_commander tools the active surface does not expose silently gets
// none of them. Reads ~/.claude.json for TC_SURFACE and ~/.claude/agents/*.md.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { claudeCodeMcpConfigPath, expandHome } = require("../harness/paths.js");

const SURFACE_TOOLS = Object.freeze({
  code: ["code_search", "code_execute", "status"],
  compact: ["command", "files", "recipe", "registry", "session", "status"],
});
// shortcut: the 60 granular names are not mirrored in JS, so under full only
// the code-only tools are known to be absent; mirror the catalogue if needed.
const NOT_ON_FULL = new Set(["code_search", "code_execute"]);
const TOOL_RE = /mcp__terminal_commander__([A-Za-z0-9_]+)/g;

/** Surface the Claude Code entry runs: TC_SURFACE, unset/unknown => code (server default). */
function claudeSurface(claudeJsonPath) {
  let raw;
  try {
    raw = JSON.parse(fs.readFileSync(claudeJsonPath, "utf8"))?.mcpServers?.terminal_commander?.env?.TC_SURFACE;
  } catch (_e) {
    raw = undefined;
  }
  const s = String(raw || "").trim().toLowerCase();
  return s === "full" || s === "compact" ? s : "code";
}

function exposes(surface, tool) {
  return surface === "full" ? !NOT_ON_FULL.has(tool) : SURFACE_TOOLS[surface].includes(tool);
}

/** The frontmatter `tools:` entry, inline or as a YAML list. */
function frontmatterTools(text) {
  if (!text.startsWith("---")) return "";
  const lines = text.split(/\r?\n/);
  const end = lines.indexOf("---", 1);
  const fm = lines.slice(1, end < 0 ? lines.length : end);
  const i = fm.findIndex((l) => /^tools\s*:/.test(l));
  if (i < 0) return "";
  let j = i + 1;
  while (j < fm.length && /^\s*-\s/.test(fm[j])) j++;
  return fm.slice(i, j).join("\n");
}

/**
 * @param {Object} [opts] platform/env for home resolution; claudeJsonPath and
 *   agentsDir override the defaults (tests).
 * @returns {string[]} one WARNING line per mismatched agent file
 */
function agentSurfaceWarnings(opts) {
  const o = opts || {};
  let claudeJson, agentsDir, files;
  try {
    claudeJson = o.claudeJsonPath || claudeCodeMcpConfigPath(o);
    agentsDir = o.agentsDir || expandHome("~/.claude/agents", o);
    files = fs.readdirSync(agentsDir).filter((f) => f.endsWith(".md")).sort();
  } catch (_e) {
    return [];
  }
  const surface = claudeSurface(claudeJson);
  const fix = surface === "full" ? "the granular tool names" : SURFACE_TOOLS[surface].map((t) => `mcp__terminal_commander__${t}`).join(", ");
  const warnings = [];
  for (const f of files) {
    let text;
    try {
      text = fs.readFileSync(path.join(agentsDir, f), "utf8");
    } catch (_e) {
      continue;
    }
    const names = [...new Set([...frontmatterTools(text).matchAll(TOOL_RE)].map((m) => m[1]))];
    const missing = names.filter((t) => !exposes(surface, t));
    if (missing.length) {
      warnings.push(
        `WARNING: agent ${f} lists terminal_commander tools the ${surface} surface does not expose ` +
          `(${missing.join(", ")}); that agent gets none of them. List ${fix} instead.`,
      );
    }
  }
  return warnings;
}

module.exports = {
  agentSurfaceWarnings,
  SURFACE_TOOLS,
};
