// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Cross-process bootstrap lock under the setup state directory.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { getStateDir } = require("../cli/setup_state.js");

const LOCK_NAME = "bootstrap.lock";
const STALE_MS = 5 * 60 * 1000;

function lockPath(opts) {
  const stateDir = opts.stateDir || getStateDir(opts);
  return path.join(stateDir, LOCK_NAME);
}

/**
 * @returns {{ acquired: boolean, path: string }}
 */
function tryAcquireBootstrapLock(opts) {
  const o = opts || {};
  const target = lockPath(o);
  try {
    fs.mkdirSync(path.dirname(target), { recursive: true });
  } catch (_e) {
    return { acquired: false, path: target };
  }
  if (fs.existsSync(target)) {
    try {
      const st = fs.statSync(target);
      if (Date.now() - st.mtimeMs > (o.staleMs || STALE_MS)) {
        fs.unlinkSync(target);
      } else {
        return { acquired: false, path: target };
      }
    } catch (_e) {
      return { acquired: false, path: target };
    }
  }
  try {
    fs.writeFileSync(target, String(process.pid), { flag: "wx", mode: 0o600 });
    return { acquired: true, path: target };
  } catch (_e) {
    return { acquired: false, path: target };
  }
}

function releaseBootstrapLock(opts) {
  const target = lockPath(opts || {});
  try {
    if (fs.existsSync(target)) fs.unlinkSync(target);
  } catch (_e) {
    /* ignore */
  }
}

module.exports = {
  tryAcquireBootstrapLock,
  releaseBootstrapLock,
  LOCK_NAME,
  STALE_MS,
};
