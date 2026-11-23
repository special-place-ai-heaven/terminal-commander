// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Tool-surface choice for `terminal-commander update`: an explicit
// --surface wins; a person at a terminal gets a menu (Enter keeps the current
// surface); anything else (agents, CI, postinstall) keeps the configured one.

"use strict";

const SURFACES = Object.freeze([
  { id: "code", label: "Code mode (code_search, code_execute, status)" },
  { id: "full", label: "MCP full (60 granular tools)" },
  { id: "compact", label: "MCP compact (6 facades)" },
]);

/** `mcp-full` / `mcp-compact` are the CLI names; TC_SURFACE keeps the server's `full` / `compact`. */
function normalizeSurface(value) {
  const v = String(value).trim().toLowerCase();
  return v === "mcp-full" ? "full" : v === "mcp-compact" ? "compact" : v;
}

/**
 * @param {Object} o
 * @param {string} [o.flag] value of --surface, already validated
 * @param {boolean} o.interactive stdin and stdout are both a TTY
 * @param {string} o.current surface configured now
 * @param {(question: string) => Promise<string>} o.ask reads one answer line
 * @returns {Promise<string|undefined>} chosen surface, or undefined to keep the configured one
 */
async function chooseSurface(o) {
  if (o.flag) return o.flag;
  if (!o.interactive) return undefined;
  const lines = SURFACES.map((s, i) => `  [${i + 1}] ${s.label}${s.id === o.current ? "  (current)" : ""}`);
  const def = SURFACES.findIndex((s) => s.id === o.current) + 1 || 1;
  for (;;) {
    const answer = normalizeSurface(await o.ask(`Tool surface:\n${lines.join("\n")}\nChoose 1-3 [${def}]: `));
    if (answer === "") return SURFACES[def - 1].id;
    const byNumber = SURFACES[Number(answer) - 1];
    const byName = SURFACES.find((s) => s.id === answer);
    if (byNumber || byName) return (byNumber || byName).id;
  }
}

module.exports = { chooseSurface, normalizeSurface, SURFACES };
