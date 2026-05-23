// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// WWS06 setup-state writer.
//
// Reads / writes bounded JSON state files under
// `%LOCALAPPDATA%\terminal-commander\` on Windows. Two files:
//
//   setup.json:
//     {
//       "schema_version": 1,
//       "distro": "<safe distro name>",
//       "cursor_scope": "global" | "project",
//       "created_at": "<iso8601>",
//       "updated_at": "<iso8601>"
//     }
//
//   pair.json:
//     {
//       "schema_version": 1,
//       "pair_id": "<uuid v7>",
//       "code": "<6-digit string>",
//       "created_at": "<iso8601>",
//       "accepted_at": "<iso8601|null>",
//       "distro": "<safe distro name|null>"
//     }
//
// NO secrets. NO tokens. NO passwords. NO env values. NO command
// history. NO private paths. Atomic write via same-directory
// `.tmp.<random>` + `renameSync`. Every path the writer touches is
// asserted to be inside the resolved scope dir.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const SCHEMA_VERSION = 1;
const MAX_STATE_BYTES = 64 * 1024;
const SETUP_FILENAME = "setup.json";
const PAIR_FILENAME = "pair.json";

const STATE_STATUSES = Object.freeze({
  OK: "ok",
  PATH_NOT_ALLOWED: "path_not_allowed",
  INVALID_JSON: "invalid_json",
  CONFIG_TOO_LARGE: "config_too_large",
  WRITE_FAILED: "write_failed",
  UNSUPPORTED_HOST: "unsupported_host",
});

// Token-shaped keys the writer must refuse to serialize even if the
// caller tries to sneak them in.
const FORBIDDEN_STATE_KEY_PATTERNS = Object.freeze([
  /_TOKEN$/i,
  /_SECRET$/i,
  /_PASSWORD$/i,
  /_PASS$/i,
  /_API_KEY$/i,
  /_APIKEY$/i,
  /^password$/i,
  /^credential$/i,
  /^credentials$/i,
]);

function isForbiddenStateKey(key) {
  if (typeof key !== "string") return true;
  for (const re of FORBIDDEN_STATE_KEY_PATTERNS) {
    if (re.test(key)) return true;
  }
  return false;
}

/**
 * Derive the Windows-side state directory from injected env.
 *   Windows: %LOCALAPPDATA%\terminal-commander\
 *   Linux/test fallback: $XDG_STATE_HOME/terminal-commander/ OR
 *                        $HOME/.local/state/terminal-commander/
 *
 * @param {Object} [opts]
 * @param {string} [opts.platform=process.platform]
 * @param {NodeJS.ProcessEnv} [opts.env=process.env]
 * @returns {string} absolute directory path
 */
function getStateDir(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const env = o.env || process.env;
  if (platform === "win32") {
    const root = env.LOCALAPPDATA;
    if (!root || root.length === 0) {
      const err = new Error(
        "terminal-commander: LOCALAPPDATA not set; cannot derive state directory",
      );
      err.code = "UNSUPPORTED_HOST";
      throw err;
    }
    return path.join(root, "terminal-commander");
  }
  // Linux / test fallback.
  const xdg = env.XDG_STATE_HOME;
  if (xdg && xdg.length > 0) {
    return path.join(xdg, "terminal-commander");
  }
  const home = env.HOME;
  if (!home || home.length === 0) {
    const err = new Error(
      "terminal-commander: neither LOCALAPPDATA nor HOME is set",
    );
    err.code = "UNSUPPORTED_HOST";
    throw err;
  }
  return path.join(home, ".local", "state", "terminal-commander");
}

function isPathInsideScope(scopeDir, child) {
  if (typeof scopeDir !== "string" || typeof child !== "string") return false;
  const sep = path.sep;
  const abs = path.resolve(scopeDir);
  const target = path.resolve(child);
  if (target === abs) return true;
  return target.startsWith(abs + sep);
}

function atomicWriteJson(target, obj, opts) {
  const o = opts || {};
  const scopeDir = o.scopeDir || path.dirname(target);
  if (!isPathInsideScope(scopeDir, target)) {
    return { ok: false, reason: STATE_STATUSES.PATH_NOT_ALLOWED };
  }
  // Refuse any forbidden key (defensive; callers should not pass them).
  for (const key of Object.keys(obj || {})) {
    if (isForbiddenStateKey(key)) {
      return { ok: false, reason: STATE_STATUSES.WRITE_FAILED };
    }
  }
  try {
    fs.mkdirSync(path.dirname(target), { recursive: true });
  } catch (_e) {
    return { ok: false, reason: STATE_STATUSES.WRITE_FAILED };
  }
  const suffix =
    typeof o.randomSuffix === "function"
      ? o.randomSuffix(target)
      : crypto.randomBytes(8).toString("hex");
  const tmp = target + ".tmp." + suffix;
  if (!isPathInsideScope(scopeDir, tmp)) {
    return { ok: false, reason: STATE_STATUSES.PATH_NOT_ALLOWED };
  }
  const contents = JSON.stringify(obj, null, 2) + "\n";
  let fd;
  try {
    fd = fs.openSync(tmp, "w", 0o600);
    fs.writeSync(fd, contents);
    try {
      fs.fsyncSync(fd);
    } catch (_e) {
      /* fsync optional */
    }
    fs.closeSync(fd);
    fd = null;
    fs.renameSync(tmp, target);
  } catch (_e) {
    if (fd != null) {
      try {
        fs.closeSync(fd);
      } catch (_ee) {
        /* ignore */
      }
    }
    try {
      if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
    } catch (_ee) {
      /* ignore */
    }
    return { ok: false, reason: STATE_STATUSES.WRITE_FAILED };
  }
  return { ok: true, path: target };
}

