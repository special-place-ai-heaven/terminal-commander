// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS05 Cursor mcp.json writer.
//
// Orchestrates load -> parse -> merge -> backup -> atomic-write for
// the `terminal-commander` MCP server stanza. Pure file I/O on the
// resolved Cursor scope directory + its `.bak` and `.tmp.<random>`
// siblings. NO `child_process` import. NO spawn. NO network. NO
// process env reads beyond what `lib/cursor/config.js` does.
//
// Atomic-write contract:
//
//   1. If the target file does not exist -> create parent `.cursor/`
//      directory if missing, write a fresh config.
//   2. If the target file exists ->
//        (a) read + parse + size-check;
//        (b) refuse on `invalid_json` / `config_too_large` /
//            `bad_shape` (typed result; file untouched);
//        (c) `mergeCursorMcpConfig` (refuse `already_exists` without
//            `force: true`);
//        (d) `backupCursorConfig` to `<path>.bak`. Refuse with
//            `backup_failed` if the `.bak` already exists AND
//            `clobber_backup` is not set;
//        (e) write `<path>.tmp.<random>` in the SAME directory, then
//            `renameSync(tmp, path)`.
//   3. Every path touched (final, tmp, bak) is asserted to be inside
//      the resolved scope dir. Violations -> `path_not_allowed`.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const {
  buildTerminalCommanderServerConfig,
  parseExistingCursorConfig,
  validateCursorConfigShape,
  mergeCursorMcpConfig,
  serializeCursorMcpConfig,
  isPathInsideScope,
  getCursorGlobalConfigPath,
  getCursorProjectConfigPath,
  CONFIG_STATUSES,
  MAX_CONFIG_BYTES,
} = require("./config.js");
const { UNSAFE_DISTRO_NAME } = require("../wsl/distro-name.js");

function buildResult(partial) {
  return {
    status: partial.status,
    path: partial.path || null,
    backup_path: partial.backup_path || null,
    server: partial.server || null,
    was_present: partial.was_present === true,
    hint: partial.hint || "",
  };
}

function hintFor(status, p) {
  switch (status) {
    case CONFIG_STATUSES.CONFIG_CREATED:
      return `terminal-commander: created Cursor mcp.json at ${p}`;
    case CONFIG_STATUSES.CONFIG_UPDATED:
      return `terminal-commander: merged terminal-commander entry into existing Cursor mcp.json at ${p}`;
    case CONFIG_STATUSES.ALREADY_EXISTS:
      return "terminal-commander: an existing terminal-commander entry was found in Cursor mcp.json; re-run with force: true to overwrite (always creates .bak first).";
    case CONFIG_STATUSES.INVALID_JSON:
      return "terminal-commander: existing Cursor mcp.json failed JSON parse; the file was NOT modified.";
    case CONFIG_STATUSES.CONFIG_TOO_LARGE:
      return "terminal-commander: existing Cursor mcp.json exceeds the 256 KiB safety cap; the file was NOT modified.";
    case CONFIG_STATUSES.PATH_NOT_ALLOWED:
      return "terminal-commander: refusing to write outside the resolved Cursor scope directory.";
    case CONFIG_STATUSES.PROJECT_ROOT_REQUIRED:
      return "terminal-commander: project-scoped Cursor config requires an explicit projectRoot.";
    case CONFIG_STATUSES.UNSAFE_DISTRO_NAME:
      return "terminal-commander: distro name failed safety whitelist; only ASCII letters, digits, '.', '_' and '-' are allowed (length 1..64).";
    case CONFIG_STATUSES.DISTRO_NOT_FOUND:
      return "terminal-commander: distro was not present in the live detectWsl whitelist; re-run without requireKnownDistro or pick a registered distro.";
    case CONFIG_STATUSES.BACKUP_FAILED:
      return "terminal-commander: failed to create .bak backup; the file was NOT modified.";
    case CONFIG_STATUSES.WRITE_FAILED:
      return "terminal-commander: atomic write step failed; the file may or may not have been modified — check .bak for the previous contents.";
    case CONFIG_STATUSES.UNSUPPORTED_HOST:
      return "terminal-commander: writeCursorMcpConfig is not supported on this host.";
    default:
      return "";
  }
}

/**
 * Copy `path` to `<path>.bak` if `path` exists. Refuses when the
 * `.bak` already exists AND `opts.clobber_backup !== true`.
 *
 * @param {string} target
 * @param {Object} [opts]
 * @param {boolean} [opts.clobber_backup=false]
 * @param {string} [opts.scopeDir]  Optional; defaults to dirname(target).
 * @returns {{ok:true, backup_path:string|null}|{ok:false, reason:string}}
 */
