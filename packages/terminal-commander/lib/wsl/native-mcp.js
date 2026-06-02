// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors
//
// When WSL interop puts /mnt/c/.../nodejs ahead of Linux npm globals,
// `terminal-commander-mcp` resolves to the Windows npm shim. Linux Node
// then fails optionalDependency resolve. Re-exec the native Linux binary.

"use strict";

const { spawn, spawnSync } = require("node:child_process");
const { LINUX_PATH_PREFIX } = require("../bootstrap/constants.js");

function isWindowsMountedShimPath(filePath) {
  const norm = String(filePath || "").replace(/\\/g, "/");
  return process.platform === "linux" && norm.startsWith("/mnt/");
}

function findNativeLinuxMcp() {
  const r = spawnSync("bash", ["-lc", `${LINUX_PATH_PREFIX}command -v terminal-commander-mcp`], {
    encoding: "utf8",
    shell: false,
  });
  if (r.status !== 0) return null;
  const line = (r.stdout || "").trim().split(/\r?\n/)[0];
  if (!line || line.startsWith("/mnt/")) return null;
  return line;
}

/**
 * If this process is the Windows shim running under WSL, replace self with
 * the Linux-native MCP binary. Returns true when a child was spawned.
 */
function tryReexecNativeLinuxMcp(argv) {
  if (!isWindowsMountedShimPath(__filename)) return false;
  const native = findNativeLinuxMcp();
  if (!native) {
    process.stderr.write(
      "terminal-commander: Windows npm shim invoked inside WSL; install native runtime: npm install -g terminal-commander (inside the distro, not on Windows PATH)\n",
    );
    return false;
  }
  const child = spawn(native, argv, { stdio: "inherit", shell: false });
  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code == null ? 1 : code);
  });
  child.on("error", (err) => {
    process.stderr.write(
      `terminal-commander: failed to spawn native MCP ${native}: ${err.code || err.message}\n`,
    );
    process.exit(126);
  });
  return true;
}

module.exports = {
  isWindowsMountedShimPath,
  findNativeLinuxMcp,
  tryReexecNativeLinuxMcp,
};
