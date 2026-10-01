// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Scoped TOML writer for Codex/Grok [mcp_servers.terminal_commander] blocks,
// including the [mcp_servers.terminal_commander.env] sub-table (TC_SESSION /
// TC_SURFACE / TC_WSL_DISTRO). Section-scoped merge only; does not parse a
// full TOML AST.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { atomicWriteWithBackup, ATOMIC_REASONS, stripBom } = require("./atomic.js");
const {
  buildTerminalCommanderCommandConfig,
  buildHarnessEnv,
} = require("../../cursor/config.js");

const SECTION_HEADER = "[mcp_servers.terminal_commander]";
const ENV_SECTION_HEADER = "[mcp_servers.terminal_commander.env]";
const SERVER_NAME = "terminal_commander";
// Added only when creating a managed entry.
const BLOCK_COMMENT =
  "# Terminal Commander MCP stdio adapter (merged by terminal-commander bootstrap).";
const MAX_CONFIG_BYTES = 256 * 1024;

const TOML_MCP_STATUSES = Object.freeze({
  CONFIG_CREATED: "config_created",
  CONFIG_UPDATED: "config_updated",
  ALREADY_EXISTS: "already_exists",
  CONFIG_TOO_LARGE: "config_too_large",
  INVALID_TOML: "invalid_toml",
  UNSUPPORTED: "unsupported",
  BACKUP_FAILED: "backup_failed",
  WRITE_FAILED: "write_failed",
});

/**
 * Conservative malformed-TOML guard. We ship no TOML AST parser (zero-dep), so
 * this only flags CLEAR corruption that the section-scoped text merge below
 * would otherwise silently mangle: a non-blank, non-comment line that opens a
 * table header with `[` but never closes it on the same line (e.g. a truncated
 * `[mcp_servers.terminal_commander` from an aborted write). It deliberately
 * does NOT attempt full validation — a false "malformed" would wrongly refuse a
 * valid config. When this returns true the caller leaves the file untouched and
 * reports INVALID_TOML, mirroring the JSON writer's malformed-safe behavior.
 *
 * @param {string} text
 * @returns {boolean}
 */
function isLikelyMalformedToml(text) {
  if (typeof text !== "string") return false;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.length === 0 || line.startsWith("#")) continue;
    // A table/array-of-tables header line starts with '['. If it starts with
    // '[' it must also contain a matching ']'; otherwise it is a broken header.
    if (line.startsWith("[") && !line.includes("]")) {
      return true;
    }
  }
  return false;
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
  return {
    ...buildHarnessEnv({
      sessionToken: o.sessionToken,
      surface: o.surface,
      distro: o.distro,
      gateDistroOnWin32: true,
      platform: o.platform,
    }),
    ...(o.providerId === "grok" || o.codexDefaults === false
      ? {}
      : { CODEX_MCP_PROTOCOL_VERSION: "2026-07-28" }),
  };
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
  if (o.providerId !== "grok" && o.codexDefaults !== false) {
    lines.push("required = true", "startup_timeout_sec = 60");
  }
  // Emit the env sub-table when there are values. Keys are bare TOML keys;
  // values are TOML basic strings (JSON.stringify is a valid TOML basic-string
  // encoder for the [A-Za-z0-9._-] + compact|full value charset these keys carry).
  const env = buildCodexEnv(o);
  const envKeys = Object.keys(env);
  if (envKeys.length > 0) {
    lines.push("", ENV_SECTION_HEADER);
    for (const key of envKeys) {
      lines.push(`${key} = ${JSON.stringify(env[key])}`);
    }
  }
  return lines.join("\n") + "\n";
}