function backupCursorConfig(target, opts) {
  const o = opts || {};
  const scopeDir = o.scopeDir || path.dirname(target);
  if (!fs.existsSync(target)) {
    return { ok: true, backup_path: null };
  }
  const backupPath = target + ".bak";
  if (!isPathInsideScope(scopeDir, backupPath)) {
    return { ok: false, reason: CONFIG_STATUSES.PATH_NOT_ALLOWED };
  }
  if (fs.existsSync(backupPath) && o.clobber_backup !== true) {
    return { ok: false, reason: CONFIG_STATUSES.BACKUP_FAILED };
  }
  try {
    fs.copyFileSync(target, backupPath);
  } catch (_err) {
    return { ok: false, reason: CONFIG_STATUSES.BACKUP_FAILED };
  }
  return { ok: true, backup_path: backupPath };
}

/**
 * Atomic write to `target`. Writes to a `<target>.tmp.<random>` sibling
 * in the SAME directory, fsyncs the tmp file, then renames to `target`.
 * Refuses if `target` is outside `scopeDir`.
 *
 * @param {string} target
 * @param {string} contents
 * @param {Object} [opts]
 * @param {string} [opts.scopeDir]
 * @param {(p:string)=>string} [opts.randomSuffix]  Injected randomness for tests.
 * @returns {{ok:true, path:string}|{ok:false, reason:string}}
 */
function atomicWrite(target, contents, opts) {
  const o = opts || {};
  const scopeDir = o.scopeDir || path.dirname(target);
  if (!isPathInsideScope(scopeDir, target)) {
    return { ok: false, reason: CONFIG_STATUSES.PATH_NOT_ALLOWED };
  }
  const parent = path.dirname(target);
  try {
    fs.mkdirSync(parent, { recursive: true });
  } catch (_err) {
    return { ok: false, reason: CONFIG_STATUSES.WRITE_FAILED };
  }
  const suffix =
    typeof o.randomSuffix === "function"
      ? o.randomSuffix(target)
      : crypto.randomBytes(8).toString("hex");
  const tmp = target + ".tmp." + suffix;
  if (!isPathInsideScope(scopeDir, tmp)) {
    return { ok: false, reason: CONFIG_STATUSES.PATH_NOT_ALLOWED };
  }
  let fd;
  try {
    fd = fs.openSync(tmp, "w", 0o600);
    fs.writeSync(fd, contents);
    try {
      fs.fsyncSync(fd);
    } catch (_e) {
      // fsync may not be supported on some test FS; non-fatal.
    }
    fs.closeSync(fd);
    fd = null;
    fs.renameSync(tmp, target);
  } catch (_err) {
    if (fd != null) {
      try {
        fs.closeSync(fd);
      } catch (_e) {
        /* ignore */
      }
    }
    try {
      if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
    } catch (_e) {
      /* ignore */
    }
    return { ok: false, reason: CONFIG_STATUSES.WRITE_FAILED };
  }
  return { ok: true, path: target };
}

/**
 * Resolve the target Cursor scope (global or project) + its scope
 * directory. Returns a typed error result on unmet preconditions.
 */
function resolveScope(opts) {
  const o = opts || {};
  if (o.scope === "project") {
    if (!o.projectRoot || typeof o.projectRoot !== "string") {
      return { ok: false, reason: CONFIG_STATUSES.PROJECT_ROOT_REQUIRED };
    }
    const p = getCursorProjectConfigPath(o.projectRoot);
    return { ok: true, path: p, scopeDir: path.dirname(p) };
  }
  // Default: global.
  let p;
  try {
    p = getCursorGlobalConfigPath({
      platform: o.platform || process.platform,
      env: o.env || process.env,
    });
  } catch (_err) {
    return { ok: false, reason: CONFIG_STATUSES.UNSUPPORTED_HOST };
  }
  return { ok: true, path: p, scopeDir: path.dirname(p) };
}

/**
 * Write or merge the Cursor MCP config so Cursor launches the
 * `terminal-commander` MCP server via the WWS04 bridge.
 *
 * @param {Object} opts
 * @param {"global"|"project"} [opts.scope="global"]
 * @param {string} [opts.projectRoot]  Required iff scope==='project'.
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @param {string} [opts.distro]  Optional WSL distro to pin in env.
 * @param {ReadonlyArray<{name:string}>} [opts.knownDistros]
 * @param {boolean} [opts.requireKnownDistro=false]
 * @param {boolean} [opts.force=false]
 * @param {boolean} [opts.clobber_backup=false]
 * @param {(p:string)=>string} [opts.randomSuffix]  Test injection.
 * @returns {{status:string, path:string|null, backup_path:string|null, server:object|null, was_present:boolean, hint:string}}
 */
