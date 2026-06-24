// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Shared atomic-write-with-backup primitive for the harness config writers.
//
// One code path for json_mcp / toml_mcp / cursor: write `<path>.tmp.<rand>` in
// the SAME directory (mode 0o600) -> fsync -> rename, after copying any existing
// target to `<path>.bak`. Every path (target, tmp, .bak) is asserted inside the
// resolved scope dir before it is touched. Consolidates three previously
// hand-rolled copies and gives every writer the same durability (fsync) and
// path-safety (scope-check) guarantees.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { isPathInsideScope } = require("../../cursor/config.js");

/** Generic outcome reasons. Callers map these to their own per-file status enum. */
const ATOMIC_REASONS = Object.freeze({
  PATH_NOT_ALLOWED: "path_not_allowed",
  BACKUP_FAILED: "backup_failed",
  WRITE_FAILED: "write_failed",
});

/**
 * Copy `target` -> `<target>.bak` if `target` exists. No-op (ok, backup_path:null)
 * when it does not. Refuses an existing `.bak` unless `clobber_backup === true`.
 *
 * @param {string} target
 * @param {Object} [opts]
 * @param {string} [opts.scopeDir]  Defaults to dirname(target).
 * @param {boolean} [opts.clobber_backup=false]
 * @returns {{ok:true, backup_path:string|null}|{ok:false, reason:string}}
 */
function backupExisting(target, opts) {
  const o = opts || {};
  const scopeDir = o.scopeDir || path.dirname(target);
  if (!fs.existsSync(target)) {
    return { ok: true, backup_path: null };
  }
  const backupPath = target + ".bak";
  if (!isPathInsideScope(scopeDir, backupPath)) {
    return { ok: false, reason: ATOMIC_REASONS.PATH_NOT_ALLOWED };
  }
  if (fs.existsSync(backupPath) && o.clobber_backup !== true) {
    return { ok: false, reason: ATOMIC_REASONS.BACKUP_FAILED };
  }
  try {
    fs.copyFileSync(target, backupPath);
  } catch (_e) {
    return { ok: false, reason: ATOMIC_REASONS.BACKUP_FAILED };
  }
  return { ok: true, backup_path: backupPath };
}

/**
 * Atomic write: create parent dir, write `<target>.tmp.<rand>` in the SAME
 * directory, fsync it, then rename onto `target`. Refuses any path outside
 * `scopeDir`. Cleans up the tmp file on failure.
 *
 * @param {string} target
 * @param {string} contents
 * @param {Object} [opts]
 * @param {string} [opts.scopeDir]  Defaults to dirname(target).
 * @param {(p:string)=>string} [opts.randomSuffix]  Injected randomness for tests.
 * @returns {{ok:true, path:string}|{ok:false, reason:string}}
 */
function atomicWrite(target, contents, opts) {
  const o = opts || {};
  const scopeDir = o.scopeDir || path.dirname(target);
  if (!isPathInsideScope(scopeDir, target)) {
    return { ok: false, reason: ATOMIC_REASONS.PATH_NOT_ALLOWED };
  }
  try {
    fs.mkdirSync(path.dirname(target), { recursive: true });
  } catch (_e) {
    return { ok: false, reason: ATOMIC_REASONS.WRITE_FAILED };
  }
  const suffix =
    typeof o.randomSuffix === "function"
      ? o.randomSuffix(target)
      : crypto.randomBytes(8).toString("hex");
  const tmp = target + ".tmp." + suffix;
  if (!isPathInsideScope(scopeDir, tmp)) {
    return { ok: false, reason: ATOMIC_REASONS.PATH_NOT_ALLOWED };
  }
  let fd;
  try {
    fd = fs.openSync(tmp, "w", 0o600);
    fs.writeSync(fd, contents);
    try {
      fs.fsyncSync(fd);
    } catch (_e) {
      // fsync may be unsupported on some test/overlay filesystems; non-fatal.
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
    return { ok: false, reason: ATOMIC_REASONS.WRITE_FAILED };
  }
  return { ok: true, path: target };
}

/**
 * Backup any existing target, then atomically write the new contents. The
 * combined primitive every config writer's tail funnels through.
 *
 * @param {string} target
 * @param {string} contents
 * @param {Object} [opts]
 * @param {string} [opts.scopeDir]  Defaults to dirname(target).
 * @param {boolean} [opts.clobber_backup=false]
 * @param {(p:string)=>string} [opts.randomSuffix]
 * @returns {{ok:true, path:string, backup_path:string|null}|{ok:false, reason:string, backup_path?:string|null}}
 */
function atomicWriteWithBackup(target, contents, opts) {
  const o = opts || {};
  const scopeDir = o.scopeDir || path.dirname(target);
  const backup = backupExisting(target, {
    scopeDir,
    clobber_backup: o.clobber_backup === true,
  });
  if (!backup.ok) {
    return { ok: false, reason: backup.reason };
  }
  const wrote = atomicWrite(target, contents, {
    scopeDir,
    randomSuffix: o.randomSuffix,
  });
  if (!wrote.ok) {
    return { ok: false, reason: wrote.reason, backup_path: backup.backup_path };
  }
  return { ok: true, path: target, backup_path: backup.backup_path };
}

module.exports = {
  atomicWrite,
  backupExisting,
  atomicWriteWithBackup,
  ATOMIC_REASONS,
};
