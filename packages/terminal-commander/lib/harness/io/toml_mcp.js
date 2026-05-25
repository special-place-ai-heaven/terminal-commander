// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// Minimal TOML writer for Codex [mcp_servers.terminal_commander] blocks.
// Section-scoped merge only; does not parse full TOML AST.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { buildTerminalCommanderCommandConfig } = require("../../cursor/config.js");

const SECTION_HEADER = "[mcp_servers.terminal_commander]";
const ENV_SECTION_HEADER = "[mcp_servers.terminal_commander.env]";
const SERVER_NAME = "terminal_commander";
const MAX_CONFIG_BYTES = 256 * 1024;

const TOML_MCP_STATUSES = Object.freeze({
  CONFIG_CREATED: "config_created",
  CONFIG_UPDATED: "config_updated",
  ALREADY_EXISTS: "already_exists",
  CONFIG_TOO_LARGE: "config_too_large",
  BACKUP_FAILED: "backup_failed",
  WRITE_FAILED: "write_failed",
});

function sectionExists(text, header) {
  const re = new RegExp(`^\\s*${header.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`, "m");
  return re.test(text);
}

function buildCodexTomlBlock(opts) {
  const o = opts || {};
  const commandConfig = buildTerminalCommanderCommandConfig(o);
  const lines = [
    "# Terminal Commander MCP stdio adapter (merged by terminal-commander bootstrap).",
    SECTION_HEADER,
    `command = ${JSON.stringify(commandConfig.command)}`,
    `args = [${commandConfig.args.map((arg) => JSON.stringify(arg)).join(", ")}]`,
  ];
  if (o.includeEnv === true) {
    lines.push("", ENV_SECTION_HEADER);
    lines.push('TC_SOCKET = "${TC_DATA}/terminal-commanderd.sock"');
  }
  return lines.join("\n") + "\n";
}

function removeSection(text, header) {
  const lines = text.split(/\r?\n/);
  const out = [];
  let skipping = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trim();
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      if (trimmed === SECTION_HEADER || trimmed === ENV_SECTION_HEADER) {
        skipping = trimmed === SECTION_HEADER;
        continue;
      }
      skipping = false;
    }
    if (skipping && trimmed.startsWith("[mcp_servers.")) {
      skipping = false;
    }
    if (!skipping) out.push(line);
  }
  return out.join("\n").replace(/\n{3,}/g, "\n\n");
}

function writeCodexTomlConfig(opts) {
  const o = opts || {};
  const target = o.path;
  if (!target) {
    return { status: TOML_MCP_STATUSES.WRITE_FAILED, path: null, hint: "" };
  }
  const scopeDir = path.dirname(target);
  let existing = "";
  const fileExisted = fs.existsSync(target);
  if (fileExisted) {
    try {
      const st = fs.statSync(target);
      if (st.size > MAX_CONFIG_BYTES) {
        return {
          status: TOML_MCP_STATUSES.CONFIG_TOO_LARGE,
          path: target,
          hint: "terminal-commander: codex config.toml too large",
        };
      }
      existing = fs.readFileSync(target, "utf8");
    } catch (_e) {
      return { status: TOML_MCP_STATUSES.WRITE_FAILED, path: target, hint: "" };
    }
    if (sectionExists(existing, SECTION_HEADER) && o.force !== true) {
      return {
        status: TOML_MCP_STATUSES.ALREADY_EXISTS,
        path: target,
        hint: "terminal-commander: [mcp_servers.terminal_commander] already in config.toml; use --force",
      };
    }
  }
  if (fileExisted) {
    const backupPath = target + ".bak";
    if (fs.existsSync(backupPath) && o.clobber_backup !== true) {
      return { status: TOML_MCP_STATUSES.BACKUP_FAILED, path: target, hint: "" };
    }
    try {
      fs.copyFileSync(target, backupPath);
    } catch (_e) {
      return { status: TOML_MCP_STATUSES.BACKUP_FAILED, path: target, hint: "" };
    }
  }
  let merged;
  if (fileExisted && o.force === true) {
    merged = removeSection(existing, SECTION_HEADER).trimEnd();
    if (merged.length > 0) merged += "\n\n";
    merged += buildCodexTomlBlock(o);
  } else if (fileExisted) {
    merged = existing.trimEnd() + "\n\n" + buildCodexTomlBlock(o);
  } else {
    merged = buildCodexTomlBlock(o);
  }
  try {
    fs.mkdirSync(scopeDir, { recursive: true });
  } catch (_e) {
    return { status: TOML_MCP_STATUSES.WRITE_FAILED, path: target, hint: "" };
  }
  const suffix = crypto.randomBytes(8).toString("hex");
  const tmp = target + ".tmp." + suffix;
  try {
    fs.writeFileSync(tmp, merged, { mode: 0o600 });
    fs.renameSync(tmp, target);
  } catch (_e) {
    try {
      if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
    } catch (_ee) {
      /* ignore */
    }
    return { status: TOML_MCP_STATUSES.WRITE_FAILED, path: target, hint: "" };
  }
  const status = fileExisted
    ? TOML_MCP_STATUSES.CONFIG_UPDATED
    : TOML_MCP_STATUSES.CONFIG_CREATED;
  return {
    status,
    path: target,
    hint: `terminal-commander: ${status.replace(/_/g, " ")} ${target}`,
    server_name: SERVER_NAME,
  };
}

module.exports = {
  writeCodexTomlConfig,
  buildCodexTomlBlock,
  TOML_MCP_STATUSES,
  SECTION_HEADER,
  SERVER_NAME,
};
