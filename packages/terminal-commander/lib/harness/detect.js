// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Read-only harness presence detection.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const {
  cursorConfigPath,
  codexConfigPath,
  ompConfigPath,
  claudeCodeSettingsPath,
  claudeCodeMcpConfigPath,
  claudeDesktopConfigPath,
  geminiConfigPath,
  grokConfigPath,
  kiloConfigPath,
  isProjectScope,
  expandHome,
} = require("./paths.js");
const { listProviders } = require("./registry.js");

function pathExists(p) {
  try {
    return fs.existsSync(p);
  } catch (_e) {
    return false;
  }
}

function detectCursor(opts) {
  const o = opts || {};
  try {
    const globalCfg = cursorConfigPath(o);
    const cursorDir = path.dirname(globalCfg);
    if (pathExists(cursorDir) || pathExists(globalCfg)) {
      return { detected: true, reason: "cursor_dir_or_config", config_path: globalCfg };
    }
  } catch (_e) {
    return { detected: false, reason: "unsupported_host" };
  }
  return { detected: false, reason: "not_found", config_path: cursorConfigPath(o) };
}

function detectCodex(opts) {
  const p = codexConfigPath(opts);
  const codexDir = path.dirname(p);
  if (pathExists(p) || pathExists(codexDir)) {
    return { detected: true, reason: "codex_config_or_dir", config_path: p };
  }
  return { detected: false, reason: "not_found", config_path: p };
}

function detectOmp(opts) {
  const p = ompConfigPath(opts);
  if (pathExists(p) || pathExists(path.dirname(p))) {
    return { detected: true, reason: "omp_config_or_dir", config_path: p };
  }
  return { detected: false, reason: "not_found", config_path: p };
}

function detectClaudeCode(opts) {
  const mcpPath = claudeCodeMcpConfigPath(opts);
  if (isProjectScope(opts)) {
    return { detected: pathExists(mcpPath), reason: pathExists(mcpPath) ? "claude_project_config" : "not_found", config_path: mcpPath };
  }
  const settingsPath = claudeCodeSettingsPath(opts);
  const claudeDir = path.dirname(settingsPath);
  if (pathExists(mcpPath)) {
    return { detected: true, reason: "claude_json", config_path: mcpPath };
  }
  if (pathExists(settingsPath) || pathExists(claudeDir)) {
    return { detected: true, reason: "claude_settings_or_dir", config_path: mcpPath };
  }
  return { detected: false, reason: "not_found", config_path: mcpPath };
}

function detectClaudeDesktop(opts) {
  const p = claudeDesktopConfigPath(opts);
  if (pathExists(p)) {
    return { detected: true, reason: "claude_desktop_config", config_path: p };
  }
  const parent = path.dirname(p);
  if (pathExists(parent)) {
    return { detected: true, reason: "claude_desktop_dir", config_path: p };
  }
  return { detected: false, reason: "not_found", config_path: p };
}

function detectGemini(opts) {
  return detectConfig(geminiConfigPath(opts), "gemini");
}

function detectGrok(opts) {
  return detectConfig(grokConfigPath(opts), "grok");
}

function detectKilo(opts) {
  const p = kiloConfigPath(opts);
  return { ...detectConfig(p, "kilo"), config_format: path.basename(p) === "mcp.json" ? "json-mcp" : "json-kilo" };
}

function detectConfig(p, id) {
  const detected = pathExists(p) || pathExists(path.dirname(p));
  return { detected, reason: detected ? `${id}_config_or_dir` : "not_found", config_path: p };
}

function detectKimi(opts) {
  const markers = [expandHome("~/.kimi", opts), expandHome("~/.config/kimi", opts)];
  for (const m of markers) {
    if (pathExists(m)) {
      return {
        detected: true,
        reason: "kimi_marker",
        config_path: null,
        stub: true,
        note: "config_path_unverified",
      };
    }
  }
  return { detected: false, reason: "not_found", stub: true };
}

const DETECTORS = Object.freeze({
  cursor: detectCursor,
  "codex-cli": detectCodex,
  omp: detectOmp,
  "claude-code": detectClaudeCode,
  "claude-desktop": detectClaudeDesktop,
  gemini: detectGemini,
  grok: detectGrok,
  "kilo-code": detectKilo,
  kimi: detectKimi,
});

function detectProvider(id, opts) {
  const fn = DETECTORS[id];
  if (!fn) return { detected: false, reason: "unknown_provider" };
  try {
    return fn(opts);
  } catch (error) {
    return { detected: false, reason: isProjectScope(opts) ? "project_root_required" : "unsupported_host" };
  }
}

function detectAllHarnesses(opts) {
  const o = opts || {};
  const results = [];
  for (const p of listProviders({ includeStubs: true })) {
    const d = detectProvider(p.id, o);
    results.push({
      id: p.id,
      label: p.label,
      stub: p.stub,
      ...d,
    });
  }
  return results;
}

module.exports = {
  detectProvider,
  detectAllHarnesses,
  detectCursor,
  detectCodex,
  detectOmp,
  detectClaudeCode,
  detectClaudeDesktop,
  detectGemini,
  detectGrok,
  detectKilo,
  detectKimi,
};