// Split at statement boundaries, keeping comments and multiline values intact.
// ponytail: this is a scoped editor, not a full TOML validator; unsupported
// managed key layouts are refused instead of being rewritten speculatively.
function tomlStatements(text) {
  const statements = [];
  let start = 0, quote = null, triple = false, comment = false, depth = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (comment) {
      if (c !== "\n") continue;
      comment = false;
    } else if (quote) {
      if (quote === '"' && c === "\\") { i++; continue; }
      if (c === quote && (!triple || text.slice(i, i + 3) === quote.repeat(3))) {
        if (triple) i += 2;
        quote = null;
      } else if (c === "\n" && !triple) {
        throw new Error("invalid_toml");
      }
      continue;
    } else if (c === "#") {
      comment = true;
      continue;
    } else if (c === '"' || c === "'") {
      quote = c;
      triple = text.slice(i, i + 3) === c.repeat(3);
      if (triple) i += 2;
      continue;
    } else if (c === "[" || c === "{") {
      depth++;
    } else if (c === "]" || c === "}") {
      if (--depth < 0) throw new Error("invalid_toml");
    }
    if (c === "\n" && depth === 0) {
      statements.push(text.slice(start, i + 1));
      start = i + 1;
    }
  }
  if (quote || depth) throw new Error("invalid_toml");
  if (start < text.length) statements.push(text.slice(start));
  return statements;
}

function tomlKeyPath(key) {
  const token = /(?:[A-Za-z0-9_-]+|"(?:[^"\\]|\\.)*"|'[^']*')/g;
  const parts = key.match(token);
  if (!parts || !/^\s*K\s*(?:\.\s*K\s*)*$/.test(key.replace(token, "K"))) {
    throw new Error("unsupported");
  }
  return parts.map((part) => part[0] === '"' ? JSON.parse(part) : part[0] === "'" ? part.slice(1, -1) : part);
}

