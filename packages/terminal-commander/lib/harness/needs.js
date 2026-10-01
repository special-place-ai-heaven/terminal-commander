// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const fs = require("node:fs");
const { listProviders } = require("./registry.js");
const { detectProvider } = require("./detect.js");
const { stripJsonc } = require("./io/json_mcp.js");

function entryConfigured(id, opts) {
  const o = opts || {};
  try {
    const d = detectProvider(id, o);
    if (!d.detected || !d.config_path) return false;
    if (!fs.existsSync(d.config_path)) return false;
    const text = fs.readFileSync(d.config_path, "utf8");
    if (id === "codex-cli" || id === "grok") return /^\s*\[mcp_servers\.terminal_commander\]\s*(?:#.*)?$/m.test(text);
    if (id === "omp") {
      const config = JSON.parse(text);
      const name = "terminal-commander";
      if ("enabledServers" in config && !Array.isArray(config.enabledServers)) return false;
      if ("disabledServers" in config && !Array.isArray(config.disabledServers)) return false;
      return Boolean(config.mcpServers?.[name]?.command) &&
        (!Array.isArray(config.enabledServers) || config.enabledServers.includes(name)) &&
        (!Array.isArray(config.disabledServers) || !config.disabledServers.includes(name));
    }
    const provider = listProviders().find((p) => p.id === id);
    const config = JSON.parse(d.config_path.endsWith(".jsonc") ? stripJsonc(text) : text);
    const entry = (d.config_format === "json-kilo" ? config.mcp : config.mcpServers)?.[provider.serverName];
    if (!entry || typeof entry !== "object") return false;
    if (entry.url || entry.httpUrl || entry.serverUrl) return true;
    return d.config_format === "json-kilo"
      ? Array.isArray(entry.command) && entry.command.length > 0 && entry.command.every((part) => typeof part === "string")
      : typeof entry.command === "string" && entry.command.length > 0 &&
        (entry.args == null || (Array.isArray(entry.args) && entry.args.every((arg) => typeof arg === "string")));
  } catch (_e) {
    return false;
  }
}

/**
 * True when any non-stub detected harness is missing the TC MCP stanza.
 */
function harnessNeedsConfiguration(opts) {
  const o = opts || {};
  for (const p of listProviders({ includeStubs: false })) {
    const d = detectProvider(p.id, o);
    if (!d.detected || d.stub) continue;
    if (!entryConfigured(p.id, o)) return true;
  }
  return false;
}

module.exports = {
  entryConfigured,
  harnessNeedsConfiguration,
};
