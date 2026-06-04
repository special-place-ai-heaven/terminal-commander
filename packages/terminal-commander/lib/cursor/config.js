// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS05 Cursor MCP config helpers (pure functions; no I/O).
//
// Cursor reads MCP server configs from `~/.cursor/mcp.json` (global)
// or `<workspace>/.cursor/mcp.json` (project). This module ships the
// pure stanza-builder + path-resolver + merge helpers consumed by
// `lib/cursor/write.js`. NO `child_process`. NO spawn. NO network.
// NO file I/O — every helper here is pure and operates on already-
// loaded JSON / strings.
//
// Default generated stanza targets the WWS04 bridge shim:
//
//   {
//     "mcpServers": {
//       "terminal-commander": {
//         "type": "stdio",
//         "command": "terminal-commander-mcp"
//       }
//     }
//   }
//
// On Windows the operator MAY optionally pin a specific WSL distro by
// passing `opts.distro` to `buildTerminalCommanderServerConfig` — the
// resulting stanza adds `"env": { "TC_WSL_DISTRO": "<distro>" }`. The
// distro name is validated by `assertSafeDistroName` (WWS03) before
// it can land in the config. No other env key is ever emitted; no
// secrets / tokens / credentials are written.
//
// The legacy `wsl.exe`-direct stanza (`{ "command": "wsl", "args":
// ["-d", "<distro>", "bash", "-lc", "terminal-commander-mcp"] }`) is
// documented as a manual fallback in `docs/integrations/cursor.md`
// §6 and in `examples/provider-harness/cursor/mcp.global.linux-wsl.json`
// — this writer does NOT generate it.

"use strict";

const path = require("node:path");
const {
  isSafeDistroName,
  assertSafeDistroName,
  UNSAFE_DISTRO_NAME,
} = require("../wsl/distro-name.js");
const { isValidSessionToken } = require("../session/mint.js");

/** Error code thrown when a TC_SESSION token fails the safety whitelist. */
const UNSAFE_SESSION_TOKEN = "UNSAFE_SESSION_TOKEN";

const SERVER_NAME = "terminal-commander";
const SERVER_COMMAND = "terminal-commander-mcp";
const SERVER_TYPE = "stdio";

const CONFIG_FILENAME = "mcp.json";
const CONFIG_DIRNAME = ".cursor";

const MAX_CONFIG_BYTES = 256 * 1024;

const CONFIG_STATUSES = Object.freeze({
  CONFIG_CREATED: "config_created",
  CONFIG_UPDATED: "config_updated",
  ALREADY_EXISTS: "already_exists",
  INVALID_JSON: "invalid_json",
  CONFIG_TOO_LARGE: "config_too_large",
  PATH_NOT_ALLOWED: "path_not_allowed",
  PROJECT_ROOT_REQUIRED: "project_root_required",
  UNSAFE_DISTRO_NAME: "unsafe_distro_name",
  DISTRO_NOT_FOUND: "distro_not_found",
  BACKUP_FAILED: "backup_failed",
  WRITE_FAILED: "write_failed",
  UNSUPPORTED_HOST: "unsupported_host",
});

function defaultMcpScriptPath() {
  return path.join(__dirname, "..", "..", "bin", "terminal-commander-mcp.js");
}

function buildTerminalCommanderCommandConfig(opts) {
  const o = opts || {};
  // Highest precedence: a resolved native exe at a STABLE per-user path.
  // The MCP client launches it directly (no npm script-launcher shim, no
  // node hop), which removes the script-interpreter-then-spawn chain that
  // heuristic AV reads as a loader. The path is a fixed per-user dir the
  // package owns (e.g. %LOCALAPPDATA%\terminal-commander\bin\...), NOT the
  // hoist-prone node_modules path, so npm updates never silently break it.
  if (o.exePath) {
    return {
      command: o.exePath,
      args: [],
    };
  }
  // Explicit node+script overrides (used by callers that know the exact
  // installed paths) -> emit the node.exe + JS-shim form.
  if (o.scriptPath || o.nodePath) {
    return {
      command: o.nodePath || process.execPath,
      args: [o.scriptPath || defaultMcpScriptPath()],
    };
  }
  // Default: emit the PATH-resolved command form. The installed package
  // puts `terminal-commander-mcp` on PATH, so the generated config is
  // portable and never leaks this machine's absolute checkout path
  // (which would embed a private \Users\ path into a shipped config).
  return {
    command: SERVER_COMMAND,
    args: [],
  };
}

/**
 * Derive the Cursor global mcp.json path from injected platform +
 * env. Never hardcodes a username; uses standard env vars only.
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @returns {string} absolute path
 */