function refreshTomlEntry(text, opts) {
  const statements = tomlStatements(text);
  const sections = [];
  let current = { name: "", keys: new Map() };
  const isManaged = (name) => name === "mcp_servers.terminal_commander" || name.startsWith("mcp_servers.terminal_commander.");
  sections.push(current);
  for (let i = 0; i < statements.length; i++) {
    const line = statements[i].trim();
    if (!line || line.startsWith("#")) continue;
    const header = line.match(/^(\[\[?)([^\n]+?)(\]\]?)\s*(?:#.*)?$/);
    if (header) {
      const name = tomlKeyPath(header[2]).map((part) => part.includes(".") ? JSON.stringify(part) : part).join(".");
      if (header[1].length !== header[3].length) throw new Error("invalid_toml");
      if (header[1].length === 2 && (name === "mcp_servers" || isManaged(name))) throw new Error("unsupported");
      current.end = i;
      current = { name, keys: new Map() };
      sections.push(current);
      continue;
    }
    const assignment = line.match(/^((?:[^="'\n]|"(?:[^"\\]|\\.)*"|'[^']*')+)\s*=/);
    if (!assignment) {
      throw new Error("invalid_toml");
    }
    if (!line.slice(assignment[0].length).trim() || line.slice(assignment[0].length).trim().startsWith("#")) throw new Error("invalid_toml");
    const keys = tomlKeyPath(assignment[1].trim());
    if ((current.name === "" && keys[0] === "mcp_servers" &&
        (keys.length === 1 || keys[1] === SERVER_NAME)) ||
        (current.name === "mcp_servers" && keys[0] === SERVER_NAME)) throw new Error("unsupported");
    if (current.name === "mcp_servers.terminal_commander" || current.name === "mcp_servers.terminal_commander.env") {
      if (keys.length !== 1 || current.keys.has(keys[0])) throw new Error("unsupported");
      current.keys.set(keys[0], i);
    }
  }
  current.end = statements.length;
  const mains = sections.filter((section) => section.name === "mcp_servers.terminal_commander");
  const envs = sections.filter((section) => section.name === "mcp_servers.terminal_commander.env");
  if (mains.length > 1 || envs.length > 1) throw new Error("unsupported");
  if (!mains.length) {
    if (envs.length || sections.some((section) => section.name.startsWith("mcp_servers.terminal_commander."))) throw new Error("unsupported");
    return { exists: false, text: text.trimEnd() + (text.trim() ? "\n\n" : "") + buildCodexTomlBlock(opts) };
  }
  const main = mains[0];
  if (["url", "httpUrl", "env"].some((key) => main.keys.has(key))) throw new Error("unsupported");
  if (main.keys.has("type") && !/^\s*type\s*=\s*['"]stdio['"]\s*(?:#.*)?$/.test(statements[main.keys.get("type")].trim())) throw new Error("unsupported");
  if (opts.force !== true) return { exists: true, text };
  const inserts = new Map();
  function update(section, key, value) {
    if (section.keys.has(key)) {
      statements[section.keys.get(key)] = value === null ? "" : `${key} = ${JSON.stringify(value)}\n`;
    } else if (value !== null) {
      inserts.set(section.end, (inserts.get(section.end) || "") + `${key} = ${JSON.stringify(value)}\n`);
    }
  }
  const launch = buildTerminalCommanderCommandConfig(opts);
  update(main, "command", launch.command);
  update(main, "args", launch.args);
  if (opts.providerId !== "grok" && opts.codexDefaults !== false) {
    if (!main.keys.has("required")) update(main, "required", true);
    if (!main.keys.has("startup_timeout_sec")) update(main, "startup_timeout_sec", 60);
  }
  const env = buildCodexEnv(opts);
  if (envs.length) {
    const section = envs[0];
    if (!Object.hasOwn(env, "TC_WSL_DISTRO")) update(section, "TC_WSL_DISTRO", null);
    for (const [key, value] of Object.entries(env)) update(section, key, value);
  } else if (Object.keys(env).length) {
    statements.push(`\n${ENV_SECTION_HEADER}\n` + Object.entries(env).map(([key, value]) => `${key} = ${JSON.stringify(value)}\n`).join(""));
  }
  let merged = "";
  for (let i = 0; i <= statements.length; i++) {
    if (inserts.has(i)) merged += (merged && !merged.endsWith("\n") ? "\n" : "") + inserts.get(i);
    if (i < statements.length) merged += statements[i];
  }
  return { exists: true, text: merged };
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
      existing = stripBom(fs.readFileSync(target, "utf8"));
    } catch (_e) {
      return { status: TOML_MCP_STATUSES.WRITE_FAILED, path: target, hint: "" };
    }
  }
  let refreshed;
  try {
    refreshed = refreshTomlEntry(existing, o);
  } catch (error) {
    return {
      status: error.message === "invalid_toml" ? TOML_MCP_STATUSES.INVALID_TOML : TOML_MCP_STATUSES.UNSUPPORTED,
      path: target,
      hint: "terminal-commander: unsupported or malformed MCP TOML configuration; file not modified",
    };
  }
  if (refreshed.exists && o.force !== true) {
    return { status: TOML_MCP_STATUSES.ALREADY_EXISTS, path: target, hint: "terminal-commander: managed server already configured; use --force" };
  }
  const merged = refreshed.text;
  if (fileExisted && merged === existing) {
    return { status: TOML_MCP_STATUSES.ALREADY_EXISTS, path: target, server_name: SERVER_NAME, hint: "terminal-commander: managed server already configured" };
  }
  // Shared atomic write-with-backup: brings this writer to fsync + tmp/.bak
  // scope-check parity with the JSON and Cursor writers (it previously skipped
  // both). Backup-of-existing is handled inside the helper.
  const wrote = atomicWriteWithBackup(target, merged, {
    scopeDir,
    clobber_backup: o.clobber_backup === true,
    randomSuffix: o.randomSuffix,
  });
  if (!wrote.ok) {
    // toml has no PATH_NOT_ALLOWED status; collapse it into WRITE_FAILED.
    const status =
      wrote.reason === ATOMIC_REASONS.PATH_NOT_ALLOWED
        ? TOML_MCP_STATUSES.WRITE_FAILED
        : wrote.reason;
    return { status, path: target, hint: "" };
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
  isLikelyMalformedToml,
  TOML_MCP_STATUSES,
  SECTION_HEADER,
  ENV_SECTION_HEADER,
  SERVER_NAME,
};
