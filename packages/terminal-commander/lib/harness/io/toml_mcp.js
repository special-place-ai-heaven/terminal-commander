// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Minimal TOML writer for Codex [mcp_servers.terminal_commander] blocks,
// including the [mcp_servers.terminal_commander.env] sub-table (TC_SESSION /
// TC_SURFACE / TC_WSL_DISTRO). Section-scoped merge only; does not parse a
// full TOML AST.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const {
  buildTerminalCommanderCommandConfig,
  isValidSessionToken,
  assertSafeDistroName,
} = require("../../cursor/config.js");

const SECTION_HEADER = "[mcp_servers.terminal_commander]";
const ENV_SECTION_HEADER = "[mcp_servers.terminal_commander.env]";
const SERVER_NAME = "terminal_commander";
// Standalone marker comment for our block. Stripped by removeSection on a
// force-rewrite so it is not duplicated each time the block is re-emitted.
const BLOCK_COMMENT =
  "# Terminal Commander MCP stdio adapter (merged by terminal-commander bootstrap).";
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

/**
 * Build the per-server env map written into Codex's
 * `[mcp_servers.terminal_commander.env]` table. Mirrors the JSON harness env
 * (buildJsonMcpStanza): TC_SESSION (per-harness daemon endpoint), TC_SURFACE
 * (compact|full tool surface), TC_WSL_DISTRO (Windows only). Codex applies
 * these LITERALLY to the spawned MCP server's process env — it performs no
 * ${VAR} expansion and clears the inherited env first (verified against
 * openai/codex codex-rs mcp_types.rs `env: HashMap<String,String>` + the
 * rmcp stdio launcher) — so only literal values belong here.
 *
 * @param {Object} [opts]
 * @param {string} [opts.sessionToken]  Validated TC_SESSION token (throws if unsafe).
 * @param {("compact"|"full")} [opts.surface]  Optional TC_SURFACE.
 * @param {string} [opts.distro]  Optional WSL distro (emitted only on win32).
 * @param {string} [opts.platform]
 * @returns {Object} env map (possibly empty)
 * @throws {Error} `.code` UNSAFE_SESSION_TOKEN on a malformed token.
 */
function buildCodexEnv(opts) {
  const o = opts || {};
  const env = {};
  if (o.sessionToken != null && o.sessionToken !== "") {
    // Symmetric with buildJsonMcpStanza: a token can name a kernel object /
    // socket path, so validate before it can land in a config.
    if (!isValidSessionToken(o.sessionToken)) {
      const err = new Error(
        "terminal-commander: TC_SESSION token failed safety whitelist; only [A-Za-z0-9._-] (1..64, at least one alphanumeric, not dot-only) is allowed",
      );
      err.code = "UNSAFE_SESSION_TOKEN";
      throw err;
    }
    env.TC_SESSION = o.sessionToken;
  }
  if (o.surface) {
    env.TC_SURFACE = o.surface;
  }
  if (o.distro && o.platform === "win32") {
    // Validate the distro charset before it lands in env: it is interpolated
    // into a `wsl -d <distro>` command downstream. Symmetric with the Cursor
    // path and the JSON harness path.
    assertSafeDistroName(o.distro);
    env.TC_WSL_DISTRO = o.distro;
  }
  return env;
}

function buildCodexTomlBlock(opts) {
  const o = opts || {};
  const commandConfig = buildTerminalCommanderCommandConfig(o);
  const lines = [
    BLOCK_COMMENT,
    SECTION_HEADER,
    `command = ${JSON.stringify(commandConfig.command)}`,
    `args = [${commandConfig.args.map((arg) => JSON.stringify(arg)).join(", ")}]`,
  ];
  // Emit the env sub-table when there are values. `includeEnv: false` is an
  // explicit opt-out. Keys are bare TOML keys; values are TOML basic strings
  // (JSON.stringify is a valid TOML basic-string encoder for the [A-Za-z0-9._-]
  // + compact|full value charset these keys carry).
  const env = o.includeEnv === false ? {} : buildCodexEnv(o);
  const envKeys = Object.keys(env);
  if (envKeys.length > 0) {
    lines.push("", ENV_SECTION_HEADER);
    for (const key of envKeys) {
      lines.push(`${key} = ${JSON.stringify(env[key])}`);
    }
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
        // Skip BOTH our main table and its `.env` sub-table (and their body
        // lines) on a force-rewrite. Setting skipping=true for the env header
        // too is what strips the old TC_SESSION/TC_SURFACE lines; otherwise
        // they would be orphaned and duplicated by the re-emitted block.
        skipping = true;
        continue;
      }
      skipping = false;
    }
    // Drop our standalone marker comment; buildCodexTomlBlock re-adds exactly one.
    if (trimmed === BLOCK_COMMENT) {
      continue;
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
  buildCodexEnv,
  TOML_MCP_STATUSES,
  SECTION_HEADER,
  ENV_SECTION_HEADER,
  SERVER_NAME,
};
