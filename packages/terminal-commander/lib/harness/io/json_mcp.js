// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// JSON mcpServers merge + atomic write (shared by Claude providers).

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { atomicWriteWithBackup, ATOMIC_REASONS, stripBom } = require("./atomic.js");

const MAX_CONFIG_BYTES = 256 * 1024;

const JSON_MCP_STATUSES = Object.freeze({
  CONFIG_CREATED: "config_created",
  CONFIG_UPDATED: "config_updated",
  ALREADY_EXISTS: "already_exists",
  INVALID_JSON: "invalid_json",
  CONFIG_TOO_LARGE: "config_too_large",
  BACKUP_FAILED: "backup_failed",
  WRITE_FAILED: "write_failed",
  UNSUPPORTED: "unsupported",
});

function parseJsonMcp(buffer) {
  if (buffer == null) return { ok: true, value: { mcpServers: {} } };
  const len = Buffer.isBuffer(buffer)
    ? buffer.length
    : Buffer.byteLength(String(buffer), "utf8");
  if (len === 0) return { ok: true, value: { mcpServers: {} } };
  if (len > MAX_CONFIG_BYTES) {
    return { ok: false, reason: JSON_MCP_STATUSES.CONFIG_TOO_LARGE };
  }
  try {
    // BOM-strip before parse: a leading UTF-8 BOM (some Windows shells/editors)
    // makes JSON.parse reject the first value.
    const value = JSON.parse(
      stripBom(Buffer.isBuffer(buffer) ? buffer.toString("utf8") : String(buffer)),
    );
    if (value == null || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, reason: JSON_MCP_STATUSES.INVALID_JSON };
    }
    if (value.mcpServers == null) value.mcpServers = {};
    if (typeof value.mcpServers !== "object" || Array.isArray(value.mcpServers)) {
      return { ok: false, reason: JSON_MCP_STATUSES.INVALID_JSON };
    }
    return { ok: true, value };
  } catch (_e) {
    return { ok: false, reason: JSON_MCP_STATUSES.INVALID_JSON };
  }
}

function mergeJsonMcpServers(existing, serverName, serverConfig, opts) {
  const o = opts || {};
  const wasPresent = Object.prototype.hasOwnProperty.call(existing.mcpServers, serverName);
  if (wasPresent && o.force !== true) {
    return { ok: false, reason: JSON_MCP_STATUSES.ALREADY_EXISTS };
  }
  const mergedServers = {};
  for (const name of Object.keys(existing.mcpServers)) {
    if (name === serverName) continue;
    mergedServers[name] = existing.mcpServers[name];
  }
  mergedServers[serverName] = serverConfig;
  return {
    ok: true,
    value: { ...existing, mcpServers: mergedServers },
    was_present: wasPresent,
  };
}

/**
 * Write MCP stanza into a JSON file with mcpServers top-level key.
 */
function writeJsonMcpConfig(opts) {
  const o = opts || {};
  const target = o.path;
  const serverName = o.serverName;
  const serverConfig = o.serverConfig;
  if (!target || !serverName || !serverConfig) {
    return { status: JSON_MCP_STATUSES.WRITE_FAILED, path: target || null, hint: "" };
  }
  const scopeDir = path.dirname(target);
  let existingBuf = null;
  if (fs.existsSync(target)) {
    try {
      const st = fs.statSync(target);
      if (st.size > MAX_CONFIG_BYTES) {
        return {
          status: JSON_MCP_STATUSES.CONFIG_TOO_LARGE,
          path: target,
          hint: `terminal-commander: config too large at ${target}`,
        };
      }
      existingBuf = fs.readFileSync(target);
    } catch (_e) {
      return { status: JSON_MCP_STATUSES.WRITE_FAILED, path: target, hint: "" };
    }
  }
  const parsed = parseJsonMcp(existingBuf);
  if (!parsed.ok) {
    return {
      status: parsed.reason,
      path: target,
      hint: `terminal-commander: invalid JSON at ${target}`,
    };
  }
  const merged = mergeJsonMcpServers(parsed.value, serverName, serverConfig, {
    force: o.force === true,
  });
  if (!merged.ok) {
    return {
      status: merged.reason,
      path: target,
      hint: `terminal-commander: entry ${serverName} already exists; use --force`,
    };
  }
  const fileExisted = existingBuf != null;
  const contents = JSON.stringify(merged.value, null, 2) + "\n";
  const wrote = atomicWriteWithBackup(target, contents, {
    scopeDir,
    clobber_backup: o.clobber_backup === true,
    randomSuffix: o.randomSuffix,
  });
  if (!wrote.ok) {
    // json_mcp has no PATH_NOT_ALLOWED status; collapse it into WRITE_FAILED
    // exactly as the previous hand-rolled writer did.
    const status =
      wrote.reason === ATOMIC_REASONS.PATH_NOT_ALLOWED
        ? JSON_MCP_STATUSES.WRITE_FAILED
        : wrote.reason;
    return { status, path: target, hint: "" };
  }
  const status = fileExisted
    ? JSON_MCP_STATUSES.CONFIG_UPDATED
    : JSON_MCP_STATUSES.CONFIG_CREATED;
  return {
    status,
    path: target,
    hint: `terminal-commander: ${status.replace(/_/g, " ")} ${target}`,
  };
}

module.exports = {
  writeJsonMcpConfig,
  parseJsonMcp,
  mergeJsonMcpServers,
  JSON_MCP_STATUSES,
  MAX_CONFIG_BYTES,
};