function writeCursorMcpConfig(opts) {
  const o = opts || {};

  // (1) Resolve target path + scope dir.
  const scope = resolveScope(o);
  if (!scope.ok) {
    return buildResult({ status: scope.reason, hint: hintFor(scope.reason) });
  }
  const target = scope.path;
  const scopeDir = scope.scopeDir;

  if (!isPathInsideScope(scopeDir, target)) {
    return buildResult({
      status: CONFIG_STATUSES.PATH_NOT_ALLOWED,
      hint: hintFor(CONFIG_STATUSES.PATH_NOT_ALLOWED),
    });
  }

  // (2) Build the terminal-commander stanza (validates distro
  // before we touch the filesystem).
  let stanza;
  try {
    stanza = buildTerminalCommanderServerConfig({
      sessionToken: o.sessionToken,
      distro: o.distro,
      knownDistros: o.knownDistros,
      requireKnownDistro: o.requireKnownDistro === true,
    });
  } catch (err) {
    if (err && err.code === UNSAFE_DISTRO_NAME) {
      return buildResult({
        status: CONFIG_STATUSES.UNSAFE_DISTRO_NAME,
        path: target,
        hint: hintFor(CONFIG_STATUSES.UNSAFE_DISTRO_NAME),
      });
    }
    if (err && err.code === "DISTRO_NOT_FOUND") {
      return buildResult({
        status: CONFIG_STATUSES.DISTRO_NOT_FOUND,
        path: target,
        hint: hintFor(CONFIG_STATUSES.DISTRO_NOT_FOUND),
      });
    }
    return buildResult({
      status: CONFIG_STATUSES.WRITE_FAILED,
      path: target,
      hint: hintFor(CONFIG_STATUSES.WRITE_FAILED),
    });
  }

  // (3) Load + parse existing config (if any).
  let existingBuf = null;
  if (fs.existsSync(target)) {
    let stat;
    try {
      stat = fs.statSync(target);
    } catch (_err) {
      return buildResult({
        status: CONFIG_STATUSES.WRITE_FAILED,
        path: target,
        hint: hintFor(CONFIG_STATUSES.WRITE_FAILED),
      });
    }
    if (stat.size > MAX_CONFIG_BYTES) {
      return buildResult({
        status: CONFIG_STATUSES.CONFIG_TOO_LARGE,
        path: target,
        hint: hintFor(CONFIG_STATUSES.CONFIG_TOO_LARGE),
      });
    }
    try {
      existingBuf = fs.readFileSync(target);
    } catch (_err) {
      return buildResult({
        status: CONFIG_STATUSES.WRITE_FAILED,
        path: target,
        hint: hintFor(CONFIG_STATUSES.WRITE_FAILED),
      });
    }
  }
  const parsed = parseExistingCursorConfig(existingBuf);
  if (!parsed.ok) {
    const status =
      parsed.reason === CONFIG_STATUSES.CONFIG_TOO_LARGE
        ? CONFIG_STATUSES.CONFIG_TOO_LARGE
        : CONFIG_STATUSES.INVALID_JSON;
    return buildResult({
      status,
      path: target,
      hint: hintFor(status),
    });
  }

  const fileExisted = existingBuf != null;

  // (4) Merge.
  const merged = mergeCursorMcpConfig(parsed.value, stanza, {
    force: o.force === true,
  });
  if (!merged.ok) {
    if (merged.reason === CONFIG_STATUSES.ALREADY_EXISTS) {
      return buildResult({
        status: CONFIG_STATUSES.ALREADY_EXISTS,
        path: target,
        server: stanza,
        was_present: true,
        hint: hintFor(CONFIG_STATUSES.ALREADY_EXISTS),
      });
    }
    return buildResult({
      status: CONFIG_STATUSES.WRITE_FAILED,
      path: target,
      hint: hintFor(CONFIG_STATUSES.WRITE_FAILED),
    });
  }
  if (!validateCursorConfigShape(merged.value)) {
    return buildResult({
      status: CONFIG_STATUSES.WRITE_FAILED,
      path: target,
      hint: hintFor(CONFIG_STATUSES.WRITE_FAILED),
    });
  }

  // (5) Backup before overwrite (only if target existed).
  let backupPath = null;
  if (fileExisted) {
    const backup = backupCursorConfig(target, {
      scopeDir,
      clobber_backup: o.clobber_backup === true,
    });
    if (!backup.ok) {
      return buildResult({
        status: backup.reason,
        path: target,
        hint: hintFor(backup.reason),
      });
    }
    backupPath = backup.backup_path;
  }

  // (6) Atomic write.
  const contents = serializeCursorMcpConfig(merged.value);
  const wrote = atomicWrite(target, contents, {
    scopeDir,
    randomSuffix: o.randomSuffix,
  });
  if (!wrote.ok) {
    return buildResult({
      status: wrote.reason,
      path: target,
      backup_path: backupPath,
      hint: hintFor(wrote.reason),
    });
  }

  const finalStatus = fileExisted
    ? CONFIG_STATUSES.CONFIG_UPDATED
    : CONFIG_STATUSES.CONFIG_CREATED;
  return buildResult({
    status: finalStatus,
    path: target,
    backup_path: backupPath,
    server: stanza,
    was_present: merged.was_present === true,
    hint: hintFor(finalStatus, target),
  });
}

module.exports = {
  writeCursorMcpConfig,
  backupCursorConfig,
  atomicWrite,
  resolveScope,
};
