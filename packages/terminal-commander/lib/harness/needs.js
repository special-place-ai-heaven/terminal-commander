// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

"use strict";

const fs = require("node:fs");
const { listProviders } = require("./registry.js");
const { detectProvider } = require("./detect.js");

function entryConfigured(id, opts) {
  const o = opts || {};
  try {
    const d = detectProvider(id, o);
    if (!d.detected || !d.config_path) return false;
    if (!fs.existsSync(d.config_path)) return false;
    const text = fs.readFileSync(d.config_path, "utf8");
    if (id === "cursor") return text.includes("terminal-commander-mcp");
    if (id === "codex-cli") return text.includes("[mcp_servers.terminal_commander]");
    if (id === "claude-code") {
      return text.includes("terminal-commander-mcp") && text.includes('"terminal_commander"');
    }
    return text.includes("terminal-commander-mcp");
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