function readJsonIfExists(target) {
  if (!fs.existsSync(target)) {
    return { ok: true, value: null };
  }
  let stat;
  try {
    stat = fs.statSync(target);
  } catch (_e) {
    return { ok: false, reason: STATE_STATUSES.WRITE_FAILED };
  }
  if (stat.size > MAX_STATE_BYTES) {
    return { ok: false, reason: STATE_STATUSES.CONFIG_TOO_LARGE };
  }
  let buf;
  try {
    buf = fs.readFileSync(target);
  } catch (_e) {
    return { ok: false, reason: STATE_STATUSES.WRITE_FAILED };
  }
  if (buf.length === 0) {
    return { ok: true, value: null };
  }
  try {
    const value = JSON.parse(buf.toString("utf8"));
    if (value == null || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, reason: STATE_STATUSES.INVALID_JSON };
    }
    return { ok: true, value };
  } catch (_e) {
    return { ok: false, reason: STATE_STATUSES.INVALID_JSON };
  }
}

function writeSetupJson(opts) {
  const o = opts || {};
  const stateDir = o.stateDir || getStateDir(o);
  const target = path.join(stateDir, SETUP_FILENAME);
  const now = (typeof o.now === "function" ? o.now() : new Date()).toISOString();
  const prior = readJsonIfExists(target);
  let created_at = now;
  if (prior.ok && prior.value && typeof prior.value.created_at === "string") {
    created_at = prior.value.created_at;
  }
  const payload = {
    schema_version: SCHEMA_VERSION,
    distro: o.distro || null,
    cursor_scope: o.cursor_scope || "global",
    created_at,
    updated_at: now,
  };
  if (Array.isArray(o.providers_configured)) {
    payload.providers_configured = o.providers_configured.slice();
  } else if (prior.ok && prior.value && Array.isArray(prior.value.providers_configured)) {
    payload.providers_configured = prior.value.providers_configured.slice();
  }
  if (typeof o.bootstrap_at === "string" && o.bootstrap_at.length > 0) {
    payload.bootstrap_at = o.bootstrap_at;
  } else if (prior.ok && prior.value && typeof prior.value.bootstrap_at === "string") {
    payload.bootstrap_at = prior.value.bootstrap_at;
  }
  if (typeof o.bootstrap_mode === "string" && o.bootstrap_mode.length > 0) {
    payload.bootstrap_mode = o.bootstrap_mode;
  }
  const w = atomicWriteJson(target, payload, {
    scopeDir: stateDir,
    randomSuffix: o.randomSuffix,
  });
  if (!w.ok) return { status: w.reason, path: target };
  return { status: STATE_STATUSES.OK, path: target, value: payload };
}

function readSetupJson(opts) {
  const o = opts || {};
  const stateDir = o.stateDir || getStateDir(o);
  const target = path.join(stateDir, SETUP_FILENAME);
  return readJsonIfExists(target);
}

function writePairJson(opts) {
  const o = opts || {};
  const stateDir = o.stateDir || getStateDir(o);
  const target = path.join(stateDir, PAIR_FILENAME);
  const w = atomicWriteJson(target, o.payload, {
    scopeDir: stateDir,
    randomSuffix: o.randomSuffix,
  });
  if (!w.ok) return { status: w.reason, path: target };
  return { status: STATE_STATUSES.OK, path: target };
}

function readPairJson(opts) {
  const o = opts || {};
  const stateDir = o.stateDir || getStateDir(o);
  const target = path.join(stateDir, PAIR_FILENAME);
  return readJsonIfExists(target);
}

module.exports = {
  getStateDir,
  isPathInsideScope,
  isForbiddenStateKey,
  atomicWriteJson,
  readJsonIfExists,
  writeSetupJson,
  readSetupJson,
  writePairJson,
  readPairJson,
  STATE_STATUSES,
  SCHEMA_VERSION,
  MAX_STATE_BYTES,
  SETUP_FILENAME,
  PAIR_FILENAME,
  FORBIDDEN_STATE_KEY_PATTERNS,
};
