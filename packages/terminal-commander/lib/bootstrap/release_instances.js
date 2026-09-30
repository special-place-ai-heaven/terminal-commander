// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// Release running Terminal Commander processes during an npm install so an
// upgrade is never blocked or left half-replaced.
//
// npm has no hook that runs before it swaps package files, and on Windows a
// running binary cannot be deleted. When an older adapter or daemon was still
// running from the npm package, npm moved the old package folder aside but
// could not delete it. The next upgrade then collided with that leftover
// (EBUSY) and npm's rollback restored files from it, mixing versions. The
// install lifecycle therefore stops every instance of this install (the npm
// package scope, including leftovers, and the stable copy), using the same
// native update-locks preflight as `terminal-commander update`, and then
// removes the leftovers. Harnesses reconnect to the MCP server on their own.

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { runUpdatePreflight } = require("../cli/update_preflight.js");

// npm renames a replaced package folder to `.<name>-<random>` in the same
// directory before deleting it.
const NPM_LEFTOVER_RE = /^\.terminal-commander-[A-Za-z0-9]+$/;

/**
 * Delete npm leftover folders of the terminal-commander package in `scopeDir`.
 * A folder is removed only when its name has npm's leftover shape AND its own
 * package.json names terminal-commander.
 */
function removeNpmLeftovers(scopeDir) {
  const removed = [];
  const failed = [];
  let entries;
  try {
    entries = fs.readdirSync(scopeDir);
  } catch (_e) {
    return { removed, failed };
  }
  for (const name of entries) {
    if (!NPM_LEFTOVER_RE.test(name)) continue;
    const dir = path.join(scopeDir, name);
    let manifest;
    try {
      manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    } catch (_e) {
      continue;
    }
    if (!manifest || manifest.name !== "terminal-commander") continue;
    try {
      fs.rmSync(dir, { recursive: true, force: true });
      removed.push(dir);
    } catch (e) {
      failed.push(`${dir} (${(e && (e.code || e.message)) || "error"})`);
    }
  }
  return { removed, failed };
}

/**
 * Stop running instances of this install, then clear npm leftovers.
 * Windows only: elsewhere replacing a running binary never blocks.
 */
async function releaseRunningInstances(opts) {
  const o = opts || {};
  const platform = o.platform || process.platform;
  const lines = [];
  if (platform !== "win32") return { lines, code: 0 };

  const packageRoot = o.packageRoot || path.resolve(__dirname, "../..");
  const code = await (o.runUpdatePreflight || runUpdatePreflight)({
    platform,
    env: o.env || process.env,
    packageRoot,
    writeStderr: (message) => lines.push(String(message).trimEnd()),
  });
  lines.push(
    code === 0
      ? "terminal-commander: stopped running Terminal Commander processes before replacing binaries."
      : `terminal-commander: WARNING stopping running Terminal Commander processes exited ${code}; continuing.`,
  );

  const leftovers = removeNpmLeftovers(path.dirname(packageRoot));
  for (const dir of leftovers.removed) {
    lines.push(`terminal-commander: removed npm leftover ${dir}`);
  }
  for (const entry of leftovers.failed) {
    lines.push(`terminal-commander: WARNING could not remove npm leftover ${entry}`);
  }
  return { lines, code };
}

module.exports = { releaseRunningInstances, removeNpmLeftovers };
