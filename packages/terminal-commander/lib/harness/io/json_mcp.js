// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// JSON mcpServers merge + atomic write (shared by Claude providers).

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { atomicWriteWithBackup, ATOMIC_REASONS, stripBom } = require("./atomic.js");
const { mergeManagedServer } = require("./managed_server.js");

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

function stripJsonc(text) {
  let out = "";
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      out += c;
      let closed = false;
      while (++i < text.length) {
        out += text[i];
        if (text[i] === "\\") { if (++i < text.length) out += text[i]; }
        else if (text[i] === '"') { closed = true; break; }
      }
      if (!closed) throw new Error("unterminated JSONC string");
    } else if (c === "/" && text[i + 1] === "/") {
      while (i + 1 < text.length && text[i + 1] !== "\n") i++;
      out += " ";
    } else if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      if (end < 0) throw new Error("unterminated JSONC comment");
      out += " ";
      i = end + 1;
    } else out += c;
  }
  // Remove trailing commas only outside strings.
  return out.replace(/"(?:\\.|[^"\\])*"|,(\s*[}\]])/g, (match, ending) => ending || match);
}

function parseJsonMcp(buffer, opts) {
  const o = opts || {};
  const mapKey = o.mapKey || "mcpServers";
  if (buffer == null) return { ok: true, value: { [mapKey]: {} } };
  const len = Buffer.isBuffer(buffer)
    ? buffer.length
    : Buffer.byteLength(String(buffer), "utf8");
  if (len === 0) return { ok: true, value: { [mapKey]: {} } };
  if (len > MAX_CONFIG_BYTES) {
    return { ok: false, reason: JSON_MCP_STATUSES.CONFIG_TOO_LARGE };
  }
  try {
    // BOM-strip before parse: a leading UTF-8 BOM (some Windows shells/editors)
    // makes JSON.parse reject the first value.
    const text = stripBom(Buffer.isBuffer(buffer) ? buffer.toString("utf8") : String(buffer));
    const value = JSON.parse(o.jsonc ? stripJsonc(text) : text);
    if (value == null || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, reason: JSON_MCP_STATUSES.INVALID_JSON };
    }
    if (value[mapKey] == null) value[mapKey] = {};
    if (typeof value[mapKey] !== "object" || Array.isArray(value[mapKey])) {
      return { ok: false, reason: JSON_MCP_STATUSES.INVALID_JSON };
    }
    return { ok: true, value };
  } catch (_e) {
    return { ok: false, reason: JSON_MCP_STATUSES.INVALID_JSON };
  }
}

// True when a server entry launches the Terminal Commander MCP adapter,
// whatever it is named: the stable exe, the node_modules exe, or node running
// the npm shim script.
function launchesTerminalCommander(entry) {
  if (entry == null || typeof entry !== "object") return false;
  const parts = [...(Array.isArray(entry.command) ? entry.command : [entry.command]), ...(Array.isArray(entry.args) ? entry.args : [])];
  return parts.some(
    (p) => typeof p === "string" && /(^|[\\/])terminal-commander-mcp(\.[a-z]+)?$/i.test(p),
  );
}

function mergeJsonMcpServers(existing, serverName, serverConfig, opts) {
  const o = opts || {};
  const mapKey = o.mapKey || "mcpServers";
  const servers = existing[mapKey] || {};
  const wasPresent = Object.prototype.hasOwnProperty.call(servers, serverName);
  // The same adapter registered under another name would be a second TC
  // server next to this one. Force (every install refresh) replaces it; a
  // plain write refuses rather than adding a duplicate.
  const otherNames = Object.keys(servers).filter(
    (name) => name !== serverName && launchesTerminalCommander(servers[name]),
  );
  if ((wasPresent || otherNames.length > 0) && o.force !== true) {
    return { ok: false, reason: JSON_MCP_STATUSES.ALREADY_EXISTS, existing_names: otherNames };
  }
  // Conflicting aliases may carry different restrictions; leave them for explicit repair.
  if (otherNames.length > (wasPresent ? 0 : 1)) return { ok: false, reason: JSON_MCP_STATUSES.UNSUPPORTED };
  const entry = mergeManagedServer(servers[wasPresent ? serverName : otherNames[0]], serverConfig, o);
  if (!entry.ok) return entry;
  const mergedServers = {};
  for (const name of Object.keys(servers)) {
    if (name === serverName || otherNames.includes(name)) continue;
    Object.defineProperty(mergedServers, name, { value: servers[name], enumerable: true, configurable: true, writable: true });
  }
  Object.defineProperty(mergedServers, serverName, { value: entry.value, enumerable: true, configurable: true, writable: true });
  return {
    ok: true,
    value: { ...existing, [mapKey]: mergedServers },
    was_present: wasPresent,
    replaced: otherNames,
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
  const parsed = parseJsonMcp(existingBuf, o);
  if (!parsed.ok) {
    return {
      status: parsed.reason,
      path: target,
      hint: `terminal-commander: invalid JSON at ${target}`,
    };
  }
  const merged = mergeJsonMcpServers(parsed.value, serverName, serverConfig, {
    ...o,
    force: o.force === true,
  });
  if (!merged.ok) {
    const others = merged.existing_names || [];
    return {
      status: merged.reason,
      path: target,
      hint:
        merged.reason === JSON_MCP_STATUSES.UNSUPPORTED
          ? "terminal-commander: remote or conflicting managed entry; file not modified"
          : others.length > 0
          ? `terminal-commander: Terminal Commander is already configured as ${others.join(", ")}; use --force to replace it with ${serverName}`
          : `terminal-commander: entry ${serverName} already exists; use --force`,
    };
  }
  if (o.enableServer === true || merged.replaced.length > 0) {
    for (const key of ["enabledServers", "disabledServers"]) {
      if (key in merged.value && !Array.isArray(merged.value[key])) {
        return {
          status: JSON_MCP_STATUSES.INVALID_JSON,
          path: target,
          hint: `terminal-commander: invalid ${key} at ${target}`,
        };
      }
    }
    for (const key of ["enabledServers", "disabledServers"]) {
      if (Array.isArray(merged.value[key])) {
        merged.value[key] = [...new Set(merged.value[key].map((name) => merged.replaced.includes(name) ? serverName : name))];
      }
    }
  }
  if (o.enableServer === true) {
    if (Array.isArray(merged.value.enabledServers) && !merged.value.enabledServers.includes(serverName)) {
      merged.value.enabledServers.push(serverName);
    }
    if (Array.isArray(merged.value.disabledServers)) {
      merged.value.disabledServers = merged.value.disabledServers.filter((name) => name !== serverName);
    }
  }
  const fileExisted = existingBuf != null;
  const contents = JSON.stringify(merged.value, null, 2) + "\n";
  if (existingBuf != null && existingBuf.toString("utf8") === contents) {
    return { status: JSON_MCP_STATUSES.ALREADY_EXISTS, path: target, hint: "terminal-commander: configuration unchanged" };
  }
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
  const replacedNote =
    merged.replaced.length > 0 ? ` (replaced duplicate entry ${merged.replaced.join(", ")})` : "";
  return {
    status,
    path: target,
    hint: `terminal-commander: ${status.replace(/_/g, " ")} ${target}${replacedNote}`,
  };
}

module.exports = {
  writeJsonMcpConfig,
  parseJsonMcp,
  mergeJsonMcpServers,
  JSON_MCP_STATUSES,
  MAX_CONFIG_BYTES,
  stripJsonc,
};