function getCursorGlobalConfigPath(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;

  let home;
  if (platform === "win32") {
    // Cursor on Windows reads %USERPROFILE%\.cursor\mcp.json.
    home = env.USERPROFILE;
    if (!home || home.length === 0) {
      throw new Error(
        "terminal-commander: USERPROFILE not set; cannot derive Cursor global config path",
      );
    }
  } else {
    home = env.HOME;
    if (!home || home.length === 0) {
      throw new Error(
        "terminal-commander: HOME not set; cannot derive Cursor global config path",
      );
    }
  }
  return path.join(home, CONFIG_DIRNAME, CONFIG_FILENAME);
}

/**
 * Project-scoped Cursor config path. Requires explicit projectRoot.
 *
 * @param {string} projectRoot
 * @returns {string} absolute path
 */
function getCursorProjectConfigPath(projectRoot) {
  if (typeof projectRoot !== "string" || projectRoot.length === 0) {
    const err = new Error(
      "terminal-commander: project_root is required for project-scoped Cursor config",
    );
    err.code = "PROJECT_ROOT_REQUIRED";
    throw err;
  }
  return path.join(projectRoot, CONFIG_DIRNAME, CONFIG_FILENAME);
}

/**
 * Build the terminal-commander MCP server stanza for Cursor.
 *
 * @param {Object} [opts]
 * @param {string} [opts.sessionToken]  Optional per-harness TC_SESSION token.
 *     When supplied, must satisfy isValidSessionToken (mirror of the Rust
 *     resolver) or this throws — a malformed token must never be written into
 *     a kernel-object / socket-path name.
 * @param {string} [opts.distro]   Optional. When supplied, must be safe.
 * @param {ReadonlyArray<{name:string}>} [opts.knownDistros]  Required
 *     iff `opts.requireKnownDistro === true`.
 * @param {boolean} [opts.requireKnownDistro=false]
 * @returns {{ type:string, command:string, args:string[], env?: { TC_SESSION?:string, TC_WSL_DISTRO?:string } }}
 * @throws {Error} with `.code` `UNSAFE_DISTRO_NAME`, `DISTRO_NOT_FOUND`, or
 *     `UNSAFE_SESSION_TOKEN`.
 */
function buildTerminalCommanderServerConfig(opts) {
  const o = opts || {};
  const commandConfig = buildTerminalCommanderCommandConfig(o);
  const stanza = {
    type: SERVER_TYPE,
    command: commandConfig.command,
    args: commandConfig.args,
  };
  const env = {};
  if (o.sessionToken != null && o.sessionToken !== "") {
    if (!isValidSessionToken(o.sessionToken)) {
      const err = new Error(
        "terminal-commander: TC_SESSION token failed safety whitelist; only [A-Za-z0-9._-] (1..64, at least one alphanumeric, not dot-only) is allowed",
      );
      err.code = UNSAFE_SESSION_TOKEN;
      throw err;
    }
    env.TC_SESSION = o.sessionToken;
  }
  if (o.distro != null && o.distro !== "") {
    // Distro safety: WWS03 whitelist + optional live whitelist
    // membership.
    assertSafeDistroName(o.distro);
    if (o.requireKnownDistro === true) {
      const known =
        Array.isArray(o.knownDistros) &&
        o.knownDistros.some((d) => d && d.name === o.distro);
      if (!known) {
        const err = new Error(
          `terminal-commander: distro '${o.distro}' not found in detectWsl whitelist`,
        );
        err.code = "DISTRO_NOT_FOUND";
        throw err;
      }
    }
    env.TC_WSL_DISTRO = o.distro;
  }
  if (Object.keys(env).length > 0) {
    stanza.env = env;
  }
  return stanza;
}

/**
 * Parse an existing Cursor mcp.json buffer.
 *
 * @param {Buffer|string} buffer
 * @returns {{ok:true, value:object}|{ok:false, reason:string}}
 */
function parseExistingCursorConfig(buffer) {
  if (buffer == null) {
    return { ok: true, value: { mcpServers: {} } };
  }
  const len = Buffer.isBuffer(buffer) ? buffer.length : Buffer.byteLength(String(buffer), "utf8");
  if (len === 0) {
    return { ok: true, value: { mcpServers: {} } };
  }
  if (len > MAX_CONFIG_BYTES) {
    return { ok: false, reason: CONFIG_STATUSES.CONFIG_TOO_LARGE };
  }
  let text;
  if (Buffer.isBuffer(buffer)) {
    text = buffer.toString("utf8");
  } else {
    text = String(buffer);
  }
  let value;
  try {
    value = JSON.parse(text);
  } catch (_e) {
    return { ok: false, reason: CONFIG_STATUSES.INVALID_JSON };
  }
  if (value == null || typeof value !== "object" || Array.isArray(value)) {
    return { ok: false, reason: "bad_shape" };
  }
  if (value.mcpServers == null) {
    value.mcpServers = {};
  } else if (typeof value.mcpServers !== "object" || Array.isArray(value.mcpServers)) {
    return { ok: false, reason: "bad_shape" };
  }
  return { ok: true, value };
}

