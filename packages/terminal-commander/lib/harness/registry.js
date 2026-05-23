// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Provider registry for harness auto-config.

"use strict";

const PROVIDERS = Object.freeze([
  {
    id: "cursor",
    label: "Cursor IDE",
    format: "json-mcp",
    serverName: "terminal-commander",
    stub: false,
  },
  {
    id: "codex-cli",
    label: "Codex CLI",
    format: "toml-codex",
    serverName: "terminal_commander",
    stub: false,
  },
  {
    id: "claude-code",
    label: "Claude Code",
    format: "json-mcp",
    serverName: "terminal_commander",
    stub: false,
  },
  {
    id: "claude-desktop",
    label: "Claude Desktop",
    format: "json-mcp",
    serverName: "terminal_commander",
    stub: false,
  },
  {
    id: "gemini",
    label: "Gemini",
    format: "stub",
    serverName: "terminal_commander",
    stub: true,
  },
  {
    id: "kimi",
    label: "Kimi",
    format: "stub",
    serverName: "terminal_commander",
    stub: true,
  },
]);

function getProvider(id) {
  return PROVIDERS.find((p) => p.id === id) || null;
}

function listProviders(opts) {
  const o = opts || {};
  if (o.includeStubs === false) {
    return PROVIDERS.filter((p) => !p.stub);
  }
  return PROVIDERS.slice();
}

module.exports = {
  PROVIDERS,
  getProvider,
  listProviders,
};
