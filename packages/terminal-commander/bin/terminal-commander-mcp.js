#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Terminal Commander Authors
//
// NPM03 shim for `terminal-commander-mcp`, extended at WWS02 to
// recognize the bridge-required branch on Windows hosts. The
// actual `wsl.exe` invocation belongs to WWS04 (`lib/wsl/spawn.js`);
// WWS02 only emits a bounded "pending WWS04" stderr line and exits
// 64 so a Windows install does not silently swallow MCP traffic
// before the real bridge is wired.
//
// Bounded behavior identical to `terminal-commanderd.js` on the
// Linux + ok path: spawn the resolved Rust binary with
// `shell: false` and `stdio: 'inherit'`. NO wsl.exe call in WWS02.

"use strict";

const { spawn } = require("child_process");
const { resolveBinary, formatResolveError } = require("../lib/resolve-binary.js");

const result = resolveBinary({ binary: "terminal-commander-mcp" });

if (result.reason === "bridge_required") {
  process.stderr.write(
    "terminal-commander: Windows host bridge mode is pending WWS04. " +
      "Until then, run 'terminal-commander-mcp' from inside a WSL distro " +
      "(e.g. 'wsl -d <distro> -- bash -lc terminal-commander-mcp'), " +
      "or wait for the WWS04 release that adds the native Windows bridge shim.\n",
  );
  process.exit(64);
}

if (result.reason !== "ok") {
  process.stderr.write(formatResolveError(result) + "\n");
  process.exit(64);
}

const child = spawn(result.binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  shell: false,
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  }
  process.exit(code == null ? 1 : code);
});

child.on("error", (err) => {
  process.stderr.write(`terminal-commander: failed to spawn ${result.binaryPath}: ${err.code || err.message}\n`);
  process.exit(126);
});