/**
 * Sanity-validate the merged config shape before write.
 *
 * @param {object} obj
 * @returns {boolean}
 */
function validateCursorConfigShape(obj) {
  if (!obj || typeof obj !== "object" || Array.isArray(obj)) return false;
  if (!obj.mcpServers || typeof obj.mcpServers !== "object" || Array.isArray(obj.mcpServers)) {
    return false;
  }
  for (const name of Object.keys(obj.mcpServers)) {
    const s = obj.mcpServers[name];
    if (!s || typeof s !== "object" || Array.isArray(s)) return false;
    if (typeof s.command !== "string" || s.command.length === 0) return false;
  }
  return true;
}

/**
 * Merge a terminal-commander stanza into an existing Cursor mcp.json
 * object. Preserves every other mcpServers entry untouched. Refuses
 * to overwrite an existing `terminal-commander` entry unless
 * `opts.force === true`.
 *
 * @param {object} existing  Parsed config (must have `mcpServers`).
 * @param {object} serverConfig  Stanza from buildTerminalCommanderServerConfig.
 * @param {Object} [opts]
 * @param {boolean} [opts.force=false]
 * @returns {{ok:true, value:object, mutated:boolean, was_present:boolean}|{ok:false, reason:string}}
 */
function mergeCursorMcpConfig(existing, serverConfig, opts) {
  const o = opts || {};
  if (!existing || typeof existing !== "object" || Array.isArray(existing)) {
    return { ok: false, reason: "bad_shape" };
  }
  if (!existing.mcpServers || typeof existing.mcpServers !== "object" || Array.isArray(existing.mcpServers)) {
    return { ok: false, reason: "bad_shape" };
  }
  const wasPresent = Object.prototype.hasOwnProperty.call(
    existing.mcpServers,
    SERVER_NAME,
  );
  if (wasPresent && o.force !== true) {
    return { ok: false, reason: CONFIG_STATUSES.ALREADY_EXISTS };
  }
  // Build the merged shape WITHOUT mutating `existing` (callers may
  // want to compare before/after).
  const mergedServers = {};
  for (const name of Object.keys(existing.mcpServers)) {
    if (name === SERVER_NAME) continue; // we are about to overwrite
    mergedServers[name] = existing.mcpServers[name];
  }
  mergedServers[SERVER_NAME] = serverConfig;
  const merged = { ...existing, mcpServers: mergedServers };
  return { ok: true, value: merged, mutated: true, was_present: wasPresent };
}

/**
 * Serialize a Cursor mcp.json object back to a UTF-8 string. Pretty-
 * printed with 2-space indent + trailing newline for diff readability.
 *
 * @param {object} obj
 * @returns {string}
 */
function serializeCursorMcpConfig(obj) {
  return JSON.stringify(obj, null, 2) + "\n";
}

/**
 * Ensure a path is inside an allowed scope directory. Returns true
 * iff `child` resolves to a path under `scopeDir` (or equal to it).
 *
 * @param {string} scopeDir
 * @param {string} child
 * @returns {boolean}
 */
function isPathInsideScope(scopeDir, child) {
  if (typeof scopeDir !== "string" || typeof child !== "string") return false;
  const sep = path.sep;
  const absScope = path.resolve(scopeDir);
  const absChild = path.resolve(child);
  if (absChild === absScope) return true;
  return absChild.startsWith(absScope + sep);
}

module.exports = {
  // pure helpers
  getCursorGlobalConfigPath,
  getCursorProjectConfigPath,
  buildTerminalCommanderCommandConfig,
  buildTerminalCommanderServerConfig,
  parseExistingCursorConfig,
  validateCursorConfigShape,
  mergeCursorMcpConfig,
  serializeCursorMcpConfig,
  isPathInsideScope,
  // constants
  SERVER_NAME,
  SERVER_COMMAND,
  SERVER_TYPE,
  CONFIG_FILENAME,
  CONFIG_DIRNAME,
  MAX_CONFIG_BYTES,
  CONFIG_STATUSES,
  // re-export distro safety predicates for tests
  isSafeDistroName,
  assertSafeDistroName,
  UNSAFE_DISTRO_NAME,
  // session token safety
  isValidSessionToken,
  UNSAFE_SESSION_TOKEN,
};
